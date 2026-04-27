# 任务 20-6：skill feature + 示例 skill 目录 + meta-agent example

## 目标

将 lattice-skill 接入 facade crate，添加示例 skill 目录，创建 meta-agent example，实现端到端验证。

## 分支

`feat/skill-facade`

## 依赖

- 任务 20-5（lattice-skill crate）

---

## 修改内容

### 1. 更新 workspace members（`Cargo.toml` 根目录）

```toml
members = [
    # ... 已有的 ...
    "crates/skill",
]
```

### 2. 更新 Facade crate（`src/lib.rs` + `Cargo.toml` 根目录）

**`Cargo.toml`** 新增：

```toml
[features]
skill = ["dep:lattice-skill", "tools"]
full  = ["runtime", "store-memory", "sandbox-local", "llm-all", "tools", "skill"]

[dependencies]
lattice-skill = { path = "crates/skill", optional = true }
```

**`src/lib.rs`** 新增：

```rust
/// Skill system — skill loading, SkillTool, and SkillToolSet.
#[cfg(feature = "skill")]
pub use lattice_skill as skill;
```

### 3. 添加示例 skill 目录

```
skills/
└── web-research/
    ├── SKILL.md                # 完整示例（见下方）
    ├── scripts/                # 空占位符
    ├── references/            # 空占位符
    └── assets/                # 空占位符
```

**`skills/web-research/SKILL.md`**：

```markdown
---
name: web-research
description: >-
  Deep research on a topic using web search and content synthesis.
  Use when the user asks to research, investigate, or compile information
  on any subject. Returns structured findings with sources.
compatibility: Requires internet access
allowed-tools: bash http_fetch
metadata:
  author: lattice
  version: "1.0.0"
  short-description: Web research and synthesis specialist
---

# Web Research

You are a research specialist. Perform thorough research on the given topic.

## Input Requirements

- A research query from the user.
- If `depth` parameter is set, perform that many search iterations (1-5).

## Execution Steps

### Step 1: Initial Search
Use available search tools to find relevant sources for the query.
Aim for at least 5 distinct sources.

### Step 2: Deep Dive
For each promising source, extract key facts, data points, and quotes.
Cross-reference claims across multiple sources.

### Step 3: Synthesis
Compile findings into a structured summary with:
- Key findings (bullet points)
- Sources consulted (with URLs)
- Confidence level (high/medium/low)

## Output Format
Return a FinalAnswer with the structured summary.
Be concise but complete. Focus on accuracy over breadth.
```

### 4. 新建 `examples/meta-agent`

```
examples/meta-agent/
├── Cargo.toml
└── src/
    └── main.rs
```

**`Cargo.toml`**：

```toml
[package]
name = "meta-agent"
version.workspace = true
edition.workspace = true

[dependencies]
lattice = { path = "../..", features = ["runtime", "store-memory", "sandbox-local", "llm-anthropic", "tools", "skill"] }
tokio = { workspace = true }
anyhow = { workspace = true }
tracing-subscriber = { workspace = true }
```

**`src/main.rs`**：

```rust
//! Meta agent example — demonstrates skill delegation with mock LLM.
//!
//! Uses MockLLMClient to simulate:
//!   1. Meta agent decides to call "skill:web-research"
//!   2. Skill agent returns a FinalAnswer
//!   3. Meta agent returns the skill result as its own FinalAnswer

use std::sync::Arc;
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use lattice::prelude::*;
use lattice::skill::{SkillLoader, SkillTool};
use lattice::core::{Decision, Event, EventFilter, ExecutionResult, LLMClient, LLMError, ToolDescription};

// ── Mock LLM ──────────────────────────────────────────────────────────────

struct MetaLLM;
#[async_trait] impl LLMClient for MetaLLM {
    async fn decide(&self, _h: &[Event], _t: &[ToolDescription], _s: &str)
        -> Result<Decision, LLMError>
    {
        // Meta agent: decide to call the skill tool
        Ok(Decision::ToolCall {
            tool: "skill:web-research".into(),
            params: serde_json::json!({ "input": "latest Rust async runtime research" }),
        })
    }
}

struct SkillLLM(Arc<Mutex<usize>>);
impl SkillLLM {
    fn new() -> Self { Self(Arc::new(Mutex::new(0))) }
}
#[async_trait] impl LLMClient for SkillLLM {
    async fn decide(&self, _h: &[Event], _t: &[ToolDescription], _s: &str)
        -> Result<Decision, LLMError>
    {
        let mut step = self.0.lock().unwrap();
        *step += 1;
        if *step == 1 {
            Ok(Decision::Thinking { reasoning: "researching...".into() })
        } else {
            Ok(Decision::FinalAnswer {
                answer: "Rust async runtimes (Tokio, async-std, Smol) continue to evolve...".into(),
            })
        }
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let store = Arc::new(MemoryStore::new());
    let session_id = store.create_session().await?;
    store.append_event(
        session_id,
        EventPayload::UserMessage { content: "Research Rust async runtimes".into() },
        Actor::Harness,
        None,
    ).await?;

    // Use MockLLM or real LLM based on environment
    let llm: Arc<dyn LLMClient> = if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        Arc::new(lattice::llm_anthropic::AnthropicClient::from_env()?)
    } else {
        Arc::new(MetaLLM)
    };

    // Build parent tools (empty for this example, skills add themselves)
    let mut tools = ToolSet::new();

    // Load skills if the directory exists
    if std::path::Path::new("skills/").exists() {
        let loader = SkillLoader::new("skills/");
        let skills = loader.load_all(Arc::new(tools.clone()), llm.clone()).await;
        for skill in skills {
            tools.register(skill)?;
        }
    }

    let control_loop = ControlLoop::builder()
        .store(store.clone())
        .llm(llm)
        .tools(Arc::new(tools))
        .system_prompt("You are a meta agent. Use skills to delegate complex subtasks.")
        .build();

    let answer = control_loop.run(session_id).await?;
    println!("Final answer: {}", answer);

    // Inspect session tree
    let children = store.child_sessions(session_id).await?;
    println!("\nSession tree: {} child session(s)", children.len());
    for child in &children {
        println!("  Skill '{}': {}", child.skill_name, child.session_id);
    }

    Ok(())
}
```

---

## 验收标准

- [ ] workspace members 包含 `crates/skill`
- [ ] facade `skill` feature 正确导出 `lattice-skill`
- [ ] `skills/web-research/SKILL.md` 存在且符合规范
- [ ] `examples/meta-agent` 编译通过
- [ ] `cargo build --example meta-agent` 成功
- [ ] `cargo fmt` + `cargo clippy` 通过
