//! In-memory implementation of SessionStore.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use lattice_core::{
    error::StoreError, Actor, Event, EventFilter, EventId, EventPayload, SessionId, SessionStore,
};
use tokio::sync::RwLock;

/// In-memory session store for development and testing.
pub struct MemoryStore {
    sessions: Arc<RwLock<HashMap<SessionId, Vec<Event>>>>,
}

impl MemoryStore {
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

        Ok(result)
    }

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
    }
}
