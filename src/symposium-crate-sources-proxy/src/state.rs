//! Shared state for tracking active research sessions.

use fxhash::FxHashSet;
use sacp::schema::SessionId;
use std::sync::Mutex;

/// Shared state tracking active research sessions.
///
/// This state is shared between:
/// - The main event loop (in Component::serve) which uses it to identify research sessions
///   when handling RequestPermissionRequest, tool calls, etc.
/// - The research_agent functions which register/unregister session_ids
///
/// Note: The oneshot::Sender for sending responses back is NOT stored here.
/// It's owned by the research_agent::run function and used directly when
/// return_response_to_user is called.
pub struct ResearchState {
    /// Set of session IDs that correspond to active research requests.
    /// The main loop checks this to decide how to handle session-specific messages.
    active_research_session_ids: Mutex<FxHashSet<SessionId>>,
}

impl ResearchState {
    /// Create a new ResearchState with no active sessions.
    pub fn new() -> Self {
        Self {
            active_research_session_ids: Mutex::new(FxHashSet::default()),
        }
    }

    /// Register a new research session ID.
    ///
    /// Called by research_agent::run after spawning a sub-agent session.
    pub fn register_session(&self, session_id: &SessionId) {
        let mut sessions = self.active_research_session_ids.lock().unwrap();
        sessions.insert(session_id.clone());
    }

    /// Check if a session ID corresponds to an active research session.
    ///
    /// Used by the main event loop to determine if special handling is needed
    /// (e.g., auto-approving Read permissions).
    pub fn is_research_session(&self, session_id: &SessionId) -> bool {
        let sessions = self.active_research_session_ids.lock().unwrap();
        sessions.contains(session_id)
    }

    /// Unregister a research session ID.
    ///
    /// Called by research_agent::run when the session completes or fails.
    pub fn unregister_session(&self, session_id: &SessionId) {
        let mut sessions = self.active_research_session_ids.lock().unwrap();
        sessions.remove(session_id);
    }
}
