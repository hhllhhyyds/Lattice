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

    let mut i = 0;
    while i < events.len() {
        match &events[i].payload {
            EventPayload::UserMessage { content } => {
                messages.push(Message::text(Role::User, content.clone()));
            }
            EventPayload::Thinking {
                reasoning,
                signature,
            } => {
                // If the very next event is a ToolCallRequested, this Thinking came from a
                // model that attaches reasoning_content to its tool-call response (e.g. DeepSeek
                // thinking mode). Merge both into a single assistant message so the reasoning
                // is passed back to the API on the next request.
                if let Some(next) = events.get(i + 1) {
                    if let EventPayload::ToolCallRequested { tool, params } = &next.payload {
                        let id = next.event_id.to_string();
                        messages.push(Message {
                            role: Role::Assistant,
                            content: vec![
                                ContentBlock::Reasoning {
                                    content: reasoning.clone(),
                                    signature: signature.clone(),
                                },
                                ContentBlock::ToolUse {
                                    id,
                                    name: tool.clone(),
                                    input: params.clone(),
                                },
                            ],
                        });
                        i += 2;
                        continue;
                    }
                }
                // Standalone Thinking (separate LLM round-trip) — omit from LLM messages.
            }
            EventPayload::ToolCallRequested { tool, params } => {
                let id = events[i].event_id.to_string();
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
                let tool_use_id = tool_use_id_from_parent(&events[i]);
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
            EventPayload::ToolCallError { error, error_kind } => {
                let tool_use_id = tool_use_id_from_parent(&events[i]);
                messages.push(Message {
                    role: Role::Tool,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id,
                        content: format_tool_error(error_kind.as_str(), error),
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
        i += 1;
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

fn format_tool_error(error_kind: &str, error: &str) -> String {
    format!("error_kind: {error_kind}\nmessage: {error}")
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
                    error_kind: lattice_core::ToolErrorKind::NotFound,
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
                assert_eq!(content, "error_kind: not_found\nmessage: command not found");
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
                signature: None,
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

    /// Thinking immediately before ToolCallRequested → single assistant message with
    /// Reasoning + ToolUse blocks (DeepSeek thinking mode round-trip).
    #[test]
    fn test_thinking_paired_with_tool_call_requested() {
        let thinking_event_id = EventId::new_v4();
        let tool_event_id = EventId::new_v4();
        let session_id = SessionId::new_v4();
        let now = chrono::Utc::now();

        let events = vec![
            make_event(EventPayload::UserMessage {
                content: "do something".into(),
            }),
            Event {
                event_id: thinking_event_id,
                session_id,
                timestamp: now,
                actor: Actor::LLM,
                payload: EventPayload::Thinking {
                    reasoning: "I should use bash".into(),
                    signature: None,
                },
                parent_event_id: None,
            },
            Event {
                event_id: tool_event_id,
                session_id,
                timestamp: now,
                actor: Actor::LLM,
                payload: EventPayload::ToolCallRequested {
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "ls"}),
                },
                parent_event_id: Some(thinking_event_id),
            },
        ];

        let messages = events_to_messages(&events);

        // UserMessage + one combined assistant message (not two separate messages)
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content.len(), 2);

        match &messages[1].content[0] {
            ContentBlock::Reasoning { content, .. } => assert_eq!(content, "I should use bash"),
            other => panic!("expected Reasoning block, got {:?}", other),
        }
        match &messages[1].content[1] {
            ContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, &tool_event_id.to_string());
                assert_eq!(name, "bash");
            }
            other => panic!("expected ToolUse block, got {:?}", other),
        }
    }

    /// Standalone Thinking (not followed by ToolCallRequested) is skipped.
    #[test]
    fn test_standalone_thinking_before_final_answer_is_skipped() {
        let events = vec![
            make_event(EventPayload::UserMessage {
                content: "hi".into(),
            }),
            make_event(EventPayload::Thinking {
                reasoning: "pondering...".into(),
                signature: None,
            }),
            make_event(EventPayload::FinalAnswer {
                answer: "hello".into(),
            }),
        ];

        let messages = events_to_messages(&events);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
        match &messages[1].content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            other => panic!("expected Text block, got {:?}", other),
        }
    }

    /// Thinking paired with ToolCallRequested does not emit a separate ToolCallRequested message.
    #[test]
    fn test_thinking_paired_consumes_both_events() {
        let session_id = SessionId::new_v4();
        let now = chrono::Utc::now();
        let tool_event_id = EventId::new_v4();

        let events = vec![
            make_event(EventPayload::UserMessage {
                content: "go".into(),
            }),
            Event {
                event_id: EventId::new_v4(),
                session_id,
                timestamp: now,
                actor: Actor::LLM,
                payload: EventPayload::Thinking {
                    reasoning: "thinking".into(),
                    signature: None,
                },
                parent_event_id: None,
            },
            Event {
                event_id: tool_event_id,
                session_id,
                timestamp: now,
                actor: Actor::LLM,
                payload: EventPayload::ToolCallRequested {
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "pwd"}),
                },
                parent_event_id: None,
            },
            make_event_with_parent(
                EventPayload::ToolCallResult {
                    stdout: "/home\n".into(),
                    stderr: String::new(),
                    exit_code: 0,
                },
                Some(tool_event_id),
            ),
        ];

        let messages = events_to_messages(&events);

        // user + assistant(reasoning+tooluse) + tool_result = 3 messages
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content.len(), 2); // Reasoning + ToolUse
        assert_eq!(messages[2].role, Role::Tool);
    }

    /// Signature in Thinking event is forwarded into ContentBlock::Reasoning for API round-trip.
    #[test]
    fn test_thinking_signature_preserved_in_reasoning_block() {
        let session_id = SessionId::new_v4();
        let tool_event_id = EventId::new_v4();
        let now = chrono::Utc::now();

        let events = vec![
            make_event(EventPayload::UserMessage {
                content: "run df".into(),
            }),
            Event {
                event_id: EventId::new_v4(),
                session_id,
                timestamp: now,
                actor: Actor::LLM,
                payload: EventPayload::Thinking {
                    reasoning: "I should run df -h".into(),
                    signature: Some("sig-xyz".into()),
                },
                parent_event_id: None,
            },
            Event {
                event_id: tool_event_id,
                session_id,
                timestamp: now,
                actor: Actor::LLM,
                payload: EventPayload::ToolCallRequested {
                    tool: "bash".into(),
                    params: serde_json::json!({"command": "df -h"}),
                },
                parent_event_id: None,
            },
        ];

        let messages = events_to_messages(&events);

        assert_eq!(messages.len(), 2);
        match &messages[1].content[0] {
            ContentBlock::Reasoning { content, signature } => {
                assert_eq!(content, "I should run df -h");
                assert_eq!(signature, &Some("sig-xyz".into()));
            }
            other => panic!("expected Reasoning block with signature, got {:?}", other),
        }
    }
}
