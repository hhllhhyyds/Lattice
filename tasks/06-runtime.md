# 任务 6：实现 ControlLoop + BasicSandboxRouter

## 目标

在 `lattice-runtime` 中实现核心决策循环和默认路由器。

## 分支

`feat/runtime`

## 依赖

- 任务 3（core 类型和 trait）

## 具体内容

### ControlLoop

```rust
pub struct ControlLoop {
    session_id: SessionId,
    store: Arc<dyn SessionStore>,
    llm: Arc<dyn LLMClient>,
    router: Arc<dyn SandboxRouter>,
    available_tools: Vec<ToolDescription>,
    system_prompt: String,
}
```

**`run()` 方法的完整决策循环**：

```
loop {
    1. store.get_events(session_id) 加载事件历史
    2. llm.decide(history, tools, system_prompt) 获取决策
    3. store.append_event(DecisionRecorded) 记录决策
    4. match decision:
       - FinalAnswer → append FinalAnswer event, return answer
       - Thinking → continue loop
       - ToolCall → append ToolCallRequested event
                  → router.route(session_id, event_id, tool, params)
                  → continue loop (结果已被 router 写入 store)
}
```

**可配置项**：
- 最大循环次数（防止无限循环），默认 50
- 系统提示词

### BasicSandboxRouter

```rust
pub struct BasicSandboxRouter {
    store: Arc<dyn SessionStore>,
    sandbox: Arc<dyn Sandbox>,
}
```

**`route()` 方法**：
1. 调用 `sandbox.execute(tool, params)`
2. 结果写入 `store.append_event(ToolCallResult 或 ToolCallError)`
3. 返回 ExecutionResult

### 测试用例（使用 mockall mock 所有 trait）

- **正常流程**：mock LLM 返回 ToolCall → FinalAnswer，验证事件序列正确
- **纯思考流程**：mock LLM 返回 Thinking → FinalAnswer，不触发沙箱
- **工具调用失败**：mock Sandbox 返回错误，验证 ToolCallError 事件被记录，LLM 能继续决策
- **最大循环保护**：mock LLM 永远返回 Thinking，验证到达上限后退出

## 验收标准

- [ ] `cargo test -p lattice-runtime` 通过
- [ ] `cargo clippy -p lattice-runtime` 零警告
- [ ] 至少 4 个测试用例覆盖上述场景
- [ ] ControlLoop 不包含任何具体工具执行代码
- [ ] ControlLoop 不直接依赖任何具体的 Store/LLM/Sandbox 实现
- [ ] 所有 pub 类型和方法有英文 doc comment
