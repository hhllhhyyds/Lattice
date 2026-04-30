//! Event fan-out infrastructure for SSE session streams.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use lattice_core::{Actor, Event, EventFilter, EventId, EventPayload, SessionId, SessionStore};
use tokio::sync::{broadcast, RwLock};

/// Per-session in-process event channels.
pub struct EventHub {
    channels: RwLock<HashMap<SessionId, broadcast::Sender<Event>>>,
}

impl EventHub {
    const CHANNEL_CAPACITY: usize = 256;

    #[must_use]
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    /// Subscribe to future events for a session, creating a channel on demand.
    pub async fn subscribe(&self, session_id: SessionId) -> broadcast::Receiver<Event> {
        let existing_sender = {
            let channels = self.channels.read().await;
            channels.get(&session_id).cloned()
        };

        if let Some(sender) = existing_sender {
            return sender.subscribe();
        }

        let mut channels = self.channels.write().await;
        channels
            .entry(session_id)
            .or_insert_with(|| broadcast::channel(Self::CHANNEL_CAPACITY).0)
            .subscribe()
    }

    /// Publish an event to all current subscribers of a session.
    pub async fn publish(&self, event: &Event) {
        let existing_sender = {
            let channels = self.channels.read().await;
            channels.get(&event.session_id).cloned()
        };

        let sender = if let Some(sender) = existing_sender {
            sender
        } else {
            let mut channels = self.channels.write().await;
            channels
                .entry(event.session_id)
                .or_insert_with(|| broadcast::channel(Self::CHANNEL_CAPACITY).0)
                .clone()
        };
        let _ = sender.send(event.clone());
    }

    /// Remove and drop the channel for a deleted session.
    pub async fn remove_session(&self, session_id: SessionId) {
        let mut channels = self.channels.write().await;
        channels.remove(&session_id);
    }
}

/// SessionStore decorator that broadcasts newly appended events.
pub struct NotifyingStore {
    inner: Arc<dyn SessionStore>,
    hub: Arc<EventHub>,
}

impl NotifyingStore {
    #[must_use]
    pub fn new(inner: Arc<dyn SessionStore>, hub: Arc<EventHub>) -> Self {
        Self { inner, hub }
    }
}

#[async_trait]
impl SessionStore for NotifyingStore {
    async fn create_session(&self) -> Result<SessionId, lattice_core::StoreError> {
        self.inner.create_session().await
    }

    async fn delete_session(&self, session_id: SessionId) -> Result<(), lattice_core::StoreError> {
        self.inner.delete_session(session_id).await?;
        self.hub.remove_session(session_id).await;
        Ok(())
    }

    async fn append_event(
        &self,
        session_id: SessionId,
        payload: EventPayload,
        actor: Actor,
        parent_event_id: Option<EventId>,
    ) -> Result<EventId, lattice_core::StoreError> {
        let event_id = self
            .inner
            .append_event(session_id, payload, actor, parent_event_id)
            .await?;

        if let Some(event) = self
            .inner
            .get_events(session_id, &EventFilter::default())
            .await?
            .into_iter()
            .find(|event| event.event_id == event_id)
        {
            self.hub.publish(&event).await;
        }

        Ok(event_id)
    }

    async fn get_events(
        &self,
        session_id: SessionId,
        filter: &EventFilter,
    ) -> Result<Vec<Event>, lattice_core::StoreError> {
        self.inner.get_events(session_id, filter).await
    }

    async fn latest_event_id(
        &self,
        session_id: SessionId,
    ) -> Result<Option<EventId>, lattice_core::StoreError> {
        self.inner.latest_event_id(session_id).await
    }
}
