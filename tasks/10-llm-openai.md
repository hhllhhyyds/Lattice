# 任务 10：实现 OpenAI 兼容后端

## 目标

在 `lattice-llm-openai` crate 中实现 `LLMClient` trait 的 OpenAI 兼容版本，支持 OpenAI 官方 API 及所有兼容接口（如本地部署、第三方代理）。

## 分支

`feat/llm-openai`

## 依赖

- 任务 8（llm-protocol 协议层）

## Crate 信息

- 名称：`lattice-llm-openai`
- 路径：`crates/llm-openai/`
- 依赖：`lattice-core`、`lattice-llm-protocol`、`reqwest`、`serde`、`serde_json`、`tokio`

## 具体内容

### 实现要点

1. `OpenAIClient` struct：持有 API key、base URL、model name、max_tokens 等配置
2. 实现 `LLMClient` trait：
   - 使用 `lattice-llm-protocol` 的 convert 模块将 `Event[]` 转换为通用 `Message[]`
   - 将通用 `Message[]` 序列化为 OpenAI Chat Completions API 格式
   - 通过 reqwest 发送 HTTP 请求
   - 解析 OpenAI 响应（choices[0].message.content / tool_calls）为通用 `LLMResponse`
   - 使用 `lattice-llm-protocol` 的 parse 模块将 `LLMResponse` 转换为 `Decision`
3. 支持 tool_calls：将 `ToolDescription[]` 转换为 OpenAI 的 tools 参数格式（function calling）
4. 支持配置 base URL（兼容本地部署如 vLLM、Ollama，以及第三方代理）
5. 错误处理：网络错误、API 错误、响应解析错误

### OpenAI API 格式要点

- Header: `Authorization: Bearer <key>`、`content-type: application/json`
- Request body: `model`、`max_tokens`、`messages`（含 system role）、`tools`
- Response: `choices[0].message.content` 和/或 `choices[0].message.tool_calls`
- tool_calls: `[{ id, type: "function", function: { name, arguments } }]`

### 测试用例

- 请求序列化：验证构建的 HTTP body 符合 OpenAI API 格式
- 响应解析：模拟 text 响应 → FinalAnswer
- 响应解析：模拟 tool_calls 响应 → ToolCall
- 错误处理：模拟 API 错误响应 → LLMError
- 注意：真实 API 调用测试标记为 `#[ignore]`

## 验收标准

- [ ] `cargo build -p lattice-llm-openai` 通过
- [ ] `cargo test -p lattice-llm-openai` 通过
- [ ] `cargo clippy -p lattice-llm-openai` 零警告
- [ ] 至少 4 个测试用例（不含 ignore 的集成测试）
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] 支持自定义 base URL
