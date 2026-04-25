//! Conversion from Lattice events to LLM messages.

use lattice_core::{Event, EventPayload, ToolDescription};

use crate::message::{ContentBlock, Message, Role};
use crate::request::ToolSpec;

/// Convert a sequence of Lattice events into LLM conversation messages.
///
/// Maps each relevant event payload to the appropriate message role and
/// content block. Events like `SessionCreated` and `StateChange` are
/// skipped as they carry no conversational content.
pub fn events_to_messages(events: &[Event]) -> Vec<Message> {
    let mut messages: Vec<Message> = Vec::new();

    // Track the last tool call event id so we can correlate ToolResult
    // with the correct tool_use_id.
    let mut last_tool_call_id: Option<String> = None;

    for event in events {
        match &event.payload {
            EventPayload::UserMessage { content } => {
                messages.push(Message::text(Role::User, content.clone()));
            }
            EventPayload::Thinking { reasoning } => {
                // Thinking is emitted as assistant text so the LLM can see
                // its own prior reasoning chain.
                messages.push(Message::text(Role::Assistant, reasoning.clone()));
            }
            EventPayload::ToolCallRequested { tool, params } => {
                let id = event.event_id.to_string();
                last_tool_call_id = Some(id.clone());
                messages.push(Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::ToolUse {
                        id,
                        name: tool.clone(),
                        input: params.clone(),
                    }],
                });
            }
            EventPayload::ToolCallResult {
                stdout,
                stderr,
                exit_code,
            } => {
                let tool_use_id = last_tool_call_id.clone().unwrap_or_default();
                let content = format_tool_result(stdout, stderr, *exit_code);
                messages.push(Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error: *exit_code != 0,
                    }],
                });
            }
            EventPayload::ToolCallError { error } => {
                let tool_use_id = last_tool_call_id.clone().unwrap_or_default();
                messages.push(Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id,
                        content: error.clone(),
                        is_error: true,
                    }],
                });
            }
            EventPayload::FinalAnswer { answer } => {
                messages.push(Message::text(Role::Assistant, answer.clone()));
            }
            // SessionCreated and StateChange carry no conversational content.
            EventPayload::SessionCreated | EventPayload::StateChange { .. } => {}
        }
    }

    messages
}

/// Convert Lattice tool descriptions into protocol tool specs.
pub fn tool_descriptions_to_specs(tools: &[ToolDescription]) -> Vec<ToolSpec> {
    tools
        .iter()
        .map(|t| ToolSpec {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.parameters_schema.clone(),
        })
        .collect()
}

/// Format tool execution output into a human-readable string.
fn format_tool_result(stdout: &str, stderr: &str, exit_code: i32) -> String {
    let mut parts = Vec::new();
    if !stdout.is_empty() {
        parts.push(format!("stdout:\n{stdout}"));
    }
    if !stderr.is_empty() {
        parts.push(format!("stderr:\n{stderr}"));
    }
    parts.push(format!("exit_code: {exit_code}"));
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use lattice_core::{Actor, EventId, SessionId};
    use uuid::Uuid;

    fn make_event(payload: EventPayload) -> Event {
        Event {
            event_id: EventId::new_v4(),
            session_id: SessionId::new_v4(),
            timestamp: Utc::now(),
            actor: Actor::System,
            payload,
            parent_event_id: None,
        }
    }

    #[test]
    fn test_user_message_conversion() {
        let events = vec![make_event(EventPayload::UserMessage {
            content: "hello".into(),
        })];
        let messages = events_to_messages(&events);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        match &messages[0].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn test_session_created_skipped() {
        let events = vec![
            make_event(EventPayload::SessionCreated),
            make_event(EventPayload::UserMessage {
                content: "hi".into(),
            }),
        ];
        let messages = events_to_messages(&events);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
    }

    #[test]
    fn test_tool_call_and_result_conversion() {
        let tool_event_id = Uuid::new_v4();
        let events = vec![
            Event {
                event_id: tool_event_id,
                session_id: SessionId::new_v4(),
                timestamp: Utc::now(),
                actor: Actor::LLM,
                payload: EventPayload::ToolCallRequested {
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "echo hi"}),
                },
                parent_event_id: None,
            },
            make_event(EventPayload::ToolCallResult {
                stdout: "hi\n".into(),
                stderr: String::new(),
                exit_code: 0,
            }),
        ];
        let messages = events_to_messages(&events);
        assert_eq!(messages.len(), 2);

        // First message: assistant tool use
        assert_eq!(messages[0].role, Role::Assistant);
        match &messages[0].content[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, &tool_event_id.to_string());
                assert_eq!(name, "bash");
                assert_eq!(input, &serde_json::json!({"command": "echo hi"}));
            }
            _ => panic!("expected tool use block"),
        }

        // Second message: tool result
        assert_eq!(messages[1].role, Role::Tool);
        match &messages[1].content[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, &tool_event_id.to_string());
                assert!(content.contains("hi\n"));
                assert!(!is_error);
            }
            _ => panic!("expected tool result block"),
        }
    }

    #[test]
    fn test_tool_call_error_conversion() {
        let events = vec![
            make_event(EventPayload::ToolCallRequested {
                tool: "bash".into(),
                params: serde_json::json!({"command": "fail"}),
            }),
            make_event(EventPayload::ToolCallError {
                error: "command not found".into(),
            }),
        ];
        let messages = events_to_messages(&events);
        assert_eq!(messages.len(), 2);
        match &messages[1].content[0] {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert_eq!(content, "command not found");
                assert!(is_error);
            }
            _ => panic!("expected tool result block"),
        }
    }

    #[test]
    fn test_empty_events() {
        let messages = events_to_messages(&[]);
        assert!(messages.is_empty());
    }

    #[test]
    fn test_tool_descriptions_to_specs() {
        let tools = vec![ToolDescription {
            name: "bash".into(),
            description: "run commands".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
        }];
        let specs = tool_descriptions_to_specs(&tools);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "bash");
    }
}
