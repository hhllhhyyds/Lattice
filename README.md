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
| 第五轮 | Skill 系统：ExecutionContext + Session Tree + SkillTool + 动态加载 | ✅ 已完成 |
| 第六轮 | 记忆与规划：RuntimeState + Planner trait + LongTermMemory | 📋 规划中 |

### 核心抽象

- **Session（记忆层）** — 不可变的事件溯源日志，append-only，保证可追溯性
- **ControlLoop（执行层）** — Agent 的大脑，调用 LLM 并路由决策
- **Sandbox（执行层）** — Agent 的双手，隔离的工具执行环境

三层工具体系：

- **Layer 1** — `lattice-core`：纯接口（`ToolExecutor` trait）
- **Layer 2** — `lattice-tools`：标准工具库（Bash、File、Glob、Grep、HTTP）
- **Layer 3** — 应用层注入：自定义工具、MCP 桥接、Skill 工具集

**Skill 系统**遵循 [Anthropic Agent Skills](https://www.agentskills.com) 开放标准，skill 作为普通工具调用，背后运行完整子 ControlLoop，实现渐进式披露三层加载。每个 skill 对应 `skills/<name>/SKILL.md`，由 `SkillLoader` 在启动时动态加载并注册为工具（名称格式 `skill__<name>`）。

## 快速开始

```bash
# Mock LLM（无需 API key）
cargo run --example hello-agent

# 真实 LLM —— Anthropic 原生 / 兼容协议（如 DeepSeek anthropic 端点）
LATTICE_LLM_PROVIDER=anthropic \
  LATTICE_API_KEY=sk-ant-xxx \
  LATTICE_ANTHROPIC_API_BASE=https://api.anthropic.com \
  LATTICE_MODEL=claude-sonnet-4-6 \
  cargo run -p real-agent -- "What is 2+2?"

# 真实 LLM —— OpenAI 兼容协议（MiniMax、vLLM、Ollama、DeepSeek 等）
LATTICE_LLM_PROVIDER=openai \
  LATTICE_API_KEY=sk-xxx \
  LATTICE_OPENAI_API_BASE=http://localhost:8000/v1 \
  LATTICE_OPENAI_MODEL=gpt-4o \
  cargo run -p real-agent -- "List files"
```

> 想要批量本地测试？复制 `.env.example` 为 `.env` 填入密钥，然后跑 `bash scripts/test-local.sh` 一键覆盖单测 + 真模型集成测试 + real-agent。

### 环境变量约定

所有二进制（`real-agent`、`lattice-server`、e2e 测试）使用同一套 `LATTICE_*` 命名。**严格模式 — 没有任何默认值，缺失的变量会立刻报错**（`real-agent` / e2e 测试在启动时；`lattice-server` 在第一个未指定 provider/model 的请求到达时）。

| 变量 | 何时必须设置 | 说明 |
|------|--------------|------|
| `LATTICE_LLM_PROVIDER` | 总是 | `anthropic` 或 `openai` |
| `LATTICE_API_KEY` | 总是 | 也接受 `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` |
| `LATTICE_ANTHROPIC_API_BASE` | provider=anthropic | 也接受 `ANTHROPIC_API_BASE` / `LATTICE_API_BASE` |
| `LATTICE_OPENAI_API_BASE` | provider=openai | 也接受 `OPENAI_API_BASE` / `LATTICE_API_BASE` |
| `LATTICE_MODEL` | anthropic 必填；openai 时若未设 `LATTICE_OPENAI_MODEL` 则必填 | Anthropic 唯一模型来源；OpenAI 的 fallback |
| `LATTICE_OPENAI_MODEL` | openai 时若未设 `LATTICE_MODEL` 则必填 | 仅 openai 读取，优先级高于 `LATTICE_MODEL` |
| `LATTICE_MCP_CONFIG` | 可选 | MCP JSON 配置路径 |

## Skill 系统

Skill 是一个独立的 Agent，封装为普通工具供父 Agent 调用。调用时会在子 Session 中运行完整的 ControlLoop，结果以事件树的形式保存。

### 目录结构

`skills/` 目录放在**项目根目录**下（与 `src/`、`crates/` 同级）：

```
Lattice/                  ← 项目根目录
├── skills/               ← skill 文件放这里
│   ├── arcgen-pipeline/
│   │   └── SKILL.md
│   ├── code-review/
│   │   └── SKILL.md
│   └── my-skill/
│       └── SKILL.md
├── src/
├── crates/
└── examples/
```

Lattice 在启动时扫描 `skills/` 目录，每个子目录必须包含一个 `SKILL.md`。

### SKILL.md 格式

```markdown
---
name: web-research                          # 必填，唯一标识，≤64 字符
description: >-                             # 必填，工具描述，≤1024 字符（LLM 选工具时看这个）
  Deep research on a topic using web search.
  Use when the user asks to research a subject.
compatibility: Requires internet access     # 可选，运行环境约束说明
allowed-tools:                              # 可选，子 Agent 可用的工具白名单
  - bash
  - http_fetch
metadata:                                   # 可选
  author: lattice
  version: "1.0.0"
  tags: [research, web]
x-lattice:                                  # 可选，Lattice 扩展
  params:                                   # 声明结构化入参（否则只有 input 字段）
    depth:
      type: integer
      description: Number of search iterations (1-5)
      required: false
      default: 3
---

# 系统提示

这里写 Skill Agent 的 system prompt，告诉它如何完成任务。
```

**字段说明：**

| 字段 | 必填 | 说明 |
|------|------|------|
| `name` | 是 | skill 的唯一 ID，最终工具名为 `skill__<name>` |
| `description` | 是 | 父 Agent 选择该 skill 时看到的描述 |
| `allowed-tools` | 否 | 子 Agent 可调用的工具白名单；省略则继承父 Agent 的全部工具 |
| `x-lattice.params` | 否 | 声明结构化入参；省略时只有一个 `input` 字符串字段 |

### 运行 meta-agent（带 Skill）

```bash
# 使用 mock LLM（无需 API key，验证 skill 加载逻辑）
cargo run -p meta-agent

# 使用真实 LLM
LATTICE_LLM_PROVIDER=anthropic \
  LATTICE_API_KEY=sk-ant-xxx \
  LATTICE_ANTHROPIC_API_BASE=https://api.anthropic.com \
  LATTICE_MODEL=claude-sonnet-4-6 \
  cargo run -p meta-agent
```

meta-agent 启动时会扫描项目根目录的 `skills/`，将每个 skill 注册为工具后运行。日志示例：

```text
INFO meta_agent: Loaded 3 skill(s)
INFO meta_agent: Registered skill: skill__web-research
INFO meta_agent: Registered skill: skill__code-review
INFO meta_agent: Registered skill: skill__arcgen-pipeline
```

### 在代码中使用 SkillLoader

```rust
use lattice::skill::SkillLoader;
use lattice::tools::ToolSet;

let base_tools = Arc::new(ToolSet::with_defaults(sandbox.clone()));
let loader = SkillLoader::new("skills/");
let skills = loader.load_all(base_tools.clone(), llm.clone()).await;

let mut tools = ToolSet::with_defaults(sandbox);
for skill in skills {
    tools.register(skill)?;
}
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
$env:LATTICE_ANTHROPIC_API_BASE="https://api.anthropic.com"
$env:LATTICE_MODEL="claude-sonnet-4-6"
cargo run -p real-agent -- "What is 2+2?"
```

**CMD**：
```cmd
set LATTICE_LLM_PROVIDER=anthropic
set LATTICE_API_KEY=sk-ant-xxx
set LATTICE_ANTHROPIC_API_BASE=https://api.anthropic.com
set LATTICE_MODEL=claude-sonnet-4-6
cargo run -p real-agent -- "What is 2+2?"
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

### 启动横幅（严格模式）

`lattice-server` 启动时会从 env 解析默认 LLM 配置并在 banner 中显示。**严格模式下没有任何默认值**，缺失变量会被诚实地标记为 `(not set)`：

```text
  Provider: anthropic
  Model:    claude-sonnet-4-6
  API Base: https://api.anthropic.com
  Status:   ready
```

零配置启动 server 不会报错（方便开发期间逐步配置），但只要请求没有显式带上 `provider` / `model`，就会立刻收到明确的错误，例如：

```text
HTTP 500
{ "error": { "code": "internal", "message": "LATTICE_LLM_PROVIDER must be set (or pass `provider` in the request body)" } }
```

因此推荐两种用法：
- **服务端预配置**：启动前导出全部所需 env（或用 `.env`），Web UI 留空 Provider/Model 直接发请求
- **请求级覆盖**：服务端不配置，每个请求显式带 `provider` + `model`（适合多租户/多模型场景）

### Web UI

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
$env:LATTICE_API_KEY="sk-xxx"
$env:LATTICE_OPENAI_API_BASE="https://api.openai.com/v1"
$env:LATTICE_OPENAI_MODEL="gpt-4o"
cargo run -p lattice-server
```

> Server 和 `real-agent`、e2e 测试共用同一套 `LATTICE_*` 变量。可以直接复用 `.env`（自动通过 `dotenvy` 加载），无需在启动 server 前手动 export。

### Web UI 使用说明

Web UI 底部的发送面板里：

- `Provider` 是**可选项**
- `模型` 是**可选项**

它们的行为是：

- **留空** → 使用服务端启动时从 env 解析的值。如果该字段在 env 中也缺失，请求会返回 5xx 并附带具体缺失的变量名
- **填写** → 仅覆盖这一次请求的对应字段（不会影响其他请求，也不会修改服务端 env）

也就是说，如果你在启动 `lattice-server` 前已经设置好了：

```powershell
$env:LATTICE_LLM_PROVIDER="openai"
$env:LATTICE_API_KEY="your_key"
$env:LATTICE_OPENAI_API_BASE="https://api.siliconflow.cn/v1"
$env:LATTICE_OPENAI_MODEL="Pro/MiniMaxAI/MiniMax-M2.5"
```

那么进入 Web UI 后，`Provider` 和 `模型` 两个输入框都可以留空，直接发送消息即可。反过来，如果 env 里缺 `LATTICE_OPENAI_API_BASE`，那么 Web UI 里**也必须显式填写 base URL**（或者重启 server 补上 env），否则请求会失败。

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
$env:LATTICE_OPENAI_API_BASE="https://api.siliconflow.cn/v1"
$env:LATTICE_OPENAI_MODEL="Pro/MiniMaxAI/MiniMax-M2.5"
$env:LATTICE_PORT="3001"

cargo run -p lattice-server
```

然后打开：

```text
http://127.0.0.1:3001
```

如果服务端已经按上面方式启动，Web UI 中推荐：

- `Provider` 留空（继承服务端启动时的 env）
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
$env:LATTICE_OPENAI_API_BASE="https://api.openai.com/v1"
$env:LATTICE_OPENAI_MODEL="gpt-4o"
$env:LATTICE_MCP_CONFIG="D:\\path\\to\\mcp.json"

cargo run -p real-agent -- "Use the MCP tools to inspect available resources"
```

`lattice-server` 示例：

```powershell
$env:LATTICE_LLM_PROVIDER="openai"
$env:LATTICE_API_KEY="your_key"
$env:LATTICE_OPENAI_API_BASE="https://api.openai.com/v1"
$env:LATTICE_OPENAI_MODEL="gpt-4o"
$env:LATTICE_MCP_CONFIG="D:\\path\\to\\mcp.json"

cargo run -p lattice-server
```

行为说明：

- 如果未设置 `LATTICE_MCP_CONFIG`，系统按无 MCP 配置启动
- 如果某个 MCP server 连接失败，其余 server 仍继续工作
- `GET /health` 会返回 `mcp_servers` 快照，包含 `state / transport / tool_count / resource_count`

## LLM Provider 支持

| Provider         | 包                      | 自定义 base URL | 状态 |
| ---------------- | ----------------------- | --------------- | ---- |
| Anthropic Claude | `lattice-llm-anthropic` | `LATTICE_ANTHROPIC_API_BASE` | ✅ |
| OpenAI 兼容      | `lattice-llm-openai`    | `LATTICE_OPENAI_API_BASE`    | ✅ |

## 文档

- [架构设计](docs/ARCHITECTURE.md)
- [技术选型](docs/TECH_STACK.md)
- [Roadmap](docs/ROADMAP.md)
- [AI 编程流程](docs/AI_WORKFLOW.md)

## License

MIT
