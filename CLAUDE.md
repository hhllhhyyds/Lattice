# Lattice — AI 编程上下文

## 项目简介

Lattice 是一个 Rust 编写的 **Agent 元框架**，核心思想来自 Anthropic Managed Agents 架构——将 Agent 的"大脑"（推理决策）与"双手"（工具执行）彻底解耦。

## 核心原则（不可违反）

1. **三组件解耦**：Session（事件日志）、Harness/ControlLoop（控制循环）、Sandbox（沙箱）是三个正交的抽象，只通过 trait 接口通信，绝不直接依赖具体实现。
2. **一切皆为事件**：所有组件间的通信、状态变更、错误都是不可变的、仅追加的事件。
3. **超越模型的不变量**：框架代码中不允许出现针对特定 LLM 模型行为的硬编码逻辑。
4. **接口有立场，实现无假设**：trait 定义严格，但对背后的技术选型（数据库、容器方案等）保持中立。
5. **ControlLoop 无状态**：控制循环不持有持久状态，可从 SessionStore 的事件流中随时恢复。
6. **Feature 组合，按需裁剪**：所有非核心功能通过 Rust feature flags 控制。core crate 零 feature（纯接口），实现 crate 独立成包，facade crate `lattice` 通过 feature 重导出。消费者只编译需要的部分。
7. **工具三层体系**：工具分三层——core 定义 `ToolExecutor` trait（Layer 1）、`lattice-tools` 提供标准工具库（Layer 2）、应用层注入自定义工具（Layer 3）。ControlLoop 通过 `ToolSet` 统一调用，不区分工具背后是沙箱执行还是进程内执行。
8. **TDD: Tests First, Code Second.**
   - For every task, write tests BEFORE implementation.
   - Tests must be reviewable independently — push tests as a separate commit or PR.
   - Wait for human approval of tests before writing implementation code.
   - Tests define the contract. Implementation fulfills it.

## 文档索引

| 文档 | 内容 |
|------|------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构设计：三组件定义、trait 接口、数据流、生命周期、工具系统 |
| [docs/TECH_STACK.md](docs/TECH_STACK.md) | 技术选型及理由 |
| [docs/ROADMAP.md](docs/ROADMAP.md) | 项目路线图：已完成里程碑 + 当前进展 + 未来规划 |
| [docs/AI_WORKFLOW.md](docs/AI_WORKFLOW.md) | AI 编程流程规范 |
| [tasks/](tasks/) | MVP 任务拆解，按顺序执行 |

## Crate 结构

```
crates/
├── core/             # 核心 trait + 类型定义（零外部依赖，纯接口）
├── runtime/          # ControlLoop 实现（接收 ToolSet）
├── store-memory/     # SessionStore 内存实现（开发/测试用）
├── sandbox-local/    # Sandbox 本地子进程实现
├── llm-protocol/     # LLM 通用协议层（消息格式转换、响应解析）
├── llm-anthropic/    # LLMClient 的 Anthropic Claude 实现
├── llm-openai/       # LLMClient 的 OpenAI 兼容实现
├── tools/            # 标准工具库（bash, file, glob, grep, http）
├── mcp/              # MCP 客户端（McpClientManager、McpToolAdapter）
├── server/           # HTTP API 服务（axum），平台服务入口

# Facade crate（根目录 src/lib.rs）
lattice               # 通过 feature flags 重导出所有子 crate
```

每个 crate 都有其专属的 `CLAUDE.md` 文件，包含该 crate 的关键类型、设计决策、已知问题和依赖关系。工作在该 crate 目录时，Claude Code 会自动加载对应的上下文。

| Crate | CLAUDE.md |
|-------|-----------|
| `core` | [crates/core/CLAUDE.md](crates/core/CLAUDE.md) |
| `runtime` | [crates/runtime/CLAUDE.md](crates/runtime/CLAUDE.md) |
| `store-memory` | [crates/store-memory/CLAUDE.md](crates/store-memory/CLAUDE.md) |
| `sandbox-local` | [crates/sandbox-local/CLAUDE.md](crates/sandbox-local/CLAUDE.md) |
| `tools` | [crates/tools/CLAUDE.md](crates/tools/CLAUDE.md) |
| `mcp` | [crates/mcp/CLAUDE.md](crates/mcp/CLAUDE.md) |
| `llm-protocol` | [crates/llm-protocol/CLAUDE.md](crates/llm-protocol/CLAUDE.md) |
| `llm-anthropic` | [crates/llm-anthropic/CLAUDE.md](crates/llm-anthropic/CLAUDE.md) |
| `llm-openai` | [crates/llm-openai/CLAUDE.md](crates/llm-openai/CLAUDE.md) |
| `server` | [crates/server/CLAUDE.md](crates/server/CLAUDE.md) |

## 跨 crate 规范

- **语言**：Rust 2021 edition
- **异步运行时**：tokio
- **命名**：snake_case（Rust 标准），trait 名用大驼峰
- **错误处理**：trait 内部用 `thiserror` 定义具体错误类型，应用层可用 `anyhow`
- **文档注释**：所有 pub 类型和方法必须有 `///` 文档注释（英文）
- **语言约定**：文档用中文，代码注释全部用英文（包括 `///` doc comment 和 `//` 行内注释）
- **测试**：每个 crate 必须有单元测试，trait 实现必须有集成测试
- **commit 信息**：`<type>(<scope>): <description>`，如 `feat(core): define SessionStore trait`
- **模块文件**：使用 `foo.rs` 而非 `foo/mod.rs`；子模块文件与父模块同级存放

## 实现任务时的工作流

1. 先读 `docs/ARCHITECTURE.md` 理解整体设计
2. 确认要实现的组件在 `docs/ROADMAP.md` 路线图范围内
3. **TDD Phase 1（测试先行）**：先写会失败的测试（单元测试 + 集成测试），定义功能的预期行为
   - 测试文件必须能编译但断言会失败（Red 阶段）
   - 用 `test: add tests for #XX (<feature name>)` 提交测试
   - **停下来等人工审查测试**
4. **人工审查测试**：确认测试覆盖了你期望的行为、无遗漏的边界情况后再继续
5. **TDD Phase 2（实现）**：人工批准测试后，实现最小代码让所有测试通过
   - 用 `feat: implement #XX (<feature name>)` 提交实现
6. 运行 `./scripts/check.sh` 确保 fmt + clippy + test + doc 全部通过
7. 如有 `.env` 文件，运行 `./scripts/test-local.sh` 跑真实 LLM 集成测试
8. AI 自我 review 后提交人工审查
9. 提交时遵守 commit 信息规范