# Lattice

**Agent 元框架** — 将 AI Agent 的"大脑"与"双手"彻底解耦。

## 什么是 Lattice？

Lattice 是一个 Rust 编写的 Agent 元框架，灵感来自 [Anthropic Managed Agents](https://www.anthropic.com/engineering/managed-agents) 的架构设计。

核心思想：Agent 的推理决策（大脑）和工具执行（双手）应该是独立的、可替换的组件，通过稳定的接口通信。

## 架构

三个核心抽象：

- **Session** — 不可变的事件溯源日志，Agent 的持久化记忆
- **ControlLoop** — Agent 的大脑，负责调用 LLM 并路由决策
- **Sandbox** — Agent 的双手，隔离的工具执行环境

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
```

## HTTP API Server

```bash
cargo run -p lattice-server
curl http://localhost:3000/health
```

## LLM Provider 支持

| Provider | 包 | 状态 |
|----------|-----|------|
| Anthropic Claude | `lattice-llm-anthropic` | ✅ |
| OpenAI 兼容 | `lattice-llm-openai` | ✅ |
| 自定义 base URL | via `LATTICE_API_BASE` | ✅ |

## 文档

- [架构设计](docs/ARCHITECTURE.md)
- [技术选型](docs/TECH_STACK.md)
- [Roadmap](docs/ROADMAP.md)
- [AI 编程流程](docs/AI_WORKFLOW.md)

## License

MIT
