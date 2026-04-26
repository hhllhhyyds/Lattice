//! Integration tests that call a real LLM API.
//!
//! Run with: cargo test -p lattice-llm-openai --all-features -- --ignored

use lattice_core::{
    Actor, Decision, Event, EventId, EventPayload, LLMClient, SessionId, ToolDescription,
};
use lattice_llm_openai::OpenAIClient;

/// Helper: create a minimal event history with a user message.
fn make_history(user_msg: &str) -> Vec<Event> {
    let session_id = SessionId::new_v4();
    let now = chrono::Utc::now();
    vec![
        Event {
            event_id: EventId::new_v4(),
            session_id,
            timestamp: now,
            actor: Actor::System,
            payload: EventPayload::SessionCreated,
            parent_event_id: None,
        },
        Event {
            event_id: EventId::new_v4(),
            session_id,
            timestamp: now,
            actor: Actor::System,
            payload: EventPayload::UserMessage {
                content: user_msg.to_string(),
            },
            parent_event_id: None,
        },
    ]
}

#[tokio::test]
#[ignore = "requires LATTICE_API_KEY"]
async fn test_real_llm_simple_question() {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("LATTICE_API_KEY").expect("LATTICE_API_KEY not set in .env");
    let api_base = std::env::var("LATTICE_API_BASE")
        .unwrap_or_else(|_| "https://api.minimax.chat/v1".to_string());
    let model = std::env::var("LATTICE_MODEL").unwrap_or_else(|_| "MiniMax-M2.7".into());

    let client = OpenAIClient::new(&api_key, &model).with_base_url(&api_base);
    let history = make_history("What is 2 + 2? Reply with just the number.");
    let decision = client
        .decide(&history, &[], "You are a helpful assistant.")
        .await
        .unwrap();

    match decision {
        Decision::FinalAnswer { answer } => {
            assert!(
                answer.contains('4'),
                "Expected answer to contain '4', got: {answer}"
            );
        }
        other => panic!("Expected FinalAnswer, got: {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires LATTICE_API_KEY"]
async fn test_real_llm_tool_call() {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("LATTICE_API_KEY").expect("LATTICE_API_KEY not set in .env");
    let api_base = std::env::var("LATTICE_API_BASE")
        .unwrap_or_else(|_| "https://api.minimax.chat/v1".to_string());
    let model = std::env::var("LATTICE_MODEL").unwrap_or_else(|_| "MiniMax-M2.7".into());

    let client = OpenAIClient::new(&api_key, &model).with_base_url(&api_base);
    let tools = vec![ToolDescription {
        name: "bash".to_string(),
        description: "Execute a bash command".to_string(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The bash command" }
            },
            "required": ["command"]
        }),
    }];
    let history = make_history("Run the command 'echo hello' using bash.");
    let decision = client
        .decide(
            &history,
            &tools,
            "You are a helpful assistant. Use tools when asked.",
        )
        .await
        .unwrap();

    match decision {
        Decision::ToolCall { tool, params } => {
            assert_eq!(tool, "bash");
            assert!(
                params.get("command").is_some(),
                "Expected command param, got: {params}"
            );
        }
        other => panic!("Expected ToolCall, got: {other:?}"),
    }
}
