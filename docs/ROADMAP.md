# Roadmap

## ✅ 第一轮：MVP（已完成）

验证核心架构的三组件解耦可行性。

| Crate                 | 内容                                                                     | 状态 |
| --------------------- | ------------------------------------------------------------------------ | ---- |
| lattice-core          | 核心 trait + 类型定义（SessionStore、LLMClient、Sandbox、ToolExecutor） | ✅    |
| lattice-runtime       | ControlLoop 决策循环                                                    | ✅    |
| lattice-store-memory  | SessionStore 内存实现                                                    | ✅    |
| lattice-sandbox-local | Sandbox 本地子进程实现                                                   | ✅    |
| hello-agent example   | 端到端验证（MockLLMClient）                                              | ✅    |
| GitHub Actions CI     | fmt + clippy + test + doc                                                | ✅    |

## ✅ 第二轮：真实 LLM 接入（已完成）

| Crate                 | 内容                                 | 状态 |
| --------------------- | ------------------------------------ | ---- |
| lattice-llm-protocol  | 通用协议层（消息格式转换、响应解析） | ✅    |
| lattice-llm-anthropic | Anthropic Claude 后端                | ✅    |
| lattice-llm-openai    | OpenAI 兼容后端                      | ✅    |

## ✅ 第三轮：真实 LLM 验证（已完成）

| 任务               | 内容                                        | 状态 |
| ------------------ | ------------------------------------------- | ---- |
| real-agent example | 用真实 LLM 端到端跑通（Anthropic / OpenAI） | ✅    |

## 🚧 第四轮：HTTP API 层（进行中）

**目标**：从库升级为可独立部署的平台服务。基于 axum 搭建 REST API，支持通过 HTTP 创建/查询/恢复会话、提交任务、实时获取结果。

### 设计原则

- **API 层是薄壳**：不引入新的业务逻辑，只做 HTTP 与 core trait 之间的桥接
- **状态管理集中**：所有运行时状态（活跃会话、ControlLoop 句柄）通过 `AppState` 统一管理
- **异步非阻塞**：Agent 任务异步执行，客户端通过轮询或事件流获取进度
- **Provider 可配置**：LLM provider 通过配置文件/环境变量指定，运行时可切换

### 任务拆解

| #  | 任务 | 分支名 | 状态 |
|----|------|--------|------|
| 12 | [Facade crate + Feature Flags](../tasks/12-facade-features.md) | `feat/facade-features` | ✅ |
| 13 | [Server crate 骨架 + 基础路由](../tasks/13-server-skeleton.md) | `feat/server-skeleton` | ✅ |
| 14 | [会话管理 API](../tasks/14-session-api.md) | `feat/session-api` | ✅ |
| 15 | [工具系统：ToolExecutor + ToolSet + 标准工具库](../tasks/15-tool-system.md) | `feat/tool-system` | ✅ |
| 16 | [任务提交与 Agent 执行 API](../tasks/16-agent-run-api.md) | `feat/agent-run-api` | ✅ |
| 17 | [SSE 实时事件流](../tasks/17-sse-stream.md) | `feat/sse-stream` | ⬜ |
| 18 | [配置管理与多 Provider 支持](../tasks/18-config-provider.md) | `feat/config-provider` | ⬜ |
| 19 | [Docker 化独立部署](../tasks/19-docker-deploy.md) | `feat/docker-deploy` | ⬜ |

## 第五轮：Skill 系统（规划中）

**目标**：实现 Lattice 的 skill 系统，使 meta agent 能够将复杂子任务委托给专门的 skill agent 执行。skill 在父 agent 视角是普通 tool 调用，背后运行完整的子 ControlLoop，支持多轮 LLM 决策、独立工具集和独立 session 树节点。

