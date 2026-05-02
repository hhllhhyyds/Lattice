use std::{collections::HashMap, future::Future, sync::Arc};

use futures_util::{SinkExt, StreamExt};
use http::{HeaderName, HeaderValue};
use lattice_core::ToolError;
use rmcp::{
    model::{CallToolRequestParams, ErrorCode, ReadResourceRequestParams, ResourceContents, Tool},
    service::{RoleClient, RunningService, RxJsonRpcMessage, ServiceError, TxJsonRpcMessage},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
        TokioChildProcess, Transport,
    },
    ServiceExt,
};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::warn;

use crate::types::{
    McpConnectionSnapshot, McpConnectionStatus, McpHttpServerConfig, McpResourceInfo,
    McpServerConfig, McpStdioServerConfig, McpToolInfo, McpWebSocketServerConfig,
};

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolCallOutput {
    pub output: String,
    pub structured_content: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
enum WebSocketTransportError {
    #[error("websocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

struct WsClientTransport {
    sink: Arc<
        Mutex<futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>,
    >,
    stream: futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

pub struct McpClientManager {
    server_configs: HashMap<String, McpServerConfig>,
    statuses: HashMap<String, McpConnectionStatus>,
    sessions: HashMap<String, RunningService<RoleClient, ()>>,
}

impl McpClientManager {
    #[must_use]
    pub fn new(server_configs: HashMap<String, McpServerConfig>) -> Self {
        let statuses = server_configs
            .iter()
            .map(|(name, config)| {
                (
                    name.clone(),
                    McpConnectionStatus::pending(name.clone(), config.transport()),
                )
            })
            .collect();

        Self {
            server_configs,
            statuses,
            sessions: HashMap::new(),
        }
    }

    #[must_use]
    pub fn list_statuses(&self) -> Vec<McpConnectionStatus> {
        let mut statuses: Vec<_> = self.statuses.values().cloned().collect();
        statuses.sort_by(|a, b| a.name.cmp(&b.name));
        statuses
    }

    #[must_use]
    pub fn list_tools(&self) -> Vec<McpToolInfo> {
        self.list_statuses()
            .into_iter()
            .flat_map(|status| status.tools)
            .collect()
    }

    #[must_use]
    pub fn list_resources(&self) -> Vec<McpResourceInfo> {
        self.list_statuses()
            .into_iter()
            .flat_map(|status| status.resources)
            .collect()
    }

    #[must_use]
    pub fn list_status_snapshots(&self) -> Vec<McpConnectionSnapshot> {
        self.list_statuses()
            .into_iter()
            .map(|status| status.snapshot())
            .collect()
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<McpToolCallOutput, ToolError> {
        let session = self.require_session(server_name)?;

        let request = match arguments {
            Value::Null => CallToolRequestParams::new(tool_name.to_string()),
            Value::Object(map) => {
                CallToolRequestParams::new(tool_name.to_string()).with_arguments(map)
            }
            other => {
                return Err(ToolError::InvalidParams(format!(
                    "MCP tool arguments must be a JSON object, got {}",
                    json_type_name(&other)
                )));
            }
        };

        let result = session.call_tool(request).await.map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "MCP tool call failed on server '{server_name}' for '{tool_name}': {err}"
            ))
        })?;

        let output = render_tool_result(&result);
        if result.is_error == Some(true) {
            return Err(ToolError::ExecutionFailed(output));
        }

        Ok(McpToolCallOutput {
            output,
            structured_content: result.structured_content,
        })
    }

    pub async fn read_resource(&self, server_name: &str, uri: &str) -> Result<String, ToolError> {
        let session = self.require_session(server_name)?;
        let result = session
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|err| map_read_resource_error(server_name, uri, &err))?;

        Ok(render_read_resource_result(&result))
    }

    pub async fn connect_all(&mut self) {
        let names: Vec<String> = self.server_configs.keys().cloned().collect();
        for name in names {
            self.connect_one(&name).await;
        }
    }

    pub async fn reconnect_all(&mut self) {
        self.close().await;
        self.statuses = self
            .server_configs
            .iter()
            .map(|(name, config)| {
                (
                    name.clone(),
                    McpConnectionStatus::pending(name.clone(), config.transport()),
                )
            })
            .collect();
        self.connect_all().await;
    }

    pub async fn close(&mut self) {
        for session in self.sessions.values_mut() {
            if let Err(err) = session.close().await {
                warn!(?err, "failed to close MCP session cleanly");
            }
        }
        self.sessions.clear();

        for (name, config) in &self.server_configs {
            self.statuses.insert(
                name.clone(),
                McpConnectionStatus::pending(name.clone(), config.transport()),
            );
        }
    }

    async fn connect_one(&mut self, name: &str) {
        let Some(config) = self.server_configs.get(name).cloned() else {
            return;
        };

        match config {
            McpServerConfig::Stdio(stdio) => self.connect_stdio(name, stdio).await,
            McpServerConfig::Http(http) => self.connect_http(name, http).await,
            McpServerConfig::Ws(ws) => self.connect_ws(name, ws).await,
        }
    }

    async fn connect_stdio(&mut self, name: &str, config: McpStdioServerConfig) {
        let mut command = Command::new(&config.command);
        command.args(&config.args);
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }
        if let Some(env) = &config.env {
            command.envs(env);
        }

        let transport = match TokioChildProcess::new(command) {
            Ok(transport) => transport,
            Err(err) => {
                self.record_failed_status(name, "stdio", err.to_string());
                return;
            }
        };

        let client = match ().serve(transport).await {
            Ok(client) => client,
            Err(err) => {
                self.record_failed_status(name, "stdio", err.to_string());
                return;
            }
        };

        self.store_connected_client(name, "stdio", client).await;
    }

    async fn connect_http(&mut self, name: &str, config: McpHttpServerConfig) {
        let headers = match parse_custom_headers(&config.headers, config.bearer_token.as_deref()) {
            Ok(headers) => headers,
            Err(err) => {
                self.record_failed_status(name, "http", err);
                return;
            }
        };

        let mut transport_config = StreamableHttpClientTransportConfig::with_uri(config.url);
        if let Some(token) = config.bearer_token {
            transport_config = transport_config.auth_header(token);
        }
        transport_config = transport_config.custom_headers(headers);

        let transport = StreamableHttpClientTransport::from_config(transport_config);
        let client = match ().serve(transport).await {
            Ok(client) => client,
            Err(err) => {
                self.record_failed_status(name, "http", err.to_string());
                return;
            }
        };

        self.store_connected_client(name, "http", client).await;
    }

    async fn connect_ws(&mut self, name: &str, config: McpWebSocketServerConfig) {
        let headers = match parse_custom_headers(&config.headers, config.bearer_token.as_deref()) {
            Ok(headers) => headers,
            Err(err) => {
                self.record_failed_status(name, "ws", err);
                return;
            }
        };

        let request = match build_websocket_request(&config.url, headers, config.bearer_token) {
            Ok(request) => request,
            Err(err) => {
                self.record_failed_status(name, "ws", err);
                return;
            }
        };

        let stream = match connect_async(request).await {
            Ok((stream, _)) => stream,
            Err(err) => {
                self.record_failed_status(name, "ws", err.to_string());
                return;
            }
        };

        let client = match connect_ws_client(stream).await {
            Ok(client) => client,
            Err(err) => {
                self.record_failed_status(name, "ws", err.to_string());
                return;
            }
        };

        self.store_connected_client(name, "ws", client).await;
    }

    async fn store_connected_client(
        &mut self,
        name: &str,
        transport: &str,
        client: RunningService<RoleClient, ()>,
    ) {
        let tools = match client.list_all_tools().await {
            Ok(tools) => tools,
            Err(err) => {
                self.record_failed_status(name, transport, err.to_string());
                return;
            }
        };

        let resources = match client.list_all_resources().await {
            Ok(resources) => resources,
            Err(err) if is_method_not_found(&err) => Vec::new(),
            Err(err) => {
                self.record_failed_status(name, transport, err.to_string());
                return;
            }
        };

        self.statuses.insert(
            name.to_string(),
            McpConnectionStatus::connected(
                name.to_string(),
                transport,
                tools
                    .into_iter()
                    .map(|tool| to_tool_info(name, tool))
                    .collect(),
                resources
                    .into_iter()
                    .map(|resource| McpResourceInfo {
                        server_name: name.to_string(),
                        name: resource.raw.name,
                        uri: resource.raw.uri,
                        description: resource.raw.description.unwrap_or_default(),
                    })
                    .collect(),
            ),
        );
        self.sessions.insert(name.to_string(), client);
    }

    fn record_failed_status(&mut self, name: &str, transport: &str, detail: impl Into<String>) {
        self.statuses.insert(
            name.to_string(),
            McpConnectionStatus::failed(name.to_string(), transport, detail),
        );
    }

    fn require_session(
        &self,
        server_name: &str,
    ) -> Result<&RunningService<RoleClient, ()>, ToolError> {
        self.sessions.get(server_name).ok_or_else(|| {
            let detail = self
                .statuses
                .get(server_name)
                .map(|status| status.detail.as_str())
                .filter(|detail| !detail.is_empty())
                .unwrap_or("server is not connected");
            ToolError::NotFound(format!(
                "MCP server '{server_name}' is not connected: {detail}"
            ))
        })
    }
}

