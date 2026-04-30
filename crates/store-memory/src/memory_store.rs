//! In-memory implementation of SessionStore.
//!
//! Uses `Arc<RwLock<HashMap<SessionId, Vec<Event>>>>` to support
//! concurrent access. Data is not persisted — process restarts
//! will lose all sessions.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use lattice_core::{
    error::StoreError, Actor, Event, EventFilter, EventId, EventPayload, SessionId, SessionStore,
};
use tokio::sync::RwLock;

/// In-memory session store for development and testing.
///
/// Not suitable for production — data is lost on process restart.
pub struct MemoryStore {
    /// Session id -> event log.
    sessions: Arc<RwLock<HashMap<SessionId, Vec<Event>>>>,
}

impl MemoryStore {
    /// Create a new empty MemoryStore.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStore for MemoryStore {
    /// Creates a new session and records a `SessionCreated` event.
    async fn create_session(&self) -> Result<SessionId, StoreError> {
        let session_id = SessionId::new_v4();
        let event = Event {
            event_id: EventId::new_v4(),
            session_id,
            timestamp: Utc::now(),
            actor: Actor::System,
            payload: EventPayload::SessionCreated,
            parent_event_id: None,
        };
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, vec![event]);
        Ok(session_id)
    }

    async fn delete_session(&self, session_id: SessionId) -> Result<(), StoreError> {
        let mut sessions = self.sessions.write().await;
        if sessions.remove(&session_id).is_some() {
            Ok(())
        } else {
            Err(StoreError::SessionNotFound(session_id))
        }
    }

    /// Appends an immutable event to the session's event log.
    async fn append_event(
        &self,
        session_id: SessionId,
        payload: EventPayload,
        actor: Actor,
        parent_event_id: Option<EventId>,
    ) -> Result<EventId, StoreError> {
        let event_id = EventId::new_v4();
        let event = Event {
            event_id,
            session_id,
            timestamp: Utc::now(),
            actor,
            payload,
            parent_event_id,
        };
        let mut sessions = self.sessions.write().await;
        let events = sessions
            .get_mut(&session_id)
            .ok_or(StoreError::SessionNotFound(session_id))?;
        events.push(event);
        Ok(event_id)
    }

    /// Retrieves events for a session, applying optional filters.
    async fn get_events(
        &self,
        session_id: SessionId,
        filter: &EventFilter,
    ) -> Result<Vec<Event>, StoreError> {
        let sessions = self.sessions.read().await;
        let events = sessions
            .get(&session_id)
            .ok_or(StoreError::SessionNotFound(session_id))?;

        let mut result: Vec<Event> = events.clone();

        if let Some(actor) = filter.actor {
            result.retain(|e| e.actor == actor);
        }

        if let Some(payload_type) = filter.payload_type {
            result.retain(|e| {
                let json = serde_json::to_value(&e.payload).ok();
                json.as_ref()
                    .and_then(|v| v.get("type"))
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t == payload_type)
            });
        }

        Ok(result)
    }

    /// Returns the event id of the most recent event, if any.
    async fn latest_event_id(&self, session_id: SessionId) -> Result<Option<EventId>, StoreError> {
        let sessions = self.sessions.read().await;
        let events = sessions
            .get(&session_id)
            .ok_or(StoreError::SessionNotFound(session_id))?;
        Ok(events.last().map(|e| e.event_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lattice_core::EventPayload;

    #[tokio::test]
    async fn test_create_session() {
        let store = MemoryStore::new();
        let id = store.create_session().await.unwrap();
        let events = store.get_events(id, &EventFilter::default()).await.unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].payload, EventPayload::SessionCreated));
    }

    #[tokio::test]
    async fn test_append_and_retrieve() {
        let store = MemoryStore::new();
        let id = store.create_session().await.unwrap();
        store
            .append_event(
                id,
                EventPayload::UserMessage {
                    content: "test".to_string(),
                },
                Actor::System,
                None,
            )
            .await
            .unwrap();
        let events = store.get_events(id, &EventFilter::default()).await.unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[1].payload,
            EventPayload::UserMessage { .. }
        ));
    }

    #[tokio::test]
    async fn test_delete_session() {
        let store = MemoryStore::new();
        let id = store.create_session().await.unwrap();

        store.delete_session(id).await.unwrap();

        let err = store
            .get_events(id, &EventFilter::default())
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::SessionNotFound(_)));
    }

    #[tokio::test]
    async fn test_delete_missing_session() {
        let store = MemoryStore::new();
        let missing = SessionId::new_v4();

        let err = store.delete_session(missing).await.unwrap_err();
        assert!(matches!(err, StoreError::SessionNotFound(id) if id == missing));
    }

    #[tokio::test]
    async fn test_filter_by_actor() {
        let store = MemoryStore::new();
        let id = store.create_session().await.unwrap();
        store
            .append_event(
                id,
                EventPayload::UserMessage {
                    content: "user".to_string(),
                },
                Actor::System,
                None,
            )
            .await
            .unwrap();
        store
            .append_event(
                id,
                EventPayload::Thinking {
                    reasoning: "thinking".to_string(),
                },
                Actor::LLM,
                None,
            )
            .await
            .unwrap();
        store
            .append_event(
                id,
                EventPayload::FinalAnswer {
                    answer: "answer".to_string(),
                },
                Actor::LLM,
                None,
            )
            .await
            .unwrap();

        let llm_events = store
            .get_events(
                id,
                &EventFilter {
                    actor: Some(Actor::LLM),
                    payload_type: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(llm_events.len(), 2);
    }

    #[tokio::test]
    async fn test_filter_by_payload_type() {
        let store = MemoryStore::new();
        let id = store.create_session().await.unwrap();

        store
            .append_event(
                id,
                EventPayload::UserMessage {
                    content: "user".to_string(),
                },
                Actor::System,
                None,
            )
            .await
            .unwrap();
        store
            .append_event(
                id,
                EventPayload::Thinking {
                    reasoning: "thinking".to_string(),
                },
                Actor::LLM,
                None,
            )
            .await
            .unwrap();
        store
            .append_event(
                id,
                EventPayload::FinalAnswer {
                    answer: "answer".to_string(),
                },
                Actor::LLM,
                None,
            )
            .await
            .unwrap();

        let thinking_events = store
            .get_events(
                id,
                &EventFilter {
                    actor: None,
                    payload_type: Some("thinking"),
                },
            )
            .await
            .unwrap();
        assert_eq!(thinking_events.len(), 1);

        let tool_events = store
            .get_events(
                id,
                &EventFilter {
                    actor: None,
                    payload_type: Some("toolCallRequested"),
                },
            )
            .await
            .unwrap();
        assert_eq!(tool_events.len(), 0);
    }

    #[tokio::test]
    async fn test_concurrent_read_write() {
        let store = MemoryStore::new();
        let id = store.create_session().await.unwrap();

        // Spawn multiple writers and readers concurrently.
        let sessions_clone = store.sessions.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..10 {
                let sessions = sessions_clone.read().await;
                let _count = sessions.get(&id).map(|e| e.len());
                drop(sessions);
                tokio::task::yield_now().await;
            }
        });

        for i in 0..5 {
            store
                .append_event(
                    id,
                    EventPayload::UserMessage {
                        content: format!("msg {i}"),
                    },
                    Actor::System,
                    None,
                )
                .await
                .unwrap();
        }

        handle.await.unwrap();

        let events = store.get_events(id, &EventFilter::default()).await.unwrap();
        // 1 session created + 5 appended
        assert_eq!(events.len(), 6);
    }

    #[tokio::test]
    async fn test_session_not_found() {
        let store = MemoryStore::new();
        let fake_id = SessionId::new_v4();
        let result = store.get_events(fake_id, &EventFilter::default()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            StoreError::SessionNotFound(_)
        ));
    }
}