**设计原则**：遵循 [Anthropic Agent Skills 开放标准](https://www.agentskills.com)，以 SKILL.md 为唯一事实来源，实现渐进式披露（Progressive Disclosure）三层加载机制。

| # | 任务 | 分支名 |
|---|------|--------|
| 20-1 | core 层扩展：ExecutionContext + EventPayload + ToolError | `feat/skill-execution-context` |
| 20-2 | SessionStore 树形扩展 + MemoryStore 子 session | `feat/skill-session-tree` |
| 20-3 | ToolSet + 已有工具适配新签名 | `feat/skill-tool-execute-ctx` |
| 20-4 | ControlLoop 构造 ExecutionContext + builder | `feat/skill-control-loop` |
| 20-5 | lattice-skill crate | `feat/skill-crate` |
| 20-6 | skill feature + 示例 skill 目录 + meta-agent example | `feat/skill-facade` |

### 渐进式披露三层加载

```
Level 1：SkillLoader 启动时加载所有 skill 的 name + description（≈30-50 tokens/skill）
         ↓ 意图匹配
Level 2：加载 SKILL.md 正文作为 system prompt
         ↓ 执行中按需
Level 3：子 agent 按需读取 references/、scripts/ 中的文件
```

### Session Tree

父 session 调用 skill 时创建子 session，子 session 拥有独立 SessionStore。父 session 记录 `SkillInvoked`/`SkillCompleted` 事件，携带子 session ID。

### API 端点预览

```
GET    /health                          → 健康检查
POST   /v1/sessions                     → 创建会话
GET    /v1/sessions                     → 列出会话
GET    /v1/sessions/:id                 → 查询会话详情
GET    /v1/sessions/:id/events          → 查询会话事件（支持过滤）
POST   /v1/sessions/:id/messages        → 提交用户消息并触发 Agent 执行
GET    /v1/sessions/:id/messages        → 获取会话消息（FinalAnswer + UserMessage）
GET    /v1/sessions/:id/stream          → SSE 事件流（实时推送）
GET    /v1/providers                    → 列出可用 LLM provider
```

## 📋 后续规划

### 第六轮：记忆与规划

**目标**：为 Lattice 添加运行时状态管理、显式规划执行层，以及长期记忆接口，补全 Agent 的"记忆-规划-执行"闭环。

#### 运行时状态管理器（RuntimeState）

**问题**：现有的 `SessionStore` 是 append-only 事件日志，适合持久化和可追溯性，但不适合运行时快速读写。规划执行器如果从事件日志重建状态，每次迭代都要 O(n) 扫描，性能差且不稳定。

**解决方案**：引入轻量 `RuntimeState` trait，作为 ControlLoop 的伴随状态：

```rust
/// Runtime state for an agent session — structured, in-memory, fast read/write.
/// Unlike SessionStore (append-only log), RuntimeState supports get/patch/reset.
#[async_trait]
pub trait RuntimeState: Send + Sync {
    /// Get a field value by key.
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, RuntimeStateError>;

    /// Set or overwrite a field.
    async fn set(&self, key: &str, value: serde_json::Value) -> Result<(), RuntimeStateError>;

    /// Delete a field.
    async fn remove(&self, key: &str) -> Result<(), RuntimeStateError>;

    /// Snapshot the entire state for persistence or replay.
    async fn snapshot(&self) -> Result<RuntimeStateSnapshot, RuntimeStateError>;

    /// Restore from a snapshot.
    async fn restore(&self, snapshot: &RuntimeStateSnapshot) -> Result<(), RuntimeStateError>;
}
```

**标准字段约定**（convention，不强制）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `step` | i32 | 当前步骤序号 |
| `tool_results` | Map | 工具执行结果缓存 `{ "bash": "output", ... }` |
| `retries` | i32 | 当前步骤重试次数 |
| `max_retries` | i32 | 最大重试次数 |
| `plan` | Array | 当前执行计划（子步骤列表） |

**与 SessionStore 的关系**：
- `SessionStore`：持久化、可追溯、append-only——事件的账本
- `RuntimeState`：临时、可读写、运行时内存——规划器的快速寄存器

#### 规划执行器（Planner trait）

**问题**：当前 ControlLoop 中，LLM 同时扮演"推理者"和"规划者"两个角色。当任务步骤固定时（如"先查数据库再发邮件再写日志"），用满血大模型做规划是 token 浪费，且规划结果不稳定。

**解决方案**：引入 `Planner` trait，将规划决策从 LLM 中枢中分离出来：

```rust
/// A planner decides the next step given current state and goals.
/// Unlike LLMClient (general reasoning), a Planner is specialized for
/// step sequencing and action selection.
#[async_trait]
pub trait Planner: Send + Sync {
    /// Given the current runtime state and LLM output, decide the next action.
    async fn plan(
        &self,
        state: &dyn RuntimeState,
        llm_output: &str,  // LLM 中枢的高层结论，不是全量上下文
        available_tools: &[ToolDescription],
    ) -> Result<Plan, PlannerError>;
}

/// A concrete planning decision.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Machine-readable next action.
    pub action: PlanAction,
    /// Plain-text reasoning for audit log.
    pub reasoning: String,
}

#[derive(Debug, Clone)]
pub enum PlanAction {
    /// Call a specific tool with parameters.
    ToolCall { tool: String, params: serde_json::Value },
    /// Halt execution and return answer.
    FinalAnswer { answer: String },
    /// Retry the current step (increment retry counter).
    Retry,
    /// Update runtime state without calling a tool.
    UpdateState { patch: serde_json::Value },
    /// Ask the LLM for clarification on a ambiguous goal.
    AskForClarification { question: String },
}
```

**两种实现路径**：

| 类型 | 适用场景 | 实现方式 |
|---|---|---|
| `SmallModelPlanner` | 复杂但非固定的业务流程 | 单独小模型（如 Qwen2.5-7B）+ 强制 JSON Schema 输出，token 成本低 |
| `FsmPlanner` | 步骤固定的流水线任务 | 纯 Rust FSM/状态机，零 LLM 调用，极致稳定 |

**与 LLM 中枢的关系**：
- LLM 中枢（`LLMClient`）：动脑，处理通用推理和最终答案
- 规划执行器（`Planner`）：动手，执行步骤决策，输出结构化动作
- 两者串联：LLM 高层结论 → Planner 拆解步骤 → 工具执行

#### 长期记忆

**问题**：append-only 事件日志会随时间无限增长，长会话的上下文窗口迟早爆炸。同时，Agent 需要从历史任务中学习（相似任务复用经验）。

**解决方案**：定义 `LongTermMemory` trait，支持事件历史摘要和检索：

```rust
/// Long-term memory store for cross-session learning and context management.
/// Backed by a vector store (Pinecone/Milvus) or a simple full-text index.
#[async_trait]
pub trait LongTermMemory: Send + Sync {
    /// Store a memory entry with optional embedding for retrieval.
    async fn store(&self, entry: MemoryEntry) -> Result<(), MemoryError>;

    /// Retrieve memories relevant to the given query.
    async fn retrieve(&self, query: &str, top_k: usize) -> Result<Vec<MemoryEntry>, MemoryError>;

    /// Summarize and archive a session's event log into a compact memory entry.
    async fn archive_session(&self, session_id: SessionId) -> Result<MemoryEntry, MemoryError>;
}

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub content: String,          // 摘要文本
    pub embedding: Option<Vec<f32>>, // 向量嵌入（可选，无向量库时可不填）
    pub tags: Vec<String>,        // 标签：["bug-fix", "auth", "migration"]
    pub session_id: Option<SessionId>,
    pub created_at: Timestamp,
}
```

**存储后端**：
- 轻量：SQLite FTS5 全文检索，无需外部服务
- 生产：Pinecone / Milvus / Qdrant 向量数据库
- 两者可叠加：SQLite 做标签+全文检索，向量库做语义检索

**使用场景**：
- 新会话开始时，检索相关历史经验注入上下文
- 上下文窗口接近上限时，自动摘要归档旧事件
- 团队共享知识库：把成功的 skill 执行结果归档复用

---

### Docker 沙箱
- 实现 Sandbox trait 的 Docker 容器版本
- 容器级别的进程隔离和资源限制
- 凭据初始化注入，运行时不可访问

### 持久化存储
- SQLite / Postgres 实现 SessionStore trait
- 支持会话跨进程恢复

### 上下文窗口管理
- 事件历史压缩/摘要策略
- 长会话的上下文滑动窗口

### MCP 工具桥接
- 实现 ToolExecutor 适配器，通过 MCP 协议调用外部工具服务器
- 作为 Layer 3（Harness-Provided）工具注入

### 凭据管理
- Vault Proxy 模式：沙箱外注入凭据
- 初始化注入模式：沙箱创建时一次性注入
