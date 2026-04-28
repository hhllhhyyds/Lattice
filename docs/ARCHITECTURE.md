# 架构设计

## 设计哲学

Lattice 的架构灵感来自 Anthropic 的 Managed Agents 博客（*Scaling Managed Agents: Decoupling the brain from the hands*），以及操作系统的虚拟化思想：

- **借鉴 OS 的抽象层**：操作系统通过 `read()` 等稳定接口屏蔽底层硬件差异。Lattice 对 Agent 运行环境做同样的事——定义稳定接口，让实现可自由替换。
- **苦涩的教训**：不在框架中嵌入针对特定模型能力的"补丁"。模型特定的适配只能以可插拔的方式挂载。
- **宠物 vs 牲口**：所有组件（会话、控制循环、沙箱）都是无状态/可恢复的，坏了就换，不需要"抢救"。
- **Feature 组合，按需裁剪**：所有非核心功能（LLM 后端、沙箱实现、存储实现、HTTP 服务）通过 Rust feature flags 控制。消费者可以只编译自己需要的部分，不引入多余依赖。core crate 保持零 feature，永远是纯接口。

## 三个核心抽象

```
┌─────────────────────────────────────────────────────────────┐
│                 ControlLoop（Agent 的大脑）                   │
│  持有 ToolSet（工具注册表），唯一有权调用 LLM                 │
│  依赖两个 trait:                                            │
│    - SessionStore（记忆层）                                  │
│    - LLMClient（推理层）                                    │
└──────────┬──────────────────────────────────┬──────────────┘
           │                                  │
      ┌────▼──────┐                    ┌─────▼───────┐
      │ Session   │                    │  ToolSet    │
      │  Store    │                    │  (Layer 1-3)│
      │ (trait)   │                    └──────┬──────┘
      └───────────┘                            │
                                        ┌──────▼──────┐
                                        │ ToolExecutor │
                                        │  (trait)    │
                                        └──────┬──────┘
                                               │
                                        ┌──────▼──────────────┐
                                        │ BashTool / FileTool  │
                                        │ / GlobTool / MCP ... │
                                        └─────────────────────┘
```

**三层工具体系**（Layer 1-3 由 `ToolSet` 统一调用，ControlLoop 不区分沙箱执行还是进程内执行）：

- **Layer 1（`lattice-core`）**：纯接口——`ToolExecutor` trait
- **Layer 2（`lattice-tools`）**：标准工具库——BashTool、FileTool 等
- **Layer 3（应用层）**：用户注入——MCP 桥接、自定义工具、Skill 工具集

---

## 1. Session（会话）—— 不可变事件日志

**定义**：Agent 运行期间产生的所有事件的不可变、仅追加、持久化序列。

**不是什么**：
- 不是 LLM 的对话历史（那是短期记忆层，规划中）
- 不是上下文窗口
- 不可修改、不可删除

### 事件类型

