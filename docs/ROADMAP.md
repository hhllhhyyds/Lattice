# Roadmap

## ✅ 第一轮：MVP（已完成）

验证核心架构的三组件解耦可行性。

| Crate                 | 内容                                                                     | 状态 |
| --------------------- | ------------------------------------------------------------------------ | ---- |
| lattice-core          | 核心 trait + 类型定义（SessionStore、LLMClient、Sandbox、SandboxRouter） | ✅    |
| lattice-runtime       | ControlLoop 决策循环 + BasicSandboxRouter                                | ✅    |
| lattice-store-memory  | SessionStore 内存实现                                                    | ✅    |
| lattice-sandbox-local | Sandbox 本地子进程实现                                                   | ✅    |
| hello-agent example   | 端到端验证（MockLLMClient）                                              | ✅    |
| GitHub Actions CI     | fmt + clippy + test + doc                                                | ✅    |

## ✅ 第二轮：真实 LLM 接入（已完成）

| Crate                 | 内容                                 | 状态 |
| --------------------- | ------------------------------------ | ---- |
| lattice-llm-protocol  | 通用协议层（消息格式转换、响应解析） | ✅    |
| lattice-llm-anthropic | Anthropic Claude 后端                | ✅    |
| lattice-llm-openai    | OpenAI 兼容后端                      | ✅    |

## ✅ 第三轮：真实 LLM 验证（已完成）

| 任务               | 内容                                        | 状态 |
| ------------------ | ------------------------------------------- | ---- |
| real-agent example | 用真实 LLM 端到端跑通（Anthropic / OpenAI） | ✅    |

## 🚧 第四轮：HTTP API 层（进行中）

**目标**：从库升级为可独立部署的平台服务。基于 axum 搭建 REST API，支持通过 HTTP 创建/查询/恢复会话、提交任务、实时获取结果。

### 设计原则

- **API 层是薄壳**：不引入新的业务逻辑，只做 HTTP 与 core trait 之间的桥接
- **状态管理集中**：所有运行时状态（活跃会话、ControlLoop 句柄）通过 `AppState` 统一管理
- **异步非阻塞**：Agent 任务异步执行，客户端通过轮询或事件流获取进度
- **Provider 可配置**：LLM provider 通过配置文件/环境变量指定，运行时可切换

### 任务拆解

| #  | 任务 | 分支名 | 状态 |
|----|------|--------|------|
| 12 | [Facade crate + Feature Flags](../tasks/12-facade-features.md) | `feat/facade-features` | ✅ |
| 13 | [Server crate 骨架 + 基础路由](../tasks/13-server-skeleton.md) | `feat/server-skeleton` | ✅ |
| 14 | [会话管理 API](../tasks/14-session-api.md) | `feat/session-api` | ⬜ |
| 15 | [任务提交与 Agent 执行 API](../tasks/15-agent-run-api.md) | `feat/agent-run-api` | ⬜ |
| 16 | [SSE 实时事件流](../tasks/16-sse-stream.md) | `feat/sse-stream` | ⬜ |
| 17 | [配置管理与多 Provider 支持](../tasks/17-config-provider.md) | `feat/config-provider` | ⬜ |
| 18 | [Docker 化独立部署](../tasks/18-docker-deploy.md) | `feat/docker-deploy` | ⬜ |

### API 端点预览

```
GET    /health                          → 健康检查
POST   /v1/sessions                     → 创建会话
GET    /v1/sessions                     → 列出会话
GET    /v1/sessions/:id                 → 查询会话详情
GET    /v1/sessions/:id/events          → 查询会话事件（支持过滤）
POST   /v1/sessions/:id/messages        → 提交用户消息并触发 Agent 执行
GET    /v1/sessions/:id/messages        → 获取会话消息（FinalAnswer + UserMessage）
GET    /v1/sessions/:id/stream          → SSE 事件流（实时推送）
GET    /v1/providers                    → 列出可用 LLM provider
```

## 📋 后续规划

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

### 多沙箱并行调度
- SandboxRouter 支持多沙箱实例池
- 按工具类型路由到不同沙箱

### 凭据管理
- Vault Proxy 模式：沙箱外注入凭据
- 初始化注入模式：沙箱创建时一次性注入
