//! Skill tool set supporting inherit/exclude/override.

use std::collections::HashSet;
use std::sync::Arc;

use lattice_core::{ExecutionContext, ExecutionResult, ToolDescription, ToolError, ToolExecutor};
use lattice_tools::ToolSet;

/// Tool set visible to a skill.
pub struct SkillToolSet {
    inherited: Arc<ToolSet>,
    own: ToolSet,
    excluded: HashSet<String>,
}

impl SkillToolSet {
    #[must_use]
    pub fn build(
        parent: Arc<ToolSet>,
        own_tools: Vec<Arc<dyn ToolExecutor>>,
        exclude: Vec<String>,
    ) -> Self {
        let mut own = ToolSet::new();
        for tool in own_tools {
            own.register_arc(tool)
                .expect("skill own tool name conflict should be prevented by caller");
        }

        Self {
            inherited: parent,
            own,
            excluded: exclude.into_iter().collect(),
        }
    }

    /// All tool descriptions visible to this skill.
    #[must_use]
    pub fn descriptions(&self) -> Vec<ToolDescription> {
        let mut all: Vec<_> = self
            .inherited
            .descriptions()
            .into_iter()
            .filter(|d| !self.excluded.contains(&d.name) && !self.own.contains(&d.name))
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

    /// Materialize the visible tools as a concrete `ToolSet`.
    pub fn into_tool_set(self) -> Result<ToolSet, ToolError> {
        let mut merged = ToolSet::new();

        for desc in self.inherited.descriptions() {
            if self.excluded.contains(&desc.name) || self.own.contains(&desc.name) {
                continue;
            }
            let tool = self
                .inherited
                .tool(&desc.name)
                .ok_or_else(|| ToolError::NotFound(desc.name.clone()))?;
            merged.register_arc(tool)?;
        }

        for desc in self.own.descriptions() {
            let tool = self
                .own
                .tool(&desc.name)
                .ok_or_else(|| ToolError::NotFound(desc.name.clone()))?;
            merged.register_arc(tool)?;
        }

        Ok(merged)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.descriptions().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use lattice_core::{
        Actor, ChildSessionInfo, Event, EventFilter, EventPayload, ExecutionContext,
        ExecutionResult, SessionId, SessionStore, StoreError, ToolDescription, ToolError,
        ToolExecutor,
    };

    use super::SkillToolSet;
    use lattice_tools::ToolSet;

    struct MockStore;

    #[async_trait]
    impl SessionStore for MockStore {
        async fn create_session(&self) -> Result<SessionId, StoreError> {
            Ok(SessionId::new_v4())
        }

        async fn delete_session(&self, _session_id: SessionId) -> Result<(), StoreError> {
            Ok(())
        }

        async fn append_event(
            &self,
            _session_id: SessionId,
            _payload: EventPayload,
            _actor: Actor,
            _parent_event_id: Option<lattice_core::EventId>,
        ) -> Result<lattice_core::EventId, StoreError> {
            Ok(lattice_core::EventId::new_v4())
        }

        async fn get_events(
            &self,
            _session_id: SessionId,
            _filter: &EventFilter,
        ) -> Result<Vec<Event>, StoreError> {
            Ok(Vec::new())
        }

        async fn create_child_session(
            &self,
            _parent_session_id: SessionId,
            _skill_name: &str,
        ) -> Result<(SessionId, Arc<dyn SessionStore>), StoreError> {
            Ok((SessionId::new_v4(), Arc::new(MockStore)))
        }

        async fn child_sessions(
            &self,
            _parent_session_id: SessionId,
        ) -> Result<Vec<ChildSessionInfo>, StoreError> {
            Ok(Vec::new())
        }

        async fn latest_event_id(
            &self,
            _session_id: SessionId,
        ) -> Result<Option<lattice_core::EventId>, StoreError> {
            Ok(None)
        }
    }

    #[derive(Clone)]
    struct StaticTool {
        name: String,
        output: String,
    }

    #[async_trait]
    impl ToolExecutor for StaticTool {
        fn description(&self) -> ToolDescription {
            ToolDescription {
                name: self.name.clone(),
                description: "tool".into(),
                parameters_schema: serde_json::json!({}),
            }
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ExecutionContext,
        ) -> Result<ExecutionResult, ToolError> {
            Ok(ExecutionResult {
                stdout: self.output.clone(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
    }

    fn ctx() -> ExecutionContext {
        ExecutionContext {
            session_id: SessionId::new_v4(),
            trigger_event_id: lattice_core::EventId::new_v4(),
            store: Arc::new(MockStore),
            depth: 0,
        }
    }

    #[tokio::test]
    async fn own_tool_overrides_parent() {
        let mut parent = ToolSet::new();
        parent
            .register(StaticTool {
                name: "echo".into(),
                output: "parent".into(),
            })
            .unwrap();

        let skill_set = SkillToolSet::build(
            Arc::new(parent),
            vec![Arc::new(StaticTool {
                name: "echo".into(),
                output: "own".into(),
            })],
            vec![],
        );

        let result = skill_set
            .execute("echo", serde_json::json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(result.stdout, "own");
    }

    #[tokio::test]
    async fn excluded_tool_returns_not_found() {
        let mut parent = ToolSet::new();
        parent
            .register(StaticTool {
                name: "grep".into(),
                output: "parent".into(),
            })
            .unwrap();

        let skill_set = SkillToolSet::build(Arc::new(parent), vec![], vec!["grep".into()]);
        let err = skill_set
            .execute("grep", serde_json::json!({}), &ctx())
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(name) if name == "grep"));
    }

    #[test]
    fn descriptions_hide_excluded_and_overridden_tools() {
        let mut parent = ToolSet::new();
        parent
            .register(StaticTool {
                name: "grep".into(),
                output: "parent".into(),
            })
            .unwrap();
        parent
            .register(StaticTool {
                name: "echo".into(),
                output: "parent".into(),
            })
            .unwrap();

        let skill_set = SkillToolSet::build(
            Arc::new(parent),
            vec![Arc::new(StaticTool {
                name: "echo".into(),
                output: "own".into(),
            })],
            vec!["grep".into()],
        );

        let names: Vec<_> = skill_set
            .descriptions()
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, vec!["echo".to_string()]);
    }
}
