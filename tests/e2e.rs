//! End-to-end integration test: ControlLoop + real LLM + ToolSet + LocalSandbox
//!
//! Run with: cargo test --all-features -- --ignored

#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use lattice_core::{Actor, EventPayload, LLMClient, SessionStore};
#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use lattice_llm_anthropic::AnthropicClient;
#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use lattice_llm_openai::OpenAIClient;
#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use lattice_runtime::ControlLoop;
#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use lattice_sandbox_local::LocalSandbox;
#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use lattice_store_memory::MemoryStore;
#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use lattice_tools::ToolSet;

#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
use std::sync::Arc;

/// Shared test body: runs the control loop and asserts a FinalAnswer is produced.
#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
async fn run_e2e(llm: Arc<dyn LLMClient>) {
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
        "You are a helpful agent. When asked to run a bash command, use the bash tool.".to_string();

    let control_loop = ControlLoop::with_options(store.clone(), llm, tools, system_prompt, 10);

    let answer = control_loop
        .run(session_id)
        .await
        .expect("control loop should complete successfully");
    println!("Final answer: {answer}");

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

#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
#[tokio::test]
#[ignore = "requires LATTICE_API_KEY and LATTICE_OPENAI_API_BASE"]
async fn test_end_to_end_openai() {
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

    let llm = Arc::new(OpenAIClient::new(&api_key, &model).with_base_url(&api_base));
    run_e2e(llm).await;
}

#[cfg(all(
    feature = "llm-openai",
    feature = "llm-anthropic",
    feature = "sandbox-local",
    feature = "tools"
))]
#[tokio::test]
#[ignore = "requires LATTICE_API_KEY and LATTICE_ANTHROPIC_API_BASE"]
async fn test_end_to_end_anthropic() {
    dotenvy::dotenv().ok();

    let Ok(api_key) = std::env::var("LATTICE_API_KEY") else {
        eprintln!("skipping: LATTICE_API_KEY not set");
        return;
    };
    let Ok(api_base) = std::env::var("LATTICE_ANTHROPIC_API_BASE") else {
        eprintln!("skipping: LATTICE_ANTHROPIC_API_BASE not set");
        return;
    };
    let Ok(model) = std::env::var("LATTICE_MODEL") else {
        eprintln!("skipping: LATTICE_MODEL not set");
        return;
    };

    let llm = Arc::new(AnthropicClient::new(&api_key, &model).with_base_url(&api_base));
    run_e2e(llm).await;
}
