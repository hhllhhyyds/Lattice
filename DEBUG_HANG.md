# 诊断 Agent 卡死问题

## 问题描述

程序在第一次 LLM 调用后（请求工具调用）卡住，没有后续输出。

## 已添加的诊断日志

### 1. LocalSandbox (sandbox-local/src/local_sandbox.rs)

添加了以下日志：
- `debug!("executing command: {}", cmd_str)` - 显示要执行的命令
- `info!("starting command execution...")` - 命令开始执行
- `info!("command timed out...")` - 命令超时
- `info!("command completed: exit_code=...")` - 命令完成
- `info!("command execution failed: {}")` - 命令失败

### 2. ControlLoop (runtime/src/control_loop.rs)

添加了以下日志：
- `info!("executing tool: {}", tool)` - 开始执行工具
- `info!("tool execution succeeded: exit_code={}")` - 工具执行成功
- `warn!("tool execution failed: {}")` - 工具执行失败
- `info!("tool call completed, continuing loop")` - 工具调用完成

### 3. OpenAIClient (llm-openai/src/client.rs)

添加了以下日志：
- `info!("sending request to OpenAI-compatible API: {} messages, {} tools")` - 发送请求
- `debug!("request payload: {}")` - 请求详情（debug 级别）
- `info!("HTTP request failed: {}")` - HTTP 请求失败
- `info!("received HTTP response: status={}")` - 收到响应

## 如何使用

### 运行带详细日志的 agent

```bash
# 设置环境变量
export LATTICE_API_KEY=your-api-key
export LATTICE_API_BASE=https://api.siliconflow.cn/v1
export LATTICE_MODEL=Pro/MiniMaxAI/MiniMax-M2.5

# 运行带 info 级别日志
RUST_LOG=info cargo run -p real-agent -- "list file"

# 运行带 debug 级别日志（包含请求详情）
RUST_LOG=debug cargo run -p real-agent -- "list file"

# 只看特定模块的日志
RUST_LOG=lattice_sandbox_local=debug,lattice_runtime=info cargo run -p real-agent -- "list file"
```

### Windows PowerShell

```powershell
$env:LATTICE_API_KEY="your-api-key"
$env:LATTICE_API_BASE="https://api.siliconflow.cn/v1"
$env:LATTICE_MODEL="Pro/MiniMaxAI/MiniMax-M2.5"
$env:RUST_LOG="info"

cargo run -p real-agent -- "list file"
```

## 预期的日志流程

正常情况下，你应该看到以下日志序列：

```
INFO real_agent: creating LLM client provider=openai model=...
INFO real_agent: session created session_id=...
INFO real_agent: task submitted, starting agent...
INFO lattice_runtime::control_loop: control loop started
INFO lattice_llm_openai::client: sending request to OpenAI-compatible API: 1 messages, 1 tools
INFO lattice_llm_openai::client: received HTTP response: status=200
INFO lattice_runtime::control_loop: LLM requested tool call tool="cmd"
INFO lattice_runtime::control_loop: executing tool: cmd
INFO lattice_sandbox_local::local_sandbox: starting command execution with timeout of 30 seconds
INFO lattice_sandbox_local::local_sandbox: command completed: exit_code=0, stdout_len=..., stderr_len=...
INFO lattice_runtime::control_loop: tool execution succeeded: exit_code=0
INFO lattice_runtime::control_loop: tool call completed, continuing loop
INFO lattice_llm_openai::client: sending request to OpenAI-compatible API: 3 messages, 1 tools
INFO lattice_llm_openai::client: received HTTP response: status=200
INFO lattice_runtime::control_loop: LLM final answer
INFO lattice_runtime::control_loop: control loop finished
```

## 可能的卡死点

根据日志输出，可以判断卡在哪里：

### 1. 卡在 "LLM requested tool call" 之后

**症状**：看到 `LLM requested tool call tool="cmd"` 但没有 `executing tool` 日志

**原因**：可能是 `append_event` 卡住了（SessionStore 问题）

### 2. 卡在 "executing tool" 之后

**症状**：看到 `executing tool: cmd` 但没有 `starting command execution` 日志

**原因**：可能是 ToolSet 查找工具时卡住了

### 3. 卡在 "starting command execution" 之后

**症状**：看到 `starting command execution` 但没有 `command completed` 或 `command timed out`

**原因**：
- 命令本身卡住（等待输入、死循环等）
- 命令执行时间超过 30 秒
- tokio 运行时问题

### 4. 卡在 "tool call completed" 之后

**症状**：看到 `tool call completed, continuing loop` 但没有下一次 `sending request to OpenAI-compatible API`

**原因**：
- 循环逻辑问题
- 重新加载事件时卡住

### 5. 卡在第二次 LLM 请求

**症状**：看到第二次 `sending request to OpenAI-compatible API` 但没有 `received HTTP response`

**原因**：
- 网络问题
- API 限流
- 请求超时（120 秒）

## 常见问题和解决方案

### 问题 1：命令超时

如果看到 `command timed out after 30 seconds`，说明命令执行时间过长。

**解决方案**：
- 检查 LLM 生成的命令是否合理
- 增加超时时间（修改 LocalSandbox::new() 中的 timeout）

### 问题 2：网络请求失败

如果看到 `HTTP request failed: error sending request`，说明网络有问题。

**解决方案**：
- 检查网络连接
- 检查 API_BASE URL 是否正确
- 检查防火墙设置
- 尝试使用代理

### 问题 3：API 返回错误

如果看到 `received HTTP response: status=4xx` 或 `status=5xx`，说明 API 返回错误。

**解决方案**：
- 检查 API_KEY 是否正确
- 检查 API 配额是否用完
- 查看完整的错误响应（使用 RUST_LOG=debug）

## 下一步

1. 运行带详细日志的 agent
2. 根据日志输出确定卡死点
3. 针对性地解决问题
