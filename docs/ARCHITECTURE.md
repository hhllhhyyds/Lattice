# 架构设计

## 设计哲学

Lattice 的架构灵感来自 Anthropic 的 Managed Agents 博客（*Scaling Managed Agents: Decoupling the brain from the hands*），以及操作系统的虚拟化思想：

- **借鉴 OS 的抽象层**：操作系统通过 `read()` 等稳定接口屏蔽底层硬件差异。Lattice 对 Agent 运行环境做同样的事——定义稳定接口，让实现可自由替换。
- **苦涩的教训**：不在框架中嵌入针对特定模型能力的"补丁"。模型特定的适配只能以可插拔的方式挂载。
- **宠物 vs 牲口**：所有组件（会话、控制循环、沙箱）都是无状态/可恢复的，坏了就换，不需要"抢救"。
- **Feature 组合，按需裁剪**：所有非核心功能（LLM 后端、沙箱实现、存储实现、HTTP 服务）通过 Rust feature flags 控制。消费者可以只编译自己需要的部分，不引入多余依赖。core crate 保持零 feature，永远是纯接口。

## 三个核心抽象

```
┌──────────────────────────────────────────────┐
│              ControlLoop (大脑)                │
│  依赖两个 trait:                               │
│  - SessionStore                               │
│  - LLMClient                                  │
│  持有 ToolSet（工具注册表）                     │
└──────┬───────────────┬───────────────┬─────────┘
       │               │               │
  ┌────▼─────┐   ┌─────▼─────┐  ┌─────▼───────────┐
  │ Session  │   │ LLMClient │  │   ToolSet       │
  │  Store   │   │           │  │ (注册表 + 执行) │
  │ (trait)  │   │ (trait)   │  │ (可组合实现)   │
  └──────────┘   └───────────┘  └───────┬─────────┘
                                        │ ToolExecutor trait
                                  ┌─────▼─────────┐
                                  │ BashTool      │
                                  │ (或任意工具)  │
                                  │  └─ Sandbox  │
                                  └──────────────┘
```

### 工具三层体系

Lattice 的工具系统分为三层：

1. **Layer 1（core）**：定义 `ToolExecutor` trait——所有工具的统一接口
2. **Layer 2（lattice-tools）**：`ToolSet` 注册表 + 标准工具实现（BashTool）
3. **Layer 3（应用层）**：注入自定义工具实现

ControlLoop 通过 `ToolSet` 统一调用工具，不区分工具背后是沙箱执行还是进程内执行。

### 1. Session（会话）—— 不可变事件日志

**定义**：Agent 运行期间产生的所有事件的不可变、仅追加、持久化序列。

**不是什么**：
- 不是 LLM 的对话历史
- 不是上下文窗口
- 不可修改、不可删除

**事件结构**：

```rust
/// 事件唯一标识
pub type EventId = Uuid;

/// 事件时间戳
pub type Timestamp = chrono::DateTime<chrono::Utc>;

/// 产生事件的角色
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Actor {
    System,
    LLM,
    Harness,
    Sandbox,
}

/// 事件负载
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventPayload {
    /// 会话创建
    SessionCreated,

    /// 用户输入任务
    UserMessage { content: String },

    /// LLM 的思考过程
    Thinking { reasoning: String },

    /// LLM 决定调用工具
    ToolCallRequested {
        tool: String,
        params: serde_json::Value,
    },

    /// 工具调用结果
    ToolCallResult {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },

    /// 工具调用失败
    ToolCallError { error: String },

    /// LLM 给出最终答案
    FinalAnswer { answer: String },

    /// 会话状态变更
    StateChange { from: String, to: String },
}

/// 一个不可变事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub timestamp: Timestamp,
    pub actor: Actor,
    pub payload: EventPayload,
    pub parent_event_id: Option<EventId>,
}
```

**SessionStore trait**：

