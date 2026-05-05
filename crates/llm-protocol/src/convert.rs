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

    for event in events {
        match &event.payload {
            EventPayload::UserMessage { content } => {
                messages.push(Message::text(Role::User, content.clone()));
            }
            EventPayload::Thinking { .. } => {
                // Internal reasoning — do not include in LLM messages.
                // The event is preserved in the store for debugging/audit,
                // but the LLM does not need to see its own prior reasoning.
            }
            EventPayload::ToolCallRequested { tool, params } => {
                let id = event.event_id.to_string();
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
                let tool_use_id = tool_use_id_from_parent(event);
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
                let tool_use_id = tool_use_id_from_parent(event);
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

fn tool_use_id_from_parent(event: &Event) -> String {
    event
        .parent_event_id
        .map(|id| id.to_string())
        .unwrap_or_default()
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
        make_event_with_parent(payload, None)
    }

    fn make_event_with_parent(payload: EventPayload, parent_event_id: Option<EventId>) -> Event {
        Event {
            event_id: EventId::new_v4(),
            session_id: SessionId::new_v4(),
            timestamp: Utc::now(),
            actor: Actor::System,
            payload,
            parent_event_id,
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
            make_event_with_parent(
                EventPayload::ToolCallResult {
                    stdout: "hi\n".into(),
                    stderr: String::new(),
                    exit_code: 0,
                },
                Some(tool_event_id),
            ),
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
        let tool_event_id = EventId::new_v4();
        let events = vec![
            Event {
                event_id: tool_event_id,
                session_id: SessionId::new_v4(),
                timestamp: Utc::now(),
                actor: Actor::LLM,
                payload: EventPayload::ToolCallRequested {
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "fail"}),
                },
                parent_event_id: None,
            },
            make_event_with_parent(
                EventPayload::ToolCallError {
                    error: "command not found".into(),
                },
                Some(tool_event_id),
            ),
        ];
        let messages = events_to_messages(&events);
        assert_eq!(messages.len(), 2);
        match &messages[1].content[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, &tool_event_id.to_string());
                assert_eq!(content, "command not found");
                assert!(is_error);
            }
            _ => panic!("expected tool result block"),
        }
    }

    #[test]
    fn test_tool_results_use_parent_event_id_when_out_of_order() {
        let first_tool_event_id = EventId::new_v4();
        let second_tool_event_id = EventId::new_v4();
        let session_id = SessionId::new_v4();
        let events = vec![
            Event {
                event_id: first_tool_event_id,
                session_id,
                timestamp: Utc::now(),
                actor: Actor::LLM,
                payload: EventPayload::ToolCallRequested {
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "ls"}),
                },
                parent_event_id: None,
            },
            Event {
                event_id: second_tool_event_id,
                session_id,
                timestamp: Utc::now(),
                actor: Actor::LLM,
                payload: EventPayload::ToolCallRequested {
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "pwd"}),
                },
                parent_event_id: None,
            },
            make_event_with_parent(
                EventPayload::ToolCallResult {
                    stdout: "ls output".into(),
                    stderr: String::new(),
                    exit_code: 0,
                },
                Some(first_tool_event_id),
            ),
            make_event_with_parent(
                EventPayload::ToolCallResult {
                    stdout: "pwd output".into(),
                    stderr: String::new(),
                    exit_code: 0,
                },
                Some(second_tool_event_id),
            ),
        ];

        let messages = events_to_messages(&events);

        match &messages[2].content[0] {
            ContentBlock::ToolResult { tool_use_id, .. } => {
                assert_eq!(tool_use_id, &first_tool_event_id.to_string());
            }
            _ => panic!("expected first tool result block"),
        }
        match &messages[3].content[0] {
            ContentBlock::ToolResult { tool_use_id, .. } => {
                assert_eq!(tool_use_id, &second_tool_event_id.to_string());
            }
            _ => panic!("expected second tool result block"),
        }
    }

    #[test]
    fn test_thinking_events_skipped() {
        let events = vec![
            make_event(EventPayload::UserMessage {
                content: "What is 2+2?".into(),
            }),
            make_event(EventPayload::Thinking {
                reasoning: "I need to calculate 2+2. The answer is 4.".into(),
            }),
            make_event(EventPayload::FinalAnswer { answer: "4".into() }),
        ];
        let messages = events_to_messages(&events);

        // Should only have UserMessage and FinalAnswer, Thinking is skipped
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);

        // Verify the assistant message is the FinalAnswer, not the Thinking
        match &messages[1].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "4"),
            _ => panic!("expected text block"),
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
