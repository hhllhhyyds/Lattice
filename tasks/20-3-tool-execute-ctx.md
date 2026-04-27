# 任务 20-3：ToolSet + 已有工具适配新签名

## 目标

更新 `ToolSet::execute` 签名，适配 `ToolExecutor::execute` 新增的 `ctx` 参数。所有已有工具（BashTool 及后续的 FileTool、GlobTool 等）的 `execute` 方法签名同步更新，行为保持不变。

## 分支

`feat/skill-tool-execute-ctx`

## 依赖

- 任务 20-1（core 层扩展：ExecutionContext + ToolExecutor 新签名）

---

## 修改内容

### 1. `ToolSet::execute`（`crates/tools/src/set.rs`）

```rust
/// Look up and execute a tool by name.
#[instrument(skip(self))]
pub async fn execute(
    &self,
    name: &str,
    params: serde_json::Value,
    ctx: &ExecutionContext,
) -> Result<lattice_core::ExecutionResult, ToolError> {
    let executor = self
        .tools
        .get(name)
        .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
    executor.execute(params, ctx).await
}
```

测试中的 mock 工具也需要同步更新签名：

```rust
async fn execute(
    &self,
    _params: serde_json::Value,
    _ctx: &ExecutionContext,
) -> Result<ExecutionResult, ToolError> {
    self.result.clone()
}
```

### 2. `BashTool::execute`（`crates/tools/src/bash.rs`）

```rust
async fn execute(
    &self,
    params: serde_json::Value,
    _ctx: &ExecutionContext,  // BashTool 当前不使用 ctx，直接忽略
) -> Result<ExecutionResult, ToolError> {
    // 实现保持不变，仅添加 ctx 参数
}
```

### 3. 更新测试中的 mock 工具

`crates/tools/src/set.rs` 中的 `MockTool`：

```rust
async fn execute(
    &self,
    _params: serde_json::Value,
    _ctx: &ExecutionContext,
) -> Result<ExecutionResult, ToolError> {
    self.result.clone()
}
```

`crates/runtime/src/control_loop.rs` 中的 `NoopTool`（如果存在）：

```rust
async fn execute(
    &self,
    _params: serde_json::Value,
    _ctx: &ExecutionContext,
) -> Result<ExecutionResult, ToolError> {
    Ok(ExecutionResult { stdout: "ok".into(), stderr: String::new(), exit_code: 0 })
}
```

---

## 测试要求

### 单元测试

**`lattice-tools`**：

- `ToolSet::execute` 正确透传 `ExecutionContext` 给工具
- `BashTool::execute` 忽略 ctx，行为与之前完全一致

**`lattice-runtime`**：

- `NoopTool` 测试 mock 工具更新签名后仍正常工作
- `ControlLoop` 中的所有内联 mock 工具签名同步更新

---

## 验收标准

- [ ] `ToolSet::execute` 签名含 `ctx: &ExecutionContext`
- [ ] `BashTool::execute` 签名同步更新，行为不变
- [ ] `crates/tools/src/set.rs` 中所有测试通过（mock 工具签名已更新）
- [ ] `crates/runtime/src/control_loop.rs` 中所有内联 mock 工具签名已更新，测试通过
- [ ] `cargo fmt` + `cargo clippy` 通过
