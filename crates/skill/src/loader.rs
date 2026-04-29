//! Skill loader scanning `skills/` directories.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lattice_core::LLMClient;
use lattice_tools::ToolSet;
use tracing::warn;

use crate::definition::{SkillDefinition, SkillValidationError};
use crate::tool::SkillTool;

/// Loads skills from a directory on the filesystem.
pub struct SkillLoader {
    skills_dir: PathBuf,
}

impl SkillLoader {
    #[must_use]
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
        }
    }

    /// Scan `skills_dir`, load every subdirectory containing a `SKILL.md`.
    pub async fn load_all(
        &self,
        parent_tools: Arc<ToolSet>,
        llm: Arc<dyn LLMClient>,
    ) -> Vec<SkillTool> {
        let mut skills = Vec::new();
        let mut entries = match tokio::fs::read_dir(&self.skills_dir).await {
            Ok(entries) => entries,
            Err(error) => {
                warn!(
                    "skills directory not found or unreadable: {:?}: {}",
                    self.skills_dir, error
                );
                return skills;
            }
        };

        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    match self
                        .load_one(&path, Arc::clone(&parent_tools), Arc::clone(&llm))
                        .await
                    {
                        Ok(skill) => skills.push(skill),
                        Err(error) => warn!("skipping skill at {:?}: {}", path, error),
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    warn!(
                        "error reading skills directory {:?}: {}",
                        self.skills_dir, error
                    );
                    break;
                }
            }
        }

        skills
    }

    pub async fn load_one(
        &self,
        dir: &Path,
        parent_tools: Arc<ToolSet>,
        llm: Arc<dyn LLMClient>,
    ) -> Result<SkillTool, SkillLoadError> {
        let skill_md_path = dir.join("SKILL.md");
        let raw = tokio::fs::read_to_string(&skill_md_path)
            .await
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    SkillLoadError::MissingSkillMd(skill_md_path.clone())
                } else {
                    SkillLoadError::Io(error)
                }
            })?;

        let (frontmatter, body) = parse_skill_md(&raw)
            .ok_or_else(|| SkillLoadError::MissingFrontmatter(skill_md_path.clone()))?;

        let definition: SkillDefinition =
            serde_yaml::from_str(frontmatter).map_err(SkillLoadError::YamlParse)?;
        definition.validate().map_err(SkillLoadError::Validation)?;

        Ok(SkillTool::new(
            definition,
            body.trim().to_string(),
            parent_tools,
            llm,
        ))
    }
}

/// Split `"---\nfrontmatter\n---\nbody"` into `(frontmatter, body)`.
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
    Validation(SkillValidationError),
    #[error("yaml parse error: {0}")]
    YamlParse(serde_yaml::Error),
    #[error("io error: {0}")]
    Io(std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use lattice_core::{Decision, Event, LLMClient, LLMError, ToolDescription, ToolExecutor};
    use lattice_tools::ToolSet;
    use tempfile::tempdir;

    use super::*;

    struct StaticLlm;

    #[async_trait]
    impl LLMClient for StaticLlm {
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

    #[test]
    fn parse_skill_md_splits_frontmatter() {
        let raw = "---\nname: demo\ndescription: Demo\n---\n# Body";
        let (frontmatter, body) = parse_skill_md(raw).unwrap();
        assert!(frontmatter.contains("name: demo"));
        assert_eq!(body, "# Body");
    }

    #[test]
    fn parse_skill_md_missing_frontmatter_returns_none() {
        assert!(parse_skill_md("# No frontmatter").is_none());
    }

    #[tokio::test]
    async fn load_one_missing_skill_md() {
        let dir = tempdir().unwrap();
        let loader = SkillLoader::new(dir.path());
        let result = loader
            .load_one(dir.path(), Arc::new(ToolSet::new()), Arc::new(StaticLlm))
            .await;
        assert!(matches!(result, Err(SkillLoadError::MissingSkillMd(_))));
    }

    #[tokio::test]
    async fn load_one_missing_frontmatter() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("demo");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Demo\nbody").unwrap();

        let loader = SkillLoader::new(dir.path());
        let result = loader
            .load_one(&skill_dir, Arc::new(ToolSet::new()), Arc::new(StaticLlm))
            .await;
        assert!(matches!(result, Err(SkillLoadError::MissingFrontmatter(_))));
    }

    #[tokio::test]
    async fn load_all_skips_invalid_skill() {
        let dir = tempdir().unwrap();

        let valid = dir.path().join("valid");
        std::fs::create_dir(&valid).unwrap();
        std::fs::write(
            valid.join("SKILL.md"),
            "---\nname: valid\ndescription: Valid skill\n---\nUse this skill.",
        )
        .unwrap();

        let invalid = dir.path().join("invalid");
        std::fs::create_dir(&invalid).unwrap();
        std::fs::write(invalid.join("SKILL.md"), "# Invalid").unwrap();

        let loader = SkillLoader::new(dir.path());
        let skills = loader
            .load_all(Arc::new(ToolSet::new()), Arc::new(StaticLlm))
            .await;

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description().name, "skill:valid");
    }
}
