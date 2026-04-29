//! In-memory implementation of SessionStore.
//!
//! Uses `Arc<RwLock<Inner>>` to support concurrent access. Data is not persisted
//! and process restarts will lose all sessions.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use lattice_core::{
    error::StoreError, Actor, ChildSessionInfo, Event, EventFilter, EventId, EventPayload,
    SessionId, SessionStore,
};
use tokio::sync::RwLock;

/// Internal state of MemoryStore.
struct Inner {
    sessions: HashMap<SessionId, Vec<Event>>,
    children: HashMap<SessionId, Vec<ChildSessionInfo>>,
}

/// In-memory session store for development and testing.
pub struct MemoryStore {
    inner: Arc<RwLock<Inner>>,
}

impl MemoryStore {
    /// Create a new empty MemoryStore.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                sessions: HashMap::new(),
                children: HashMap::new(),
            })),
        }
    }
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
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
        let mut inner = self.inner.write().await;
        inner.sessions.insert(session_id, vec![event]);
        Ok(session_id)
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
        let mut inner = self.inner.write().await;
        let events = inner
            .sessions
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
        let inner = self.inner.read().await;
        let events = inner
            .sessions
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

    async fn create_child_session(
        &self,
        parent_session_id: SessionId,
        skill_name: &str,
    ) -> Result<(SessionId, Arc<dyn SessionStore>), StoreError> {
        {
            let inner = self.inner.read().await;
            if !inner.sessions.contains_key(&parent_session_id) {
                return Err(StoreError::SessionNotFound(parent_session_id));
            }
        }

        let child_store: Arc<dyn SessionStore> = Arc::new(MemoryStore::new());
        let child_session_id = child_store.create_session().await?;
        let info = ChildSessionInfo {
            session_id: child_session_id,
            store: Arc::clone(&child_store),
            skill_name: skill_name.to_string(),
            created_at: Utc::now(),
        };

        let mut inner = self.inner.write().await;
        inner
            .children
            .entry(parent_session_id)
            .or_default()
            .push(info);

        Ok((child_session_id, child_store))
    }

    async fn child_sessions(
        &self,
        parent_session_id: SessionId,
    ) -> Result<Vec<ChildSessionInfo>, StoreError> {
        let inner = self.inner.read().await;
        Ok(inner
            .children
            .get(&parent_session_id)
            .cloned()
            .unwrap_or_default())
    }

    /// Returns the event id of the most recent event, if any.
    async fn latest_event_id(&self, session_id: SessionId) -> Result<Option<EventId>, StoreError> {
        let inner = self.inner.read().await;
        let events = inner
            .sessions
            .get(&session_id)
            .ok_or(StoreError::SessionNotFound(session_id))?;
        Ok(events.last().map(|e| e.event_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[tokio::test]
    async fn create_child_session_returns_independent_store() {
        let store = MemoryStore::new();
        let parent_id = store.create_session().await.unwrap();

        let (child_id, child_store) = store
            .create_child_session(parent_id, "web-research")
            .await
            .unwrap();

        assert_ne!(child_id, parent_id);

        child_store
            .append_event(
                child_id,
                EventPayload::UserMessage {
                    content: "child msg".into(),
                },
                Actor::Harness,
                None,
            )
            .await
            .unwrap();

        let parent_events = store
            .get_events(parent_id, &EventFilter::default())
            .await
            .unwrap();
        assert_eq!(parent_events.len(), 1);
        assert!(matches!(
            parent_events[0].payload,
            EventPayload::SessionCreated
        ));
    }

    #[tokio::test]
    async fn child_sessions_returns_correct_info() {
        let store = MemoryStore::new();
        let parent_id = store.create_session().await.unwrap();

        let (id1, _) = store
            .create_child_session(parent_id, "skill-a")
            .await
            .unwrap();
        let (id2, _) = store
            .create_child_session(parent_id, "skill-b")
            .await
            .unwrap();

        let children = store.child_sessions(parent_id).await.unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|c| c.session_id == id1));
        assert!(children.iter().any(|c| c.session_id == id2));
        assert!(children.iter().any(|c| c.skill_name == "skill-a"));
        assert!(children.iter().any(|c| c.skill_name == "skill-b"));
    }

    #[tokio::test]
    async fn multiple_children_accumulated() {
        let store = MemoryStore::new();
        let parent_id = store.create_session().await.unwrap();

        for i in 0..5 {
            store
                .create_child_session(parent_id, &format!("skill-{i}"))
                .await
                .unwrap();
        }

        let children = store.child_sessions(parent_id).await.unwrap();
        assert_eq!(children.len(), 5);
    }

    #[tokio::test]
    async fn child_sessions_parent_not_found() {
        let store = MemoryStore::new();
        let fake = SessionId::new_v4();
        let result = store.child_sessions(fake).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_session_not_found() {
        let store = MemoryStore::new();
        let fake_id = SessionId::new_v4();
        let result = store.get_events(fake_id, &EventFilter::default()).await;
        assert!(matches!(
            result.unwrap_err(),
            StoreError::SessionNotFound(_)
        ));
    }
}
