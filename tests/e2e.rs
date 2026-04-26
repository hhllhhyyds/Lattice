//! End-to-end integration test: ControlLoop + real LLM + ToolSet + LocalSandbox
//!
//! Run with: cargo test --all-features -- --ignored

#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
use lattice_core::{Actor, EventPayload, LLMClient, SessionStore};
#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
use lattice_llm_openai::OpenAIClient;
#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
use lattice_runtime::ControlLoop;
#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
use lattice_sandbox_local::LocalSandbox;
#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
use lattice_store_memory::MemoryStore;
#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
use lattice_tools::ToolSet;

#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
use std::sync::Arc;

#[cfg(all(feature = "llm-openai", feature = "sandbox-local", feature = "tools"))]
#[tokio::test]
#[ignore = "requires LATTICE_API_KEY"]
async fn test_end_to_end_agent_run() {
    dotenvy::dotenv().ok();

    let api_key =
        std::env::var("LATTICE_API_KEY").expect("LATTICE_API_KEY not set in .env");
    let api_base = std::env::var("LATTICE_API_BASE")
        .unwrap_or_else(|_| "https://api.minimax.chat/v1".to_string());
    let model =
        std::env::var("LATTICE_MODEL").unwrap_or_else(|_| "MiniMax-M2.7".into());

    let llm: Arc<dyn LLMClient> =
        Arc::new(OpenAIClient::new(&api_key, &model).with_base_url(&api_base));
    let store: Arc<MemoryStore> = Arc::new(MemoryStore::new());
    let sandbox = Arc::new(LocalSandbox::new());
    let tools = Arc::new(ToolSet::with_defaults(sandbox));

    let session_id = store.create_session().await.unwrap();

    // Append a user message so the LLM has content to process.
    // SessionCreated is skipped by the protocol layer.
    store
        .append_event(
            session_id,
            EventPayload::UserMessage {
                content: "What is 2 + 2? Reply with just the number.".to_string(),
            },
            Actor::System,
            None,
        )
        .await
        .unwrap();

    let system_prompt =
        "You are a helpful agent. When asked to run a bash command, use the bash tool."
            .to_string();

    let control_loop =
        ControlLoop::with_options(store.clone(), llm, tools, system_prompt, 10);

    let result = control_loop.run(session_id).await;

    // Assert the loop completed successfully and returned a final answer.
    let answer = result.expect("control loop should complete successfully");
    println!("Final answer: {answer}");

    // Verify the event log contains SessionCreated and FinalAnswer.
    let events = store
        .get_events(session_id, &lattice_core::EventFilter::default())
        .await
        .unwrap();

    assert!(
        events.len() >= 2,
        "expected at least SessionCreated and FinalAnswer events, got {} events: {:?}",
        events.len(),
        events
    );

    let has_final_answer = events
        .iter()
        .any(|e| matches!(e.payload, EventPayload::FinalAnswer { .. }));
    assert!(
        has_final_answer,
        "expected FinalAnswer in event log, got: {:?}",
        events
    );
}
