# 任务 1：初始化 workspace + crate 骨架

## 目标

搭建 Cargo workspace，创建所有 crate 的空骨架，确保 `cargo build` 通过。

## 分支

`feat/init-workspace`

## 具体内容

1. 根目录 `Cargo.toml` 定义 workspace，包含所有 crate 和 example
2. 在 `[workspace.dependencies]` 中统一管理公共依赖版本
3. 创建以下 crate（各自有 `Cargo.toml` + 空 `src/lib.rs`）：
   - `crates/core/` — `lattice-core`
   - `crates/runtime/` — `lattice-runtime`（依赖 lattice-core）
   - `crates/store-memory/` — `lattice-store-memory`（依赖 lattice-core）
   - `crates/sandbox-local/` — `lattice-sandbox-local`（依赖 lattice-core）
4. 创建 example：
   - `examples/hello-agent/` — `hello-agent`（依赖 runtime、store-memory、sandbox-local）
5. 配置 `.gitignore`（target/、.DS_Store 等）
6. 配置 `rustfmt.toml`（统一格式化风格）
7. 配置 `clippy.toml` 或 workspace 级别的 clippy lint

## 公共依赖（workspace.dependencies）

```toml
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
mockall = "0.13"
```

## 验收标准

- [ ] `cargo build` 通过
- [ ] `cargo test` 通过（无测试也不报错）
- [ ] `cargo clippy` 零警告
- [ ] `cargo fmt --check` 通过
- [ ] 各 crate 之间依赖关系正确
