# 任务 12：Facade Crate + Feature Flags

## 目标

创建 `lattice` facade crate，通过 Rust feature flags 重导出所有子 crate。让消费者可以用一个依赖 + feature 组合来精确控制编译范围。同时更新 `lattice-server`（后续任务）的 feature 设计。

这是第四轮的基础设施任务——先把 feature flag 体系搭好，后续所有新 crate 都按此规范接入。

## 分支

`feat/facade-features`

## 依赖

- 任务 1-11（所有已有 crate 已完成）

## 具体内容

### 1. 创建 facade crate

在项目根目录创建 `src/lib.rs` 和根级 `Cargo.toml` 中添加 `[lib]` 配置（或在 `crates/` 下新建 `lattice/`，二选一——推荐根目录方案，因为 facade 代表整个项目的对外入口）。

**方案：根目录 facade**

根 `Cargo.toml` 从纯 workspace 升级为 workspace + lib：

```toml
[package]
name = "lattice"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
rust-version.workspace = true
description = "A Rust meta-framework for building AI agents, inspired by Anthropic's managed agents architecture."

[lib]
path = "src/lib.rs"

[features]
default = ["runtime", "store-memory", "sandbox-local"]

# Core runtime
runtime = ["dep:lattice-runtime"]

# Store backends
store-memory = ["dep:lattice-store-memory"]

# Sandbox implementations
sandbox-local = ["dep:lattice-sandbox-local"]

# LLM backends
llm-protocol = ["dep:lattice-llm-protocol"]
llm-anthropic = ["llm-protocol", "dep:lattice-llm-anthropic"]
llm-openai = ["llm-protocol", "dep:lattice-llm-openai"]
llm-all = ["llm-anthropic", "llm-openai"]

# Convenience
full = ["runtime", "store-memory", "sandbox-local", "llm-all"]

[dependencies]
lattice-core = { path = "crates/core" }
lattice-runtime = { path = "crates/runtime", optional = true }
lattice-store-memory = { path = "crates/store-memory", optional = true }
lattice-sandbox-local = { path = "crates/sandbox-local", optional = true }
lattice-llm-protocol = { path = "crates/llm-protocol", optional = true }
lattice-llm-anthropic = { path = "crates/llm-anthropic", optional = true }
lattice-llm-openai = { path = "crates/llm-openai", optional = true }
```

### 2. Facade `src/lib.rs`

```rust
//! # Lattice
//!
//! A Rust meta-framework for building AI agents.
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `runtime` | ✅ | ControlLoop and BasicSandboxRouter |
//! | `store-memory` | ✅ | In-memory SessionStore implementation |
//! | `sandbox-local` | ✅ | Local process Sandbox implementation |
//! | `llm-protocol` | ❌ | Common LLM protocol layer |
//! | `llm-anthropic` | ❌ | Anthropic Claude LLM backend |
//! | `llm-openai` | ❌ | OpenAI-compatible LLM backend |
//! | `llm-all` | ❌ | All LLM backends |
//! | `full` | ❌ | Everything |

/// Core traits and types. Always available.
pub use lattice_core as core;

/// ControlLoop implementation.
#[cfg(feature = "runtime")]
pub use lattice_runtime as runtime;

/// In-memory SessionStore.
#[cfg(feature = "store-memory")]
pub use lattice_store_memory as store_memory;

/// Local process Sandbox.
#[cfg(feature = "sandbox-local")]
pub use lattice_sandbox_local as sandbox_local;

/// Common LLM protocol layer.
#[cfg(feature = "llm-protocol")]
pub use lattice_llm_protocol as llm_protocol;

/// Anthropic Claude LLM backend.
#[cfg(feature = "llm-anthropic")]
pub use lattice_llm_anthropic as llm_anthropic;

/// OpenAI-compatible LLM backend.
#[cfg(feature = "llm-openai")]
pub use lattice_llm_openai as llm_openai;
```

### 3. 更新 examples 依赖

将 `hello-agent` 和 `real-agent` 的依赖从直接引用各子 crate 改为通过 facade：

**hello-agent/Cargo.toml：**
```toml
[dependencies]
lattice = { path = "../..", default-features = true }
# ... 其他依赖不变
```

**real-agent/Cargo.toml：**
```toml
[dependencies]
lattice = { path = "../..", features = ["full"] }
# ... 其他依赖不变
```

同时更新 examples 中的 `use` 语句，例如：
```rust
// Before:
use lattice_core::{...};
use lattice_runtime::{...};

// After:
use lattice::core::{...};
use lattice::runtime::{...};
```

### 4. 验证 feature 组合

确保以下编译场景全部通过：

```bash
# 默认 features
cargo build

# 无 features（只有 core）
cargo build --no-default-features

# 只要 openai
cargo build --no-default-features --features "runtime,store-memory,sandbox-local,llm-openai"

# 只要 anthropic
cargo build --no-default-features --features "runtime,store-memory,sandbox-local,llm-anthropic"

# 全家桶
cargo build --features full

# 所有 feature 组合的 clippy
cargo clippy --all-features -- -D warnings

# 所有 feature 组合的测试
cargo test --all-features

# 无 feature 的测试
cargo test --no-default-features
```

### 5. CI 更新

在 `.github/workflows/ci.yml` 中新增 feature 矩阵测试：

```yaml
strategy:
  matrix:
    features:
      - ""                    # default
      - "--no-default-features"
      - "--all-features"
      - "--no-default-features --features runtime,store-memory,sandbox-local,llm-anthropic"
      - "--no-default-features --features runtime,store-memory,sandbox-local,llm-openai"
```

## 验收标准

- [ ] `lattice` facade crate 创建完成
- [ ] feature flags 全部定义并可用
- [ ] `cargo build --no-default-features` 编译通过（只有 core）
- [ ] `cargo build --features full` 编译通过（全家桶）
- [ ] examples 改为使用 facade crate
- [ ] `cargo test --all-features` 全部通过
- [ ] `cargo test --no-default-features` 通过
- [ ] CI 增加 feature 矩阵测试
- [ ] `src/lib.rs` 有完整的 feature flag 文档注释
- [ ] `cargo clippy --all-features` 零警告
- [ ] 所有 pub 类型和方法有英文 doc comment

## 设计说明

- **为什么根目录 facade 而不是 `crates/lattice/`？** facade 代表项目的对外入口，放根目录语义最清晰。`cargo doc --open` 直接展示 `lattice` crate 文档。发布到 crates.io 时包名就是 `lattice`。
- **为什么 default 不包含 LLM 后端？** LLM 后端依赖 `reqwest`（引入 TLS、HTTP 等重量级依赖）。纯库用户（只想跑测试或用 mock）不需要这些。需要时明确 opt-in。
- **为什么 `llm-anthropic` 自动启用 `llm-protocol`？** LLM 后端依赖协议层做消息转换，这是强依赖关系，用户不需要手动管理。