```rust
/// 事件负载
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EventPayload {
    SessionCreated,

    UserMessage { content: String },

    Thinking { reasoning: String },

    ToolCallRequested { tool: String, params: serde_json::Value },

    ToolCallResult { stdout: String, stderr: String, exit_code: i32 },

    ToolCallError { error: String },

    FinalAnswer { answer: String },

    StateChange { from: String, to: String },

    // — Skill 系统扩展 —
    SkillInvoked { skill_name: String, child_session_id: SessionId },
    SkillCompleted { skill_name: String, child_session_id: SessionId },
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

### SessionStore trait

```rust
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create_session(&self) -> Result<SessionId, StoreError>;

    async fn append_event(
        &self,
        session_id: SessionId,
        payload: EventPayload,
        actor: Actor,
        parent_event_id: Option<EventId>,
    ) -> Result<EventId, StoreError>;

    async fn get_events(
        &self,
        session_id: SessionId,
        filter: &EventFilter,
    ) -> Result<Vec<Event>, StoreError>;

    async fn latest_event_id(&self, session_id: SessionId) -> Result<Option<EventId>, StoreError>;

    // — Skill 系统扩展 —
    async fn create_child_session(
        &self,
        parent_session_id: SessionId,
        skill_name: &str,
    ) -> Result<(SessionId, Arc<dyn SessionStore>), StoreError>;

    async fn child_sessions(
        &self,
        parent_session_id: SessionId,
    ) -> Result<Vec<ChildSessionInfo>, StoreError>;
}
```

### Session Tree（Skill 系统）

Session 支持树形扩展：父 session 调用 skill 时创建子 session，子 session 拥有独立的 SessionStore。父 session 的事件日志记录 `SkillInvoked`/`SkillCompleted` 事件，携带子 session ID，供可观测性查询。

```
Root SessionStore
├── session-001 (meta agent)
│   └── SkillInvoked → session-002 (skill: web-research, 独立 MemoryStore)
└── session-003 (另一个 meta agent)
```

---

## 2. ControlLoop（控制循环）—— Agent 的大脑

**定义**：唯一有权调用 LLM 的组件。负责加载事件历史、构建提示、解析 LLM 决策、路由工具调用。

**关键特性**：
- **无状态**：不持有持久状态，所有状态从 SessionStore 恢复
- **可崩溃恢复**：用同一个 session_id 重新创建 ControlLoop，从事件日志断点继续
- **深度感知**：通过 `depth` 字段追踪 skill 嵌套层级，防无限递归

### 决策循环

```
loop {
    1. 从 SessionStore 加载事件历史
    2. 从 ToolSet 获取工具描述，调用 LLM
    3. 记录决策事件
    4. match 决策:
       - FinalAnswer → 记录结果，退出循环
       - Thinking   → 继续循环
       - ToolCall   → 构造 ExecutionContext，ToolSet.execute()，记录结果/错误，继续循环
}
```

### LLMClient trait

```rust
#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn decide(
        &self,
        history: &[Event],
        available_tools: &[ToolDescription],
        system_prompt: &str,
    ) -> Result<Decision, LLMError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Decision {
    Thinking { reasoning: String },
    ToolCall { tool: String, params: serde_json::Value },
    FinalAnswer { answer: String },
}
```

### ExecutionContext

```rust
/// Maximum allowed skill nesting depth. A meta agent has depth=0;
/// its direct skill children have depth=1, and so on.
pub const MAX_SKILL_DEPTH: u32 = 8;

