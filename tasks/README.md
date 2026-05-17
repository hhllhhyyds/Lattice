# 任务列表

## 任务编号规范

### 主任务编号

主任务使用整数编号（1, 2, 3, ...），按照开发轮次顺序递增：
- 第一轮：1-7
- 第二轮：8-10
- 第三轮：11
- 第四轮：12-19
- 第五轮：20-1 到 20-6（使用子编号）

### 补充任务编号

当需要在已完成的轮次中添加补充任务时，使用小数编号（X.Y）：
- **格式**：`主任务编号.补充序号`
- **示例**：
  - `10.1` - 第 10 个任务的第 1 个补充任务
  - `10.2` - 第 10 个任务的第 2 个补充任务
  - `15.1` - 第 15 个任务的第 1 个补充任务

### 补充任务的使用场景

补充任务用于：
1. **Bug 修复**：修复已完成任务中发现的问题
2. **功能增强**：在已有功能基础上添加增强
3. **技术债务**：解决已知的技术债务
4. **性能优化**：优化已有功能的性能

### 文件命名规范

- **主任务**：`NN-task-name.md`（例如：`10-llm-openai.md`）
- **补充任务**：`NN.M-task-name.md`（例如：`10.1-multi-tool-call-support.md`）

### 任务状态标记

- ✅ - 已完成
- 🚧 - 进行中
- ⬜ - 未开始

### 示例

```markdown
## 第二轮：真实 LLM 接入 ✅

| #   | 任务                                                | 分支名               | 状态 |
| --- | --------------------------------------------------- | -------------------- | ---- |
| 8   | [LLM 通用协议层设计](08-llm-protocol.md)            | `feat/llm-protocol`  | ✅    |
| 9   | [实现 Anthropic (Claude) 后端](09-llm-anthropic.md) | `feat/llm-anthropic` | ✅    |
| 10  | [实现 OpenAI 兼容后端](10-llm-openai.md)            | `feat/llm-openai`    | ✅    |
| 10.1 | [支持多工具并行调用（串行执行）](10.1-multi-tool-call-support.md) | `feat/multi-tool-call` | 🚧 |
```

---

## 第一轮：MVP ✅

| #   | 任务                                                  | 分支名                | 状态 |
| --- | ----------------------------------------------------- | --------------------- | ---- |
| 1   | [初始化 workspace + crate 骨架](01-init-workspace.md) | `feat/init-workspace` | ✅    |
| 2   | [搭建 CI/CD](02-setup-ci.md)                          | `feat/setup-ci`       | ✅    |
| 3   | [实现 core 类型和 trait](03-core-traits.md)           | `feat/core-traits`    | ✅    |
| 4   | [实现 MemoryStore](04-store-memory.md)                | `feat/store-memory`   | ✅    |
| 5   | [实现 LocalSandbox](05-sandbox-local.md)              | `feat/sandbox-local`  | ✅    |
| 6   | [实现 ControlLoop](06-runtime.md)                     | `feat/runtime`        | ✅    |
| 7   | [实现 hello-agent example](07-hello-agent.md)         | `feat/hello-agent`    | ✅    |

## 第二轮：真实 LLM 接入 ✅

| #   | 任务                                                | 分支名               | 状态 |
| --- | --------------------------------------------------- | -------------------- | ---- |
| 8   | [LLM 通用协议层设计](08-llm-protocol.md)            | `feat/llm-protocol`  | ✅    |
| 9   | [实现 Anthropic (Claude) 后端](09-llm-anthropic.md) | `feat/llm-anthropic` | ✅    |
| 10  | [实现 OpenAI 兼容后端](10-llm-openai.md)            | `feat/llm-openai`    | ✅    |
| 10.1 | [支持多工具并行调用（串行执行）](10.1-multi-tool-call-support.md) | `feat/multi-tool-call` | 🚧 |

## 第三轮：真实 LLM 验证 ✅

| #   | 任务                                        | 分支名            | 状态 |
| --- | ------------------------------------------- | ----------------- | ---- |
| 11  | [实现 real-agent example](11-real-agent.md) | `feat/real-agent` | ✅    |

## 第四轮：HTTP API 层 🚧

**目标**：从库升级为可独立部署的平台服务

| #   | 任务                                                                                     | 分支名                 | 状态 |
| --- | ---------------------------------------------------------------------------------------- | ---------------------- | ---- |
| 12  | [Facade crate + Feature Flags](12-facade-features.md)                                    | `feat/facade-features` | ✅    |
| 13  | [Server crate 骨架 + 基础路由](13-server-skeleton.md)                                    | `feat/server-skeleton` | ✅    |
| 13.1 | [Web UI Markdown 渲染](13.1-ui-markdown-rendering.md)                                  | `feat/ui-markdown-rendering` | ✅ |
| 14  | [会话管理 API](14-session-api.md)                                                        | `feat/session-api`     | ✅    |
| 15  | [工具系统：ToolExecutor + ToolSet + 标准工具库](15-tool-system.md)                       | `feat/tool-system`     | ✅    |
| 16  | [任务提交与 Agent 执行 API](16-agent-run-api.md)                                         | `feat/agent-run-api`   | ⬜    |
| 17  | [SSE 实时事件流](17-sse-stream.md)                                                       | `feat/sse-stream`      | ⬜    |
| 18  | [配置管理与多 Provider 支持](18-config-provider.md)                                      | `feat/config-provider` | ⬜    |
| 19  | [Docker 化独立部署](19-docker-deploy.md)                                                 | `feat/docker-deploy`   | ⬜    |

## 第五轮：Skill 系统 🚧

**目标**：实现 Lattice 的 skill 系统，使 meta agent 能将复杂子任务委托给专门的 skill agent 执行。skill 在父 agent 视角是普通 tool 调用，背后运行完整的子 ControlLoop，支持多轮 LLM 决策、独立工具集和独立 session 树节点。

**设计原则**：遵循 [Anthropic Agent Skills 开放标准](https://www.agentskills.com)，以 SKILL.md 为唯一事实来源，实现渐进式披露三层加载。

| #    | 任务                                                           | 分支名                    | 状态 |
|------|---------------------------------------------------------------|---------------------------|------|
| 20-1 | [core 层扩展：ExecutionContext + EventPayload + ToolError](20-1-execution-context.md) | `feat/skill-execution-context` | ⬜ |
| 20-2 | [SessionStore 树形扩展 + MemoryStore 子 session](20-2-session-tree.md)               | `feat/skill-session-tree`      | ⬜ |
| 20-3 | [ToolSet + 已有工具适配新签名](20-3-tool-execute-ctx.md)                              | `feat/skill-tool-execute-ctx` | ⬜ |
| 20-4 | [ControlLoop 构造 ExecutionContext + builder](20-4-control-loop-ctx.md)               | `feat/skill-control-loop`      | ⬜ |
| 20-5 | [lattice-skill crate：SkillDefinition + SkillTool + SkillToolSet + SkillLoader](20-5-skill-crate.md) | `feat/skill-crate`    | ⬜ |
| 20-6 | [skill feature + 示例 skill 目录 + meta-agent example](20-6-skill-facade.md)         | `feat/skill-facade`           | ⬜ |

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
