//! Skill definition parsed from `SKILL.md`.

use indexmap::IndexMap;
use serde::Deserialize;

/// Parsed from SKILL.md YAML frontmatter.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub compatibility: Option<String>,
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,
    pub metadata: Option<SkillMetadata>,
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

/// Lattice-specific extensions under the `x-lattice` namespace.
#[derive(Debug, Clone, Deserialize)]
pub struct LatticeExtension {
    pub params: Option<IndexMap<String, ParamSchema>>,
}

/// Schema for a skill input parameter.
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
        if self.name.trim().is_empty() {
            return Err(SkillValidationError::MissingField("name".into()));
        }
        if self.name.len() > 64 {
            return Err(SkillValidationError::FieldTooLong {
                field: "name".into(),
                max: 64,
                actual: self.name.len(),
            });
        }
        if self.description.trim().is_empty() {
            return Err(SkillValidationError::MissingField("description".into()));
        }
        if self.description.len() > 1024 {
            return Err(SkillValidationError::FieldTooLong {
                field: "description".into(),
                max: 1024,
                actual: self.description.len(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SkillValidationError {
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("field '{field}' exceeds max length {max} (got {actual})")]
    FieldTooLong {
        field: String,
        max: usize,
        actual: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> SkillDefinition {
        SkillDefinition {
            name: "web-research".into(),
            description: "Research the web".into(),
            compatibility: None,
            allowed_tools: None,
            metadata: None,
            lattice: None,
        }
    }

    #[test]
    fn validate_ok() {
        definition().validate().unwrap();
    }

    #[test]
    fn validate_rejects_empty_name() {
        let mut def = definition();
        def.name.clear();
        assert!(matches!(
            def.validate().unwrap_err(),
            SkillValidationError::MissingField(field) if field == "name"
        ));
    }

    #[test]
    fn validate_rejects_long_name() {
        let mut def = definition();
        def.name = "a".repeat(65);
        assert!(matches!(
            def.validate().unwrap_err(),
            SkillValidationError::FieldTooLong { field, .. } if field == "name"
        ));
    }

    #[test]
    fn validate_rejects_long_description() {
        let mut def = definition();
        def.description = "a".repeat(1025);
        assert!(matches!(
            def.validate().unwrap_err(),
            SkillValidationError::FieldTooLong { field, .. } if field == "description"
        ));
    }
}
