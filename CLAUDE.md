# Lattice — AI 编程上下文

## 项目简介

Lattice 是一个 Rust 编写的 **Agent 元框架**，核心思想来自 Anthropic Managed Agents 架构——将 Agent 的"大脑"（推理决策）与"双手"（工具执行）彻底解耦。

## 核心原则（不可违反）

1. **三组件解耦**：Session（事件日志）、Harness/ControlLoop（控制循环）、Sandbox（沙箱）是三个正交的抽象，只通过 trait 接口通信，绝不直接依赖具体实现。
2. **一切皆为事件**：所有组件间的通信、状态变更、错误都是不可变的、仅追加的事件。
3. **超越模型的不变量**：框架代码中不允许出现针对特定 LLM 模型行为的硬编码逻辑。
4. **接口有立场，实现无假设**：trait 定义严格，但对背后的技术选型（数据库、容器方案等）保持中立。
5. **ControlLoop 无状态**：控制循环不持有持久状态，可从 SessionStore 的事件流中随时恢复。

## 文档索引

| 文档 | 内容 |
|------|------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构设计：三组件定义、trait 接口、数据流、生命周期 |
| [docs/TECH_STACK.md](docs/TECH_STACK.md) | 技术选型及理由 |
| [docs/MVP_SCOPE.md](docs/MVP_SCOPE.md) | MVP 范围定义：做什么、不做什么 |
| [docs/AI_WORKFLOW.md](docs/AI_WORKFLOW.md) | AI 编程流程规范 |
| [tasks/](tasks/) | MVP 任务拆解，按顺序执行 |

## Crate 结构

```
crates/
├── core/           # 核心 trait + 类型定义（零外部依赖，纯接口）
├── runtime/        # ControlLoop 实现
├── store-memory/   # SessionStore 内存实现（开发/测试用）
├── sandbox-local/  # Sandbox 本地子进程实现
```

## 代码规范

- **语言**：Rust 2021 edition
- **异步运行时**：tokio
- **命名**：snake_case（Rust 标准），trait 名用大驼峰
- **错误处理**：trait 内部用 `thiserror` 定义具体错误类型，应用层可用 `anyhow`
- **文档注释**：所有 pub 类型和方法必须有 `///` 文档注释（英文）
- **语言约定**：文档用中文，代码注释全部用英文（包括 `///` doc comment 和 `//` 行内注释）
- **测试**：每个 crate 必须有单元测试，trait 实现必须有集成测试
- **commit 信息**：`<type>(<scope>): <description>`，如 `feat(core): define SessionStore trait`

## 实现任务时的工作流

1. 先读 `docs/ARCHITECTURE.md` 理解整体设计
2. 确认要实现的组件在 `docs/MVP_SCOPE.md` 范围内
3. 按架构文档中的 trait 定义编写代码
4. 写测试，确保 `cargo test` 全部通过
5. 确保 `cargo clippy` 无警告
6. 提交时遵守 commit 信息规范
