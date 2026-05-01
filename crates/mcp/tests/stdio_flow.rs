use std::collections::HashMap;
use std::path::PathBuf;

use lattice_mcp::{
    McpClientManager, McpConnectionState, McpHttpServerConfig, McpServerConfig,
    McpStdioServerConfig, McpWebSocketServerConfig,
};
use rmcp::{
    model::ReadResourceRequestParams, service::RoleClient, transport::TokioChildProcess, ServiceExt,
};
use tokio::process::Command;

fn fixture_binary(name: &str) -> String {
    let key = format!("CARGO_BIN_EXE_{name}");
    let path = std::env::var_os(&key).unwrap_or_else(|| panic!("missing fixture binary: {key}"));
    PathBuf::from(path).to_string_lossy().into_owned()
}

async fn connect_fixture_client(
    command: String,
    cwd: Option<PathBuf>,
    env: Option<HashMap<String, String>>,
) -> rmcp::service::RunningService<RoleClient, ()> {
    let mut process = Command::new(command);
    if let Some(cwd) = cwd {
        process.current_dir(cwd);
    }
    if let Some(env) = env {
        process.envs(env);
    }

    let transport = TokioChildProcess::new(process).expect("fixture process should start");
    ().serve(transport)
        .await
        .expect("fixture client should connect")
}

#[tokio::test]
async fn new_manager_starts_all_servers_pending() {
    let mut configs = HashMap::new();
    configs.insert(
        "fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_server"),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );
    configs.insert(
        "remote".to_string(),
        McpServerConfig::Http(McpHttpServerConfig {
            url: "https://example.com/mcp".to_string(),
            headers: HashMap::new(),
        }),
    );

    let manager = McpClientManager::new(configs);
    let statuses = manager.list_statuses();

    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].name, "fixture");
    assert_eq!(statuses[0].state, McpConnectionState::Pending);
    assert_eq!(statuses[0].transport, "stdio");
    assert!(statuses[0].tools.is_empty());
    assert!(statuses[0].resources.is_empty());

    assert_eq!(statuses[1].name, "remote");
    assert_eq!(statuses[1].state, McpConnectionState::Pending);
    assert_eq!(statuses[1].transport, "http");
    assert!(statuses[1].tools.is_empty());
    assert!(statuses[1].resources.is_empty());
}

#[tokio::test]
async fn stdio_manager_connects_and_discovers_tools_and_resources() {
    let mut configs = HashMap::new();
    configs.insert(
        "fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_server"),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].state, McpConnectionState::Connected);
    assert_eq!(statuses[0].tools.len(), 1);
    assert_eq!(statuses[0].tools[0].name, "hello");
    assert_eq!(statuses[0].resources.len(), 1);
    assert_eq!(statuses[0].resources[0].uri, "fixture://readme");

    assert_eq!(manager.list_tools().len(), 1);
    assert_eq!(manager.list_resources().len(), 1);

    manager.close().await;
    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Pending);
    assert!(statuses[0].tools.is_empty());
    assert!(statuses[0].resources.is_empty());
}

#[tokio::test]
async fn stdio_manager_tolerates_missing_resource_listing() {
    let mut configs = HashMap::new();
    configs.insert(
        "fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_tools_only_server"),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Connected);
    assert_eq!(statuses[0].tools.len(), 1);
    assert_eq!(statuses[0].tools[0].description, "");
    assert!(statuses[0].resources.is_empty());
}

#[tokio::test]
async fn stdio_manager_fails_on_non_method_not_found_resource_error() {
    let mut configs = HashMap::new();
    configs.insert(
        "broken-resources".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_broken_resources_server"),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Failed);
    assert_eq!(statuses[0].transport, "stdio");
    assert!(statuses[0].detail.contains("fixture resources unavailable"));
}

#[tokio::test]
async fn stdio_manager_marks_failed_process_start() {
    let mut configs = HashMap::new();
    configs.insert(
        "broken".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: "__missing_lattice_mcp_fixture__".to_string(),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Failed);
    assert!(statuses[0].detail.contains("missing") || !statuses[0].detail.is_empty());
}

#[tokio::test]
async fn reconnect_all_reestablishes_sessions() {
    let mut configs = HashMap::new();
    configs.insert(
        "fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_server"),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;
    manager.reconnect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Connected);
    assert_eq!(statuses[0].tools[0].name, "hello");
}

