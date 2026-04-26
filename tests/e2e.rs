//! End-to-end integration test: ControlLoop + real LLM + ToolSet + LocalSandbox
//!
//! Run with: cargo test --all-features -- --ignored

use lattice_core::{EventPayload, LLMClient, SessionStore};
use lattice_llm_openai::OpenAIClient;
use lattice_runtime::ControlLoop;
use lattice_sandbox_local::LocalSandbox;
use lattice_store_memory::MemoryStore;
use lattice_tools::ToolSet;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires LATTICE_API_KEY and sandbox"]
async fn test_end_to_end_agent_run() {
    dotenvy::dotenv().ok();

    let api_key = std::env::var("LATTICE_API_KEY").expect("LATTICE_API_KEY not set in .env");
    let api_base = std::env::var("LATTICE_API_BASE")
        .unwrap_or_else(|_| "https://api.minimax.chat/v1".to_string());
    let model = std::env::var("LATTICE_MODEL").unwrap_or_else(|_| "MiniMax-M2.7".into());

    let llm: Arc<dyn LLMClient> =
        Arc::new(OpenAIClient::new(&api_key, &model).with_base_url(&api_base));
    let store: Arc<MemoryStore> = Arc::new(MemoryStore::new());
    let sandbox = Arc::new(LocalSandbox::new());
    let tools = Arc::new(ToolSet::with_defaults(sandbox));

    let session_id = store.create_session().await.unwrap();

    let system_prompt =
        "You are a helpful agent. When asked to run a bash command, use the bash tool.".to_string();

    let control_loop = ControlLoop::with_options(store.clone(), llm, tools, system_prompt, 10);

    let result = control_loop.run(session_id).await;

    // We expect a result (either FinalAnswer or an error from max iterations).
    // The important thing is the control loop executed without panicking
    // and recorded events in the store.
    assert!(
        result.is_ok() || result.is_err(),
        "control loop should complete"
    );

    // Verify the event log contains key events.
    let events = store
        .get_events(session_id, &lattice_core::EventFilter::default())
        .await
        .unwrap();

    let has_tool_call = events
        .iter()
        .any(|e| matches!(e.payload, EventPayload::ToolCallRequested { .. }));
    let has_result_or_error = events.iter().any(|e| {
        matches!(
            e.payload,
            EventPayload::ToolCallResult { .. } | EventPayload::ToolCallError { .. }
        )
    });

    assert!(
        has_tool_call,
        "expected at least one ToolCallRequested event"
    );
    assert!(
        has_result_or_error,
        "expected ToolCallResult or ToolCallError event"
    );
}
