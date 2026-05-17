//! Session-related types and the SessionStore trait.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::StoreError;
use crate::{Actor, Event, EventFilter, EventId, EventPayload, SessionId, Timestamp};

/// Information about a child session.
#[derive(Clone)]
pub struct ChildSessionInfo {
    pub session_id: SessionId,
    pub store: Arc<dyn SessionStore>,
    pub skill_name: String,
    pub created_at: Timestamp,
}

impl fmt::Debug for ChildSessionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChildSessionInfo")
            .field("session_id", &self.session_id)
            .field("skill_name", &self.skill_name)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Session store — event log persistence.
///
/// Implementations must be append-only: events can never be modified or deleted.
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Create a new session and return its id.
    async fn create_session(&self) -> Result<SessionId, StoreError>;

    /// Delete an entire session and all of its events.
    async fn delete_session(&self, session_id: SessionId) -> Result<(), StoreError>;

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

    /// Create a child session under the given parent.
    async fn create_child_session(
        &self,
        parent_session_id: SessionId,
        skill_name: &str,
    ) -> Result<(SessionId, Arc<dyn SessionStore>), StoreError>;

    /// List all child sessions for a given parent.
    async fn child_sessions(
        &self,
        parent_session_id: SessionId,
    ) -> Result<Vec<ChildSessionInfo>, StoreError>;

    /// Get the latest event id for a session.
    async fn latest_event_id(&self, session_id: SessionId) -> Result<Option<EventId>, StoreError>;
}
