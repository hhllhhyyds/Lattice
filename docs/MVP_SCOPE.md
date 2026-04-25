# MVP 范围定义

## 目标

用最小代码验证 Lattice 核心架构的三组件解耦是否真正可行：

> 收到任务 → LLM 决定调用工具 → 沙箱执行 → 返回结果 → LLM 给出最终答案

## MVP 包含的 Crate

### 1. lattice-core

**职责**：纯 trait 定义 + 公共类型

**内容**：
- `Event`、`EventPayload`、`Actor`、`Decision` 等核心类型
- `SessionStore` trait
- `LLMClient` trait
- `Sandbox` trait
- `SandboxRouter` trait
- `ToolDescription` 类型
- 所有错误类型（`StoreError`、`LLMError`、`SandboxError`、`RouterError`）

**约束**：零外部运行时依赖。只依赖 serde、uuid、chrono、async-trait、thiserror。

### 2. lattice-runtime

**职责**：ControlLoop 的具体实现

**内容**：
- `ControlLoop` struct：持有 `Arc<dyn SessionStore>`、`Arc<dyn LLMClient>`、`Arc<dyn SandboxRouter>`
- `ControlLoop::run()` 方法：完整的决策循环
- `BasicSandboxRouter` struct：默认的路由器实现，接收 `Arc<dyn Sandbox>` + `Arc<dyn SessionStore>`

**依赖**：lattice-core、tokio、tracing

### 3. lattice-store-memory

**职责**：SessionStore 的内存实现

**内容**：
- `MemoryStore` struct：用 `Arc<RwLock<HashMap<SessionId, Vec<Event>>>>` 存储
- 完整实现 `SessionStore` trait

**用途**：开发和测试，不用于生产

### 4. lattice-sandbox-local

**职责**：Sandbox 的本地子进程实现

**内容**：
- `LocalSandbox` struct：用 `tokio::process::Command` 执行命令
- 完整实现 `Sandbox` trait
- 基本的超时控制

**安全**：初版不做进程隔离，仅用于开发验证

### 5. examples/hello-agent

**职责**：端到端验证

**内容**：
- 一个 `MockLLMClient`：硬编码决策序列（先调用 bash，再给出最终答案）
- 组装所有组件，跑通完整流程
- 打印事件日志，验证事件溯源正确

## MVP 不包含的（后续迭代）

| 功能 | 原因 |
|------|------|
| HTTP API 服务（axum） | 先验证核心库，API 层后加 |
| 真实 LLM 调用（Anthropic/OpenAI） | 用 mock 先跑通，避免 API 依赖阻塞开发 |
| Docker/Firecracker 沙箱 | 本地子进程足够验证架构 |
| SQLite/Postgres 存储 | 内存存储足够验证架构 |
| 凭据管理 / Vault Proxy | 初版不涉及真实凭据 |
| 多沙箱并行调度 | 初版单沙箱串行 |
| 上下文窗口管理/压缩 | 初版事件全量传给 LLM |
| CI/CD | 文档和 MVP 代码完成后再搭 |

## 验收标准

1. `cargo build` 全部编译通过
2. `cargo test` 所有测试通过
3. `cargo clippy` 零警告
4. `cargo run --example hello-agent` 端到端跑通，控制台输出完整事件流
5. 三个核心 trait（SessionStore、LLMClient、Sandbox）的实现可以独立替换，不影响 ControlLoop 代码
