use std::collections::HashMap;
use std::path::PathBuf;

use lattice_mcp::{McpClientManager, McpConnectionState, McpServerConfig, McpStdioServerConfig};

fn fixture_binary(name: &str) -> String {
    let key = format!("CARGO_BIN_EXE_{name}");
    let path = std::env::var_os(&key).unwrap_or_else(|| panic!("missing fixture binary: {key}"));
    PathBuf::from(path).to_string_lossy().into_owned()
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
    assert!(statuses[0].resources.is_empty());
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
