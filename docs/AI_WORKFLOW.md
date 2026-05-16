# AI 编程流程规范

## 概述

Lattice 项目全程使用 AI 辅助编程（Claude Code）。本文档定义了从设计到交付的完整工作流，确保每个环节都能被 AI 正确理解和执行。

## 核心原则

1. **文档即真相**：所有设计决策都写在仓库文档中，不存在于任何人脑子里
2. **CLAUDE.md 是入口**：Claude Code 通过 CLAUDE.md 找到所有需要的上下文
3. **小步快跑**：每次任务范围小而明确，一个 PR 解决一个问题
4. **测试先行**：每次实现必须附带测试
5. **TDD：测试优先于实现**：测试是规格的具体化；先让人审查测试（确认要做什么），再实现代码（确认怎么做）

## 工作流阶段

### 阶段 1：Spec（需求/设计）

**由人完成**，AI 辅助讨论。

- 在 `docs/` 下写清楚要做什么
- 更新 CLAUDE.md 中的文档索引
- 设计决策通过对话敲定，结论写入文档

**产出**：更新后的文档（ARCHITECTURE.md、MVP_SCOPE.md 等）

### 阶段 2：Plan（任务拆解）

**由 AI 完成**，人审核。

- 基于 docs/ 中的设计文档，将实现拆解为具体的、可独立完成的任务
- 每个任务写成一个 GitHub Issue 或 `tasks/` 目录下的 markdown 文件
- 任务描述包含：目标、涉及的 crate、依赖的 trait/类型、验收标准

**任务编号规范**：

- **主任务**：使用整数编号（1, 2, 3, ...），按开发轮次顺序递增
- **补充任务**：使用小数编号（X.Y），用于在已完成轮次中添加补充任务
  - 格式：`主任务编号.补充序号`
  - 示例：`10.1` 表示第 10 个任务的第 1 个补充任务
  - 使用场景：Bug 修复、功能增强、技术债务、性能优化

- **文件命名**：
  - 主任务：`NN-task-name.md`（例如：`10-llm-openai.md`）
  - 补充任务：`NN.M-task-name.md`（例如：`10.1-multi-tool-call-support.md`）

- **任务状态**：
  - ✅ - 已完成
  - 🚧 - 进行中
  - ⬜ - 未开始

详细规范见 [tasks/README.md](../tasks/README.md)。

