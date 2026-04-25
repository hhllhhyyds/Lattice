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
│  依赖三个 trait:                               │
│  - SessionStore                               │
│  - LLMClient                                  │
│  - SandboxRouter                              │
└──────┬───────────────┬───────────────┬───────┘
       │               │               │
  ┌────▼─────┐   ┌─────▼─────┐  ┌─────▼───────┐
  │ Session  │   │ LLMClient │  │ SandboxRouter│
  │  Store   │   │           │  │              │
  │ (trait)  │   │ (trait)   │  │ (trait)      │
  └──────────┘   └───────────┘  └──────────────┘
```

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
- **不执行工具**：只负责路由，通过 SandboxRouter 将调用委托出去

**决策循环**：

```
loop {
    1. 从 SessionStore 加载事件历史
    2. 构建 LLM 提示（历史 + 可用工具 + 系统指令）
    3. 调用 LLMClient.decide()
    4. 记录决策事件
    5. match 决策:
       - FinalAnswer → 记录结果，退出循环
       - Thinking → 继续循环
       - ToolCall → 通过 SandboxRouter 路由执行，继续循环
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

/// 工具描述（注入给 LLM）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescription {
    pub name: String,
    pub description: String,
    pub parameters_schema: serde_json::Value,
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

**SandboxRouter trait**：

```rust
#[async_trait]
pub trait SandboxRouter: Send + Sync {
    /// 将工具调用路由到合适的沙箱并执行
    /// 执行结果会作为新事件写入 SessionStore
    async fn route(
        &self,
        session_id: SessionId,
        parent_event_id: EventId,
        tool: &str,
        params: serde_json::Value,
    ) -> Result<ExecutionResult, RouterError>;
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

5. [ControlLoop] 调用 LLM
   → LLMClient.decide(history, tools)
   → 记录 DecisionRecorded 事件

6. [LLM 决定调用工具]
   → 记录 ToolCallRequested 事件
   → SandboxRouter.route(tool, params)

7. [SandboxRouter] 获取/创建沙箱
   → Sandbox.execute(command, params)
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
llm-anthropic = ["dep:lattice-llm-anthropic", "dep:lattice-llm-protocol"]
llm-openai = ["dep:lattice-llm-openai", "dep:lattice-llm-protocol"]
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
