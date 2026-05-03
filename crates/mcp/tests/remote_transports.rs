use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use lattice_mcp::{
    McpClientManager, McpConnectionState, McpHttpServerConfig, McpServerConfig,
    McpWebSocketServerConfig,
};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request as WsRequest, Response as WsResponse},
        Message,
    },
};

#[derive(Clone, Default)]
struct ManualMcpState {
    expected_auth: Option<String>,
    expected_custom_header: Option<(String, String)>,
    initialize_seen: Arc<Notify>,
}

impl ManualMcpState {
    fn expect_bearer_and_header(token: &str, name: &str, value: &str) -> Self {
        Self {
            expected_auth: Some(format!("Bearer {token}")),
            expected_custom_header: Some((name.to_string(), value.to_string())),
            initialize_seen: Arc::new(Notify::new()),
        }
    }

    fn validate_headers(&self, headers: &HeaderMap) -> Result<(), String> {
        if let Some(expected_auth) = &self.expected_auth {
            let actual = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "missing authorization header".to_string())?;
            if actual != expected_auth {
                return Err(format!(
                    "unexpected authorization header: expected '{expected_auth}', got '{actual}'"
                ));
            }
        }

        if let Some((name, expected_value)) = &self.expected_custom_header {
            let actual = headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("missing custom header '{name}'"))?;
            if actual != expected_value {
                return Err(format!(
                    "unexpected custom header for '{name}': expected '{expected_value}', got '{actual}'"
                ));
            }
        }

        Ok(())
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2025-03-26",
        "capabilities": {
            "tools": {},
            "resources": {}
        },
        "serverInfo": {
            "name": "fixture-remote",
            "version": "1.0.0"
        }
    })
}

fn tools_result() -> Value {
    json!({
        "tools": [
            {
                "name": "hello",
                "description": "fixture remote hello",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"]
                }
            }
        ]
    })
}

fn resources_result() -> Value {
    json!({
        "resources": [
            {
                "uri": "fixture://readme",
                "name": "Fixture Readme",
                "description": "Fixture resource"
            }
        ]
    })
}

fn response_for_body(body: &Value) -> Option<Value> {
    let method = body.get("method")?.as_str()?;
    let id = body.get("id").cloned();
    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": initialize_result()
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": tools_result()
        })),
        "resources/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": resources_result()
        })),
        _ => None,
    }
}

async fn manual_http_mcp_handler(
    State(state): State<ManualMcpState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(err) = state.validate_headers(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            err,
        )
            .into_response();
    }

    let Ok(body) = serde_json::from_slice::<Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            "invalid JSON body".to_string(),
        )
            .into_response();
    };

    match body.get("method").and_then(Value::as_str) {
        Some("initialize") => {
            state.initialize_seen.notify_one();
            (
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, "application/json"),
                    (
                        axum::http::HeaderName::from_static("mcp-session-id"),
                        "fixture-http-session",
                    ),
                ],
                json!({
                    "jsonrpc": "2.0",
                    "id": body.get("id"),
                    "result": initialize_result()
                })
                .to_string(),
            )
                .into_response()
        }
        Some("notifications/initialized") => (
            StatusCode::ACCEPTED,
            [
                (axum::http::header::CONTENT_TYPE, "application/json"),
                (
                    axum::http::HeaderName::from_static("mcp-session-id"),
                    "fixture-http-session",
                ),
            ],
            String::new(),
        )
            .into_response(),
        Some("tools/list") | Some("resources/list") => (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, "application/json"),
                (
                    axum::http::HeaderName::from_static("mcp-session-id"),
                    "fixture-http-session",
                ),
            ],
            response_for_body(&body)
                .expect("tool/resource response should exist")
                .to_string(),
        )
            .into_response(),
        Some(other) => (
            StatusCode::BAD_REQUEST,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            format!("unexpected MCP method: {other}"),
        )
            .into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            "missing MCP method".to_string(),
        )
            .into_response(),
    }
}

async fn spawn_http_mcp_server(state: ManualMcpState) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local HTTP MCP listener");
    let addr = listener.local_addr().expect("resolve bound local address");
    let app = Router::new()
        .route("/mcp", post(manual_http_mcp_handler))
        .with_state(state);
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve HTTP MCP listener");
    });
    (addr, handle)
}

#[allow(clippy::result_large_err)]
async fn run_manual_ws_mcp_connection(
    stream: TcpStream,
    state: ManualMcpState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state_for_callback = state.clone();
    let callback = move |request: &WsRequest, response: WsResponse| {
        let mut headers = HeaderMap::new();
        for (name, value) in request.headers() {
            headers.insert(name.clone(), value.clone());
        }
        state_for_callback
            .validate_headers(&headers)
            .map_err(|message| {
                let mut error_response = http::Response::new(Some(message));
                *error_response.status_mut() = StatusCode::UNAUTHORIZED;
                error_response
            })?;
        Ok(response)
    };

    let mut websocket = accept_hdr_async(stream, callback)
        .await
        .map_err(|err| format!("accept websocket handshake: {err}"))?;

    while let Some(message) = websocket.next().await {
        let message = message?;
        if !message.is_text() && !message.is_binary() {
            continue;
        }

        let payload = if message.is_text() {
            message.into_text()?.to_string()
        } else {
            String::from_utf8(message.into_data().to_vec())?
        };
        let body: Value = serde_json::from_str(&payload)?;

        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                state.initialize_seen.notify_one();
                websocket
                    .send(Message::text(
                        json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id"),
                            "result": initialize_result()
                        })
                        .to_string(),
                    ))
                    .await?;
            }
            Some("notifications/initialized") => {}
            Some("tools/list") | Some("resources/list") => {
                websocket
                    .send(Message::text(
                        response_for_body(&body)
                            .expect("tool/resource response should exist")
                            .to_string(),
                    ))
                    .await?;
            }
            Some(other) => {
                websocket
                    .send(Message::text(
                        json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id"),
                            "error": {
                                "code": -32601,
                                "message": format!("unexpected MCP method: {other}")
                            }
                        })
                        .to_string(),
                    ))
                    .await?;
            }
            None => break,
        }
    }

    Ok(())
}

