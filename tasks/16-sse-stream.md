# 任务 16：SSE 实时事件流

> ⚠️ **STATUS: DRAFT — 未经人工 review，内容可能调整。**

## 目标

实现 Server-Sent Events (SSE) 端点，让客户端能实时订阅会话事件流，观察 Agent 的思考和执行过程。

## 分支

`feat/sse-stream`

## 依赖

- 任务 15（Agent 执行 API — 需要有运行中的任务来产生事件）

## 具体内容

### 1. API 端点

#### GET /v1/sessions/:id/stream — SSE 事件流

查询参数：
- `after`（可选）：只推送此 event_id 之后的事件（用于断线重连）
- `include_history`（可选，默认 false）：是否先推送历史事件

响应头：
```
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
```

事件格式：
```
event: session_event
data: {"event_id":"uuid","timestamp":"...","actor":"LLM","payload":{"type":"Thinking","reasoning":"..."}}

event: session_event
data: {"event_id":"uuid","timestamp":"...","actor":"LLM","payload":{"type":"ToolCallRequested","tool":"bash","params":{...}}}

event: session_event
data: {"event_id":"uuid","timestamp":"...","actor":"Sandbox","payload":{"type":"ToolCallResult","stdout":"...","stderr":"","exit_code":0}}

event: session_event
data: {"event_id":"uuid","timestamp":"...","actor":"LLM","payload":{"type":"FinalAnswer","answer":"..."}}

event: done
data: {"session_id":"uuid","status":"completed"}
```

### 2. 事件发布/订阅机制

需要一个进程内的 pub/sub 机制，让 ControlLoop 产生的事件能推送到 SSE 连接。

**方案：基于 `tokio::sync::broadcast`**

```rust
/// 在 AppState 中新增
pub struct AppState {
    // ... 已有字段
    /// 每个会话的事件广播通道
    pub event_channels: Arc<RwLock<HashMap<SessionId, broadcast::Sender<Event>>>>,
}

impl AppState {
    /// 获取或创建会话的事件通道
    pub fn get_event_channel(&self, session_id: SessionId) -> broadcast::Receiver<Event> {
        // ...
    }

    /// 发布事件到通道（在事件写入 SessionStore 后调用）
    pub fn publish_event(&self, event: &Event) {
        // ...
    }
}
```

**关键问题：如何让 ControlLoop 发布事件？**

ControlLoop 当前直接调用 `SessionStore::append_event()`。有两个选择：

- **方案 A（推荐）**：创建一个 `NotifyingStore` 包装器，实现 `SessionStore` trait，在 `append_event` 时同时发布到 broadcast channel
- **方案 B**：修改 ControlLoop 接口，注入一个事件回调

推荐方案 A，不修改 core 代码：

```rust
pub struct NotifyingStore<S: SessionStore> {
    inner: S,
    channels: Arc<RwLock<HashMap<SessionId, broadcast::Sender<Event>>>>,
}

#[async_trait]
impl<S: SessionStore> SessionStore for NotifyingStore<S> {
    async fn append_event(&self, ...) -> Result<EventId, StoreError> {
        let event_id = self.inner.append_event(...).await?;
        // 发布到 broadcast channel
        self.publish(session_id, &event);
        Ok(event_id)
    }
    // ... 其他方法委托给 inner
}
```

### 3. SSE Handler 实现

```rust
async fn session_stream(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<SessionId>,
    Query(params): Query<StreamParams>,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>, AppError> {
    // 1. 验证会话存在
    // 2. 如果 include_history，先查询历史事件
    // 3. 订阅 broadcast channel
    // 4. 返回 SSE 流：先发历史事件，再发实时事件
    // 5. 收到 FinalAnswer 或 StateChange(to="completed") 后发送 done 事件
}
```

### 4. 连接管理

- **心跳**：每 15 秒发送 SSE comment（`: keepalive`）防止连接超时
- **断线重连**：客户端通过 `Last-Event-ID` 头或 `after` 参数指定断点，服务端从该点继续推送
- **超时清理**：会话完成后 30 秒关闭 SSE 连接
- **通道清理**：会话完成且无活跃订阅者时，清理 broadcast channel

### 5. axum SSE 支持

axum 原生支持 SSE：

```rust
use axum::response::sse::{Event as SseEvent, Sse};
use futures::stream::Stream;
```

## 验收标准

- [ ] GET /v1/sessions/:id/stream 返回 SSE 事件流
- [ ] 提交任务后，SSE 实时推送 Thinking → ToolCall → ToolResult → FinalAnswer
- [ ] `include_history=true` 时先推送历史事件
- [ ] `after` 参数支持断线重连
- [ ] 心跳机制正常工作
- [ ] 任务完成后发送 `done` 事件
- [ ] NotifyingStore 不修改 core crate 代码
- [ ] 有集成测试验证 SSE 流的正确性
- [ ] 多个 SSE 客户端可同时订阅同一会话
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] `cargo clippy` 零警告

## 设计说明

- **为什么 SSE 而不是 WebSocket？** SSE 更简单（单向推送场景），HTTP/2 下性能相当。客户端只需读不需写。浏览器原生支持 `EventSource` API。如果后续需要双向通信（如中途取消），可以通过独立的 POST 端点实现。
- **为什么用 broadcast 而不是 mpsc？** 一个会话可能有多个 SSE 客户端订阅（如调试面板 + CLI），broadcast 天然支持多消费者。
- **NotifyingStore 的优势**：纯装饰器模式，不修改 core trait，不影响已有的 ControlLoop 逻辑，且对 ControlLoop 完全透明。
