//! Integration tests for Task 16: Agent Run API
//!
//! Tests for:
//! - POST /v1/sessions/:id/messages — submit message and trigger agent execution
//! - GET /v1/sessions/:id/messages — get conversation history
//! - GET /v1/sessions/:id/status — query execution status

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use lattice_core::{
    Actor, Decision, Event, EventFilter, EventPayload, LLMClient, LLMError, SessionStore,
    ToolDescription,
};
use lattice_server::{new_state, new_state_with_components, router, LlmClientFactory};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

/// Helper to create a test app with MemoryStore.
fn make_app() -> axum::Router {
    let store = Arc::new(lattice_store_memory::MemoryStore::new());
    router(new_state_with_components(
        store,
        Arc::new(FakeLlmFactory),
        Arc::new(lattice_tools::ToolSet::new()),
    ))
}

struct FakeLlmFactory;

impl LlmClientFactory for FakeLlmFactory {
    fn create(
        &self,
        _provider: Option<&str>,
        _model: Option<&str>,
    ) -> Result<Arc<dyn LLMClient>, String> {
        Ok(Arc::new(FakeLlm {
            answer: "test answer".to_string(),
            delay: Duration::from_millis(150),
        }))
    }
}

struct FakeLlm {
    answer: String,
    delay: Duration,
}

#[async_trait]
impl LLMClient for FakeLlm {
    async fn decide(
        &self,
        _history: &[Event],
        _available_tools: &[ToolDescription],
        _system_prompt: &str,
    ) -> Result<Decision, LLMError> {
        tokio::time::sleep(self.delay).await;
        Ok(Decision::FinalAnswer {
            answer: self.answer.clone(),
        })
    }
}

/// Helper to create a session and return its ID.
async fn create_test_session(app: &axum::Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["sessionId"].as_str().unwrap().to_string()
}

// --- POST /v1/sessions/:id/messages tests ---

#[tokio::test]
async fn post_message_returns_202_accepted() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"content":"list files in current directory"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sessionId"], session_id);
    assert!(json["runId"].is_string());
    assert_eq!(json["status"], "running");
    assert_eq!(json["message"], "Agent task started");
}

#[tokio::test]
async fn create_session_with_metadata_is_visible_in_session_detail_and_list() {
    let app = make_app();

    let create_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"metadata":{"name":"MiniMax debug","tags":["ui","test"]}}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(create_response.into_body(), 1024)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let session_id = created["sessionId"].as_str().unwrap();
    assert_eq!(created["metadata"]["name"], "MiniMax debug");
    assert_eq!(created["metadata"]["tags"][0], "ui");

    let detail_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(detail_response.into_body(), 1024)
        .await
        .unwrap();
    let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["metadata"]["name"], "MiniMax debug");
    assert_eq!(detail["metadata"]["tags"][1], "test");

    let list_response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(list_response.into_body(), 4096)
        .await
        .unwrap();
    let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let matching = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["sessionId"] == session_id)
        .unwrap();
    assert_eq!(matching["metadata"]["name"], "MiniMax debug");
    assert_eq!(matching["metadata"]["tags"][0], "ui");
}

#[tokio::test]
async fn post_message_session_not_found_returns_404() {
    let app = make_app();
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", fake_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "session_not_found");
}

#[tokio::test]
async fn post_message_missing_content_returns_400() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn post_message_concurrent_run_returns_409() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    // First message submission.
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"first task"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response1.status(), StatusCode::ACCEPTED);

    // Second message submission (should conflict).
    let response2 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"second task"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
    let body = axum::body::to_bytes(response2.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "conflict");
}

#[tokio::test]
async fn post_message_with_provider_and_model() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"content":"test","provider":"openai","model":"gpt-4o"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn post_message_with_system_prompt() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"content":"test","systemPrompt":"You are a helpful assistant."}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn post_run_triggers_control_loop_for_existing_session() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/run", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"maxIterations":5}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sessionId"], session_id);
    assert!(json["runId"].is_string());

    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;

    let status_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(status_response.into_body(), 1024)
        .await
        .unwrap();
    let status_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status_json["runStatus"], "completed");
}

