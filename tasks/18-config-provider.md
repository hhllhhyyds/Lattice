# 任务 18：配置管理与多 Provider 支持

> ⚠️ **STATUS: DRAFT — 未经人工 review，内容可能调整。**

## 目标

实现服务器配置文件机制，支持声明式配置多个 LLM provider，运行时按名称选择。让服务器从"硬编码环境变量"升级为"配置驱动"。

## 分支

`feat/config-provider`

## 依赖

- 任务 15（Agent 执行 API — provider 工厂函数）
- 任务 12（facade crate — feature flags 体系）

## 具体内容

### 1. 配置文件格式

`lattice.toml`（搜索顺序：`./lattice.toml` → `~/.config/lattice/lattice.toml`）：

```toml
[server]
host = "0.0.0.0"
port = 3000

[server.cors]
allowed_origins = ["*"]   # 生产环境应收紧

# 默认 provider（POST /v1/sessions/:id/messages 不指定时使用）
default_provider = "openai-local"

# Provider 定义
[[providers]]
name = "openai-local"
kind = "openai"
api_key_env = "OPENAI_API_KEY"        # 从此环境变量读取 API key
base_url = "http://localhost:8000/v1"
default_model = "gpt-4o"

[[providers]]
name = "anthropic"
kind = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
default_model = "claude-sonnet-4-20250514"

[[providers]]
name = "openai-official"
kind = "openai"
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"
default_model = "gpt-4o"

[sandbox]
kind = "local"    # 目前只支持 local，后续支持 docker
# timeout_seconds = 30   # 预留

[logging]
level = "info"    # trace/debug/info/warn/error
format = "pretty" # pretty/json
```

### 2. 配置解析

```rust
#[derive(Debug, Deserialize)]
pub struct LatticeConfig {
    pub server: ServerConfig,
    pub providers: Vec<ProviderConfig>,
    pub default_provider: String,
    pub sandbox: SandboxConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub kind: ProviderKind,       // openai | anthropic
    pub api_key_env: String,       // 环境变量名
    pub base_url: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
}
```

使用 `toml` crate 解析，`serde` 反序列化。

### 3. Provider 注册表

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderConfig>,
    default_provider: String,
}

impl ProviderRegistry {
    /// Create an LLMClient by provider name.
    /// Returns an error if the provider's backend was not enabled at compile time.
    pub fn create_client(
        &self,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Arc<dyn LLMClient>, ConfigError>;

    /// List all available providers (only those enabled at compile time).
    pub fn list_providers(&self) -> Vec<ProviderInfo>;
}
```

Provider 注册使用条件编译，只注册编译时启用的后端：

```rust
fn register_provider(config: &ProviderConfig) -> Result<Arc<dyn LLMClient>, ConfigError> {
    match config.kind {
        #[cfg(feature = "anthropic")]
        ProviderKind::Anthropic => { /* create AnthropicClient */ },

        #[cfg(feature = "openai")]
        ProviderKind::OpenAI => { /* create OpenAIClient */ },

        #[allow(unreachable_patterns)]
        _ => Err(ConfigError::ProviderNotCompiled {
            name: config.name.clone(),
            kind: format!("{:?}", config.kind),
        }),
    }
}
```

### 4. API 端点

#### GET /v1/providers — 列出可用 provider

响应 200：
```json
{
    "providers": [
        {
            "name": "openai-local",
            "kind": "openai",
            "default_model": "gpt-4o",
            "is_default": true
        },
        {
            "name": "anthropic",
            "kind": "anthropic",
            "default_model": "claude-sonnet-4-20250514",
            "is_default": false
        }
    ]
}
```

### 5. 更新现有代码

- `main.rs`：启动时加载配置文件 → 创建 ProviderRegistry → 注入 AppState
- 任务 14 的 POST handler：从 ProviderRegistry 获取 LLMClient，不再直接读环境变量
- AppState 新增 `registry: Arc<ProviderRegistry>` 字段

### 6. CLI 参数

```bash
# 指定配置文件
lattice-server --config ./my-config.toml

# 覆盖端口
lattice-server --port 8080

# 环境变量覆盖
LATTICE_PORT=8080 lattice-server
```

优先级：CLI 参数 > 环境变量 > 配置文件 > 默认值。

使用 `clap` crate 解析 CLI 参数。

## 验收标准

- [ ] 支持从 `lattice.toml` 加载配置
- [ ] 支持配置多个 LLM provider
- [ ] POST /v1/sessions/:id/messages 可通过 `provider` 参数选择 provider
- [ ] GET /v1/providers 返回可用 provider 列表
- [ ] 不指定 provider 时使用默认配置
- [ ] CLI 参数和环境变量可覆盖配置文件
- [ ] 配置文件不存在时使用合理的默认值（等同于当前行为）
- [ ] API key 从环境变量读取，不硬编码在配置文件中
- [ ] 有测试覆盖配置解析和 ProviderRegistry
- [ ] 所有 pub 类型和方法有英文 doc comment
- [ ] `cargo clippy` 零警告

## 新增依赖

```toml
toml = "0.8"
clap = { version = "4", features = ["derive"] }
```

## 设计说明

- **为什么 TOML 不是 YAML/JSON？** Rust 生态标准配置格式，Cargo.toml 就是 TOML。可读性好，注释友好。
- **为什么 API key 用环境变量而不是直接写配置文件？** 安全最佳实践。配置文件可以版本控制，API key 不行。`api_key_env` 字段指定从哪个环境变量读取，灵活且安全。
- **为什么不在 core trait 层处理配置？** 配置是 server 的关注点，不是框架的关注点。core 只定义接口，不关心实例如何被创建。