fn to_tool_info(server_name: &str, tool: Tool) -> McpToolInfo {
    McpToolInfo {
        server_name: server_name.to_string(),
        name: tool.name.into_owned(),
        description: tool.description.map(|d| d.into_owned()).unwrap_or_default(),
        input_schema: tool.input_schema.as_ref().clone(),
    }
}

fn is_method_not_found(err: &ServiceError) -> bool {
    matches!(err, ServiceError::McpError(error) if error.code == ErrorCode::METHOD_NOT_FOUND)
}

fn parse_custom_headers(
    raw_headers: &HashMap<String, String>,
    bearer_token: Option<&str>,
) -> Result<HashMap<HeaderName, HeaderValue>, String> {
    if bearer_token.is_some()
        && raw_headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
    {
        return Err(
            "authorization header must not be set when bearer_token is configured".to_string(),
        );
    }

    let mut headers = HashMap::with_capacity(raw_headers.len());
    for (name, value) in raw_headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| format!("invalid MCP header name '{name}': {err}"))?;
        let header_value = HeaderValue::from_str(value)
            .map_err(|err| format!("invalid MCP header value for '{name}': {err}"))?;
        headers.insert(header_name, header_value);
    }
    Ok(headers)
}

fn build_websocket_request(
    url: &str,
    headers: HashMap<HeaderName, HeaderValue>,
    bearer_token: Option<String>,
) -> Result<http::Request<()>, String> {
    let mut request = url
        .into_client_request()
        .map_err(|err| format!("invalid websocket request for '{url}': {err}"))?;

    for (name, value) in headers {
        request.headers_mut().insert(name, value);
    }

    if let Some(token) = bearer_token {
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|err| format!("invalid websocket bearer token: {err}"))?;
        request
            .headers_mut()
            .insert(HeaderName::from_static("authorization"), value);
    }

    Ok(request)
}

