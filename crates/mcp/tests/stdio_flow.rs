use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use lattice_core::ToolExecutor;
use lattice_mcp::{
    load_mcp_manager_from_env, load_mcp_server_configs_from_path, mcp_tool_name,
    register_mcp_tools, ListMcpResourcesTool, McpClientManager, McpConnectionState,
    McpHttpServerConfig, McpServerConfig, McpStdioServerConfig, McpToolAdapter,
    McpWebSocketServerConfig, ReadMcpResourceTool, LATTICE_MCP_CONFIG_ENV,
};
use lattice_tools::ToolSet;
use rmcp::{
    model::ReadResourceRequestParams, service::RoleClient, transport::TokioChildProcess, ServiceExt,
};
use tokio::process::Command;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

fn write_temp_mcp_config(command: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("lattice-mcp-{unique}.json"));
    let json = serde_json::json!({
        "mcpServers": {
            "fixture": {
                "type": "stdio",
                "command": command,
                "args": []
            }
        }
    });
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    path
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
            bearer_token: None,
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
    assert_eq!(statuses[0].tools.len(), 2);
    assert!(statuses[0].tools.iter().any(|tool| tool.name == "hello"));
    assert!(statuses[0].tools.iter().any(|tool| tool.name == "fail"));
    assert_eq!(statuses[0].resources.len(), 1);
    assert_eq!(statuses[0].resources[0].uri, "fixture://readme");

    assert_eq!(manager.list_tools().len(), 2);
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
    assert!(statuses[0].tools.iter().any(|tool| tool.name == "hello"));
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
    assert_eq!(statuses[0].tools.len(), 2);
    assert_eq!(statuses[0].resources.len(), 1);
}

#[tokio::test]
async fn connect_all_marks_unreachable_http_and_ws_transports_failed() {
    let mut configs = HashMap::new();
    configs.insert(
        "http-remote".to_string(),
        McpServerConfig::Http(McpHttpServerConfig {
            url: "https://example.com/mcp".to_string(),
            bearer_token: None,
            headers: HashMap::new(),
        }),
    );
    configs.insert(
        "ws-remote".to_string(),
        McpServerConfig::Ws(McpWebSocketServerConfig {
            url: "wss://example.com/mcp".to_string(),
            bearer_token: None,
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
    assert!(!statuses[0].detail.is_empty());
    assert!(statuses[0].tools.is_empty());
    assert!(statuses[0].resources.is_empty());

    assert_eq!(statuses[1].name, "ws-remote");
    assert_eq!(statuses[1].state, McpConnectionState::Failed);
    assert_eq!(statuses[1].transport, "ws");
    assert!(!statuses[1].detail.is_empty());
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
            bearer_token: None,
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
    assert_eq!(tools.len(), 2);
    assert!(tools
        .iter()
        .any(|tool| { tool.server_name == "z-fixture" && tool.name == "hello" }));
    assert!(tools
        .iter()
        .any(|tool| { tool.server_name == "z-fixture" && tool.name == "fail" }));

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

#[tokio::test]
async fn manager_can_call_mcp_tool() {
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

    let output = manager
        .call_tool("fixture", "hello", serde_json::json!({ "name": "lattice" }))
        .await
        .unwrap();
    assert_eq!(output.output, "fixture-hello:lattice");
    assert_eq!(output.structured_content, None);
}

#[test]
fn load_mcp_server_configs_from_path_parses_json_file() {
    let path = write_temp_mcp_config("fixture-command");
    let configs = load_mcp_server_configs_from_path(&path).unwrap();
    std::fs::remove_file(path).ok();

    let Some(McpServerConfig::Stdio(config)) = configs.get("fixture") else {
        panic!("expected stdio fixture config");
    };
    assert_eq!(config.command, "fixture-command");
}

#[tokio::test]
async fn load_mcp_manager_from_env_and_register_mcp_tools() {
    let _guard = ENV_LOCK.lock().await;
    let path = write_temp_mcp_config(&fixture_binary("fixture_mcp_server"));
    unsafe {
        std::env::set_var(LATTICE_MCP_CONFIG_ENV, &path);
    }

    let manager = load_mcp_manager_from_env()
        .await
        .unwrap()
        .expect("manager should load from env config");
    let snapshots = manager.list_status_snapshots();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].state, McpConnectionState::Connected);

    let mut toolset = ToolSet::new();
    register_mcp_tools(&mut toolset, manager).unwrap();
    assert!(toolset.contains("list_mcp_resources"));
    assert!(toolset.contains("read_mcp_resource"));
    assert!(toolset.contains(&mcp_tool_name("fixture", "hello")));

    unsafe {
        std::env::remove_var(LATTICE_MCP_CONFIG_ENV);
    }
    std::fs::remove_file(path).ok();
}

#[tokio::test]
async fn manager_can_read_mcp_resource() {
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

    let output = manager
        .read_resource("fixture", "fixture://readme")
        .await
        .unwrap();
    assert_eq!(output, "fixture resource contents");
}

#[tokio::test]
async fn manager_reports_missing_mcp_resource() {
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

    let err = manager
        .read_resource("fixture", "fixture://missing")
        .await
        .unwrap_err();
    assert!(matches!(err, lattice_core::ToolError::NotFound(_)));
    assert!(err.to_string().contains("fixture://missing"));
}

#[tokio::test]
async fn manager_connection_snapshots_include_counts() {
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
        "http-remote".to_string(),
        McpServerConfig::Http(McpHttpServerConfig {
            url: "https://example.com/mcp".to_string(),
            bearer_token: None,
            headers: HashMap::new(),
        }),
    );

    let mut manager = McpClientManager::new(configs);
    manager.connect_all().await;

    let snapshots = manager.list_status_snapshots();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].name, "fixture");
    assert_eq!(snapshots[0].tool_count, 2);
    assert_eq!(snapshots[0].resource_count, 1);
    assert_eq!(snapshots[1].name, "http-remote");
    assert_eq!(snapshots[1].tool_count, 0);
    assert_eq!(snapshots[1].resource_count, 0);
    assert_eq!(snapshots[1].state, McpConnectionState::Failed);
}

