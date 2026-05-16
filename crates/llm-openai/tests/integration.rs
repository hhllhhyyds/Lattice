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
#[ignore = "requires LATTICE_API_KEY and LATTICE_OPENAI_API_BASE"]
async fn test_real_llm_simple_question() {
    dotenvy::dotenv().ok();
    let Ok(api_key) = std::env::var("LATTICE_API_KEY") else {
        eprintln!("skipping: LATTICE_API_KEY not set");
        return;
    };
    let Ok(api_base) = std::env::var("LATTICE_OPENAI_API_BASE") else {
        eprintln!("skipping: LATTICE_OPENAI_API_BASE not set");
        return;
    };
    let model =
        match std::env::var("LATTICE_OPENAI_MODEL").or_else(|_| std::env::var("LATTICE_MODEL")) {
            Ok(m) => m,
            Err(_) => {
                eprintln!("skipping: LATTICE_OPENAI_MODEL (or LATTICE_MODEL) not set");
                return;
            }
        };

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
#[ignore = "requires LATTICE_API_KEY and LATTICE_OPENAI_API_BASE"]
async fn test_real_llm_tool_call() {
    dotenvy::dotenv().ok();
    let Ok(api_key) = std::env::var("LATTICE_API_KEY") else {
        eprintln!("skipping: LATTICE_API_KEY not set");
        return;
    };
    let Ok(api_base) = std::env::var("LATTICE_OPENAI_API_BASE") else {
        eprintln!("skipping: LATTICE_OPENAI_API_BASE not set");
        return;
    };
    let model =
        match std::env::var("LATTICE_OPENAI_MODEL").or_else(|_| std::env::var("LATTICE_MODEL")) {
            Ok(m) => m,
            Err(_) => {
                eprintln!("skipping: LATTICE_OPENAI_MODEL (or LATTICE_MODEL) not set");
                return;
            }
        };

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

    // Models with thinking mode (e.g. DeepSeek) return ThinkingToolCall when
    // reasoning_content is present alongside the tool call.
    let (tool, params) = match decision {
        Decision::ToolCall { tool, params } => (tool, params),
        Decision::ThinkingToolCall { tool, params, .. } => (tool, params),
        other => panic!("Expected ToolCall or ThinkingToolCall, got: {other:?}"),
    };
    assert_eq!(tool, "bash");
    assert!(
        params.get("command").is_some(),
        "Expected command param, got: {params}"
    );
}
