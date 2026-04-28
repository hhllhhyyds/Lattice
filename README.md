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
