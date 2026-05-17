//! Skill tool delegating work to a child control loop.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{
    Actor, EventFilter, EventPayload, ExecutionContext, ExecutionResult, LLMClient,
    ToolDescription, ToolError, ToolExecutor, MAX_SKILL_DEPTH,
};
use lattice_runtime::ControlLoop;
use lattice_tools::ToolSet;

use crate::definition::{ParamSchema, SkillDefinition};
use crate::tool_set::SkillToolSet;

/// A tool that wraps a skill definition.
pub struct SkillTool {
    definition: SkillDefinition,
    system_prompt: String,
    parent_tools: Arc<ToolSet>,
    llm: Arc<dyn LLMClient>,
}

impl SkillTool {
    #[must_use]
    pub fn new(
        definition: SkillDefinition,
        system_prompt: String,
        parent_tools: Arc<ToolSet>,
        llm: Arc<dyn LLMClient>,
    ) -> Self {
        Self {
            definition,
            system_prompt,
            parent_tools,
            llm,
        }
    }

    fn tool_name(&self) -> String {
        format!("skill__{}", self.definition.name)
    }

    fn parameters_schema(&self) -> serde_json::Value {
        if let Some(params) = self
            .definition
            .lattice
            .as_ref()
            .and_then(|lattice| lattice.params.as_ref())
        {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for (name, schema) in params {
                properties.insert(name.clone(), schema_to_json(schema));
                if schema.required.unwrap_or(false) {
                    required.push(serde_json::Value::String(name.clone()));
                }
            }

            let mut root = serde_json::Map::new();
            root.insert("type".into(), serde_json::Value::String("object".into()));
            root.insert("properties".into(), serde_json::Value::Object(properties));
            root.insert("required".into(), serde_json::Value::Array(required));
            return serde_json::Value::Object(root);
        }

        serde_json::json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "Input to pass to the skill agent"
                }
            },
            "required": ["input"]
        })
    }

    fn child_input(&self, params: &serde_json::Value) -> Result<String, ToolError> {
        if let Some(input) = params.get("input") {
            return input
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| ToolError::InvalidParams("'input' must be a string".into()));
        }

        if params.is_null() || params == &serde_json::json!({}) {
            return Ok(String::new());
        }

        serde_json::to_string(params)
            .map_err(|e| ToolError::InvalidParams(format!("failed to serialize params: {e}")))
    }

    fn excluded_parent_tools(&self) -> Vec<String> {
        let Some(allowed) = self.definition.allowed_tools.as_ref() else {
            return Vec::new();
        };

        let allowed: HashSet<_> = allowed.iter().cloned().collect();
        self.parent_tools
            .descriptions()
            .into_iter()
            .filter(|tool| !allowed.contains(&tool.name))
            .map(|tool| tool.name)
            .collect()
    }
}

