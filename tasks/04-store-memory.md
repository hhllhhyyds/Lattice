# 任务 4：实现 MemoryStore

## 目标

在 `lattice-store-memory` 中实现 `SessionStore` trait 的内存版本，用于开发和测试。

## 分支

`feat/store-memory`

## 依赖

- 任务 3（core 类型和 trait）

## 具体内容

### 数据结构

```rust
pub struct MemoryStore {
    // 使用 Arc<RwLock<...>> 支持并发访问
    sessions: Arc<RwLock<HashMap<SessionId, Vec<Event>>>>,
}
```

### 实现要点

1. `create_session` — 创建新的空事件列表，返回 SessionId
2. `append_event` — 构建完整 Event（自动填充 event_id、timestamp），追加到对应会话
3. `get_events` — 支持 EventFilter 过滤（按类型、按范围）
4. `latest_event_id` — 返回最后一个事件的 event_id
5. 所有方法使用 `tokio::sync::RwLock`，读多写少场景友好

### 测试用例

- 创建会话 → 追加事件 → 读取事件 → 验证顺序和内容
- 过滤查询 — 按事件类型过滤
- 并发读写 — 多个 tokio task 同时操作
- 错误场景 — 查询不存在的 session_id

## 验收标准

- [ ] `cargo test -p lattice-store-memory` 通过
- [ ] `cargo clippy -p lattice-store-memory` 零警告
- [ ] 至少 4 个测试用例覆盖上述场景
- [ ] 所有 pub 类型和方法有英文 doc comment