```rust
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// 创建一个新会话，返回 session_id
    async fn create_session(&self) -> Result<SessionId, StoreError>;

    /// 追加事件（仅追加，不可修改）
    async fn append_event(
        &self,
        session_id: SessionId,
        payload: EventPayload,
        actor: Actor,
        parent_event_id: Option<EventId>,
    ) -> Result<EventId, StoreError>;

    /// 查询事件
    async fn get_events(
        &self,
        session_id: SessionId,
        filter: &EventFilter,
    ) -> Result<Vec<Event>, StoreError>;

    /// 获取会话的最新事件序号（用于恢复断点）
    async fn latest_event_id(
        &self,
        session_id: SessionId,
    ) -> Result<Option<EventId>, StoreError>;
}
```

### 2. ControlLoop（控制循环）—— Agent 的大脑

**定义**：唯一有权调用 LLM 的组件。负责加载事件历史、构建提示、解析 LLM 决策、路由工具调用。

**关键特性**：
- **无状态**：不持有持久状态，所有状态从 SessionStore 恢复
- **可崩溃恢复**：进程崩溃后，用同一个 session_id 重新创建 ControlLoop，从事件日志断点继续
- **工具委托**：通过 ToolSet 执行工具调用，记录结果或错误事件

**决策循环**：

```
loop {
    1. 从 SessionStore 加载事件历史
    2. 从 ToolSet 获取可用工具描述，构建 LLM 提示
    3. 调用 LLMClient.decide()
    4. 记录决策事件
    5. match 决策:
       - FinalAnswer → 记录结果，退出循环
       - Thinking → 继续循环
       - ToolCall → 通过 ToolSet.execute(tool, params) 执行，记录结果/错误，继续循环
}
```

**LLMClient trait**：

```rust
#[async_trait]
pub trait LLMClient: Send + Sync {
    /// 基于事件历史和可用工具做出决策
    async fn decide(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> Result<Decision, LLMError>;
}

/// LLM 的决策
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Decision {
    Thinking { reasoning: String },
    ToolCall { tool: String, params: serde_json::Value },
    FinalAnswer { answer: String },
}
```

### 3. Sandbox（沙箱）—— Agent 的双手

**定义**：隔离的工具执行环境，按需创建，可替换。

**关键特性**：
- **隔离**：沙箱内的代码无法访问框架内部或凭据
- **可替换**：崩溃了换一个新的，ControlLoop 视之为一次工具调用失败
- **按需启动**：只在 LLM 真正需要执行工具时才创建

**Sandbox trait**：

