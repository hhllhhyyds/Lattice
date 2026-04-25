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

## 第二轮：真实 LLM 接入

| # | 任务 | 分支名 | 状态 |
|---|------|--------|------|
| 8 | [LLM 通用协议层设计](08-llm-protocol.md) | `feat/llm-protocol` | ⬜ |
| 9 | [实现 Anthropic (Claude) 后端](09-llm-anthropic.md) | `feat/llm-anthropic` | ⬜ |
| 10 | [实现 OpenAI 兼容后端](10-llm-openai.md) | `feat/llm-openai` | ⬜ |
| 11 | [实现 real-agent example](11-real-agent.md) | `feat/real-agent` | ⬜ |

## 后续规划（待拆解）

### HTTP API 层
- 基于 axum 搭建 REST API 服务
- 支持通过 HTTP 创建/查询/恢复会话、提交任务、获取结果
- 从库升级为可独立部署的平台服务

### Docker 沙箱
- 实现 Sandbox trait 的 Docker 容器版本
- 容器级别的进程隔离和资源限制
- 凭据初始化注入，运行时不可访问