#[tokio::test]
async fn manager_rejects_non_object_tool_arguments() {
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

    let err = manager
        .call_tool("fixture", "hello", serde_json::json!(["bad"]))
        .await
        .unwrap_err();
    assert!(matches!(err, lattice_core::ToolError::InvalidParams(_)));
    assert!(err.to_string().contains("JSON object"));
}

#[tokio::test]
async fn manager_surfaces_remote_tool_error() {
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

    let err = manager
        .call_tool("fixture", "fail", serde_json::json!({ "reason": "boom" }))
        .await
        .unwrap_err();
    assert!(matches!(err, lattice_core::ToolError::ExecutionFailed(_)));
    assert!(err.to_string().contains("fixture-fail:boom"));
}

#[tokio::test]
async fn mcp_tool_adapter_registers_into_toolset_and_executes() {
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
    let manager = Arc::new(manager);

    let mut set = ToolSet::new();
    for tool in McpToolAdapter::from_manager(manager) {
        if tool.description().name.ends_with("__hello") {
            set.register(tool).unwrap();
            break;
        }
    }

    let result = set
        .execute(
            &mcp_tool_name("fixture", "hello"),
            serde_json::json!({ "name": "bridge" }),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "fixture-hello:bridge");
}

#[tokio::test]
async fn mcp_resource_tools_register_into_toolset_and_execute() {
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
    let manager = Arc::new(manager);

    let mut set = ToolSet::new();
    set.register(ListMcpResourcesTool::new(manager.clone()))
        .unwrap();
    set.register(ReadMcpResourceTool::new(manager.clone()))
        .unwrap();

    let listed = set
        .execute("list_mcp_resources", serde_json::json!({}))
        .await
        .unwrap();
    assert!(listed.stdout.contains("fixture fixture://readme"));

    let read = set
        .execute(
            "read_mcp_resource",
            serde_json::json!({
                "server": "fixture",
                "uri": "fixture://readme"
            }),
        )
        .await
        .unwrap();
    assert_eq!(read.stdout, "fixture resource contents");
}

#[tokio::test]
async fn list_mcp_resources_handles_empty_servers() {
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
    let tool = ListMcpResourcesTool::new(Arc::new(manager));

    let result = tool.execute(serde_json::json!({})).await.unwrap();
    assert_eq!(result.stdout, "(no MCP resources)");
}

#[tokio::test]
async fn list_mcp_resources_rejects_non_object_params() {
    let manager = Arc::new(McpClientManager::new(HashMap::new()));
    let tool = ListMcpResourcesTool::new(manager);

    let err = tool.execute(serde_json::json!(["bad"])).await.unwrap_err();
    assert!(matches!(err, lattice_core::ToolError::InvalidParams(_)));
}

#[tokio::test]
async fn read_mcp_resource_rejects_invalid_params() {
    let manager = Arc::new(McpClientManager::new(HashMap::new()));
    let tool = ReadMcpResourceTool::new(manager);

    let err = tool
        .execute(serde_json::json!({ "server": 1, "uri": true }))
        .await
        .unwrap_err();
    assert!(matches!(err, lattice_core::ToolError::InvalidParams(_)));
    assert!(err
        .to_string()
        .contains("invalid read_mcp_resource arguments"));
}