#[tokio::test]
async fn stdio_manager_honors_cwd_and_env_configuration() {
    let cwd = std::env::current_dir().expect("workspace cwd should exist");
    let mut env = HashMap::new();
    env.insert("LATTICE_MCP_FIXTURE".to_string(), "1".to_string());

    let mut configs = HashMap::new();
    configs.insert(
        "fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_server"),
            args: vec![],
            env: Some(env),
            cwd: Some(cwd),
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Connected);
    assert_eq!(statuses[0].resources.len(), 1);
    assert_eq!(statuses[0].resources[0].description, "");
}

#[tokio::test]
async fn reconnect_all_recovers_failed_stdio_session() {
    let mut configs = HashMap::new();
    configs.insert(
        "fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: "__missing_lattice_mcp_fixture__".to_string(),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;
    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Failed);

    let mut recovered_configs = HashMap::new();
    recovered_configs.insert(
        "fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_server"),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );

    let mut recovered = McpClientManager::new(recovered_configs);
    recovered.reconnect_all().await;

    let statuses = recovered.list_statuses();
    assert_eq!(statuses[0].state, McpConnectionState::Connected);
    assert_eq!(statuses[0].tools.len(), 1);
    assert_eq!(statuses[0].resources.len(), 1);
}

#[tokio::test]
async fn connect_all_marks_unsupported_http_and_ws_transports_failed() {
    let mut configs = HashMap::new();
    configs.insert(
        "http-remote".to_string(),
        McpServerConfig::Http(McpHttpServerConfig {
            url: "https://example.com/mcp".to_string(),
            headers: HashMap::new(),
        }),
    );
    configs.insert(
        "ws-remote".to_string(),
        McpServerConfig::Ws(McpWebSocketServerConfig {
            url: "wss://example.com/mcp".to_string(),
            headers: HashMap::new(),
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].name, "http-remote");
    assert_eq!(statuses[0].state, McpConnectionState::Failed);
    assert_eq!(statuses[0].transport, "http");
    assert!(statuses[0].detail.contains("unsupported MCP transport"));
    assert!(statuses[0].tools.is_empty());
    assert!(statuses[0].resources.is_empty());

    assert_eq!(statuses[1].name, "ws-remote");
    assert_eq!(statuses[1].state, McpConnectionState::Failed);
    assert_eq!(statuses[1].transport, "ws");
    assert!(statuses[1].detail.contains("unsupported MCP transport"));
    assert!(statuses[1].tools.is_empty());
    assert!(statuses[1].resources.is_empty());
}

#[tokio::test]
async fn list_statuses_tools_and_resources_are_sorted_and_aggregated() {
    let mut configs = HashMap::new();
    configs.insert(
        "z-fixture".to_string(),
        McpServerConfig::Stdio(McpStdioServerConfig {
            command: fixture_binary("fixture_mcp_server"),
            args: vec![],
            env: None,
            cwd: None,
        }),
    );
    configs.insert(
        "a-http".to_string(),
        McpServerConfig::Http(McpHttpServerConfig {
            url: "https://example.com/mcp".to_string(),
            headers: HashMap::new(),
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let statuses = manager.list_statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].name, "a-http");
    assert_eq!(statuses[0].state, McpConnectionState::Failed);
    assert_eq!(statuses[1].name, "z-fixture");
    assert_eq!(statuses[1].state, McpConnectionState::Connected);

    let tools = manager.list_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].server_name, "z-fixture");
    assert_eq!(tools[0].name, "hello");

    let resources = manager.list_resources();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].server_name, "z-fixture");
    assert_eq!(resources[0].uri, "fixture://readme");

    manager.close().await;
    let statuses = manager.list_statuses();
    assert_eq!(statuses[0].name, "a-http");
    assert_eq!(statuses[0].state, McpConnectionState::Pending);
    assert!(statuses[0].tools.is_empty());
    assert!(statuses[0].resources.is_empty());
    assert_eq!(statuses[1].name, "z-fixture");
    assert_eq!(statuses[1].state, McpConnectionState::Pending);
    assert!(statuses[1].tools.is_empty());
    assert!(statuses[1].resources.is_empty());
}

#[tokio::test]
async fn direct_client_can_read_fixture_resource() {
    let client = connect_fixture_client(fixture_binary("fixture_mcp_server"), None, None).await;

    let resources = client
        .list_all_resources()
        .await
        .expect("resource listing should succeed");
    assert_eq!(resources.len(), 1);

    let read = client
        .read_resource(ReadResourceRequestParams::new("fixture://readme"))
        .await
        .expect("resource read should succeed");
    assert_eq!(read.contents.len(), 1);
}