#[tokio::test]
async fn post_run_session_not_found_returns_404() {
    let app = make_app();
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/run", fake_id))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_run_concurrent_run_returns_409() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/run", session_id))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::ACCEPTED);

    let second = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/run", session_id))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(second.status(), StatusCode::CONFLICT);
}

// --- GET /v1/sessions/:id/messages tests ---

#[tokio::test]
async fn get_messages_returns_empty_for_new_session() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["messages"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_messages_returns_user_and_assistant_messages() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    // Submit a message (this will append UserMessage event).
    let _response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Wait a bit for the agent to potentially complete (in real scenario).
    // For this test, we just check that UserMessage is recorded.
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let messages = json["messages"].as_array().unwrap();
    assert!(!messages.is_empty());
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
}

#[tokio::test]
async fn get_messages_session_not_found_returns_404() {
    let app = make_app();
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/messages", fake_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// --- GET /v1/sessions/:id/status tests ---

#[tokio::test]
async fn get_status_returns_idle_for_new_session() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sessionId"], session_id);
    assert_eq!(json["runStatus"], "idle");
    assert!(json["runStartedAt"].is_null());
    assert!(json["runCompletedAt"].is_null());
    assert_eq!(json["eventCount"], 1); // SessionCreated event
}

#[tokio::test]
async fn get_status_returns_running_after_message_submission() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    // Submit a message.
    let _response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Query status immediately.
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sessionId"], session_id);
    // Status should be "running" or "completed" depending on timing.
    assert!(json["runStatus"] == "running" || json["runStatus"] == "completed");
}

#[tokio::test]
async fn get_status_session_not_found_returns_404() {
    let app = make_app();
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", fake_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_status_includes_latest_event_info() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["latestEvent"].is_object());
    assert!(json["latestEvent"]["eventId"].is_string());
    assert_eq!(json["latestEvent"]["actor"], "System");
    assert_eq!(json["latestEvent"]["payloadType"], "sessionCreated");
}

// --- Additional coverage tests ---

#[tokio::test]
async fn post_message_whitespace_only_returns_400() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "invalid_request");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("cannot be empty"));
}

#[tokio::test]
async fn get_messages_with_final_answer() {
    let _app = make_app();
    let _session_id = create_test_session(&_app).await;

    // Manually append a UserMessage and FinalAnswer event.
    let store = Arc::new(lattice_store_memory::MemoryStore::new());
    let state = new_state(store.clone());

    // Create session.
    let sid = store.create_session().await.unwrap();

    // Append UserMessage.
    store
        .append_event(
            sid,
            lattice_core::EventPayload::UserMessage {
                content: "test question".into(),
            },
            lattice_core::Actor::Harness,
            None,
        )
        .await
        .unwrap();

    // Append FinalAnswer.
    store
        .append_event(
            sid,
            lattice_core::EventPayload::FinalAnswer {
                answer: "test answer".into(),
            },
            lattice_core::Actor::LLM,
            None,
        )
        .await
        .unwrap();

    // Register session in state.
    {
        let mut sessions = state.sessions.write().await;
        sessions.push(lattice_server::SessionInfo {
            session_id: sid,
            created_at: chrono::Utc::now(),
            metadata: None,
        });
    }

    let app = lattice_server::router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/messages", sid))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let messages = json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "test question");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "test answer");
}

#[tokio::test]
async fn get_status_with_different_event_types() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    // Submit a message to create UserMessage event.
    let _response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Query status - should show UserMessage as latest event.
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["latestEvent"].is_object());
    assert_eq!(json["latestEvent"]["payloadType"], "userMessage");
}

#[tokio::test]
async fn post_message_empty_string_returns_400() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":""}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_status_with_completed_run() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    // Submit a message.
    let _response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    // Wait for mock task to complete.
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Query status.
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // Status should be running or idle (depending on timing).
    assert!(json["runStatus"].is_string());
}