```rust
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// 执行命令
    async fn execute(
        &self,
        command: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, SandboxError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

**ToolExecutor trait**：

```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Return the tool description for LLM consumption.
    fn description(&self) -> ToolDescription;

    /// Execute the tool with the given parameters.
    async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError>;
}
```

**ToolSet**：

```rust
pub struct ToolSet { ... }
impl ToolSet {
    pub fn register(&mut self, tool: impl ToolExecutor + 'static) -> Result<(), ToolError> { ... }
    pub fn descriptions(&self) -> Vec<ToolDescription> { ... }
    pub async fn execute(&self, name: &str, params: serde_json::Value) -> Result<ExecutionResult, ToolError> { ... }
}
```

## 数据流与生命周期

完整的一次 Agent 任务执行：

```
1. [客户端] 创建 Session
   → SessionStore.create_session()
   → 记录 SessionCreated 事件

2. [客户端] 提交用户任务
   → SessionStore.append_event(UserMessage)

3. [系统] 创建 ControlLoop，绑定 session_id
   → ControlLoop.run() 开始

4. [ControlLoop] 加载事件历史
   → SessionStore.get_events()

5. [ControlLoop] 从 ToolSet 获取工具描述，调用 LLM
   → LLMClient.decide(history, tools)
   → 记录 DecisionRecorded 事件

6. [LLM 决定调用工具]
   → 记录 ToolCallRequested 事件
   → ToolSet.execute(tool, params)

7. [ToolExecutor] 执行工具（如 BashTool → Sandbox.execute）
   → 记录 ToolCallResult 或 ToolCallError 事件

8. [ControlLoop] 看到结果，回到步骤 4

9. [LLM 给出最终答案]
   → 记录 FinalAnswer 事件
   → ControlLoop.run() 返回

10. [客户端] 查询结果
    → SessionStore.get_events(filter: FinalAnswer)
```

## 崩溃恢复

```
ControlLoop 崩溃
  → 系统检测到
  → 用同一个 session_id 创建新的 ControlLoop
  → ControlLoop.run() 调用 SessionStore.get_events()
  → 发现最后一个事件是 ToolCallRequested（没有对应的 Result）
  → LLM 看到未完成的调用，决定重试或跳过
  → 继续执行
```

## 安全边界

- **凭据隔离**：Sandbox 实现不能直接访问 SessionStore 或 LLM 凭据
- **初始化注入**：敏感信息只在沙箱创建阶段注入（如环境变量），运行时代码无法获取注入动作本身
- **Vault Proxy**（未来）：第三方令牌通过外部代理注入，沙箱永远接触不到原始凭据

## Feature Flag 设计

### 设计原则

1. **core 零 feature**：`lattice-core` 是纯接口层，不包含任何可选功能，所有消费者必须依赖它
2. **实现 crate 独立成包**：每个实现（LLM 后端、存储后端、沙箱实现）是独立 crate，消费者通过 `Cargo.toml` 按需引入
3. **Facade crate 提供便利**：`lattice` facade crate 通过 feature flags 重导出所有子 crate，方便"全家桶"使用
4. **server 按需编译**：`lattice-server` 通过 feature 控制启用哪些 provider、存储和沙箱后端
5. **默认最小化**：default feature 只包含最基础的功能，用户明确 opt-in 额外能力

### Facade Crate：`lattice`

```toml
[package]
name = "lattice"

[features]
default = ["runtime", "store-memory", "sandbox-local"]

# 核心组件（总是可用，通过 lattice-core）
runtime = ["dep:lattice-runtime"]

# 存储后端
store-memory = ["dep:lattice-store-memory"]
# store-sqlite = ["dep:lattice-store-sqlite"]   # 未来
# store-postgres = ["dep:lattice-store-postgres"] # 未来

# 沙箱实现
sandbox-local = ["dep:lattice-sandbox-local"]
# sandbox-docker = ["dep:lattice-sandbox-docker"] # 未来

# LLM 后端
llm-protocol = ["dep:lattice-llm-protocol"]
llm-anthropic = ["llm-protocol", "dep:lattice-llm-anthropic"]
llm-openai = ["llm-protocol", "dep:lattice-llm-openai"]
llm-all = ["llm-anthropic", "llm-openai"]

# 便利组合
full = ["runtime", "store-memory", "sandbox-local", "llm-all"]

[dependencies]
lattice-core = { path = "crates/core" }           # 始终依赖
lattice-runtime = { path = "crates/runtime", optional = true }
lattice-store-memory = { path = "crates/store-memory", optional = true }
lattice-sandbox-local = { path = "crates/sandbox-local", optional = true }
lattice-llm-protocol = { path = "crates/llm-protocol", optional = true }
lattice-llm-anthropic = { path = "crates/llm-anthropic", optional = true }
lattice-llm-openai = { path = "crates/llm-openai", optional = true }
```

`crates/server` 是独立的 `lattice-server` crate，有自己的 feature flags（default=[anthropic, openai]），受 workspace 管理。

### 消费者使用示例

```toml
# 只要核心 + Anthropic
lattice = { version = "0.1", default-features = false, features = ["runtime", "store-memory", "sandbox-local", "llm-anthropic"] }

# 全家桶
lattice = { version = "0.1", features = ["full"] }

# 极简：只要接口定义（写自己的实现）
lattice = { version = "0.1", default-features = false }
```

### Server Feature Flags

```toml
[package]
name = "lattice-server"

[features]
default = ["anthropic", "openai"]

# LLM Provider
anthropic = ["lattice-llm-anthropic"]
openai = ["lattice-llm-openai"]

# 存储后端（未来）
# sqlite = ["lattice-store-sqlite"]

# 沙箱后端（未来）
# docker = ["lattice-sandbox-docker"]
```

### 代码中的条件编译

在 server 中按 feature 注册 provider：

```rust
pub fn register_providers(registry: &mut ProviderRegistry, config: &[ProviderConfig]) {
    for provider in config {
        match provider.kind {
            #[cfg(feature = "anthropic")]
            ProviderKind::Anthropic => { /* register */ },

            #[cfg(feature = "openai")]
            ProviderKind::OpenAI => { /* register */ },

            #[allow(unreachable_patterns)]
            _ => tracing::warn!("Provider {:?} not enabled at compile time", provider.kind),
        }
    }
}
```

## 工具系统

### 设计背景

参考 Anthropic Managed Agents 的设计，工具（tool）的核心接口是 `execute(name, input) → output`。Brain（ControlLoop）不关心工具背后是一个沙箱容器、一个进程内函数、还是一个远程 MCP 服务器——只关心 name + input 进去，output 出来。

Lattice 在此基础上做了一个重要的分层：将「工具描述」（给 LLM 看的元数据）与「工具执行」（实际运行逻辑）分离。一个 Sandbox 可以支持多个工具（如 LocalSandbox 同时支持 bash 和 python），一个工具也可以不需要 Sandbox（如 file_read 在进程内直接执行）。

### 三层工具体系

```
┌─────────────────────────────────────────┐
│  Layer 3: Harness-Provided Tools        │  ← 应用层注入
│  (MCP servers, custom business tools)   │
├─────────────────────────────────────────┤
│  Layer 2: Standard Tool Library         │  ← 框架提供，可选启用
│  (bash, file_read, file_write, glob,    │
│   grep, http...)                        │
├─────────────────────────────────────────┤
│  Layer 1: Tool Infrastructure           │  ← core 层，纯接口
│  (ToolDescription, ToolExecutor trait,  │
│   ToolSet)                              │
└─────────────────────────────────────────┘
```

**Layer 1：Tool Infrastructure（`lattice-core`）**

纯接口定义，零实现。包括已有的 `ToolDescription` 和新增的 `ToolExecutor` trait。

**Layer 2：Standard Tool Library（`lattice-tools`）**

框架自带的常用工具实现。每个工具是独立的 struct，实现 `ToolExecutor`。通过 feature flags 按需编译，默认只包含 bash。

**Layer 3：Harness-Provided Tools（应用层）**

框架消费者自行注册的工具——MCP 桥接、业务专用工具等。通过相同的 `ToolExecutor` trait 和 `ToolSet::add()` 接口注入，与内置工具同等公民。

### 核心接口

#### ToolExecutor trait（`lattice-core`）

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

#### ToolSet（`lattice-tools`）

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
    pub fn new() -> Self { ... }

    /// Build a ToolSet with all default tools enabled by feature flags.
    /// Currently includes BashTool (if the `bash` feature is enabled).
    #[cfg(feature = "bash")]
    pub fn with_defaults(sandbox: Arc<dyn Sandbox>) -> Self {
        let mut set = Self::new();
        set.register(BashTool::new(sandbox)).unwrap();
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
}
```

### 两类工具执行模式

工具按执行方式分为两类，但对 ControlLoop 来说完全透明：

**沙箱工具（Sandbox-backed）**：需要隔离执行环境的工具。`ToolExecutor::execute()` 内部持有 `Arc<dyn Sandbox>` 引用，委托给沙箱执行。

```rust
/// Bash tool — delegates to a Sandbox for isolated execution.
pub struct BashTool {
    sandbox: Arc<dyn Sandbox>,
}

#[async_trait]
impl ToolExecutor for BashTool {
    fn description(&self) -> ToolDescription { /* bash tool schema */ }

    async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError> {
        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams(
                "missing or invalid 'command' field (expected string)".to_string(),
            ))?;
        self.sandbox
            .execute(command, params.clone())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }
}
```

**进程内工具（In-process）**：不需要沙箱的轻量工具，直接在框架进程内执行。

```rust
/// File read tool — reads files in-process, no sandbox needed.
pub struct FileReadTool {
    allowed_paths: Vec<PathBuf>,
}

#[async_trait]
impl ToolExecutor for FileReadTool {
    fn description(&self) -> ToolDescription { /* file_read tool schema */ }

    async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams("missing 'path' field".to_string()))?;
        let content = tokio::fs::read_to_string(path).await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        Ok(ExecutionResult {
            stdout: content,
            stderr: String::new(),
            exit_code: 0,
        })
    }
}
```

> **注意**：FileReadTool 及其他进程内工具（glob、grep、http）尚未实现，此处为设计示例。目前已实现的标准工具仅有 BashTool。

### ToolSet 与 SandboxRouter 的关系

`ToolSet` 统一了工具描述和工具执行两个职责，取代了之前的 `Vec<ToolDescription>` + `SandboxRouter` 分离模式。

`SandboxRouter` trait 和 `BasicSandboxRouter` 已被移除。ToolSet 完全替代了它们的职责——工具路由逻辑内化到 ToolSet 中，沙箱工具通过 `ToolExecutor` 实现内部持有 `Arc<dyn Sandbox>` 来委托执行。ControlLoop 改为持有 `Arc<ToolSet>`：

```rust
// Before:
let control_loop = ControlLoop::with_options(store, llm, router, vec![], prompt, max_iter);

// After:
let tools = Arc::new(ToolSet::with_defaults(sandbox));
let control_loop = ControlLoop::new(store, llm, tools);

// Register custom tools:
let mut tools = ToolSet::with_defaults(sandbox);
tools.register(MyCustomTool::new()).unwrap();
let control_loop = ControlLoop::new(store, llm, Arc::new(tools));
```

### 测试覆盖率

使用 `cargo-llvm-cov` 检测代码覆盖率。覆盖率指标按 crate 维度统计（line coverage、branch coverage），作为 CI 的一部分运行，不阻断合并。

- **工具**：`cargo-llvm-cov` + `cargo-llvm-cov-action`
- **CI 上传**：通过 Codecov action 上传至 codecov.io
- **报告格式**：LCOV（`--lcov`），生成 lcov.info
- **原则**：新增代码应附带测试，保持覆盖率不下降

## Crate 结构

```
crates/
├── core/           # SessionStore, LLMClient, Sandbox, ToolExecutor traits + core types
├── tools/          # lattice-tools: 标准工具库
│   ├── set.rs      #   ToolSet 注册表
│   └── bash.rs     #   BashTool（沙箱工具）
├── runtime/        # ControlLoop（接收 Arc<ToolSet>）
├── store-memory/   # SessionStore 内存实现
├── sandbox-local/  # Sandbox 本地子进程实现
├── llm-protocol/   # LLM 通用协议层（消息格式转换、响应解析）
├── llm-anthropic/  # Anthropic Claude 后端
├── llm-openai/     # OpenAI 兼容后端
└── server/         # HTTP API 服务（axum）
```

#### Feature Flags

```toml
# crates/tools/Cargo.toml
[package]
name = "lattice-tools"

[features]
default = ["bash"]
bash = ["lattice-sandbox-local"]

[dependencies]
lattice-core = { path = "../core" }
lattice-sandbox-local = { path = "../sandbox-local", optional = true }
```

Facade crate 同步更新：

```toml
# 根 Cargo.toml (lattice facade)
[features]
default = ["runtime", "store-memory", "sandbox-local", "tools"]
tools = ["dep:lattice-tools"]
full = ["runtime", "store-memory", "sandbox-local", "llm-all", "tools"]
```

### 未来扩展

- **MCP 桥接工具**：实现 `ToolExecutor`，内部通过 MCP 协议调用外部 MCP 服务器。作为 Layer 3 工具由应用层注入。
- **沙箱工厂（SandboxFactory）**：当工具需要按需创建沙箱时（如 Docker 容器），引入 factory 模式，实现 Anthropic 提出的 `provision({resources})` 语义。
- **凭据隔离**：参考 Anthropic 的两种模式——资源绑定（token 注入沙箱初始化）和 Vault Proxy（工具通过代理访问凭据）。框架层面预留接口，不在工具实现中硬编码凭据处理。
- **工具权限控制**：参考 Claude Code 的 permission 模型，支持按工具名配置 allow/deny 规则。
