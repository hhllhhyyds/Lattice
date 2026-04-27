# 任务 20-5：lattice-skill crate — SkillDefinition + SkillTool + SkillToolSet + SkillLoader

## 目标

新建 `lattice-skill` crate，实现 Skill 系统的核心逻辑：skill 定义解析、skill 工具集管理、skill 工具执行器、skill 动态加载器。

**设计原则**：遵循 [Anthropic Agent Skills 开放标准](https://www.agentskills.com)，以 SKILL.md 为唯一事实来源。

## 分支

`feat/skill-crate`

## 依赖

- 任务 20-1（ExecutionContext + ToolExecutor 新签名）
- 任务 20-2（SessionStore 子 session 方法）
- 任务 20-4（ControlLoop builder + depth）

---

## crate 结构

```
crates/skill/
├── Cargo.toml
└── src/
    ├── lib.rs            # re-exports
    ├── definition.rs     # SkillDefinition（从 SKILL.md frontmatter 解析）
    ├── tool_set.rs       # SkillToolSet（继承 + 排除 + 覆盖）
    ├── tool.rs           # SkillTool（实现 ToolExecutor）
    └── loader.rs         # 动态加载：扫描目录，解析 SKILL.md，注册 skill
```

---

## 模块设计

### `src/definition.rs` — SkillDefinition

从 SKILL.md YAML frontmatter 解析。标准字段遵循 Anthropic Agent Skills 开放标准，Lattice 扩展在 `x-lattice` 命名空间下。

```rust
//! Skill definition — parsed from SKILL.md YAML frontmatter.

use indexmap::IndexMap;
use serde::Deserialize;

/// Parsed from SKILL.md YAML frontmatter.
/// Standard fields follow the Anthropic Agent Skills open standard.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillDefinition {
    /// Skill identifier. Max 64 chars. Kebab-case recommended (standard).
    pub name: String,
    /// Human-readable description for LLM tool selection. Max 1024 chars (standard).
    pub description: String,
    /// Compatible agent platforms (standard, optional).
    pub compatibility: Option<String>,
    /// Pre-approved tools this skill is allowed to use (standard, optional).
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,
    /// Metadata block (author, version, tags, etc.) (standard, optional).
    pub metadata: Option<SkillMetadata>,
    /// Lattice-specific extensions under x-lattice namespace.
    #[serde(rename = "x-lattice")]
    pub lattice: Option<LatticeExtension>,
}

/// Standard metadata fields.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    pub author: Option<String>,
    pub version: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(rename = "short-description")]
    pub short_description: Option<String>,
}

/// Lattice-specific extensions (x-lattice namespace).
#[derive(Debug, Clone, Deserialize)]
pub struct LatticeExtension {
    pub params: Option<IndexMap<String, ParamSchema>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParamSchema {
    #[serde(rename = "type")]
    pub type_: String,
    pub description: Option<String>,
    pub required: Option<bool>,
    pub default: Option<serde_json::Value>,
}

impl SkillDefinition {
    /// Validate the definition after parsing.
    pub fn validate(&self) -> Result<(), SkillValidationError> {
        if self.name.is_empty() {
            return Err(SkillValidationError::MissingField("name".into()));
        }
        if self.name.len() > 64 {
            return Err(SkillValidationError::FieldTooLong { field: "name".into(), max: 64, actual: self.name.len() });
        }
        if self.description.len() > 1024 {
            return Err(SkillValidationError::FieldTooLong { field: "description".into(), max: 1024, actual: self.description.len() });
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SkillValidationError {
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("field '{field}' exceeds max length {max} (got {actual})")]
    FieldTooLong { field: String, max: usize, actual: usize },
}
```

### `src/tool_set.rs` — SkillToolSet

Skill 的工具集：继承父工具集 + 添加自有工具 + 排除父工具，三者优先级正确。

```rust
//! Skill tool set — inherits, excludes, and overrides parent tools.

use std::collections::HashSet;
use std::sync::Arc;

use lattice_core::{ExecutionContext, ExecutionResult, ToolDescription, ToolError, ToolExecutor};
use lattice_tools::ToolSet;

/// Skill tool set supporting three operations on the parent ToolSet:
///
/// - **Inherit**: all parent tools are visible unless excluded
/// - **Exclude**: specific parent tools can be hidden
/// - **Override/Own**: skill-defined tools take precedence over parent tools
pub struct SkillToolSet {
    inherited: Arc<ToolSet>,
    own: ToolSet,
    excluded: HashSet<String>,
}

impl SkillToolSet {
    pub fn build(
        parent: Arc<ToolSet>,
        own_tools: Vec<Box<dyn ToolExecutor>>,
        exclude: Vec<String>,
    ) -> Self {
        let excluded: HashSet<_> = exclude.into_iter().collect();
        let mut own = ToolSet::new();
        for tool in own_tools {
            own.register_boxed(tool).expect("skill own tool name conflict");
        }
        Self { inherited: parent, own, excluded }
    }

    /// All tool descriptions visible to this skill.
    pub fn descriptions(&self) -> Vec<ToolDescription> {
        let mut all: Vec<_> = self.inherited.descriptions()
            .into_iter()
            .filter(|d| !self.excluded.contains(&d.name))
            .collect();
        all.extend(self.own.descriptions());
        all
    }

    pub async fn execute(
        &self,
        name: &str,
        params: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError> {
        if self.own.contains(name) {
            self.own.execute(name, params, ctx).await
        } else if !self.excluded.contains(name) {
            self.inherited.execute(name, params, ctx).await
        } else {
            Err(ToolError::NotFound(name.to_string()))
        }
    }

    pub fn len(&self) -> usize {
        self.own.len() + self.inherited.len() - self.excluded.len()
    }
}
```

### `src/tool.rs` — SkillTool

实现 `ToolExecutor` trait。负责子 session 创建、子 ControlLoop 执行、结果回传。

```rust
//! Skill tool — delegates to a child ControlLoop with a separate session.

use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{
    Actor, EventFilter, EventPayload, ExecutionContext, ExecutionResult,
    LLMClient, MAX_SKILL_DEPTH, SessionId, SessionStore, ToolDescription, ToolError, ToolExecutor,
};

use super::definition::SkillDefinition;
use super::tool_set::SkillToolSet;

/// A tool that wraps a skill — executes it as a child ControlLoop.
pub struct SkillTool {
    definition: SkillDefinition,
    /// Markdown body of SKILL.md — used as system prompt for the skill agent.
    system_prompt: String,
    parent_tools: Arc<lattice_tools::ToolSet>,
    llm: Arc<dyn LLMClient>,
}

impl SkillTool {
    pub fn new(
        definition: SkillDefinition,
        system_prompt: String,
        parent_tools: Arc<lattice_tools::ToolSet>,
        llm: Arc<dyn LLMClient>,
    ) -> Self {
        Self { definition, system_prompt, parent_tools, llm }
    }

    fn tool_name(&self) -> String {
        format!("skill:{}", self.definition.name)
    }
}

#[async_trait]
impl ToolExecutor for SkillTool {
    fn description(&self) -> ToolDescription {
        ToolDescription {
            name: self.tool_name(),
            description: self.definition.description.clone(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "Input to pass to the skill agent",
                    }
                },
                "required": ["input"]
            }),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError> {
        // 1. Depth check
        if ctx.depth >= MAX_SKILL_DEPTH {
            return Err(ToolError::MaxDepthExceeded(MAX_SKILL_DEPTH));
        }

        // 2. Create child session
        let skill_name = &self.definition.name;
        let (child_session_id, child_store) = ctx
            .store
            .create_child_session(ctx.session_id, skill_name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // 3. Record SkillInvoked in parent session
        ctx.store
            .append_event(
                ctx.session_id,
                EventPayload::SkillInvoked { skill_name: skill_name.clone(), child_session_id },
                Actor::Harness,
                Some(ctx.trigger_event_id),
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // 4. Inject initial user message into child session
        let input = params.get("input").and_then(|v| v.as_str()).unwrap_or("").to_string();
        child_store
            .append_event(child_session_id, EventPayload::UserMessage { content: input }, Actor::Harness, None)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // 5. Build skill tool set
        let excluded = self.definition.allowed_tools
            .as_ref()
            .map(|v| v.iter().cloned().collect::<HashSet<_>>())
            .unwrap_or_default();
        let skill_tool_set = SkillToolSet::build(self.parent_tools.clone(), vec![], excluded.into_iter().collect());

        // 6. Spawn child ControlLoop with depth + 1
        let child_loop = crate::ControlLoop::builder()
            .store(child_store.clone())
            .llm(self.llm.clone())
            .tools(Arc::new(skill_tool_set))
            .system_prompt(&self.system_prompt)
            .depth(ctx.depth + 1)
            .build();

        child_loop.run(child_session_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // 7. Extract FinalAnswer from child session
        let events = child_store
            .get_events(child_session_id, &EventFilter::default())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let answer = events.iter()
            .find_map(|e| match &e.payload {
                EventPayload::FinalAnswer { answer } => Some(answer.clone()),
                _ => None,
            })
            .ok_or_else(|| ToolError::ExecutionFailed("skill produced no FinalAnswer".into()))?;

        // 8. Record SkillCompleted in parent session
        ctx.store
            .append_event(
                ctx.session_id,
                EventPayload::SkillCompleted { skill_name: skill_name.clone(), child_session_id },
                Actor::Harness,
                Some(ctx.trigger_event_id),
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ExecutionResult { stdout: answer, stderr: String::new(), exit_code: 0 })
    }
}
```

### `src/loader.rs` — SkillLoader

动态加载 skills/ 目录下的所有 skill。单个失败仅 warn，不影响其他。

```rust
//! Skill loader — scans a directory and loads all SKILL.md definitions.

use std::path::PathBuf;

use lattice_core::{LLMClient, ToolExecutor};
use lattice_tools::ToolSet;

use super::definition::SkillDefinition;
use super::tool::SkillTool;

/// Loads skills from a directory on the filesystem.
///
/// Each subdirectory of `skills_dir` is one skill; SKILL.md is the only required file.
/// Follows the Anthropic Agent Skills directory structure.
pub struct SkillLoader { skills_dir: PathBuf }

impl SkillLoader {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self { skills_dir: skills_dir.into() }
    }

    /// Scan `skills_dir`, load every subdirectory containing a SKILL.md.
    /// One failed skill does not abort loading others — failures are logged at WARN level.
    pub async fn load_all(
        &self,
        parent_tools: Arc<ToolSet>,
        llm: Arc<dyn LLMClient>,
    ) -> Vec<SkillTool> {
        let mut skills = vec![];
        let mut entries = match tokio::fs::read_dir(&self.skills_dir).await {
            Ok(e) => e,
            Err(e) => { tracing::warn!("skills directory not found: {:?}: {}", self.skills_dir, e); return skills; }
        };
        while let Some(entry) = entries.next_entry().await.expect("read_dir ok") {
            let path = entry.path();
            if path.is_dir() {
                match self.load_one(&path, parent_tools.clone(), llm.clone()).await {
                    Ok(skill) => skills.push(skill),
                    Err(e) => tracing::warn!("skipping skill at {:?}: {}", path, e),
                }
            }
        }
        skills
    }

    async fn load_one(
        &self,
        dir: &PathBuf,
        parent_tools: Arc<ToolSet>,
        llm: Arc<dyn LLMClient>,
    ) -> Result<SkillTool, SkillLoadError> {
        let skill_md_path = dir.join("SKILL.md");
        let raw = tokio::fs::read_to_string(&skill_md_path)
            .await
            .map_err(|_| SkillLoadError::MissingSkillMd(skill_md_path.clone()))?;

        let (frontmatter, body) = parse_skill_md(&raw)
            .ok_or_else(|| SkillLoadError::MissingFrontmatter(skill_md_path.clone()))?;

        let definition: SkillDefinition = serde_yaml::from_str(frontmatter)
            .map_err(SkillLoadError::YamlParse)?;

        definition.validate().map_err(SkillLoadError::Validation)?;

        let system_prompt = body.trim().to_string();
        Ok(SkillTool::new(definition, system_prompt, parent_tools, llm))
    }
}

/// Split "---\nfrontmatter\n---\nbody" into (frontmatter, body).
pub fn parse_skill_md(raw: &str) -> Option<(&str, &str)> {
    let rest = raw.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((&rest[..end], &rest[end + 5..]))
}

#[derive(Debug, thiserror::Error)]
pub enum SkillLoadError {
    #[error("SKILL.md not found at {0:?}")]
    MissingSkillMd(PathBuf),
    #[error("SKILL.md missing YAML frontmatter at {0:?}")]
    MissingFrontmatter(PathBuf),
    #[error("validation error: {0}")]
    Validation(#[from] super::definition::SkillValidationError),
    #[error("yaml parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

### `Cargo.toml`

```toml
[package]
name = "lattice-skill"
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
rust-version.workspace = true

[dependencies]
lattice-core    = { path = "../core" }
lattice-tools   = { path = "../tools" }
lattice-runtime = { path = "../runtime" }
async-trait  = { workspace = true }
serde        = { workspace = true, features = ["derive"] }
serde_json   = { workspace = true }
serde_yaml   = { workspace = true }
indexmap     = { workspace = true, features = ["serde"] }
tokio        = { workspace = true, features = ["fs"] }
thiserror    = { workspace = true }
tracing      = { workspace = true }
```

---

## 测试要求

### 单元测试

**`lattice-skill`**：

- `SkillDefinition::validate()`：name 为空报错；name 超 64 字符报错；description 超 1024 字符报错；正常值通过
- `SkillToolSet`：
  - own 工具优先于继承工具（同名时）
  - excluded 工具返回 NotFound
  - `descriptions()` 正确排除 excluded 工具
- `SkillTool::execute`：`depth >= MAX_SKILL_DEPTH` 返回 `MaxDepthExceeded`
- `SkillLoader`：
  - 缺少 SKILL.md → `MissingSkillMd`
  - 缺少 frontmatter → `MissingFrontmatter`
  - 单 skill 失败不影响其他 skill
- `parse_skill_md`：
  - 正确拆分 `"---\nYAML\n---\nMD"` → `("YAML", "MD")`
  - 缺少 `---` → `None`

### 集成测试

- meta agent 调用 skill，父 session 有 `SkillInvoked` + `SkillCompleted` 事件，子 session 有完整事件链
- skill 深度超限返回 `MaxDepthExceeded`，父 agent 收到 `ToolCallError` 事件，整体不崩溃

---

## 验收标准

- [ ] `lattice-skill` crate 编译通过，包含 `SkillDefinition`（含验证）、`SkillToolSet`（继承/exclude/覆盖）、`SkillTool`（实现 ToolExecutor）、`SkillLoader`（扫描 + 解析 SKILL.md）
- [ ] `SkillTool::execute`：depth 检查 → 子 session 创建 → 子 ControlLoop 执行 → 结果回传 → 父子 session 事件记录
- [ ] `SkillLoader` 单 skill 失败仅 warn 不 panic
- [ ] `MAX_SKILL_DEPTH = 8`，超限返回 `ToolError::MaxDepthExceeded`
- [ ] 所有上述单元测试通过
- [ ] `cargo fmt` + `cargo clippy` + `cargo test -p lattice-skill` 通过