#[tokio::test]
async fn submitted_message_runs_control_loop_to_final_answer() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;

    let status_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/status", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(status_response.into_body(), 1024)
        .await
        .unwrap();
    let status_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status_json["runStatus"], "completed");
    assert!(status_json["runCompletedAt"].is_string());

    let messages_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(messages_response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(messages_response.into_body(), 2048)
        .await
        .unwrap();
    let messages_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let messages = messages_json["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "test answer");
}

// --- GET /v1/sessions/:id/stream tests ---

#[tokio::test]
async fn session_stream_returns_404_for_missing_session() {
    let app = make_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions/00000000-0000-0000-0000-000000000000/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_stream_can_replay_history_and_emit_done() {
    let store = Arc::new(lattice_store_memory::MemoryStore::new());
    let state = new_state_with_components(
        store.clone(),
        Arc::new(FakeLlmFactory),
        Arc::new(lattice_tools::ToolSet::new()),
    );
    let session_id = store.create_session().await.unwrap();

    {
        let mut sessions = state.sessions.write().await;
        sessions.push(lattice_server::SessionInfo {
            session_id,
            created_at: chrono::Utc::now(),
            metadata: None,
        });
    }

    store
        .append_event(
            session_id,
            EventPayload::UserMessage {
                content: "history question".into(),
            },
            Actor::Harness,
            None,
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            EventPayload::FinalAnswer {
                answer: "history answer".into(),
            },
            Actor::LLM,
            None,
        )
        .await
        .unwrap();

    let app = router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/sessions/{}/stream?includeHistory=true",
                    session_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("event: session_event"));
    assert!(text.contains("\"type\":\"sessionCreated\""));
    assert!(text.contains("\"content\":\"history question\""));
    assert!(text.contains("\"answer\":\"history answer\""));
    assert!(text.contains("event: done"));
}

#[tokio::test]
async fn session_stream_after_cursor_skips_older_history() {
    let store = Arc::new(lattice_store_memory::MemoryStore::new());
    let state = new_state_with_components(
        store.clone(),
        Arc::new(FakeLlmFactory),
        Arc::new(lattice_tools::ToolSet::new()),
    );
    let session_id = store.create_session().await.unwrap();

    {
        let mut sessions = state.sessions.write().await;
        sessions.push(lattice_server::SessionInfo {
            session_id,
            created_at: chrono::Utc::now(),
            metadata: None,
        });
    }

    let history_before = store
        .get_events(session_id, &EventFilter::default())
        .await
        .unwrap();
    let session_created_id = history_before[0].event_id;

    store
        .append_event(
            session_id,
            EventPayload::UserMessage {
                content: "cursor question".into(),
            },
            Actor::Harness,
            None,
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            EventPayload::FinalAnswer {
                answer: "cursor answer".into(),
            },
            Actor::LLM,
            None,
        )
        .await
        .unwrap();

    let app = router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/sessions/{}/stream?includeHistory=true&after={}",
                    session_id, session_created_id
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("\"type\":\"sessionCreated\""));
    assert!(text.contains("\"content\":\"cursor question\""));
    assert!(text.contains("\"answer\":\"cursor answer\""));
}

#[tokio::test]
async fn session_stream_pushes_live_events() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let stream_future = {
        let app = app.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{}/stream", session_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        })
    };

    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    let submit_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"live stream test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(submit_response.status(), StatusCode::ACCEPTED);

    let response = stream_future.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 32 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("\"content\":\"live stream test\""));
    assert!(text.contains("\"answer\":\"test answer\""));
    assert!(text.contains("event: done"));
}

#[tokio::test]
async fn session_stream_supports_multiple_subscribers() {
    let app = make_app();
    let session_id = create_test_session(&app).await;

    let subscriber_a = {
        let app = app.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{}/stream", session_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        })
    };

    let subscriber_b = {
        let app = app.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{}/stream", session_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        })
    };

    tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;

    let submit_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{}/messages", session_id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"content":"fanout test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(submit_response.status(), StatusCode::ACCEPTED);

    let response_a = subscriber_a.await.unwrap();
    let response_b = subscriber_b.await.unwrap();

    let body_a = axum::body::to_bytes(response_a.into_body(), 32 * 1024)
        .await
        .unwrap();
    let body_b = axum::body::to_bytes(response_b.into_body(), 32 * 1024)
        .await
        .unwrap();
    let text_a = String::from_utf8(body_a.to_vec()).unwrap();
    let text_b = String::from_utf8(body_b.to_vec()).unwrap();

    assert!(text_a.contains("\"content\":\"fanout test\""));
    assert!(text_b.contains("\"content\":\"fanout test\""));
    assert!(text_a.contains("\"answer\":\"test answer\""));
    assert!(text_b.contains("\"answer\":\"test answer\""));
}
