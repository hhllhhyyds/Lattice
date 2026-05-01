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
        std::future::ready(Ok(ListResourcesResult {
            resources: vec![
                rmcp::model::RawResource::new("fixture://readme", "Fixture Readme")
                    .with_description("Fixture resource")
                    .with_audience(vec![Role::User]),
            ],
            next_cursor: None,
            meta: None,
        }))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        if request.uri == "fixture://readme" {
            Ok(ReadResourceResult::new(vec![ResourceContents::text(
                "fixture resource contents",
                "fixture://readme",
            )]))
        } else {
            Err(McpError::resource_not_found(
                format!("unknown resource: {}", request.uri),
                None,
            ))
        }
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
