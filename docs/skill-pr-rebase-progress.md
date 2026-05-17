# Skill PR Rebase 进展记录

## 背景

PR #95 和 #96 与 main 分支存在冲突，需要 rebase 到最新 main。

## PR 关系

```
main (0a3e8e5)
  └─ base: e590959
       ├─ feat/skill-system-rebase (#95): 3 commits
       │    52ac613 feat: add lattice-skill crate
       │    6790f84 fix: adapt skill system to latest main changes
       │    3731966 fix: reconcile skill rebase with latest main
       │
       └─ feat/skill-facade (#96): 以上 3 commits + 1 commit
            c30da05 feat: add skill feature and meta-agent example
```

#96 包含 #95 的全部提交，因此策略是先 rebase #95，再 rebase #96。

## Main 在 base 之后新增的提交

```
4eb565b feat: harden real-LLM integration end-to-end
f63c42a test: add tests for #107 (ui-markdown-rendering)
49cf28c feat: implement #107 (ui-markdown-rendering)
979893f docs(server): update CLAUDE.md and task for #107
d95f044 docs: update tasks/README.md and TECH_STACK.md for #107
0a3e8e5 docs: sync architecture docs with current code state
```

## 冲突文件（两个 PR 完全相同）

| 文件 | 说明 |
|------|------|
| `Cargo.lock` | 自动合并 |
| `crates/core/src/event.rs` | 自动合并 |
| `crates/llm-protocol/src/convert.rs` | 自动合并 |
| `crates/runtime/src/control_loop.rs` | **需手动解决** |
| `crates/server/src/lib.rs` | 自动合并 |
| `crates/store-memory/src/memory_store.rs` | 自动合并 |
| `examples/real-agent/src/main.rs` | 自动合并 |

## control_loop.rs 冲突原因

- **Main** 在 `4eb565b` 中新增了 `Decision::ThinkingToolCall` 变体，并将工具执行重构为 `execute_tool_call()` 辅助方法，签名为 `tools.execute(&tool, params).await`
- **Skill 分支** 引入了 `ExecutionContext`，将签名改为 `tools.execute(&tool, params, &ctx).await`，并内联了执行逻辑
- **ThinkingToolCall 路径**的冲突块中，skill 分支代码引用了未定义的 `req_event_id`（该变量只在 `ToolCall` 路径中被定义）

## 解决方案

在 `ThinkingToolCall` 路径中：先补全 `ToolCallRequested` 事件的创建（获得 `req_event_id`），再用 skill 分支的 `ExecutionContext` 内联方式执行工具。

## 当前状态

| 步骤 | 状态 |
|------|------|
| 安装 gh CLI | ✅ 完成（`~/.local/bin/gh`）|
| gh 登录 GitHub | ✅ 完成 |
| 查询 PR 冲突状态 | ✅ 完成 |
| 本地 checkout `feat/skill-system-rebase` | ✅ 完成 |
| `feat/skill-system-rebase` rebase onto main | ✅ **完成**，已生成新 commit `fbd8739` |
| 验证 #95 编译 | ⏸ **暂停** |
| Force-push #95 到远程 | ⏸ 待完成 |
| Rebase `feat/skill-facade` (#96) | ⏸ 待完成 |
| Force-push #96 到远程 | ⏸ 待完成 |

## Skill 定义与挂载（已完成）

### 参考来源

对齐 claw-code (`/home/leiqiaojie2/claw-code`) 的 skill 体系：
- Skill = 目录 + `SKILL.md`（YAML frontmatter + Markdown body）
- frontmatter 包含 `name`、`description`、`allowed-tools`（可选）、`x-lattice.params`（可选）
- Markdown body 作为 skill 子 agent 的 system prompt

### 新增文件

| 文件 | 说明 |
|------|------|
| `skills/code-review/SKILL.md` | 代码审查 skill，继承全部父工具（无 `allowed-tools` 限制） |

### 修改文件

| 文件 | 改动 |
|------|------|
| `Cargo.toml` | 新增 `skill = ["dep:lattice-skill", "runtime", "tools"]` feature；`full` 包含 `skill`；加 `lattice-skill` 可选依赖 |
| `src/lib.rs` | 新增 `#[cfg(feature = "skill")] pub use lattice_skill as skill;` |
| `examples/real-agent/src/main.rs` | 两阶段构建工具集（base_tools → parent_tools → skill_tools → agent_tools）；挂载 SkillLoader；更新 system prompt |

### 挂载原理

```
run()
 ├─ sandbox = Arc::new(LocalSandbox::new())
 ├─ base_tools = ToolSet::with_defaults(sandbox) + MCP      ← Phase 1
 ├─ parent_tools = Arc::new(base_tools)
 ├─ skill_loader = SkillLoader::new("./skills")
 ├─ skill_tools = skill_loader.load_all(parent_tools, llm)   ← 每个 skill 持有 parent_tools
 ├─ agent_tools = ToolSet::with_defaults(sandbox) + MCP     ← Phase 2
 │   + register(skill:code-review)
 └─ ControlLoop::with_options(store, llm, agent_tools, ...)
```

当 LLM 调用 `skill:code-review` 时：
1. `SkillTool::execute()` 创建子 session
2. 子 ControlLoop 以 `parent_tools`（shell、MCP 等）+ SKILL.md body 为 system prompt 运行
3. 子 agent 完成后返回 `FinalAnswer` 作为工具结果

## 当前状态（更新）

| 步骤 | 状态 |
|------|------|
| 安装 gh CLI | ✅ |
| gh 登录 GitHub | ✅ |
| `feat/skill-system-rebase` rebase onto main | ✅ 新 commit `fbd8739` |
| 验证 #95 编译 | ⏸ Rust/Cargo 未安装，无法本地编译 |
| Force-push #95 到远程 | ⏸ 待完成 |
| Rebase `feat/skill-facade` (#96) | ⏸ 待完成 |
| Force-push #96 到远程 | ⏸ 待完成 |
| 创建 `skills/code-review/SKILL.md` | ✅ |
| Facade skill feature 接入 | ✅ |
| real-agent 挂载 skill | ✅ |