**产出**：任务列表（Issue 或 tasks/*.md）

### 阶段 3：Implement（实现）

**TDD Phase 1 — 写测试（先于实现）**：

- 每个任务启动一个 Claude Code session
- Claude Code 读取 CLAUDE.md → 找到相关文档 → 理解上下文
- **先写会失败的测试**，定义功能的预期行为（单元测试 + 集成测试）
- 测试文件必须能编译，但断言会失败（Red 阶段）
- 一个任务对应一个 feature 分支：`feat/<task-name>`
- 提交消息格式：`test: add tests for #XX (<feature name>)`
- **停下来，等人工审查测试**

**TDD Phase 2 — 实现代码（等测试批准后）**：

- 人工批准测试后，Claude Code 实现最小代码让所有测试通过
- 提交消息格式：`feat: implement #XX (<feature name>)`

**给 Claude Code 的标准指令模板（Phase 1）**：

```
读取 CLAUDE.md 了解项目上下文。

任务：<具体任务描述>

要求：
1. 先写会失败的测试（单元测试 + 集成测试），定义预期行为
2. 测试必须能编译，但断言会失败（Red）
3. 用 "test: add tests for #XX (<feature name>)" 提交
4. 停下来等人工审查测试
```

**给 Claude Code 的标准指令模板（Phase 2）**：

```
人工已批准测试。现在实现最小代码让所有测试通过。

要求：
1. 实现前先跑测试确认处于 Red 状态
2. 实现后确保所有测试通过（Green）
3. 如需要可做重构（Refactor）
4. 运行 `./scripts/check.sh` 确保全部通过
5. 用 "feat: implement #XX (<feature name>)" 提交
```

### TDD 为什么重要

测试是最容易理解的规格形式。人工审查测试代码时，审查的是**功能要做什么**（而非代码怎么实现）。这比混在一起的测试+实现更容易理解，也更容易发现理解偏差。

- **质量**：Bug 在规格阶段捕获，而非实现后
- **沟通**：人和 AI 通过测试共享同一份理解
- **效率**：不会在错误理解的需求上浪费实现工作
- **可追溯**：先写测试的 commit 证明了测试是独立于实现完成的

### 阶段 4：Test（测试验证）

**由 CI 自动完成 + 人工审查**。

- `./scripts/check.sh` — fmt + clippy + test + doc 一键检查
- `./scripts/test-local.sh` — 包含真实 LLM 集成测试（需要 `.env`）

#### 测试覆盖率

使用 `cargo-llvm-cov` 检测代码覆盖率。覆盖率报告作为 CI 的一部分运行，不阻断合并，但作为参考指标。

- **本地运行**：`./scripts/check.sh` 中的步骤 3 包含覆盖率检测（需要先 `cargo install cargo-llvm-cov`）
- **CI 上传**：coverage job 通过 `cargo-llvm-cov-action` 上传至 Codecov（需要 `CODECOV_TOKEN` secret）
- **指标**：按 crate 维度统计 line coverage 和 branch coverage
- **目标**：非数值目标，保持覆盖率不下降为主要原则；新增代码应附带对应的测试

### 阶段 5：AI Review（自动代码审查）

**由 Claude Code 完成**，在人工 review 之前执行。

每个 feature 分支完成后，在 Claude Code 中执行以下 review 指令：

```
读取 CLAUDE.md 了解项目上下文。

Review 当前分支相对于 main 的所有改动（git diff main...HEAD）。

检查以下维度：
1. 架构合规 — 是否符合 docs/ARCHITECTURE.md 的 trait 定义和设计原则
2. 代码质量 — 错误处理、命名、代码组织
3. 测试覆盖 — 是否覆盖了 tasks/ 中要求的测试场景
4. 文档注释 — pub 类型和方法是否有英文 doc comment
5. 安全边界 — ControlLoop 是否直接依赖了具体实现
6. 语言规范 — 代码注释是否全部使用英文

输出格式：
- ✅ 通过的项
- ⚠️ 建议改进的项（非阻塞）
- ❌ 必须修改的项（阻塞合并）

如果有 ❌ 项，直接修复后重新提交。
如果只有 ⚠️ 项，列出建议，交由人工判断是否修改。
```

**流程**：
1. Claude Code 完成实现 → 自己跑 review 指令
2. 有 ❌ 项 → 自行修复 → 重新 review → 直到无 ❌
3. 输出 review 报告
4. 交给人工做最终审查

### 阶段 6：更新 DOCS（文档同步）

**由 Claude Code 完成**，在 AI Review 通过后、人工 Review 之前执行。

代码改完了不算完——文档必须跟着代码走。每次任务完成后，从修改的代码位置出发，**自下而上**扫描哪些文档需要更新：

```
从修改的代码位置向上追溯：
1. 修改的代码属于哪个 crate？
   → 更新 crates/<crate>/CLAUDE.md（如该 crate 有专属 CLAUDE.md）
2. 修改影响了哪些 trait 接口、组件交互、数据流？
   → 更新 docs/ARCHITECTURE.md
3. 引入了新依赖？
   → 更新 docs/TECH_STACK.md
4. 任务在 tasks/ 中有对应条目？
   → 更新 tasks/README.md 和 tasks/<当前任务>.md（状态 ⬜ → ✅）
5. 任务属于哪个里程碑？
   → 更新 docs/ROADMAP.md
6. 对整个项目结构或核心原则有影响？
   → 更新根目录 CLAUDE.md（如文档索引、crate 结构图、跨 crate 规范）
```

**每个 crate 的 CLAUDE.md 更新原则**：当修改涉及该 crate 内部实现时，必须同步更新该 crate 的 CLAUDE.md。CLAUDE.md 的定位是"该 crate 内部的关键类型、设计决策、已知问题"，而非全局视角——所以它最接近代码，最需要跟着代码走。

**根目录 CLAUDE.md 的更新范围**（全局视角，只写顶层结论）：
1. 新增 crate → 更新"文档索引"和"crate 结构"两节
2. 修改了跨 crate 规范（如新的 trait、新的接口约定）→ 更新对应段落
3. 里程碑完成 → 更新 ROADMAP.md 状态

**文档检查清单**（每次任务完成后逐项确认）：

| 文档 | 触发条件 |
|------|----------|
| `crates/<crate>/CLAUDE.md` | 修改了该 crate 内部实现 |
| `docs/ARCHITECTURE.md` | 新增/修改了 trait、数据结构、组件交互 |
| `docs/TECH_STACK.md` | 新增依赖 |
| `docs/ROADMAP.md` | 任务状态变更 |
| `tasks/README.md` | 任务状态变更 |
| `tasks/<任务>.md` | 验收标准完成 |
| `CLAUDE.md`（根目录）| crate 结构变化、核心原则变化 |

**原则**：
- 文档即真相——如果文档和代码不一致，后来的人（包括 AI）会被误导
- 先改完文档，再提交代码。不要留"回头补文档"的债
- 不确定该不该改的文档，宁可多改一行，也别漏掉

**给 Claude Code 的文档更新指令**（可附加在实现指令末尾）：

```
实现完成后，从修改的代码向上追溯，执行以下文档检查：
1. 更新 crates/<crate>/CLAUDE.md（如果修改了该 crate 内部）
2. 更新 docs/ARCHITECTURE.md（trait/组件/数据流变化）
3. 更新 docs/TECH_STACK.md（新依赖）
4. 更新 docs/ROADMAP.md（任务状态 ⬜ → ✅）
5. 更新 tasks/README.md 和 tasks/<当前任务>.md
6. 评估是否需要更新根目录 CLAUDE.md
```

### 阶段 7：人工 Review（最终审查）

**由人完成**，是合并前的最后一道关。

- 查看 AI Review 报告
- 浏览代码改动，关注设计判断和架构方向
- 通过 → 进入 Merge
- 不通过 → 反馈给 Claude Code 修改

### 阶段 8：Merge & Deploy

- Squash merge 到 main
- 后期：GitHub Actions 自动运行测试 + 发布

## 分支策略

```
main                    ← 稳定分支，永远可编译
├── feat/<task-name>    ← 功能分支
├── fix/<issue>         ← 修复分支
└── docs/<topic>        ← 文档分支
```

## Commit 规范

格式：`<type>(<scope>): <description>`

类型：
- `feat` — 新功能
- `fix` — 修复
- `docs` — 文档
- `test` — 测试
- `refactor` — 重构
- `chore` — 构建/工具

示例：
- `feat(core): define Event and SessionStore trait`
- `feat(runtime): implement ControlLoop decision cycle`
- `test(store-memory): add integration tests for MemoryStore`
- `docs: update ARCHITECTURE.md with sandbox lifecycle`

## 目录结构约定

```
Lattice/
├── CLAUDE.md                 # AI 编程入口
├── README.md                 # 项目说明
├── Cargo.toml                # workspace 定义
├── docs/
│   ├── ARCHITECTURE.md       # 架构设计
│   ├── TECH_STACK.md         # 技术选型
│   ├── MVP_SCOPE.md          # MVP 范围
│   └── AI_WORKFLOW.md        # 本文档
├── crates/
│   ├── core/                 # 核心 trait + 类型
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── event.rs      # Event、EventPayload、Actor
│   │       ├── session.rs    # SessionStore trait、SessionId
│   │       ├── llm.rs        # LLMClient trait、Decision、ToolDescription
│   │       ├── sandbox.rs    # Sandbox trait、ExecutionResult
│   │       ├── tool.rs       # ToolDescription、ToolExecutor trait
│   │       └── error.rs      # 所有错误类型
│   ├── runtime/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── control_loop.rs
│   ├── tools/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs        # ToolSet + re-exports
│   │       ├── set.rs        # ToolSet 实现
│   │       └── bash.rs       # BashTool 实现
│   ├── store-memory/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── sandbox-local/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── llm-protocol/          # 通用 LLM 协议层
│   ├── llm-anthropic/         # Anthropic Claude 后端
│   ├── llm-openai/            # OpenAI 兼容后端
│   └── server/                # HTTP API 服务（axum）
└── examples/
    ├── hello-agent/           # Mock LLM 端到端示例
    └── real-agent/            # 真实 LLM 端到端示例
```
