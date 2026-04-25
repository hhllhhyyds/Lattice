# 任务 13：Server crate 骨架 + 基础路由

> ⚠️ **STATUS: DRAFT — 未经人工 review，内容可能调整。**

## 目标

创建 `lattice-server` crate，搭建 axum HTTP 服务器骨架，实现健康检查端点和全局状态管理结构。Server 通过 feature flags 控制编译哪些 LLM provider。

## 分支

`feat/server-skeleton`

## 依赖

- 任务 12（facade crate + feature flags 体系）
- 任务 6（runtime — ControlLoop）
- 任务 4（store-memory — MemoryStore）

## 具体内容

### 1. 创建 `crates/server/` crate

```toml
[package]
name = "lattice-server"
version.workspace = true
edition.workspace = true

[features]
default = ["anthropic", "openai"]

# LLM provider features — 编译时控制启用哪些后端
anthropic = ["lattice-llm-anthropic", "lattice-llm-protocol"]
openai = ["lattice-llm-openai", "lattice-llm-protocol"]

[dependencies]
lattice-core = { path = "../core" }
lattice-runtime = { path = "../runtime" }
lattice-store-memory = { path = "../store-memory" }
lattice-sandbox-local = { path = "../sandbox-local" }
lattice-llm-protocol = { path = "../llm-protocol", optional = true }
lattice-llm-anthropic = { path = "../llm-anthropic", optional = true }
lattice-llm-openai = { path = "../llm-openai", optional = true }
axum = "0.8"
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tower-http = { version = "0.6", features = ["cors", "trace"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
```

### 2. AppState 设计

全局共享状态，通过 `Arc` 在 handler 间共享：

```rust
pub struct AppState {
    /// Session store (currently MemoryStore, swappable via trait)
    pub store: Arc<dyn SessionStore>,
    /// Active ControlLoop task handles
    pub active_runs: Arc<RwLock<HashMap<SessionId, RunHandle>>>,
    /// Server start time (for uptime reporting)
    pub started_at: chrono::DateTime<chrono::Utc>,
}

pub struct RunHandle {
    pub session_id: SessionId,
    pub status: RunStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub abort_handle: tokio::task::AbortHandle,
}

pub enum RunStatus {
    Running,
    Completed,
    Failed(String),
}
```

### 3. 路由结构

```rust
fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        // 后续任务逐步添加：
        // .nest("/v1", v1_routes())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())  // 开发阶段宽松 CORS
        .with_state(Arc::new(state))
}
```

### 4. 健康检查端点

```
GET /health → 200 OK
{
    "status": "ok",
    "version": env!("CARGO_PKG_VERSION"),
    "uptime_seconds": <elapsed>,
    "features": {
        "anthropic": true/false,
        "openai": true/false
    }
}
```

健康检查响应包含编译时启用的 feature 信息，方便客户端/运维了解当前构建能力：

```rust
fn enabled_features() -> serde_json::Value {
    serde_json::json!({
        "anthropic": cfg!(feature = "anthropic"),
        "openai": cfg!(feature = "openai"),
    })
}
```

### 5. 启动入口 `main.rs`

- 从环境变量读取 `LATTICE_HOST`（默认 `0.0.0.0`）和 `LATTICE_PORT`（默认 `3000`）
- 初始化 tracing subscriber
- 创建 MemoryStore 作为默认 SessionStore
- 启动时打印 banner、监听地址和启用的 features
- 启动 axum 服务器

### 6. 更新 workspace

- 在根 `Cargo.toml` 的 `members` 中添加 `crates/server`
- 在 `workspace.dependencies` 中添加 `axum` 和 `tower-http`

### 7. 验证 feature 组合编译

```bash
# 默认（anthropic + openai）
cargo build -p lattice-server

# 只要 anthropic
cargo build -p lattice-server --no-default-features --features anthropic

# 只要 openai
cargo build -p lattice-server --no-default-features --features openai

# 无 LLM provider（纯 HTTP 骨架）
cargo build -p lattice-server --no-default-features
```

## 验收标准

- [ ] `cargo build -p lattice-server` 通过
- [ ] `cargo build -p lattice-server --no-default-features` 通过
- [ ] `cargo run -p lattice-server` 启动 HTTP 服务器
- [ ] `curl http://localhost:3000/health` 返回 200 OK + JSON 响应（含 features 信息）
- [ ] AppState 结构定义清晰，支持后续扩展
- [ ] LLM provider 依赖受 feature flag 控制
- [ ] CORS 和 tracing 中间件已配置
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] `cargo clippy --all-features` 零警告
- [ ] 有基础的集成测试（用 axum::test 发请求验证 health 端点）

## 设计说明

- **为什么是独立 crate 而不是 example？** server 有自己的路由逻辑、状态管理、中间件等，复杂度远超 example。且后续会持续迭代，独立 crate 更易管理。
- **为什么先用 MemoryStore？** 保持 MVP 精神，先跑通整个链路。持久化存储是后续独立任务。
- **CORS 为什么 permissive？** 开发阶段不限制。生产化部署（任务 18）时收紧。
- **为什么 server 也有 feature flags？** 部署场景不同：如果只用 OpenAI 兼容接口，不需要编译 Anthropic SDK 的依赖。保持构建产物精简。
