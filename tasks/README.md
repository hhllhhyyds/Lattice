# 任务列表

## 第一轮：MVP ✅

| # | 任务 | 分支名 | 状态 |
|---|------|--------|------|
| 1 | [初始化 workspace + crate 骨架](01-init-workspace.md) | `feat/init-workspace` | ✅ |
| 2 | [搭建 CI/CD](02-setup-ci.md) | `feat/setup-ci` | ✅ |
| 3 | [实现 core 类型和 trait](03-core-traits.md) | `feat/core-traits` | ✅ |
| 4 | [实现 MemoryStore](04-store-memory.md) | `feat/store-memory` | ✅ |
| 5 | [实现 LocalSandbox](05-sandbox-local.md) | `feat/sandbox-local` | ✅ |
| 6 | [实现 ControlLoop + BasicSandboxRouter](06-runtime.md) | `feat/runtime` | ✅ |
| 7 | [实现 hello-agent example](07-hello-agent.md) | `feat/hello-agent` | ✅ |

## 第二轮：真实 LLM 接入 ✅

| # | 任务 | 分支名 | 状态 |
|---|------|--------|------|
| 8 | [LLM 通用协议层设计](08-llm-protocol.md) | `feat/llm-protocol` | ✅ |
| 9 | [实现 Anthropic (Claude) 后端](09-llm-anthropic.md) | `feat/llm-anthropic` | ✅ |
| 10 | [实现 OpenAI 兼容后端](10-llm-openai.md) | `feat/llm-openai` | ✅ |

## 第三轮：真实 LLM 验证 ✅

| # | 任务 | 分支名 | 状态 |
|---|------|--------|------|
| 11 | [实现 real-agent example](11-real-agent.md) | `feat/real-agent` | ✅ |

## 第四轮：HTTP API 层 🚧

**目标**：从库升级为可独立部署的平台服务

| # | 任务 | 分支名 | 状态 |
|---|------|--------|------|
| 12 | [Facade crate + Feature Flags](12-facade-features.md) | `feat/facade-features` | ⬜ |
| 13 | [Server crate 骨架 + 基础路由](13-server-skeleton.md) | `feat/server-skeleton` | ⬜ |
| 14 | [会话管理 API](14-session-api.md) | `feat/session-api` | ⬜ |
| 15 | [任务提交与 Agent 执行 API](15-agent-run-api.md) | `feat/agent-run-api` | ⬜ |
| 16 | [SSE 实时事件流](16-sse-stream.md) | `feat/sse-stream` | ⬜ |
| 17 | [配置管理与多 Provider 支持](17-config-provider.md) | `feat/config-provider` | ⬜ |
| 18 | [Docker 化独立部署](18-docker-deploy.md) | `feat/docker-deploy` | ⬜ |

## 后续规划（待拆解）

### Docker 沙箱
- 实现 Sandbox trait 的 Docker 容器版本
- 容器级别的进程隔离和资源限制
- 凭据初始化注入，运行时不可访问

### 持久化存储
- SQLite / Postgres 实现 SessionStore trait
- 支持会话跨进程恢复

### 上下文窗口管理
- 事件历史压缩/摘要策略
- 长会话的上下文滑动窗口
