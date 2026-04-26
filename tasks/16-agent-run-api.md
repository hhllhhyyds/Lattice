# 任务 16：任务提交与 Agent 执行 API

> ⚠️ **STATUS: DRAFT — 未经人工 review，内容可能调整。**

## 目标

实现核心业务端点：客户端通过 HTTP 提交用户消息，服务端异步启动 ControlLoop 执行 Agent 任务，客户端可轮询获取结果。

## 分支

`feat/agent-run-api`

## 依赖

- 任务 14（会话管理 API）
- 任务 15（工具系统 — ToolSet 提供工具注册和执行）
- 任务 8-10（LLM 后端，至少一个可用）
- 任务 12（facade crate — feature flags 控制 provider 编译）

## 具体内容

### 1. API 端点

#### POST /v1/sessions/:id/messages — 提交消息并触发执行

请求体：
```json
{
    "content": "列出当前目录的文件并解释每个文件的用途",
    "provider": "openai",
    "model": "gpt-4o",
    "system_prompt": "You are a helpful assistant with access to bash."
}
```

- `content`（必填）：用户消息
- `provider`（可选）：LLM provider，默认使用服务器配置
- `model`（可选）：模型名称
- `system_prompt`（可选）：系统提示词

响应 202 Accepted：
```json
{
    "session_id": "uuid",
    "run_id": "uuid",
    "status": "running",
    "message": "Agent task started"
}
```

**实现逻辑**：
1. 验证 session 存在
2. 检查是否已有 running 的任务（同一会话不允许并发执行）
3. 向 SessionStore 追加 `UserMessage` 事件
4. 创建 LLMClient 实例（根据 provider 参数或默认配置）
5. 创建 SandboxRouter（使用 BasicSandboxRouter + LocalSandbox）
6. spawn tokio task 运行 `ControlLoop::run()`
7. 将 RunHandle 存入 `AppState::active_runs`
8. 立即返回 202

#### GET /v1/sessions/:id/messages — 获取会话消息

返回会话中的 `UserMessage` 和 `FinalAnswer` 事件（面向用户的对话视图）：

响应 200：
```json
{
    "messages": [
        {
            "role": "user",
            "content": "列出当前目录的文件",
            "timestamp": "..."
        },
        {
            "role": "assistant",
            "content": "当前目录包含以下文件...",
            "timestamp": "..."
        }
    ]
}
```

#### GET /v1/sessions/:id/status — 查询执行状态

响应 200：
```json
{
    "session_id": "uuid",
    "run_status": "running | completed | failed | idle",
    "run_started_at": "...",
    "run_completed_at": null,
    "event_count": 15,
    "latest_event": {
        "event_id": "uuid",
        "actor": "LLM",
        "payload_type": "ToolCallRequested",
        "timestamp": "..."
    }
}
```

### 2. ControlLoop 异步执行管理

关键设计点：

```rust
async fn start_agent_run(
    state: &AppState,
    session_id: SessionId,
    llm_client: Arc<dyn LLMClient>,
    system_prompt: String,
) -> Result<RunHandle, AppError> {
    // 检查是否已有运行中的任务
    if state.is_session_running(session_id) {
        return Err(AppError::Conflict("Session already has a running task"));
    }

    let store = state.store.clone();
    let sandbox = Arc::new(LocalSandbox::new());
    let tools = Arc::new(ToolSet::with_defaults(sandbox));

    let join_handle = tokio::spawn(async move {
        let control_loop = ControlLoop::new(store, llm_client, tools);
        control_loop.run(session_id).await
    });

    // 注册 RunHandle
    let handle = RunHandle::new(session_id, join_handle);
    state.register_run(handle.clone());

    // spawn 后台任务监控完成状态
    tokio::spawn(monitor_run_completion(state.clone(), session_id, join_handle));

    Ok(handle)
}
```

### 3. 默认工具注册

通过 `ToolSet::with_defaults()` 自动注册所有按 feature 启用的工具（见任务 15）。不再需要手动构建 `Vec<ToolDescription>`。

### 4. LLM Provider 工厂

简单的工厂函数，根据请求参数或默认配置创建 LLMClient：

```rust
fn create_llm_client(
    provider: &str,
    api_key: &str,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<Arc<dyn LLMClient>, AppError>
```

**注意**：API key 从服务器环境变量读取，不从客户端请求传入（安全考虑）。客户端只指定 provider 和 model。

## 验收标准

- [ ] POST /v1/sessions/:id/messages 可触发 Agent 异步执行
- [ ] GET /v1/sessions/:id/messages 返回对话历史
- [ ] GET /v1/sessions/:id/status 正确反映运行状态
- [ ] 同一会话不允许并发执行（返回 409 Conflict）
- [ ] Agent 完成后 RunHandle 状态正确更新
- [ ] 端到端测试：提交任务 → 轮询状态 → 获取结果
- [ ] 支持通过请求参数切换 provider/model
- [ ] 错误场景覆盖：会话不存在、并发冲突、LLM 调用失败
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] `cargo clippy` 零警告

## 设计说明

- **为什么 202 而非 200？** Agent 任务可能耗时数十秒到数分钟，同步等待不合理。202 表示"已接受，正在处理"。
- **为什么 API key 不从客户端传？** 安全最佳实践。API key 是服务器配置，不通过网络传输。多租户场景下用认证 + 授权替代。
- **为什么同一会话不并发？** ControlLoop 假设事件流是线性的，并发写入会导致状态混乱。如果需要并行，应创建多个会话。
