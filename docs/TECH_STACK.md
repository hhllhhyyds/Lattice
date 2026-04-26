# 技术选型

## 总览

| 层面 | 选型 | 理由 |
|------|------|------|
| 语言 | Rust 2021 edition | trait 强制抽象边界，async 适配 I/O，类型安全 |
| 异步运行时 | tokio | 生态最成熟，tower 中间件可复用 |
| 序列化 | serde + serde_json | Rust 序列化标配，事件日志 JSON 格式 |
| HTTP 框架 | axum（后期） | 基于 tower，轻量，暴露 Agent API 用 |
| LLM 调用 | 自封装 trait + reqwest | 不绑死 SDK，保持 LLMClient trait 抽象 |
| 事件存储 | 初期内存，后期 SQLite/Postgres | 先跑通，存储层通过 trait 随时替换 |
| 沙箱执行 | 初期 tokio::process，后期 Docker/Firecracker | 最小可运行优先 |
| 错误处理 | thiserror + anyhow | trait 内用 thiserror 具体类型，应用层 anyhow |
| 日志追踪 | tracing + tracing-subscriber | 异步友好，结构化日志 |
| 测试 | cargo test + mockall | trait 天然适合 mock |
| 唯一 ID | uuid (v4) | 事件和会话的唯一标识 |
| 时间戳 | chrono | UTC 时间戳 |
| CI/CD | GitHub Actions（后期） | 自动化测试、lint、发布 |

## 各选型详细说明

### Rust 2021 Edition

- trait 系统完美匹配"接口有立场，实现无假设"的设计原则
- async/await 天然适配 LLM API 调用、沙箱执行等 I/O 密集操作
- `Arc<dyn Trait>` 支持跨线程安全共享
- 编译期保证内存安全，适合长期运行的 Agent 服务

### tokio

- Rust 异步生态的事实标准
- `tokio::process` 可直接用于本地沙箱实现
- `tokio::sync` 提供异步锁、channel 等并发原语
- 后期 axum 也基于 tokio

### serde + serde_json

- 事件日志需要序列化存储，JSON 格式可读性好，便于调试
- `#[serde(tag = "type")]` 可优雅处理事件枚举的序列化

### thiserror + anyhow

- **thiserror**：在 core crate 中为每个 trait 定义精确的错误类型（`StoreError`、`LLMError`、`SandboxError`、`ToolError`）
- **anyhow**：在 runtime 和 example 中做顶层错误聚合
- 分层清晰：trait 消费者看到精确错误，应用层简化处理

### mockall

- 所有核心组件都是 trait，mockall 可以自动生成 mock 实现
- 测试 ControlLoop 时可以 mock SessionStore、LLMClient、ToolSet
- 无需启动真实 LLM 或沙箱即可跑完整控制循环测试

## 依赖版本策略

- 使用 workspace 级别的依赖管理（`[workspace.dependencies]`）
- 所有 crate 共享同一版本的公共依赖
- 初版不锁死具体版本，使用兼容范围（如 `tokio = "1"`）
