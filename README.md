# Lattice

[![codecov](https://codecov.io/gh/hhllhhyyds/lattice/branch/main/graph/badge.svg)](https://codecov.io/gh/hhllhhyyds/lattice)

**Agent 元框架** — 将 AI Agent 的"大脑"与"双手"彻底解耦。

## 什么是 Lattice？

Lattice 是一个 Rust 编写的 Agent 元框架，灵感来自 [Anthropic Managed Agents](https://www.anthropic.com/engineering/managed-agents) 的架构设计。

核心思想：Agent 的推理决策（大脑）和工具执行（双手）应该是独立的、可替换的组件，通过稳定的接口通信。

## 架构

### 演进路线（详细见 [ROADMAP](docs/ROADMAP.md)）

| 轮次 | 内容 | 状态 |
|---|---|---|
| 第一轮 | MVP：core traits + MemoryStore + LocalSandbox + ControlLoop | ✅ 已完成 |
| 第二轮 | 真实 LLM 接入：LLM protocol + Anthropic + OpenAI | ✅ 已完成 |
| 第三轮 | 真实 LLM 验证：端到端跑通 | ✅ 已完成 |
| 第四轮 | HTTP API 层：REST API + SSE 实时事件流 + Docker 部署 | 🚧 进行中 |
| 第五轮 | Skill 系统：ExecutionContext + Session Tree + SkillTool + 动态加载 | 📋 规划中 |
| 第六轮 | 记忆与规划：RuntimeState + Planner trait + LongTermMemory | 📋 规划中 |

### 核心抽象

- **Session（记忆层）** — 不可变的事件溯源日志，append-only，保证可追溯性
- **ControlLoop（执行层）** — Agent 的大脑，调用 LLM 并路由决策
- **Sandbox（执行层）** — Agent 的双手，隔离的工具执行环境

三层工具体系：

- **Layer 1** — `lattice-core`：纯接口（`ToolExecutor` trait）
- **Layer 2** — `lattice-tools`：标准工具库（Bash、File、Glob、Grep、HTTP）
- **Layer 3** — 应用层注入：自定义工具、MCP 桥接、Skill 工具集

**Skill 系统**（第五轮）遵循 [Anthropic Agent Skills](https://www.agentskills.com) 开放标准，skill 作为普通工具调用，背后运行完整子 ControlLoop，实现渐进式披露三层加载。

## 快速开始

```bash
# Mock LLM（无需 API key）
cargo run --example hello-agent

# 真实 LLM
LATTICE_LLM_PROVIDER=anthropic LATTICE_API_KEY=sk-ant-xxx \
  LATTICE_MODEL=claude-sonnet-4-6 \
  cargo run --example real-agent -- "What is 2+2?"

# OpenAI 兼容（包括 MiniMax、vLLM、Ollama 等）
LATTICE_API_KEY=sk-xxx LATTICE_API_BASE=http://localhost:8000/v1 \
  cargo run --example real-agent -- "List files"

# Codex CLI login (uses your ChatGPT/Codex login, no API key)
# First run `codex` and choose "Sign in with ChatGPT"
LATTICE_LLM_PROVIDER=codex \
  cargo run --example real-agent -- "What is 2+2?"
```

## 平台支持

Lattice 支持以下平台：
- Linux (x86_64, aarch64)
- macOS (Intel, Apple Silicon)
- Windows 10/11 (x86_64)

### Windows 用户注意事项

在 Windows 上，环境变量设置语法不同：

**PowerShell**：
```powershell
$env:LATTICE_LLM_PROVIDER="anthropic"
$env:LATTICE_API_KEY="sk-ant-xxx"
$env:LATTICE_MODEL="claude-sonnet-4-6"
cargo run --example real-agent -- "What is 2+2?"
```

**Codex CLI login (PowerShell)**:
```powershell
# First make sure Codex CLI is signed in.
codex

$env:LATTICE_LLM_PROVIDER="codex"
Remove-Item Env:LATTICE_MODEL -ErrorAction SilentlyContinue
cargo run --example real-agent -- "What is 2+2?"
```

Codex CLI mode runs local `codex exec`, so authentication, token refresh, model availability, and the ChatGPT/Codex backend protocol are handled by Codex CLI. This mode defaults to `gpt-5.5`; `gpt-4o` is not a supported model name for Codex CLI when using a ChatGPT/Codex account.

**CMD**：
```cmd
set LATTICE_LLM_PROVIDER=anthropic
set LATTICE_API_KEY=sk-ant-xxx
set LATTICE_MODEL=claude-sonnet-4-6
cargo run --example real-agent -- "What is 2+2?"
```

**开发脚本**：
- Unix/Linux/macOS: `./scripts/check.sh`
- Windows: `.\scripts\check.ps1`

## HTTP API Server

```bash
cargo run -p lattice-server
curl http://localhost:3000/health
```

启动后直接打开浏览器访问：

```text
http://127.0.0.1:3000
```

当前主干内置了一个最小可用 Web UI，可用于：

- 创建 session
- 提交消息并触发 agent run
- 查看消息历史
- 查看事件列表
- 查看运行状态

Web UI 现在已经接入会话级 **SSE 实时事件流**：

- 进入会话时会先回放已有历史事件
- 新事件会实时推送到页面
- 消息区、事件区、状态区会自动更新
- 不需要前端持续轮询 `status` / `events` / `messages`

如果要让 Web UI 真正调用模型，需要先配置服务端使用的 LLM 环境变量，例如：

```powershell
$env:LATTICE_LLM_PROVIDER="openai"
$env:OPENAI_API_KEY="sk-xxx"
$env:LATTICE_MODEL="gpt-4o"
cargo run -p lattice-server
```

### Web UI 使用说明

Web UI 底部的发送面板里：

- `Provider` 是**可选项**
- `模型` 是**可选项**

它们的行为是：

- **不填写 `Provider` / `模型`**  
  本次请求直接使用服务端启动时的默认环境变量
- **填写 `Provider` / `模型`**  
  仅覆盖这一次请求的 provider / model

也就是说，如果你在启动 `lattice-server` 前已经设置好了：

```powershell
$env:LATTICE_LLM_PROVIDER="openai"
$env:LATTICE_API_BASE="https://api.siliconflow.cn/v1"
$env:LATTICE_MODEL="Pro/MiniMaxAI/MiniMax-M2.5"
```

那么进入 Web UI 后，`Provider` 和 `模型` 两个输入框都可以留空，直接发送消息即可。

### Web UI 实时效果

启动 `lattice-server` 后，打开浏览器：

```text
http://127.0.0.1:3000
```

然后：

1. 创建一个 session
2. 发送一条消息
3. 直接观察页面

你会看到：

- 消息区自动追加 user / assistant 消息
- 事件区实时出现 `userMessage`、`thinking`、`toolCallRequested`、`toolCallResult`、`finalAnswer`
- 状态从 `running` 自动变成 `completed`

这套刷新机制来自后端的 `/v1/sessions/:id/stream` SSE 接口，而不是前端定时轮询。

### MiniMax / SiliconFlow 示例

Lattice 当前通过 **OpenAI-compatible** 适配器接入 MiniMax、SiliconFlow、vLLM、Ollama 等服务。

这里的：

```text
LATTICE_LLM_PROVIDER="openai"
```

表示“使用 OpenAI 兼容请求格式”，**不是**要求你使用 OpenAI 官方服务。

例如，通过 SiliconFlow 调用 MiniMax：

```powershell
$env:LATTICE_LLM_PROVIDER="openai"
$env:LATTICE_API_KEY="your_key"
$env:LATTICE_API_BASE="https://api.siliconflow.cn/v1"
$env:LATTICE_MODEL="Pro/MiniMaxAI/MiniMax-M2.5"
$env:LATTICE_PORT="3001"

cargo run -p lattice-server
```

然后打开：

```text
http://127.0.0.1:3001
```

如果服务端已经按上面方式启动，Web UI 中推荐：

- `Provider` 留空（使用服务端默认值）
- `模型` 留空，或显式填写 `Pro/MiniMaxAI/MiniMax-M2.5`

### MCP 配置接入

`real-agent` 和 `lattice-server` 现在都支持在启动时加载 MCP 配置，并把 MCP tools 注入 `ToolSet`。

当前入口是环境变量：

```text
LATTICE_MCP_CONFIG=/path/to/mcp.json
```

配置文件格式直接复用 `mcpServers` JSON。当前支持：

- `stdio`
- `http`
- `ws`

`stdio` 示例：

```json
{
  "mcpServers": {
    "fixture": {
      "type": "stdio",
      "command": "python",
      "args": ["./path/to/server.py"]
    }
  }
}
```

远端 MCP 示例，支持 `bearer_token` 和自定义 headers：

```json
{
  "mcpServers": {
    "remote-http": {
      "type": "http",
      "url": "https://mcp.example.com/mcp",
      "bearer_token": "your_token",
      "headers": {
        "x-client-id": "lattice"
      }
    },
    "remote-ws": {
      "type": "ws",
      "url": "wss://mcp.example.com/mcp",
      "bearer_token": "your_token",
      "headers": {
        "x-client-id": "lattice"
      }
    }
  }
}
```

配置约束：

- `bearer_token` 会自动写入 `Authorization: Bearer ...`
- 如果已经配置 `bearer_token`，就不要再在 `headers` 里手动填写 `Authorization`
- 远端 `http/ws` 连接失败时只影响对应 server，不会阻塞其他 MCP server 启动

`real-agent` 示例：

```powershell
$env:LATTICE_LLM_PROVIDER="openai"
$env:LATTICE_API_KEY="your_key"
$env:LATTICE_MODEL="gpt-4o"
$env:LATTICE_MCP_CONFIG="D:\\path\\to\\mcp.json"

cargo run -p real-agent -- "Use the MCP tools to inspect available resources"
```

`lattice-server` 示例：

```powershell
$env:LATTICE_LLM_PROVIDER="openai"
$env:LATTICE_API_KEY="your_key"
$env:LATTICE_MODEL="gpt-4o"
$env:LATTICE_MCP_CONFIG="D:\\path\\to\\mcp.json"

cargo run -p lattice-server
```

行为说明：

- 如果未设置 `LATTICE_MCP_CONFIG`，系统按无 MCP 配置启动
- 如果某个 MCP server 连接失败，其余 server 仍继续工作
- `GET /health` 会返回 `mcp_servers` 快照，包含 `state / transport / tool_count / resource_count`

## LLM Provider 支持

| Provider         | 包                      | 状态 |
| ---------------- | ----------------------- | ---- |
| Anthropic Claude | `lattice-llm-anthropic` | ✅    |
| OpenAI 兼容      | `lattice-llm-openai`    | ✅    |
| 自定义 base URL  | via `LATTICE_API_BASE`  | ✅    |

## 文档

- [架构设计](docs/ARCHITECTURE.md)
- [技术选型](docs/TECH_STACK.md)
- [Roadmap](docs/ROADMAP.md)
- [AI 编程流程](docs/AI_WORKFLOW.md)

## License

MIT
