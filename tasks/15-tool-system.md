# 任务 15：工具系统 — ToolExecutor + ToolSet + 标准工具库

## 目标

引入三层工具体系，替换现有的 `SandboxRouter` + 硬编码工具模式。实现 `ToolExecutor` trait、`ToolSet` 统一路由、`BashTool` 标准工具，并改造 `ControlLoop` 使用新的工具系统。

## 分支

`feat/tool-system`

## 依赖

- 任务 3（core traits）
- 任务 5（LocalSandbox）
- 任务 6（ControlLoop）
- 任务 12（facade crate + feature flags）

## 背景

当前的工具处理链路：

```
硬编码 Vec<ToolDescription> → LLM 选工具 → SandboxRouter 无脑转发 → 单一 Sandbox 执行
```

问题：
1. 没有工具注册机制（调用方手动构建 `Vec<ToolDescription>`）
2. `BasicSandboxRouter` 忽略 tool name，所有调用丢给同一个 Sandbox
3. 无法支持进程内工具（file_read、http 等不需要沙箱的工具）
4. `SandboxRouter` trait 的 `route()` 接口耦合了事件记录职责（session_id, parent_event_id），混淆了工具执行和事件持久化

改造后的链路：

```
ToolSet（注册表） → LLM 选工具 → ToolSet.execute(name, params) → 路由到对应 ToolExecutor → 返回结果
```

## 具体内容

### 阶段 1：core 层 — 新增 ToolExecutor trait

**文件**：`crates/core/src/tool.rs`（已有 `ToolDescription`，在此扩展）

```rust
/// A tool that can be executed by the agent.
///
/// Implementations can be in-process (file read, HTTP fetch) or delegate
/// to a Sandbox (bash, python). The ControlLoop treats all tools identically
/// through this trait.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Return the tool description for LLM consumption.
    fn description(&self) -> ToolDescription;

    /// Execute the tool with the given parameters.
    async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError>;
}
```

**新增错误类型**：`crates/core/src/error.rs`

```rust
/// Error from a tool executor.
#[derive(Debug, Clone, Error)]
pub enum ToolError {
    /// Tool not found in the registry.
    #[error("tool not found: {0}")]
    NotFound(String),

    /// Invalid parameters provided to the tool.
    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    /// Tool execution failed.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Execution timed out.
    #[error("timeout after {timeout_secs}s")]
    Timeout { timeout_secs: u64 },

    /// Generic tool error.
    #[error("tool error: {0}")]
    Other(String),
}
```

**更新 `crates/core/src/lib.rs`**：
- `pub use tool::ToolExecutor;`
- `pub use error::ToolError;`

**删除**：
- `crates/core/src/router.rs`（`SandboxRouter` trait）
- `error.rs` 中的 `RouterError`
- `lib.rs` 中的 `pub mod router;` 和对应 re-exports

### 阶段 2：新建 lattice-tools crate

**目录结构**：

```
crates/tools/
├── Cargo.toml
└── src/
    ├── lib.rs          # ToolSet + re-exports
    ├── set.rs          # ToolSet 实现
    └── bash.rs         # BashTool 实现
```

#### ToolSet（`src/set.rs`）

```rust
/// A collection of tools available to the agent.
///
/// ToolSet serves two roles:
/// 1. Provide tool descriptions to the LLM (via `descriptions()`)
/// 2. Route tool calls to the correct executor (via `execute()`)
pub struct ToolSet {
    tools: HashMap<String, Box<dyn ToolExecutor>>,
}

impl ToolSet {
    /// Create an empty ToolSet.
    pub fn new() -> Self { ... }

    /// Build a ToolSet with all default tools enabled by compiled features.
    pub fn with_defaults(sandbox: Arc<dyn Sandbox>) -> Self {
        let mut set = Self::new();
        #[cfg(feature = "bash")]
        set.register(BashTool::new(sandbox));
        set
    }

    /// Register a tool. Returns error if a tool with the same name already exists.
    pub fn register(&mut self, tool: impl ToolExecutor + 'static) -> Result<(), ToolError> { ... }

    /// List all tool descriptions (passed to LLMClient::decide).
    pub fn descriptions(&self) -> Vec<ToolDescription> { ... }

    /// Look up and execute a tool by name.
    pub async fn execute(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, ToolError> { ... }

    /// Check if a tool is registered.
    pub fn contains(&self, name: &str) -> bool { ... }

    /// Number of registered tools.
    pub fn len(&self) -> usize { ... }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool { ... }
}
```

