use std::future::Future;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ListResourcesRequestMethod, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct HelloArgs {
    name: String,
}

#[allow(dead_code)]
#[derive(Clone)]
struct FixtureToolsOnlyServer {
    tool_router: ToolRouter<Self>,
}

impl FixtureToolsOnlyServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    fn list_resources_not_supported() -> McpError {
        McpError::method_not_found::<ListResourcesRequestMethod>()
    }
}

#[tool_router]
impl FixtureToolsOnlyServer {
    #[tool(description = "Return a tools-only fixture greeting")]
    fn hello(&self, Parameters(HelloArgs { name }): Parameters<HelloArgs>) -> String {
        format!("fixture-hello:{name}")
    }
}

#[tool_handler]
impl ServerHandler for FixtureToolsOnlyServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Tools-only fixture MCP server")
    }

    fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::ListResourcesResult, McpError>>
           + rmcp::service::MaybeSendFuture
           + '_ {
        std::future::ready(Err(Self::list_resources_not_supported()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = FixtureToolsOnlyServer::new()
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_tool_formats_fixture_response() {
        let server = FixtureToolsOnlyServer::new();
        let result = server.hello(Parameters(HelloArgs {
            name: "lattice".to_string(),
        }));
        assert_eq!(result, "fixture-hello:lattice");
    }

    #[test]
    fn server_info_only_enables_tools() {
        let info = FixtureToolsOnlyServer::new().get_info();
        assert_eq!(
            info.instructions.as_deref(),
            Some("Tools-only fixture MCP server")
        );
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_none());
    }

    #[test]
    fn list_resources_returns_method_not_found() {
        let err = FixtureToolsOnlyServer::list_resources_not_supported();

        assert_eq!(err.code, rmcp::model::ErrorCode::METHOD_NOT_FOUND);
    }
}