#[async_trait]
impl ToolExecutor for SkillTool {
    fn description(&self) -> ToolDescription {
        ToolDescription {
            name: self.tool_name(),
            description: self.definition.description.clone(),
            parameters_schema: self.parameters_schema(),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> Result<ExecutionResult, ToolError> {
        if ctx.depth >= MAX_SKILL_DEPTH {
            return Err(ToolError::MaxDepthExceeded(MAX_SKILL_DEPTH));
        }

        let skill_name = self.definition.name.clone();
        let (child_session_id, child_store) = ctx
            .store
            .create_child_session(ctx.session_id, &skill_name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        ctx.store
            .append_event(
                ctx.session_id,
                EventPayload::SkillInvoked {
                    skill_name: skill_name.clone(),
                    child_session_id,
                },
                Actor::Harness,
                Some(ctx.trigger_event_id),
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let input = self.child_input(&params)?;
        child_store
            .append_event(
                child_session_id,
                EventPayload::UserMessage { content: input },
                Actor::Harness,
                None,
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let skill_tool_set = SkillToolSet::build(
            Arc::clone(&self.parent_tools),
            Vec::new(),
            self.excluded_parent_tools(),
        );
        let child_tools = Arc::new(
            skill_tool_set
                .into_tool_set()
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
        );

        let child_loop = ControlLoop::builder()
            .store(Arc::clone(&child_store))
            .llm(Arc::clone(&self.llm))
            .tools(child_tools)
            .system_prompt(self.system_prompt.clone())
            .depth(ctx.depth + 1)
            .build();

        child_loop
            .run(child_session_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let events = child_store
            .get_events(child_session_id, &EventFilter::default())
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let answer = events
            .iter()
            .find_map(|event| match &event.payload {
                EventPayload::FinalAnswer { answer } => Some(answer.clone()),
                _ => None,
            })
            .ok_or_else(|| ToolError::ExecutionFailed("skill produced no FinalAnswer".into()))?;

        ctx.store
            .append_event(
                ctx.session_id,
                EventPayload::SkillCompleted {
                    skill_name,
                    child_session_id,
                },
                Actor::Harness,
                Some(ctx.trigger_event_id),
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ExecutionResult {
            stdout: answer,
            stderr: String::new(),
            exit_code: 0,
        })
    }
}

fn schema_to_json(schema: &ParamSchema) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert(
        "type".into(),
        serde_json::Value::String(schema.type_.clone()),
    );
    if let Some(description) = &schema.description {
        object.insert(
            "description".into(),
            serde_json::Value::String(description.clone()),
        );
    }
    if let Some(default) = &schema.default {
        object.insert("default".into(), default.clone());
    }
    serde_json::Value::Object(object)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use lattice_core::{Decision, Event, LLMClient, LLMError, ToolDescription};
    use lattice_store_memory::MemoryStore;

    use super::*;

    #[derive(Clone)]
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
                answer: "skill answer".into(),
            })
        }
    }

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

    #[tokio::test]
    async fn depth_limit_returns_error() {
        let store: Arc<dyn lattice_core::SessionStore> = Arc::new(MemoryStore::new());
        let session_id = store.create_session().await.unwrap();
        let trigger_event_id = store
            .append_event(
                session_id,
                EventPayload::ToolCallRequested {
                    tool: "skill__web-research".into(),
                    params: serde_json::json!({"input":"x"}),
                },
                Actor::LLM,
                None,
            )
            .await
            .unwrap();

        let tool = SkillTool::new(
            definition(),
            "You are a skill".into(),
            Arc::new(ToolSet::new()),
            Arc::new(StaticLlm),
        );
        let ctx = ExecutionContext {
            session_id,
            trigger_event_id,
            store,
            depth: MAX_SKILL_DEPTH,
        };

        let err = tool
            .execute(serde_json::json!({"input":"hello"}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::MaxDepthExceeded(8)));
    }

    #[tokio::test]
    async fn execute_records_parent_and_child_events() {
        let parent_store: Arc<dyn lattice_core::SessionStore> = Arc::new(MemoryStore::new());
        let parent_session_id = parent_store.create_session().await.unwrap();
        let trigger_event_id = parent_store
            .append_event(
                parent_session_id,
                EventPayload::ToolCallRequested {
                    tool: "skill__web-research".into(),
                    params: serde_json::json!({"input":"hello"}),
                },
                Actor::LLM,
                None,
            )
            .await
            .unwrap();

        let tool = SkillTool::new(
            definition(),
            "You are a skill".into(),
            Arc::new(ToolSet::new()),
            Arc::new(StaticLlm),
        );
        let ctx = ExecutionContext {
            session_id: parent_session_id,
            trigger_event_id,
            store: Arc::clone(&parent_store),
            depth: 0,
        };

        let result = tool
            .execute(serde_json::json!({"input":"hello"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.stdout, "skill answer");

        let parent_events = parent_store
            .get_events(parent_session_id, &EventFilter::default())
            .await
            .unwrap();
        let child_session_id = parent_events
            .iter()
            .find_map(|event| match event.payload {
                EventPayload::SkillInvoked {
                    child_session_id, ..
                } => Some(child_session_id),
                _ => None,
            })
            .unwrap();

        assert!(parent_events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SkillInvoked { .. })));
        assert!(parent_events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SkillCompleted { .. })));

        let children = parent_store
            .child_sessions(parent_session_id)
            .await
            .unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].session_id, child_session_id);

        let child_events = children[0]
            .store
            .get_events(child_session_id, &EventFilter::default())
            .await
            .unwrap();
        assert!(child_events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::UserMessage { .. })));
        assert!(child_events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::FinalAnswer { .. })));
    }
}
