# AI 编程流程规范

## 概述

Lattice 项目全程使用 AI 辅助编程（Claude Code）。本文档定义了从设计到交付的完整工作流，确保每个环节都能被 AI 正确理解和执行。

## 核心原则

1. **文档即真相**：所有设计决策都写在仓库文档中，不存在于任何人脑子里
2. **CLAUDE.md 是入口**：Claude Code 通过 CLAUDE.md 找到所有需要的上下文
3. **小步快跑**：每次任务范围小而明确，一个 PR 解决一个问题
4. **测试先行**：每次实现必须附带测试

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

**产出**：任务列表（Issue 或 tasks/*.md）

### 阶段 3：Implement（实现）

**由 Claude Code 完成**。

- 每个任务启动一个 Claude Code session
- Claude Code 读取 CLAUDE.md → 找到相关文档 → 理解上下文 → 编写代码
- 一个任务对应一个 feature 分支：`feat/<task-name>`

**给 Claude Code 的标准指令模板**：

```
读取 CLAUDE.md 了解项目上下文。

任务：<具体任务描述>

要求：
1. 按照 docs/ARCHITECTURE.md 中的 trait 定义实现
2. 写单元测试和集成测试
3. 确保 cargo test、cargo clippy 通过
4. 完成后提交到 feat/<branch-name> 分支
```

### 阶段 4：Test（测试验证）

**由 CI 自动完成 + 人工审查**。

- `cargo test` — 单元测试和集成测试
- `cargo clippy` — lint 检查
- `cargo fmt --check` — 格式检查
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` — 文档生成检查

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

### 阶段 6：人工 Review（最终审查）

**由人完成**，是合并前的最后一道关。

- 查看 AI Review 报告
- 浏览代码改动，关注设计判断和架构方向
- 通过 → 进入 Merge
- 不通过 → 反馈给 Claude Code 修改

### 阶段 7：Merge & Deploy

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
│   │       ├── router.rs     # SandboxRouter trait
│   │       └── error.rs      # 所有错误类型
│   ├── runtime/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── control_loop.rs
│   │       └── basic_router.rs
│   ├── store-memory/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   └── sandbox-local/
│       ├── Cargo.toml
│       └── src/lib.rs
└── examples/
    └── hello-agent/
        ├── Cargo.toml
        └── src/main.rs
```