async fn spawn_ws_mcp_server(state: ManualMcpState) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local websocket MCP listener");
    let addr = listener.local_addr().expect("resolve bound local address");
    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept websocket client");
        run_manual_ws_mcp_connection(stream, state)
            .await
            .expect("serve websocket MCP session");
    });
    (addr, handle)
}

#[tokio::test]
async fn http_remote_connects_and_discovers_capabilities() {
    let state = ManualMcpState::expect_bearer_and_header("http-token", "x-lattice-test", "http");
    let notified = state.initialize_seen.clone();
    let (addr, server_handle) = spawn_http_mcp_server(state).await;

    let mut headers = HashMap::new();
    headers.insert("x-lattice-test".to_string(), "http".to_string());

    let mut configs = HashMap::new();
    configs.insert(
        "http-remote".to_string(),
        McpServerConfig::Http(McpHttpServerConfig {
            url: format!("http://{addr}/mcp"),
            bearer_token: Some("http-token".to_string()),
            headers,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    tokio::time::timeout(std::time::Duration::from_secs(5), notified.notified())
        .await
        .expect("HTTP initialize request should arrive");

    let statuses = manager.list_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "http-remote");
    assert_eq!(statuses[0].state, McpConnectionState::Connected);
    assert_eq!(statuses[0].transport, "http");
    assert_eq!(statuses[0].tools.len(), 1);
    assert_eq!(statuses[0].tools[0].name, "hello");
    assert_eq!(statuses[0].resources.len(), 1);
    assert_eq!(statuses[0].resources[0].uri, "fixture://readme");

    manager.close().await;
    server_handle.abort();
}

#[tokio::test]
async fn websocket_remote_connects_and_discovers_capabilities() {
    let state = ManualMcpState::expect_bearer_and_header("ws-token", "x-lattice-test", "ws");
    let notified = state.initialize_seen.clone();
    let (addr, server_handle) = spawn_ws_mcp_server(state).await;

    let mut headers = HashMap::new();
    headers.insert("x-lattice-test".to_string(), "ws".to_string());

    let mut configs = HashMap::new();
    configs.insert(
        "ws-remote".to_string(),
        McpServerConfig::Ws(McpWebSocketServerConfig {
            url: format!("ws://{addr}/mcp"),
            bearer_token: Some("ws-token".to_string()),
            headers,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    tokio::time::timeout(std::time::Duration::from_secs(5), notified.notified())
        .await
        .expect("websocket initialize request should arrive");

    let statuses = manager.list_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "ws-remote");
    assert_eq!(statuses[0].state, McpConnectionState::Connected);
    assert_eq!(statuses[0].transport, "ws");
    assert_eq!(statuses[0].tools.len(), 1);
    assert_eq!(statuses[0].tools[0].name, "hello");
    assert_eq!(statuses[0].resources.len(), 1);
    assert_eq!(statuses[0].resources[0].uri, "fixture://readme");

    manager.close().await;
    server_handle.abort();
}

#[tokio::test]
async fn bearer_token_conflicts_with_explicit_authorization_header() {
    let mut http_headers = HashMap::new();
    http_headers.insert(
        "Authorization".to_string(),
        "Bearer explicit-http".to_string(),
    );
    let mut ws_headers = HashMap::new();
    ws_headers.insert(
        "authorization".to_string(),
        "Bearer explicit-ws".to_string(),
    );

    let mut configs = HashMap::new();
    configs.insert(
        "http-remote".to_string(),
        McpServerConfig::Http(McpHttpServerConfig {
            url: "http://127.0.0.1:1/mcp".to_string(),
            bearer_token: Some("http-token".to_string()),
            headers: http_headers,
        }),
    );
    configs.insert(
        "ws-remote".to_string(),
        McpServerConfig::Ws(McpWebSocketServerConfig {
            url: "ws://127.0.0.1:1/mcp".to_string(),
            bearer_token: Some("ws-token".to_string()),
            headers: ws_headers,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].state, McpConnectionState::Failed);
    assert!(statuses[0]
        .detail
        .contains("authorization header must not be set when bearer_token is configured"));
    assert_eq!(statuses[1].state, McpConnectionState::Failed);
    assert!(statuses[1]
        .detail
        .contains("authorization header must not be set when bearer_token is configured"));
}