#### BashTool（`src/bash.rs`）

```rust
/// Bash tool — delegates command execution to a Sandbox.
///
/// Expects params: `{ "command": "ls -la" }`
pub struct BashTool {
    sandbox: Arc<dyn Sandbox>,
}

impl BashTool {
    pub fn new(sandbox: Arc<dyn Sandbox>) -> Self { ... }
}

#[async_trait]
impl ToolExecutor for BashTool {
    fn description(&self) -> ToolDescription {
        ToolDescription {
            name: "bash".to_string(),
            description: "Execute a bash command in a sandboxed environment. \
                Use this for running shell commands, scripts, and system operations.".to_string(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError> {
        let command = params.get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'command' field".into()))?;
        self.sandbox
            .execute("bash", serde_json::json!({ "command": command }))
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }
}
```

#### Cargo.toml

```toml
[package]
name = "lattice-tools"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
rust-version.workspace = true

[features]
default = ["bash"]
bash = []

[dependencies]
lattice-core = { path = "../core" }
lattice-sandbox-local = { path = "../sandbox-local" }
async-trait = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
```

### 阶段 3：改造 ControlLoop

**文件**：`crates/runtime/src/control_loop.rs`

改动要点：

1. **移除 `Arc<dyn SandboxRouter>` 依赖**，改为接收 `Arc<ToolSet>`（或 `&ToolSet`）
2. **移除 `available_tools: Vec<ToolDescription>`** 字段——ToolSet 自带 descriptions
3. **工具调用不再由 ControlLoop 记录事件**——事件记录职责仍在 ControlLoop（调用 store），但工具执行委托给 ToolSet
4. **新增 builder 模式**（可选，更好的 API 体验）

改造前后对比：

```rust
// Before:
pub struct ControlLoop {
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    router: Arc<dyn SandboxRouter>,          // ← 移除
    available_tools: Vec<ToolDescription>,    // ← 移除
    system_prompt: String,
    max_iterations: usize,
}

// After:
pub struct ControlLoop {
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    tools: Arc<ToolSet>,                     // ← 新增
    system_prompt: String,
    max_iterations: usize,
}
```

决策循环中工具调用的改造：

```rust
// Before:
Decision::ToolCall { tool, params } => {
    // ... record ToolCallRequested ...
    if let Err(e) = self.router.route(session_id, req_event_id, &tool, params).await {
        // record ToolCallError
    }
}

// After:
Decision::ToolCall { tool, params } => {
    // ... record ToolCallRequested ...
    match self.tools.execute(&tool, params).await {
        Ok(result) => {
            self.store.append_event(
                session_id,
                EventPayload::ToolCallResult {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                },
                Actor::Sandbox,
                Some(req_event_id),
            ).await?;
        }
        Err(e) => {
            self.store.append_event(
                session_id,
                EventPayload::ToolCallError { error: e.to_string() },
                Actor::Sandbox,
                Some(req_event_id),
            ).await?;
        }
    }
}
```

**注意**：事件记录职责从 `BasicSandboxRouter` 回收到 `ControlLoop`。这更合理——ControlLoop 是唯一写事件的组件，工具只负责执行。

### 阶段 4：删除 SandboxRouter

**删除文件**：
- `crates/core/src/router.rs`
- `crates/runtime/src/router.rs`（`BasicSandboxRouter`）

**更新文件**：
- `crates/core/src/lib.rs`：移除 `pub mod router;`、`pub use router::SandboxRouter;`、`pub use error::RouterError;`
- `crates/core/src/error.rs`：移除 `RouterError`
- `crates/runtime/src/lib.rs`：移除 `pub use router::BasicSandboxRouter;`、`mod router;`
- `docs/ARCHITECTURE.md`：更新「三个核心抽象」章节，将 SandboxRouter 从三组件图中移除，替换为 ToolSet 的描述。更新数据流章节中引用 SandboxRouter 的步骤。

### 阶段 5：更新 examples

#### hello-agent

```rust
// Before:
use lattice::runtime::{BasicSandboxRouter, ControlLoop};
let router = Arc::new(BasicSandboxRouter::new(sandbox, store.clone()));
let control_loop = ControlLoop::new(store, llm, router);

// After:
use lattice::tools::ToolSet;
use lattice::runtime::ControlLoop;
let tools = Arc::new(ToolSet::with_defaults(sandbox));
let control_loop = ControlLoop::new(store, llm, tools);
```