/// Execution context passed to every tool invocation.
pub struct ExecutionContext {
    pub session_id: SessionId,
    pub trigger_event_id: EventId,
    pub store: Arc<dyn SessionStore>,
    pub depth: u32,
}
```

---

## 3. Sandbox（沙箱）—— Agent 的双手

**定义**：隔离的工具执行环境，按需创建，可替换。

**关键特性**：
- **隔离**：沙箱内的代码无法访问框架内部或凭据
- **可替换**：崩溃后换一个，ControlLoop 视之为一次工具调用失败
- **跨平台**：`LocalSandbox` 根据 OS 自动选择 shell
  - Unix/Linux/macOS: `sh -c`
  - Windows: `cmd.exe /C`

### Sandbox trait

```rust
#[async_trait]
pub trait Sandbox: Send + Sync {
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

---

## 工具系统

### ToolExecutor trait（Layer 1）

```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn description(&self) -> ToolDescription;

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError>;
}
```

### ToolSet（Layer 1-3 统一入口）

```rust
pub struct ToolSet {
    tools: HashMap<String, Box<dyn ToolExecutor>>,
}

impl ToolSet {
    pub fn new() -> Self { ... }

    #[cfg(feature = "bash")]
    pub fn with_defaults(sandbox: Arc<dyn Sandbox>) -> Self { ... }

    /// Register a tool. Error if name conflicts.
    pub fn register(&mut self, tool: impl ToolExecutor + 'static) -> Result<(), ToolError> { ... }

    /// All tool descriptions for LLM consumption.
    pub fn descriptions(&self) -> Vec<ToolDescription> { ... }

    /// Route a tool call to the correct executor.
    pub async fn execute(
        &self,
        name: &str,
        params: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError> { ... }
}
```

### ToolSet 与 SandboxRouter 的关系

`ToolSet` 统一了工具描述和工具执行两个职责，取代了之前的 `Vec<ToolDescription>` + `SandboxRouter` 分离模式。

`SandboxRouter` trait 和 `BasicSandboxRouter` 已被移除。ToolSet 完全替代了它们的职责——工具路由逻辑内化到 ToolSet 中，沙箱工具通过 `ToolExecutor` 实现内部持有 `Arc<dyn Sandbox>` 来委托执行。

---

## 数据流与生命周期

```
1. [客户端] 创建 Session
   → SessionStore.create_session()

2. [客户端] 提交用户任务
   → SessionStore.append_event(UserMessage)

3. [系统] 创建 ControlLoop，绑定 session_id
   → ControlLoop.run() 开始

4. [ControlLoop] 加载事件历史
   → SessionStore.get_events()

5. [ControlLoop] 从 ToolSet 获取工具描述，调用 LLM
   → LLMClient.decide(history, tools)

6. [LLM 决定调用工具]
   → 记录 ToolCallRequested 事件
   → ToolSet.execute(tool, params, ctx)

7. [ToolExecutor] 执行工具
   → 记录 ToolCallResult 或 ToolCallError 事件

8. [ControlLoop] 回到步骤 4

9. [LLM 给出最终答案]
   → 记录 FinalAnswer 事件
   → ControlLoop.run() 返回
```

---

## 崩溃恢复

```
ControlLoop 崩溃
  → 用同一个 session_id 创建新的 ControlLoop
  → SessionStore.get_events() 发现最后一个事件是 ToolCallRequested
  → LLM 看到未完成的调用，决定重试或跳过
  → 继续执行
```

---

## 安全边界

- **凭据隔离**：Sandbox 实现不能直接访问 SessionStore 或 LLM 凭据
- **初始化注入**：敏感信息只在沙箱创建阶段注入（如环境变量），运行时代码无法获取注入动作本身
- **Vault Proxy**（未来）：第三方令牌通过外部代理注入，沙箱永远接触不到原始凭据

---

## Feature Flag 设计

### 设计原则

1. **core 零 feature**：`lattice-core` 是纯接口层，不包含任何可选功能
2. **实现 crate 独立成包**：每个实现是独立 crate，消费者通过 `Cargo.toml` 按需引入
3. **Facade crate 提供便利**：`lattice` facade crate 通过 feature flags 重导出所有子 crate
4. **默认最小化**：default feature 只包含最基础的功能，用户明确 opt-in 额外能力

### Facade Crate：`lattice`

```toml
[features]
default = ["runtime", "store-memory", "sandbox-local", "tools"]

runtime = ["dep:lattice-runtime"]
store-memory = ["dep:lattice-store-memory"]
sandbox-local = ["dep:lattice-sandbox-local"]
tools = ["dep:lattice-tools"]
llm-protocol = ["dep:lattice-llm-protocol"]
llm-anthropic = ["llm-protocol", "dep:lattice-llm-anthropic"]
llm-openai = ["llm-protocol", "dep:lattice-llm-openai"]
llm-all = ["llm-anthropic", "llm-openai"]
skill = ["dep:lattice-skill", "tools"]        # Skill 系统
full = ["runtime", "store-memory", "sandbox-local", "llm-all", "tools", "skill"]
```

### Crate 结构

```
crates/
├── core/             # 纯接口：SessionStore, LLMClient, Sandbox, ToolExecutor, ExecutionContext
├── runtime/          # ControlLoop 实现
├── store-memory/     # MemoryStore 实现
├── sandbox-local/   # LocalSandbox 实现（跨平台 shell）
├── tools/           # ToolSet + 标准工具（BashTool 等）
├── skill/           # Skill 系统（SkillDefinition, SkillTool, SkillLoader）  # 规划中
├── llm-protocol/    # LLM 通用协议层
├── llm-anthropic/   # Anthropic Claude 后端
├── llm-openai/      # OpenAI 兼容后端
└── server/          # HTTP API 服务（axum）
```

---

## 未来扩展

- **MCP 桥接工具**：实现 `ToolExecutor`，内部通过 MCP 协议调用外部 MCP 服务器。作为 Layer 3 工具由应用层注入。
- **沙箱工厂（SandboxFactory）**：按需创建沙箱（如 Docker 容器），引入 factory 模式。
- **凭据隔离**：资源绑定（token 注入沙箱初始化）和 Vault Proxy 两种模式。
- **工具权限控制**：参考 Claude Code 的 permission 模型，按工具名配置 allow/deny 规则。
- **RuntimeState**（第六轮）：运行时结构化状态，供规划执行器快速读写，独立于 append-only 事件日志。
- **Planner trait**（第六轮）：将步骤决策从 LLM 中枢分离，支持小模型规划器或 FSM 规则引擎。
- **LongTermMemory**（第六轮）：跨会话记忆，支持向量检索或 SQLite FTS5 全文检索。

---

## 测试覆盖率

使用 `cargo-llvm-cov` 检测代码覆盖率。覆盖率指标按 crate 维度统计（line coverage、branch coverage），作为 CI 的一部分运行，不阻断合并。

- **工具**：`cargo-llvm-cov` + `cargo-llvm-cov-action`
- **CI 上传**：通过 Codecov action 上传至 codecov.io
- **报告格式**：LCOV（`--lcov`），生成 lcov.info
- **原则**：新增代码应附带测试，保持覆盖率不下降
