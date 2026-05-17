//! Integration test: load the real skills/ directory via SkillLoader.

use std::sync::Arc;

use async_trait::async_trait;
use lattice::core::{Decision, Event, LLMClient, LLMError, ToolDescription, ToolExecutor};
use lattice::skill::SkillLoader;
use lattice::tools::ToolSet;

struct FakeLlm;

#[async_trait]
impl LLMClient for FakeLlm {
    async fn decide(
        &self,
        _history: &[Event],
        _available_tools: &[ToolDescription],
        _system_prompt: &str,
    ) -> Result<Decision, LLMError> {
        Ok(Decision::FinalAnswer {
            answer: "ok".into(),
        })
    }
}

#[tokio::test]
async fn skills_dir_loads_all_skills() {
    let loader = SkillLoader::new("./skills");
    let skills = loader
        .load_all(Arc::new(ToolSet::new()), Arc::new(FakeLlm))
        .await;

    // Print what we found
    for skill in &skills {
        let desc = skill.description();
        println!(
            "skill loaded: {} — {}",
            desc.name,
            &desc.description[..60.min(desc.description.len())]
        );
    }

    // We expect at least the 3 skills we've authored
    assert!(
        skills.len() >= 3,
        "expected at least 3 skills, got {}",
        skills.len()
    );

    let names: Vec<_> = skills
        .iter()
        .map(|s| s.description().name.clone())
        .collect();
    assert!(
        names.contains(&"skill:code-review".to_string()),
        "missing skill:code-review"
    );
    assert!(
        names.contains(&"skill:arcgen-pipeline".to_string()),
        "missing skill:arcgen-pipeline"
    );
}

#[tokio::test]
async fn each_skill_has_nonempty_description() {
    let loader = SkillLoader::new("./skills");
    let skills = loader
        .load_all(Arc::new(ToolSet::new()), Arc::new(FakeLlm))
        .await;

    for skill in &skills {
        let desc = skill.description();
        assert!(!desc.name.is_empty(), "skill has empty name");
        assert!(
            !desc.description.is_empty(),
            "skill '{}' has empty description",
            desc.name
        );
    }
}
