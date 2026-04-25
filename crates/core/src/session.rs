//! Session-related types and the SessionStore trait.

use async_trait::async_trait;

use crate::error::StoreError;
use crate::{Actor, Event, EventFilter, EventId, EventPayload, SessionId};

/// Session store — event log persistence.
///
/// Implementations must be append-only: events can never be modified or deleted.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Create a new session and return its id.
    async fn create_session(&self) -> Result<SessionId, StoreError>;

    /// Append an immutable event to the session.
    ///
    /// Returns the newly assigned `EventId`.
    async fn append_event(
        &self,
        session_id: SessionId,
        payload: EventPayload,
        actor: Actor,
        parent_event_id: Option<EventId>,
    ) -> Result<EventId, StoreError>;

    /// Retrieve events for a session, optionally filtered.
    async fn get_events(
        &self,
        session_id: SessionId,
        filter: &EventFilter,
    ) -> Result<Vec<Event>, StoreError>;

    /// Get the latest event id for a session.
    async fn latest_event_id(&self, session_id: SessionId) -> Result<Option<EventId>, StoreError>;
}
