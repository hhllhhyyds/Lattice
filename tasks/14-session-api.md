# 任务 14：会话管理 API

> ⚠️ **STATUS: DRAFT — 未经人工 review，内容可能调整。**

## 目标

实现会话的 CRUD API，支持通过 HTTP 创建会话、列出会话、查询会话详情和事件历史。这是所有后续 API 的基础。

## 分支

`feat/session-api`

## 依赖

- 任务 13（server 骨架 + AppState）

## 具体内容

### 1. API 端点

#### POST /v1/sessions — 创建会话

请求体（可选）：
```json
{
    "metadata": {
        "name": "my-task",
        "tags": ["test"]
    }
}
```

响应 201：
```json
{
    "session_id": "uuid",
    "created_at": "2026-04-25T10:00:00Z",
    "status": "created",
    "event_count": 1
}
```

实现：调用 `SessionStore::create_session()` → 返回 session_id。

#### GET /v1/sessions — 列出会话

响应 200：
```json
{
    "sessions": [
        {
            "session_id": "uuid",
            "created_at": "...",
            "status": "created | running | completed | failed",
            "event_count": 5
        }
    ]
}
```

**注意**：当前 `SessionStore` trait 没有 `list_sessions` 方法。有两个选择：
- **方案 A**：在 AppState 中维护一个 `sessions: Arc<RwLock<Vec<SessionId>>>` 作为索引
- **方案 B**：扩展 `SessionStore` trait 新增 `list_sessions()` 方法

推荐 **方案 A**（不修改 core trait，保持向后兼容）。如果后续持久化存储自带 list 能力，可以在那时再扩展 trait。

#### GET /v1/sessions/:id — 查询会话详情

响应 200：
```json
{
    "session_id": "uuid",
    "created_at": "...",
    "status": "running",
    "event_count": 12,
    "latest_event_id": "uuid",
    "run_info": {
        "started_at": "...",
        "status": "running"
    }
}
```

响应 404：会话不存在。

#### GET /v1/sessions/:id/events — 查询事件

查询参数：
- `actor`（可选）：过滤特定 actor（System/LLM/Harness/Sandbox）
- `event_type`（可选）：过滤特定事件类型
- `after`（可选）：只返回此 event_id 之后的事件
- `limit`（可选，默认 100）：最大返回数量

响应 200：
```json
{
    "events": [
        {
            "event_id": "uuid",
            "session_id": "uuid",
            "timestamp": "...",
            "actor": "LLM",
            "payload": { "type": "FinalAnswer", "answer": "..." },
            "parent_event_id": null
        }
    ],
    "has_more": false
}
```

### 2. 错误响应格式

统一错误格式：
```json
{
    "error": {
        "code": "session_not_found",
        "message": "Session with id xxx does not exist"
    }
}
```

实现一个 `AppError` 枚举，实现 `IntoResponse`：
- `SessionNotFound` → 404
- `InvalidRequest(String)` → 400
- `InternalError(String)` → 500

### 3. 请求/响应类型

在 `crates/server/src/api/` 下组织：
```
src/
├── main.rs
├── state.rs          # AppState
├── error.rs          # AppError
├── api/
│   ├── mod.rs
│   ├── sessions.rs   # 会话相关 handler
│   └── types.rs      # 请求/响应 DTO
```

### 4. 会话元数据追踪

在 AppState 中新增：
```rust
pub struct SessionInfo {
    pub session_id: SessionId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<SessionMetadata>,
}

pub struct SessionMetadata {
    pub name: Option<String>,
    pub tags: Vec<String>,
}
```

## 验收标准

- [ ] 4 个端点全部实现并可用
- [ ] 创建会话 → 查询会话 → 查询事件全链路跑通
- [ ] 错误格式统一，404/400/500 响应正确
- [ ] 有集成测试覆盖各端点的正常和异常场景
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] `cargo clippy` 零警告
