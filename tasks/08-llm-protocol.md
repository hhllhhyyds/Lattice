# 任务 8：LLM 通用协议层设计

## 目标

在 `lattice-llm-protocol` crate 中实现 LLM 请求/响应的通用抽象层，屏蔽不同 LLM 提供商的 API 差异。

## 分支

`feat/llm-protocol`

## 背景

真实 LLM 返回的是自然语言文本或 tool_use 结构，不是 `Decision` 枚举。需要一个协议层负责：
1. 把 Lattice 的 `Event[]` 历史转换成 LLM 能理解的 messages 格式
2. 把 LLM 的原始响应解析成 `Decision` 枚举
3. 定义通用的 HTTP 请求/响应结构

## Crate 信息

- 名称：`lattice-llm-protocol`
- 路径：`crates/llm-protocol/`
- 依赖：`lattice-core`、`serde`、`serde_json`、`reqwest`

## 具体内容

### 文件结构

```
crates/llm-protocol/src/
├── lib.rs          # 模块声明 + re-export
├── message.rs      # 通用消息格式（role + content blocks）
├── request.rs      # 通用请求结构（messages、tools、system prompt）
├── response.rs     # 通用响应结构（text / tool_use / error）
├── convert.rs      # Event[] → messages 转换逻辑
└── parse.rs        # 通用响应 → Decision 解析逻辑
```

### 通用消息格式

```rust
/// Role in a conversation.
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A content block within a message.
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

/// A single message in the conversation.
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}
```

### Event 转换逻辑

将 Lattice 的 `Event[]` 转换成 `Vec<Message>`：

| EventPayload | 转换为 |
|---|---|
| UserMessage | Role::User + Text |
| Thinking | Role::Assistant + Text |
| ToolCallRequested | Role::Assistant + ToolUse |
| ToolCallResult | Role::Tool + ToolResult (is_error: false) |
| ToolCallError | Role::Tool + ToolResult (is_error: true) |
| FinalAnswer | Role::Assistant + Text |
| SessionCreated / StateChange | 跳过 |

### 通用请求/响应

```rust
/// A provider-agnostic LLM request.
pub struct LLMRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
}

/// A tool specification for the LLM.
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A provider-agnostic LLM response.
pub enum LLMResponse {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    Mixed { blocks: Vec<ContentBlock> },
    Error { message: String },
}
```

### 响应解析逻辑

`LLMResponse` → `Decision`：

| LLMResponse | Decision |
|---|---|
| Text | FinalAnswer { answer: text } |
| ToolUse | ToolCall { tool: name, params: input } |
| Mixed（含 ToolUse）| ToolCall（取第一个 ToolUse）|
| Mixed（纯 Text）| FinalAnswer |
| Error | 返回 LLMError |

## 测试用例

- Event 历史转换：构造包含各种 EventPayload 的事件序列 → 验证转换后的 messages 结构正确
- 响应解析：Text → FinalAnswer
- 响应解析：ToolUse → ToolCall
- 响应解析：Mixed blocks → 正确提取 ToolCall
- 空历史 / 边界情况

## 验收标准

- [ ] `cargo build -p lattice-llm-protocol` 通过
- [ ] `cargo test -p lattice-llm-protocol` 通过
- [ ] `cargo clippy -p lattice-llm-protocol` 零警告
- [ ] 至少 5 个测试用例
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] 不包含任何 provider 特定逻辑（Anthropic/OpenAI 的差异留给各自 crate）
