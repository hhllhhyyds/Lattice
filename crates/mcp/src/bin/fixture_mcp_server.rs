use std::future::Future;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        AnnotateAble, ListResourcesResult, ReadResourceRequestParams, ReadResourceResult,
        ResourceContents, Role, ServerCapabilities, ServerInfo,
    },
    schemars,
    service::{MaybeSendFuture, RequestContext},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct HelloArgs {
    name: String,
}

#[allow(dead_code)]
#[derive(Clone)]
struct FixtureServer {
    tool_router: ToolRouter<Self>,
}

impl FixtureServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    fn fixture_resources() -> ListResourcesResult {
        ListResourcesResult {
            resources: vec![
                rmcp::model::RawResource::new("fixture://readme", "Fixture Readme")
                    .with_audience(vec![Role::User]),
            ],
            next_cursor: None,
            meta: None,
        }
    }

    fn fixture_resource_contents(uri: &str) -> Result<ReadResourceResult, McpError> {
        if uri == "fixture://readme" {
            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                "fixture resource contents",
                "fixture://readme",
            )]))
        } else {
            Err(McpError::resource_not_found(
                format!("unknown resource: {uri}"),
                None,
            ))
        }
    }
}

#[tool_router]
impl FixtureServer {
    #[tool(description = "Return a fixture greeting")]
    fn hello(&self, Parameters(HelloArgs { name }): Parameters<HelloArgs>) -> String {
        format!("fixture-hello:{name}")
    }
}

#[tool_handler]
impl ServerHandler for FixtureServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions("Fixture MCP server")
    }

    fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + MaybeSendFuture + '_ {
        std::future::ready(Ok(Self::fixture_resources()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        Self::fixture_resource_contents(&request.uri)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = FixtureServer::new()
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
        let server = FixtureServer::new();
        let result = server.hello(Parameters(HelloArgs {
            name: "lattice".to_string(),
        }));
        assert_eq!(result, "fixture-hello:lattice");
    }

    #[test]
    fn server_info_enables_tools_and_resources() {
        let info = FixtureServer::new().get_info();
        assert_eq!(info.instructions.as_deref(), Some("Fixture MCP server"));
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_some());
    }

    #[test]
    fn fixture_resources_returns_fixture_resource() {
        let result = FixtureServer::fixture_resources();

        assert_eq!(result.resources.len(), 1);
        assert_eq!(result.resources[0].uri, "fixture://readme");
        assert_eq!(result.resources[0].name, "Fixture Readme");
    }

    #[test]
    fn read_resource_returns_contents_for_known_uri() {
        let result = FixtureServer::fixture_resource_contents("fixture://readme").unwrap();

        assert_eq!(result.contents.len(), 1);
    }

    #[test]
    fn read_resource_rejects_unknown_uri() {
        let err = FixtureServer::fixture_resource_contents("fixture://missing").unwrap_err();

        assert!(err.message.contains("unknown resource"));
    }
}