async fn connect_ws_client(
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<RunningService<RoleClient, ()>, rmcp::service::ClientInitializeError> {
    let (sink, stream) = stream.split();
    let transport = WsClientTransport {
        sink: Arc::new(Mutex::new(sink)),
        stream,
    };
    ().serve(transport).await
}

impl Transport<RoleClient> for WsClientTransport {
    type Error = WebSocketTransportError;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let sink = Arc::clone(&self.sink);
        async move {
            let payload = serde_json::to_string(&item)?;
            let mut sink = sink.lock().await;
            sink.send(Message::Text(payload)).await?;
            Ok(())
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        while let Some(message) = self.stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<RxJsonRpcMessage<RoleClient>>(&text) {
                        Ok(message) => return Some(message),
                        Err(err) => {
                            warn!(?err, "dropping invalid websocket MCP text frame");
                        }
                    }
                }
                Ok(Message::Binary(bytes)) => {
                    match std::str::from_utf8(&bytes).ok().and_then(|text| {
                        serde_json::from_str::<RxJsonRpcMessage<RoleClient>>(text).ok()
                    }) {
                        Some(message) => return Some(message),
                        None => warn!("dropping invalid websocket MCP binary frame"),
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                Ok(Message::Close(_)) => return None,
                Err(err) => {
                    warn!(?err, "websocket MCP stream closed with transport error");
                    return None;
                }
            }
        }

        None
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        let mut sink = self.sink.lock().await;
        sink.send(Message::Close(None)).await?;
        Ok(())
    }
}

fn render_tool_result(result: &rmcp::model::CallToolResult) -> String {
    let text_parts: Vec<String> = result
        .content
        .iter()
        .filter_map(|content| content.raw.as_text().map(|text| text.text.clone()))
        .collect();

    if !text_parts.is_empty() {
        return text_parts.join("\n");
    }

    if let Some(structured) = &result.structured_content {
        return serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string());
    }

    String::new()
}

fn render_read_resource_result(result: &rmcp::model::ReadResourceResult) -> String {
    result
        .contents
        .iter()
        .map(render_resource_contents)
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_resource_contents(content: &ResourceContents) -> String {
    match content {
        ResourceContents::TextResourceContents { text, .. } => text.clone(),
        ResourceContents::BlobResourceContents { blob, .. } => blob.clone(),
    }
}

fn map_read_resource_error(server_name: &str, uri: &str, err: &ServiceError) -> ToolError {
    match err {
        ServiceError::McpError(error) if error.code == ErrorCode::RESOURCE_NOT_FOUND => {
            ToolError::NotFound(format!(
                "MCP resource not found on server '{server_name}': {uri}"
            ))
        }
        ServiceError::McpError(error) if error.code == ErrorCode::INVALID_PARAMS => {
            ToolError::InvalidParams(format!(
                "MCP resource read rejected by server '{server_name}' for '{uri}': {}",
                error.message
            ))
        }
        ServiceError::McpError(error) if error.code == ErrorCode::METHOD_NOT_FOUND => {
            ToolError::ExecutionFailed(format!(
                "MCP server '{server_name}' does not support resource reads: {}",
                error.message
            ))
        }
        _ => ToolError::ExecutionFailed(format!(
            "MCP resource read failed on server '{server_name}' for '{uri}': {err}"
        )),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