#### real-agent

```rust
// Before:
let tools = vec![ToolDescription { name: "bash".to_string(), ... }];
let router = Arc::new(BasicSandboxRouter::new(sandbox, store.clone()));
let control_loop = ControlLoop::with_options(store, llm, router, tools, prompt, 50);

// After:
let tools = Arc::new(ToolSet::with_defaults(sandbox));
let control_loop = ControlLoop::with_options(store, llm, tools, prompt, 50);
```

### 阶段 6：更新 Facade crate

**`src/lib.rs`（根目录）**：新增 `tools` 模块的 re-export。

**`Cargo.toml`（根目录）**：

```toml
[features]
default = ["runtime", "store-memory", "sandbox-local", "tools"]

tools = ["dep:lattice-tools"]
tools-full = ["tools", "lattice-tools/full"]
full = ["runtime", "store-memory", "sandbox-local", "llm-all", "tools-full"]

[dependencies]
lattice-tools = { path = "crates/tools", optional = true }
```

### 阶段 7：更新 server crate

`crates/server/src/lib.rs`：在 Task 16（Agent 执行 API）中会用到 `ToolSet::with_defaults()`。本次只需确保 `Cargo.toml` 添加对 `lattice-tools` 的依赖，不需要改 server 代码。

```toml
# crates/server/Cargo.toml 新增
lattice-tools = { path = "../tools" }
```

## 实现顺序

严格按阶段顺序执行，每个阶段完成后确保编译通过：

1. **阶段 1**：core 新增 ToolExecutor + ToolError（此时 SandboxRouter 仍存在，两者并存）
2. **阶段 2**：新建 lattice-tools crate（ToolSet + BashTool）
3. **阶段 3**：改造 ControlLoop（切换到 ToolSet，不再依赖 SandboxRouter）
4. **阶段 4**：删除 SandboxRouter + BasicSandboxRouter + RouterError
5. **阶段 5**：更新 examples
6. **阶段 6**：更新 facade crate
7. **阶段 7**：更新 server crate 依赖

**关键**：阶段 1-2 是纯增量（不破坏任何现有代码），阶段 3-4 是破坏性变更（同一个 commit 完成）。

## 测试要求

### lattice-tools 单元测试

```rust
// ToolSet 测试
#[test]
fn register_and_lookup() {
    // 注册 BashTool → descriptions 包含 bash → contains("bash") == true
}

#[test]
fn duplicate_name_returns_error() {
    // 注册两个同名工具 → 第二次返回错误
}

#[test]
fn execute_unknown_tool_returns_not_found() {
    // execute("nonexistent", {}) → ToolError::NotFound
}

#[tokio::test]
async fn bash_tool_executes_via_sandbox() {
    // 用 mock Sandbox 验证 BashTool 正确委托执行
}

#[tokio::test]
async fn bash_tool_invalid_params() {
    // 缺少 command 字段 → ToolError::InvalidParams
}
```

### ControlLoop 集成测试

- 现有的 4 个 ControlLoop 测试全部迁移到使用 ToolSet
- 测试 ToolSet 中工具不存在时 ControlLoop 记录 ToolCallError 事件

### 端到端测试

- hello-agent 和 real-agent 编译并运行正常

## 验收标准

- [x] `ToolExecutor` trait 定义在 `lattice-core`，有完整 doc comment
- [x] `ToolError` 替代 `RouterError`，覆盖所有错误场景
- [x] `ToolSet` 实现工具注册、描述列表、按名执行
- [x] `BashTool` 实现 `ToolExecutor`，正确委托 Sandbox 执行
- [x] `ControlLoop` 改为使用 `ToolSet`，不再依赖 `SandboxRouter`
- [x] 事件记录职责回收到 `ControlLoop`（不在工具执行层记录）
- [x] `SandboxRouter` trait、`BasicSandboxRouter`、`RouterError` 全部删除
- [x] `Sandbox` trait 保留不变（仍然是隔离执行环境的抽象）
- [x] 两个 example 更新并正常编译运行
- [x] Facade crate 新增 `tools` feature，`full` feature 包含 `tools`
- [x] 所有 pub 类型和方法有英文 doc comment
- [x] 四项检查全部通过：
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test --all-targets --all-features`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`
