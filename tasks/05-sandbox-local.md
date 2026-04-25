# 任务 5：实现 LocalSandbox

## 目标

在 `lattice-sandbox-local` 中实现 `Sandbox` trait 的本地子进程版本。

## 分支

`feat/sandbox-local`

## 依赖

- 任务 3（core 类型和 trait）

## 具体内容

### 实现方式

使用 `tokio::process::Command` 在本地执行命令。

```rust
pub struct LocalSandbox {
    /// 命令执行的工作目录（可选）
    work_dir: Option<PathBuf>,
    /// 执行超时时间
    timeout: Duration,
}
```

### 实现要点

1. `execute` 方法接收 tool name 和 params
2. 初版只支持 `bash` 工具：从 params 中提取 `command` 字段，用 `sh -c` 执行
3. 捕获 stdout、stderr、exit_code
4. 超时控制：使用 `tokio::time::timeout` 包裹执行
5. 超时时返回 `SandboxError::Timeout`

### 测试用例

- 执行简单命令 `echo hello` → 验证 stdout
- 执行失败命令 → 验证 exit_code 非零
- 超时测试 — 执行 `sleep 10` 配合短超时 → 验证返回 Timeout 错误
- stderr 捕获 — 执行输出到 stderr 的命令

## 验收标准

- [ ] `cargo test -p lattice-sandbox-local` 通过
- [ ] `cargo clippy -p lattice-sandbox-local` 零警告
- [ ] 至少 4 个测试用例覆盖上述场景
- [ ] 所有 pub 类型和方法有英文 doc comment

## 安全说明

初版不做进程隔离（不使用 namespace/cgroup），仅用于开发验证。生产级沙箱隔离留给 sandbox-docker 等后续 crate。
