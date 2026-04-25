# 任务 9：实现 Anthropic (Claude) 后端

## 目标

在 `lattice-llm-anthropic` crate 中实现 `LLMClient` trait 的 Anthropic Claude 版本。

## 分支

`feat/llm-anthropic`

## 依赖

- 任务 8（llm-protocol 协议层）

## Crate 信息

- 名称：`lattice-llm-anthropic`
- 路径：`crates/llm-anthropic/`
- 依赖：`lattice-core`、`lattice-llm-protocol`、`reqwest`、`serde`、`serde_json`、`tokio`

## 具体内容

### 实现要点

1. `AnthropicClient` struct：持有 API key、base URL、model name、max_tokens 等配置
2. 实现 `LLMClient` trait：
   - 使用 `lattice-llm-protocol` 的 convert 模块将 `Event[]` 转换为通用 `Message[]`
   - 将通用 `Message[]` 序列化为 Anthropic Messages API 格式
   - 通过 reqwest 发送 HTTP 请求到 `https://api.anthropic.com/v1/messages`
   - 解析 Anthropic 响应（content blocks: text / tool_use）为通用 `LLMResponse`
   - 使用 `lattice-llm-protocol` 的 parse 模块将 `LLMResponse` 转换为 `Decision`
3. 支持 tool_use：将 `ToolDescription[]` 转换为 Anthropic 的 tools 参数格式
4. 错误处理：网络错误、API 错误（rate limit、auth）、响应解析错误
5. 支持配置 base URL（兼容代理和本地部署）

### Anthropic API 格式要点

- Header: `x-api-key`、`anthropic-version: 2023-06-01`、`content-type: application/json`
- Request body: `model`、`max_tokens`、`system`、`messages`、`tools`
- Response: `content` 数组，每个元素有 `type`（"text" / "tool_use"）
- tool_use block: `{ type: "tool_use", id, name, input }`

### 测试用例

- 请求序列化：验证构建的 HTTP body 符合 Anthropic API 格式
- 响应解析：模拟 Anthropic text 响应 → FinalAnswer
- 响应解析：模拟 Anthropic tool_use 响应 → ToolCall
- 错误处理：模拟 API 错误响应 → LLMError
- 注意：真实 API 调用测试标记为 `#[ignore]`，需手动设置 API key 才跑

## 验收标准

- [ ] `cargo build -p lattice-llm-anthropic` 通过
- [ ] `cargo test -p lattice-llm-anthropic` 通过
- [ ] `cargo clippy -p lattice-llm-anthropic` 零警告
- [ ] 至少 4 个测试用例（不含 ignore 的集成测试）
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] 支持自定义 base URL
