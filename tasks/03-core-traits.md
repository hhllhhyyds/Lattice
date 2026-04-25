# 任务 3：实现 core 类型和 trait

## 目标

在 `lattice-core` 中定义所有核心类型和 trait，这是整个框架的接口契约。

## 分支

`feat/core-traits`

## 参考文档

- `docs/ARCHITECTURE.md` — 完整的 trait 定义和类型结构

## 具体内容

### 文件结构

```
crates/core/src/
├── lib.rs          # 模块声明 + re-export
├── event.rs        # Event, EventId, EventPayload, Actor, Timestamp
├── session.rs      # SessionId, SessionStore trait, EventFilter
├── llm.rs          # LLMClient trait, Decision, ToolDescription
├── sandbox.rs      # Sandbox trait, ExecutionResult
├── router.rs       # SandboxRouter trait
└── error.rs        # StoreError, LLMError, SandboxError, RouterError
```

### 类型定义

按 `docs/ARCHITECTURE.md` 中的定义实现：

- `Event` struct — 不可变事件，包含 event_id、session_id、timestamp、actor、payload、parent_event_id
- `EventPayload` enum — SessionCreated、UserMessage、Thinking、ToolCallRequested、ToolCallResult、ToolCallError、FinalAnswer、StateChange
- `Actor` enum — System、LLM、Harness、Sandbox
- `Decision` enum — Thinking、ToolCall、FinalAnswer
- `ToolDescription` struct — name、description、parameters_schema
- `ExecutionResult` struct — stdout、stderr、exit_code
- `EventFilter` struct — 按事件类型、时间范围等过滤

### Trait 定义

- `SessionStore` — create_session、append_event、get_events、latest_event_id
- `LLMClient` — decide
- `Sandbox` — execute
- `SandboxRouter` — route

### 错误类型（thiserror）

- `StoreError` — SessionNotFound、SerializationError 等
- `LLMError` — RequestFailed、InvalidResponse 等
- `SandboxError` — ExecutionFailed、Timeout 等
- `RouterError` — SandboxUnavailable、ExecutionFailed 等

## 验收标准

- [ ] `cargo build -p lattice-core` 通过
- [ ] `cargo test -p lattice-core` 通过
- [ ] `cargo clippy -p lattice-core` 零警告
- [ ] `cargo doc -p lattice-core --no-deps` 通过
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] 零外部运行时依赖（只依赖 serde、uuid、chrono、async-trait、thiserror）
