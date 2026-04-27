# 任务 20-2：SessionStore 树形扩展 + MemoryStore 子 session

## 目标

扩展 `SessionStore` trait 支持创建子 session，构建 session tree。`MemoryStore` 实现子 session 支持。

## 分支

`feat/skill-session-tree`

## 依赖

- 任务 20-1（core 层扩展：EventPayload 新增变体）

---

## SessionStore trait 扩展

### `crates/core/src/session.rs`

在 `SessionStore` trait 中新增两个方法：

```rust
/// Create a child session under the given parent.
///
/// The child session is independent — its events are stored in the
/// returned child store. The parent store records the relationship
/// for `child_sessions()` queries.
async fn create_child_session(
    &self,
    parent_session_id: SessionId,
    skill_name: &str,
) -> Result<(SessionId, Arc<dyn SessionStore>), StoreError>;

/// List all child sessions for a given parent.
async fn child_sessions(
    &self,
    parent_session_id: SessionId,
) -> Result<Vec<ChildSessionInfo>, StoreError>;
```

新增 `ChildSessionInfo` 结构体：

```rust
/// Information about a child session (returned by SessionStore::child_sessions).
#[derive(Debug, Clone)]
pub struct ChildSessionInfo {
    pub session_id: SessionId,
    pub store: Arc<dyn SessionStore>,
    pub skill_name: String,
    pub created_at: Timestamp,
}
```

### `crates/core/src/lib.rs`

确保 re-export：

```rust
pub use session::{SessionStore, ChildSessionInfo};
```

---

## MemoryStore 实现

### `crates/store-memory/src/memory_store.rs`

**重构内部结构**：

将原来的 `sessions: Arc<RwLock<HashMap<SessionId, Vec<Event>>>>` 包装为 `Inner` 结构体：

```rust
/// Internal state of MemoryStore.
struct Inner {
    sessions: HashMap<SessionId, Vec<Event>>,
    /// Tree relationship: parent_session_id → list of child sessions
    children: HashMap<SessionId, Vec<ChildSessionInfo>>,
}

pub struct MemoryStore {
    inner: Arc<RwLock<Inner>>,
}
```

**`create_child_session` 实现要点**：

1. 验证 `parent_session_id` 存在
2. 创建独立的子 `MemoryStore`（new）
3. 在子 store 中调用 `create_session()` 创建根 session
4. 在父 store 的 `children` 中记录 `ChildSessionInfo`
5. 返回 `(child_session_id, Arc::new(child_store))`

**`child_sessions` 实现要点**：

从 `inner.children` 中查找父 session 对应的 `Vec<ChildSessionInfo>`，不存在时返回空 Vec。

**注意**：`MemoryStore` 内部调用 `create_session()` 时需要访问自己的 `inner`，而非通过 trait 接口（避免异步锁冲突）。子 store 使用独立的 `inner` 实例，保证父子 session 完全隔离。

---

## 测试要求

### 单元测试（`crates/store-memory/src/memory_store.rs`）

```rust
#[tokio::test]
async fn create_child_session_returns_independent_store() {
    let store = MemoryStore::new();
    let parent_id = store.create_session().await.unwrap();

    let (child_id, child_store) = store
        .create_child_session(parent_id, "web-research")
        .await
        .unwrap();

    // Child session id is different from parent
    assert_ne!(child_id, parent_id);

    // Events in child store are NOT visible in parent store
    child_store
        .append_event(
            child_id,
            EventPayload::UserMessage { content: "child msg".into() },
            Actor::Harness,
            None,
        )
        .await
        .unwrap();

    let parent_events = store.get_events(parent_id, &EventFilter::default()).await.unwrap();
    assert!(parent_events.iter().all(|e| !matches!(e.payload, EventPayload::UserMessage { .. })));
}

#[tokio::test]
async fn child_sessions_returns_correct_info() {
    let store = MemoryStore::new();
    let parent_id = store.create_session().await.unwrap();

    let (id1, _) = store.create_child_session(parent_id, "skill-a").await.unwrap();
    let (id2, _) = store.create_child_session(parent_id, "skill-b").await.unwrap();

    let children = store.child_sessions(parent_id).await.unwrap();
    assert_eq!(children.len(), 2);
    let names: Vec<_> = children.iter().map(|c| c.skill_name.as_str()).collect();
    assert!(names.contains(&"skill-a"));
    assert!(names.contains(&"skill-b"));
}

#[tokio::test]
async fn multiple_children_accumulated() {
    let store = MemoryStore::new();
    let parent_id = store.create_session().await.unwrap();

    for i in 0..5 {
        store
            .create_child_session(parent_id, format!("skill-{i}"))
            .await
            .unwrap();
    }

    let children = store.child_sessions(parent_id).await.unwrap();
    assert_eq!(children.len(), 5);
}

#[tokio::test]
async fn child_sessions_parent_not_found() {
    let store = MemoryStore::new();
    let fake = SessionId::new_v4();
    let result = store.child_sessions(fake).await;
    // Should return empty vec, not error (parent just has no children)
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}
```

---

## 验收标准

- [ ] `SessionStore` trait 新增 `create_child_session` / `child_sessions` 方法
- [ ] `ChildSessionInfo` 定义在 `lattice-core`
- [ ] `MemoryStore` 实现两个新方法
- [ ] 子 session 事件完全隔离（子 store 的事件不在父 store 中可见）
- [ ] `child_sessions` 对不存在的父 session 返回空 Vec（而非错误）
- [ ] 以上测试全部通过
- [ ] `cargo fmt` + `cargo clippy` 通过
