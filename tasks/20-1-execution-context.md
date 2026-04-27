# 任务 20-1：core 层扩展 — ExecutionContext + EventPayload + ToolError

## 目标

在 `lattice-core` 中引入 skill 系统所需的全部基础设施类型：
`ExecutionContext`、`MAX_SKILL_DEPTH`、`ToolError::MaxDepthExceeded`、`EventPayload` 新增变体。

**这是 skill 系统的第一阶段**，所有后续阶段都依赖本次新增的类型。

## 分支

`feat/skill-execution-context`

## 依赖

- 任务 15（工具系统：ToolExecutor + ToolSet）
- 任务 3（core 类型和 trait）

---

## 新增类型

### 1. `ExecutionContext`（`crates/core/src/tool.rs`）

在文件末尾添加：

```rust
/// Maximum allowed skill nesting depth.
///
/// A meta agent has depth=0; its direct skill children have depth=1, and so on.
/// When depth reaches MAX_SKILL_DEPTH, tool execution must fail with
/// `ToolError::MaxDepthExceeded` to prevent infinite recursion.
pub const MAX_SKILL_DEPTH: u32 = 8;

/// Execution context passed to every tool invocation by the ControlLoop.
///
/// Allows tools to inspect the calling session, record correlated events,
/// and enforce depth limits for skill nesting.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// The session this tool call belongs to.
    pub session_id: SessionId,
    /// The event id of the `ToolCallRequested` event that triggered this execution.
    pub trigger_event_id: EventId,
    /// The session store for reading/writing events.
    pub store: Arc<dyn SessionStore>,
    /// Nesting depth: 0 for the top-level meta agent, 1 for direct skill children, etc.
    pub depth: u32,
}
```

**注意**：`Arc<dyn SessionStore>` 已通过 `crates/core/src/lib.rs` 的 re-export 可用。
如果出现循环依赖，在 `lib.rs` 中添加显式 `use crate::session::{SessionStore, ChildSessionInfo};`。

### 2. `ToolError::MaxDepthExceeded`（`crates/core/src/error.rs`）

在 `ToolError` 枚举中添加变体：

```rust
/// Maximum skill nesting depth exceeded.
#[error("max skill depth {0} exceeded")]
MaxDepthExceeded(u32),
```

### 3. `EventPayload` 新增变体（`crates/core/src/event.rs`）

在 `EventPayload` 枚举中添加：

```rust
/// A skill was invoked — recorded in the parent session.
SkillInvoked {
    skill_name: String,
    child_session_id: SessionId,
},

/// A skill completed — recorded in the parent session.
SkillCompleted {
    skill_name: String,
    child_session_id: SessionId,
},
```

### 4. `ToolExecutor` 签名变更（`crates/core/src/tool.rs`）

`execute` 方法签名从：

```rust
async fn execute(&self, params: serde_json::Value) -> Result<ExecutionResult, ToolError>;
```

变更为：

```rust
async fn execute(
    &self,
    params: serde_json::Value,
    ctx: &ExecutionContext,
) -> Result<ExecutionResult, ToolError>;
```

### 5. 更新 `crates/core/src/lib.rs`

确保 re-exports 包含新增类型：

```rust
pub use session::{SessionStore, ChildSessionInfo};
pub use tool::{ExecutionContext, MAX_SKILL_DEPTH, ToolExecutor};
pub use error::ToolError;
```

---

## 测试要求

### 单元测试（`crates/core/src/tool.rs`）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_context_clone() {
        let ctx = ExecutionContext {
            session_id: SessionId::new_v4(),
            trigger_event_id: EventId::new_v4(),
            store: Arc::new(MockStore),
            depth: 3,
        };
        let cloned = ctx.clone();
        assert_eq!(cloned.depth, 3);
    }

    #[test]
    fn max_skill_depth_value() {
        assert_eq!(MAX_SKILL_DEPTH, 8);
    }
}
```

### 单元测试（`crates/core/src/error.rs`）

```rust
#[test]
fn test_max_depth_exceeded_display() {
    let err = ToolError::MaxDepthExceeded(8);
    assert!(err.to_string().contains("max skill depth"));
    assert!(err.to_string().contains("8"));
}
```

### 单元测试（`crates/core/src/event.rs`）

```rust
#[test]
fn test_skill_invoked_serde_roundtrip() {
    let child = SessionId::new_v4();
    let payload = EventPayload::SkillInvoked {
        skill_name: "web-research".into(),
        child_session_id: child,
    };
    let json = serde_json::to_string(&payload).unwrap();
    let parsed: EventPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(payload, parsed);
}

#[test]
fn test_skill_completed_serde_roundtrip() {
    let child = SessionId::new_v4();
    let payload = EventPayload::SkillCompleted {
        skill_name: "web-research".into(),
        child_session_id: child,
    };
    let json = serde_json::to_string(&payload).unwrap();
    let parsed: EventPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(payload, parsed);
}
```

---

## 验收标准

- [ ] `MAX_SKILL_DEPTH = 8` 定义在 `lattice-core`
- [ ] `ExecutionContext` 有完整 doc comment，包含所有字段说明
- [ ] `ToolError::MaxDepthExceeded` 新增，显示格式为 `"max skill depth 8 exceeded"`
- [ ] `EventPayload` 新增 `SkillInvoked` 和 `SkillCompleted` 变体，serde 序列化格式为 `"skillInvoked"` / `"skillCompleted"`
- [ ] `ToolExecutor::execute` 签名含 `ctx: &ExecutionContext`
- [ ] 所有新增类型在 `lattice-core` 的 `lib.rs` 中 re-export
- [ ] 上述三个测试文件中的测试全部通过
- [ ] `cargo fmt --all -- --check` 和 `cargo clippy --all-targets -- -D warnings` 通过
