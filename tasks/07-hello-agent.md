# 任务 7：实现 hello-agent example

## 目标

端到端跑通完整 Agent 流程，验证三组件解耦架构可行。

## 分支

`feat/hello-agent`

## 依赖

- 任务 4（MemoryStore）
- 任务 5（LocalSandbox）
- 任务 6（ControlLoop + BasicSandboxRouter）

## 具体内容

### MockLLMClient

创建一个硬编码决策序列的 mock LLM：

```rust
pub struct MockLLMClient {
    decisions: Mutex<VecDeque<Decision>>,
}
```

预设决策序列：
1. `ToolCall { tool: "bash", params: { "command": "echo Hello from Lattice!" } }`
2. `FinalAnswer { answer: "命令执行成功，输出: Hello from Lattice!" }`

### main.rs 流程

```rust
#[tokio::main]
async fn main() {
    // 1. 创建组件
    let store = Arc::new(MemoryStore::new());
    let llm = Arc::new(MockLLMClient::new(vec![...]));
    let sandbox = Arc::new(LocalSandbox::new(None, Duration::from_secs(30)));
    let router = Arc::new(BasicSandboxRouter::new(store.clone(), sandbox));

    // 2. 创建会话
    let session_id = store.create_session().await?;

    // 3. 追加用户任务
    store.append_event(session_id, EventPayload::UserMessage {
        content: "Run 'echo Hello from Lattice!' and tell me the output.".into(),
    }, Actor::System, None).await?;

    // 4. 启动 ControlLoop
    let agent = ControlLoop::new(session_id, store.clone(), llm, router, tools, system_prompt);
    let answer = agent.run().await?;

    // 5. 打印结果
    println!("Agent answer: {}", answer);

    // 6. 打印完整事件日志
    let events = store.get_events(session_id, &EventFilter::default()).await?;
    for event in &events {
        println!("{:?}", event);
    }
}
```

### 预期输出

```
Agent answer: 命令执行成功，输出: Hello from Lattice!

Event log:
[SessionCreated]
[UserMessage] Run 'echo Hello from Lattice!' and tell me the output.
[DecisionRecorded] ToolCall { bash, echo Hello from Lattice! }
[ToolCallRequested] bash, echo Hello from Lattice!
[ToolCallResult] stdout: "Hello from Lattice!\n", exit_code: 0
[DecisionRecorded] FinalAnswer { ... }
[FinalAnswer] 命令执行成功，输出: Hello from Lattice!
```

## 验收标准

- [ ] `cargo run --example hello-agent` 成功执行
- [ ] 控制台输出完整事件流
- [ ] 事件顺序正确，因果链（parent_event_id）正确
- [ ] 替换 MemoryStore 为另一个 SessionStore 实现不需要修改 ControlLoop 代码
- [ ] 替换 LocalSandbox 为另一个 Sandbox 实现不需要修改 ControlLoop 代码
