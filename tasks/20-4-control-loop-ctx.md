# 任务 20-4：ControlLoop 构造 ExecutionContext

## 目标

更新 `ControlLoop`，引入 `depth` 字段和 builder pattern，在 dispatch 工具调用时构造并透传 `ExecutionContext`。

## 分支

`feat/skill-control-loop`

## 依赖

- 任务 20-1（ExecutionContext 定义）
- 任务 20-3（ToolSet::execute 新签名）

---

## 修改内容

### `crates/runtime/src/control_loop.rs`

**结构体新增字段**：

```rust
pub struct ControlLoop {
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    tools: Arc<ToolSet>,
    system_prompt: String,
    max_iterations: usize,
    depth: u32,  // 新增，默认 0
}
```

**`new()` 默认 depth=0**：

```rust
pub fn new(
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    tools: Arc<ToolSet>,
) -> Self {
    Self {
        store,
        llm,
        tools,
        system_prompt: "You are a helpful agent.".to_string(),
        max_iterations: DEFAULT_MAX_ITERATIONS,
        depth: 0,
    }
}
```

**新增 builder 方法**：

```rust
/// Set the skill nesting depth (used when spawning child ControlLoop instances).
pub fn depth(mut self, depth: u32) -> Self {
    self.depth = depth;
    self
}

impl ControlLoop {
    /// Fluent builder for ControlLoop.
    pub fn builder() -> ControlLoopBuilder {
        ControlLoopBuilder::new()
    }
}

pub struct ControlLoopBuilder {
    store: Option<Arc<dyn SessionStore>>,
    llm: Option<Arc<dyn LLMClient>>,
    tools: Option<Arc<ToolSet>>,
    system_prompt: Option<String>,
    max_iterations: Option<usize>,
    depth: Option<u32>,
}

impl ControlLoopBuilder {
    pub fn new() -> Self { Self {
        store: None, llm: None, tools: None,
        system_prompt: None, max_iterations: None, depth: None,
    } }

    pub fn store(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.store = Some(store); self
    }
    pub fn llm(mut self, llm: Arc<dyn LLMClient>) -> Self {
        self.llm = Some(llm); self
    }
    pub fn tools(mut self, tools: Arc<ToolSet>) -> Self {
        self.tools = Some(tools); self
    }
    pub fn system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = Some(prompt.to_string()); self
    }
    pub fn depth(mut self, depth: u32) -> Self {
        self.depth = Some(depth); self
    }

    pub fn build(self) -> ControlLoop {
        ControlLoop {
            store: self.store.expect("store required"),
            llm: self.llm.expect("llm required"),
            tools: self.tools.expect("tools required"),
            system_prompt: self.system_prompt.unwrap_or_else(|| "You are a helpful agent.".to_string()),
            max_iterations: self.max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS),
            depth: self.depth.unwrap_or(0),
        }
    }
}
```

**dispatch 工具时构造 ctx**：

在 `Decision::ToolCall` 分支中：

```rust
let ctx = ExecutionContext {
    session_id,
    trigger_event_id: req_event_id,
    store: self.store.clone(),
    depth: self.depth,
};

match self.tools.execute(&tool, params, &ctx).await {
    Ok(result) => { /* record ToolCallResult */ }
    Err(e)     => { /* record ToolCallError  */ }
}
```

---

## 测试要求

### 单元测试（`crates/runtime/src/control_loop.rs`）

```rust
#[test]
fn builder_default_depth_is_zero() {
    let builder = ControlLoopBuilder::new();
    assert_eq!(builder.depth, None);
    let loop_ = builder
        .store(Arc::new(TestStore::new()))
        .llm(Arc::new(TestLLM::new(Decision::FinalAnswer { answer: "x".into() })))
        .tools(Arc::new(ToolSet::new()))
        .build();
    assert_eq!(loop_.depth, 0);
}

#[test]
fn builder_depth_override() {
    let loop_ = ControlLoopBuilder::new()
        .store(Arc::new(TestStore::new()))
        .llm(Arc::new(TestLLM::new(Decision::FinalAnswer { answer: "x".into() })))
        .tools(Arc::new(ToolSet::new()))
        .depth(3)
        .build();
    assert_eq!(loop_.depth, 3);
}

#[tokio::test]
async fn execution_context_passed_to_tool() {
    // Struct to capture the ctx passed to a tool
    static CAPTURED_CTX: std::sync::OnceLock<ExecutionContext> = std::sync::OnceLock::new();

    struct CapturingTool;
    #[async_trait]
    impl ToolExecutor for CapturingTool {
        fn description(&self) -> ToolDescription {
            ToolDescription {
                name: "capture".into(),
                description: "captures ctx".into(),
                parameters_schema: serde_json::json!({}),
            }
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            ctx: &ExecutionContext,
        ) -> Result<ExecutionResult, ToolError> {
            let _ = CAPTURED_CTX.set(ctx.clone());
            Ok(ExecutionResult { stdout: "ok".into(), stderr: String::new(), exit_code: 0 })
        }
    }

    let session_id = SessionId::new_v4();
    let store = Arc::new(TestStore::new());
    store.insert_session(session_id, vec![Event {
        event_id: EventId::new_v4(),
        session_id,
        timestamp: chrono::Utc::now(),
        actor: Actor::System,
        payload: EventPayload::SessionCreated,
        parent_event_id: None,
    }]);

    struct TwoStepLLM(Arc<Mutex<bool>>);
    #[async_trait]
    impl LLMClient for TwoStepLLM {
        async fn decide(&self, _h: &[Event], _t: &[ToolDescription], _s: &str)
            -> Result<Decision, LLMError>
        {
            let mut called = self.0.lock().unwrap();
            if !*called {
                *called = true;
                Ok(Decision::ToolCall { tool: "capture".into(), params: serde_json::json!({}) })
            } else {
                Ok(Decision::FinalAnswer { answer: "done".into() })
            }
        }
    }

    let mut tools = ToolSet::new();
    tools.register(CapturingTool).unwrap();

    let loop_ = ControlLoop::builder()
        .store(store.clone())
        .llm(Arc::new(TwoStepLLM::new()))
        .tools(Arc::new(tools))
        .depth(5)
        .build();

    loop_.run(session_id).await.unwrap();

    let ctx = CAPTURED_CTX.get().expect("ctx was captured");
    assert_eq!(ctx.depth, 5);
    assert_eq!(ctx.session_id, session_id);
}
```

---

## 验收标准

- [ ] `ControlLoop` 新增 `depth` 字段（默认 0）
- [ ] `ControlLoopBuilder` 实现，支持 `depth()` builder 方法
- [ ] `ControlLoop::builder()` 可用
- [ ] dispatch 时 `ExecutionContext` 构造正确，`depth` 字段透传
- [ ] 上述单元测试全部通过
- [ ] `cargo fmt` + `cargo clippy` 通过
