use std::future::Future;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars,
    service::MaybeSendFuture,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct HelloArgs {
    name: String,
}

#[allow(dead_code)]
#[derive(Clone)]
struct FixtureBrokenResourcesServer {
    tool_router: ToolRouter<Self>,
}

impl FixtureBrokenResourcesServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    fn broken_resources_error() -> McpError {
        McpError::internal_error("fixture resources unavailable", None)
    }
}

#[tool_router]
impl FixtureBrokenResourcesServer {
    #[tool(description = "Return a fixture greeting")]
    fn hello(&self, Parameters(HelloArgs { name }): Parameters<HelloArgs>) -> String {
        format!("fixture-hello:{name}")
    }
}

#[tool_handler]
impl ServerHandler for FixtureBrokenResourcesServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions("Broken resources fixture MCP server")
    }

    fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListResourcesResult, McpError>> + MaybeSendFuture + '_
    {
        std::future::ready(Err(Self::broken_resources_error()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = FixtureBrokenResourcesServer::new()
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_resources_error_is_internal_error() {
        let err = FixtureBrokenResourcesServer::broken_resources_error();
        assert_eq!(err.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(err.message.contains("fixture resources unavailable"));
    }
}
