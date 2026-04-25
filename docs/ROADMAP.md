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

## 📋 后续规划

### HTTP API 层
- 基于 axum 搭建 REST API 服务
- 支持通过 HTTP 创建/查询/恢复会话、提交任务、获取结果
- 从库升级为可独立部署的平台服务

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
