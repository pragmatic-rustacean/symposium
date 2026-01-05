//! History Actor for VS Code Language Model Provider
//!
//! The HistoryActor owns all session state and handles history matching.
//! It receives messages from both VS Code (via the JrConnectionCx handler)
//! and from SessionActors (outgoing parts). This centralizes all mutable
//! state in one actor with proper &mut access.

use futures::channel::mpsc;
use futures::StreamExt;
use uuid::Uuid;

use super::session_actor::{AgentDefinition, SessionActor};
use super::{
    ContentPart, Message, ProvideResponseRequest, ProvideResponseResponse,
    ResponseCompleteNotification, ResponsePartNotification, ROLE_ASSISTANT,
};
use sacp::JrConnectionCx;

use super::LmBackendToVsCode;

// ============================================================================
// Messages to HistoryActor
// ============================================================================

/// Messages that can be sent to the HistoryActor's mailbox.
pub enum HistoryActorMessage {
    /// A request from VS Code
    FromVsCode {
        request: ProvideResponseRequest,
        request_id: serde_json::Value,
        request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    },
    /// A cancel notification from VS Code
    CancelFromVsCode { request_id: serde_json::Value },
    /// A message from a SessionActor
    FromSession {
        session_id: Uuid,
        message: SessionToHistoryMessage,
    },
}

/// Messages from SessionActor to HistoryActor
pub enum SessionToHistoryMessage {
    /// A response part to forward to VS Code
    Part(ContentPart),
    /// The response is complete
    Complete,
    /// The session encountered an error
    Error(String),
}

// ============================================================================
// Handle for sending to HistoryActor
// ============================================================================

/// Handle for sending messages to the HistoryActor.
/// SessionActors hold this to send parts back.
#[derive(Clone)]
pub struct HistoryActorHandle {
    tx: mpsc::UnboundedSender<HistoryActorMessage>,
}

impl HistoryActorHandle {
    /// Send a message from a session to the history actor.
    pub fn send_from_session(&self, session_id: Uuid, message: SessionToHistoryMessage) {
        let _ = self.tx.unbounded_send(HistoryActorMessage::FromSession {
            session_id,
            message,
        });
    }

    /// Send a VS Code request to the history actor.
    pub fn send_from_vscode(
        &self,
        request: ProvideResponseRequest,
        request_id: serde_json::Value,
        request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    ) {
        let _ = self.tx.unbounded_send(HistoryActorMessage::FromVsCode {
            request,
            request_id,
            request_cx,
        });
    }

    /// Send a cancel notification from VS Code.
    pub fn send_cancel_from_vscode(&self, request_id: serde_json::Value) {
        let _ = self
            .tx
            .unbounded_send(HistoryActorMessage::CancelFromVsCode { request_id });
    }
}

// ============================================================================
// Session Data (history tracking per session)
// ============================================================================

/// Data for a single session, owned by HistoryActor.
struct SessionData {
    /// The session actor handle
    actor: SessionActor,
    /// The agent definition (for matching)
    agent_definition: AgentDefinition,
    /// Committed messages: complete history VS Code has acknowledged
    committed: Vec<Message>,
    /// Provisional messages: what we've received plus assistant response being built
    provisional_messages: Vec<Message>,
    /// Current streaming state
    streaming: Option<StreamingState>,
}

/// State when actively streaming a response
struct StreamingState {
    /// The JSON-RPC request ID of the in-flight request
    request_id: serde_json::Value,
    /// The request context for responding when done
    request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
}

/// Result of matching incoming messages against session history.
struct HistoryMatch {
    /// New messages to process (after matched prefix)
    new_messages: Vec<Message>,
    /// Whether the provisional work was canceled
    canceled: bool,
}

impl SessionData {
    fn new(actor: SessionActor, agent_definition: AgentDefinition) -> Self {
        Self {
            actor,
            agent_definition,
            committed: Vec::new(),
            provisional_messages: Vec::new(),
            streaming: None,
        }
    }

    /// Check if incoming messages match our expected history and return match info.
    fn match_history(&self, incoming: &[Message]) -> Option<HistoryMatch> {
        let committed_len = self.committed.len();

        // Incoming must at least start with committed
        if incoming.len() < committed_len {
            return None;
        }
        if &incoming[..committed_len] != self.committed.as_slice() {
            return None;
        }

        let after_committed = &incoming[committed_len..];

        // Check if the new messages have the provisional messages as a prefix
        if !after_committed.starts_with(&self.provisional_messages) {
            // They do not. This must be a cancellation of the provisional content.
            return Some(HistoryMatch {
                new_messages: after_committed.to_vec(),
                canceled: true,
            });
        }

        Some(HistoryMatch {
            new_messages: after_committed[self.provisional_messages.len()..].to_vec(),
            canceled: false,
        })
    }

    /// Record that we're sending a response part.
    fn record_part(&mut self, part: ContentPart) {
        match self.provisional_messages.last_mut() {
            Some(msg) if msg.role == ROLE_ASSISTANT => {
                msg.content.push(part);
            }
            _ => {
                self.provisional_messages.push(Message {
                    role: ROLE_ASSISTANT.to_string(),
                    content: vec![part],
                });
            }
        }
    }

    /// Commit the provisional exchange.
    fn commit_provisional(&mut self) {
        self.committed.append(&mut self.provisional_messages);
    }

    /// Discard provisional.
    fn discard_provisional(&mut self) {
        self.provisional_messages.clear();
    }

    /// Start a new provisional exchange.
    fn start_provisional(&mut self, messages: Vec<Message>) {
        self.provisional_messages = messages;
    }
}

// ============================================================================
// HistoryActor
// ============================================================================

/// The HistoryActor owns all session state and handles history matching.
pub struct HistoryActor {
    /// Mailbox receiver
    rx: mpsc::UnboundedReceiver<HistoryActorMessage>,
    /// Handle for creating new session actors
    handle: HistoryActorHandle,
    /// Connection to VS Code for sending notifications
    cx: JrConnectionCx<LmBackendToVsCode>,
    /// All sessions
    sessions: Vec<SessionData>,
}

impl HistoryActor {
    /// Create a new HistoryActor and return a handle to it.
    pub fn new(cx: JrConnectionCx<LmBackendToVsCode>) -> (Self, HistoryActorHandle) {
        let (tx, rx) = mpsc::unbounded();
        let handle = HistoryActorHandle { tx };
        let actor = Self {
            rx,
            handle: handle.clone(),
            cx,
            sessions: Vec::new(),
        };
        (actor, handle)
    }

    /// Run the actor's main loop.
    pub async fn run(mut self) -> Result<(), sacp::Error> {
        while let Some(msg) = self.rx.next().await {
            match msg {
                HistoryActorMessage::FromVsCode {
                    request,
                    request_id,
                    request_cx,
                } => {
                    self.handle_vscode_request(request, request_id, request_cx)?;
                }
                HistoryActorMessage::CancelFromVsCode { request_id } => {
                    self.handle_vscode_cancel(request_id);
                }
                HistoryActorMessage::FromSession {
                    session_id,
                    message,
                } => {
                    self.handle_session_message(session_id, message)?;
                }
            }
        }
        Ok(())
    }

    /// Handle a request from VS Code.
    fn handle_vscode_request(
        &mut self,
        request: ProvideResponseRequest,
        request_id: serde_json::Value,
        request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    ) -> Result<(), sacp::Error> {
        tracing::debug!(?request, "HistoryActor: received VS Code request");

        // Find session with best history match
        let best_match = self
            .sessions
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.match_history(&request.messages).map(|m| (i, m)))
            .max_by_key(|(_, m)| !m.canceled); // prefer non-canceled matches

        let (session_idx, history_match) = if let Some((idx, history_match)) = best_match {
            tracing::debug!(
                session_id = %self.sessions[idx].actor.session_id(),
                canceled = history_match.canceled,
                new_message_count = history_match.new_messages.len(),
                "matched existing session"
            );
            (idx, history_match)
        } else {
            // No matching session - create a new one
            let actor = SessionActor::spawn(self.handle.clone(), request.agent.clone())?;
            tracing::debug!(
                session_id = %actor.session_id(),
                "created new session"
            );
            self.sessions
                .push(SessionData::new(actor, request.agent.clone()));
            let history_match = HistoryMatch {
                new_messages: request.messages.clone(),
                canceled: false,
            };
            (self.sessions.len() - 1, history_match)
        };

        let session_data = &mut self.sessions[session_idx];

        // Handle cancellation if needed
        if history_match.canceled {
            session_data.discard_provisional();
        }

        // If there are no new messages, respond immediately
        if history_match.new_messages.is_empty() {
            return request_cx.respond(ProvideResponseResponse {});
        }

        // Commit any previous provisional (new messages confirm it was accepted)
        if !history_match.canceled {
            session_data.commit_provisional();
        }

        // Start new provisional with the new messages
        session_data.start_provisional(history_match.new_messages.clone());

        // Store streaming state
        session_data.streaming = Some(StreamingState {
            request_id,
            request_cx,
        });

        // Send to session actor
        session_data
            .actor
            .send_messages(history_match.new_messages, history_match.canceled);

        Ok(())
    }

    /// Handle a cancel notification from VS Code.
    fn handle_vscode_cancel(&mut self, request_id: serde_json::Value) {
        tracing::debug!(?request_id, "HistoryActor: received cancel");

        // Find the session streaming this request
        if let Some(session_data) = self
            .sessions
            .iter_mut()
            .find(|s| matches!(&s.streaming, Some(st) if st.request_id == request_id))
        {
            session_data.streaming = None;
            session_data.actor.cancel();
            tracing::debug!(
                session_id = %session_data.actor.session_id(),
                "cancelled streaming response"
            );
        } else {
            tracing::warn!(?request_id, "cancel for unknown request");
        }
    }

    /// Handle a message from a SessionActor.
    fn handle_session_message(
        &mut self,
        session_id: Uuid,
        message: SessionToHistoryMessage,
    ) -> Result<(), sacp::Error> {
        let Some(session_data) = self
            .sessions
            .iter_mut()
            .find(|s| s.actor.session_id() == session_id)
        else {
            tracing::warn!(%session_id, "message from unknown session");
            return Ok(());
        };

        // Get the request_id first (before mutable borrows)
        let Some(request_id) = session_data
            .streaming
            .as_ref()
            .map(|s| s.request_id.clone())
        else {
            tracing::warn!(%session_id, "message but not streaming");
            return Ok(());
        };

        match message {
            SessionToHistoryMessage::Part(part) => {
                // Record the part in provisional history
                session_data.record_part(part.clone());

                // Forward to VS Code
                self.cx
                    .send_notification(ResponsePartNotification { request_id, part })?;
            }
            SessionToHistoryMessage::Complete => {
                // Send completion notification
                self.cx
                    .send_notification(ResponseCompleteNotification { request_id })?;

                // Respond to the request
                let streaming = session_data.streaming.take().unwrap();
                streaming.request_cx.respond(ProvideResponseResponse {})?;
            }
            SessionToHistoryMessage::Error(err) => {
                tracing::error!(%session_id, %err, "session error");
                // Take streaming and respond with error
                if let Some(streaming) = session_data.streaming.take() {
                    streaming
                        .request_cx
                        .respond_with_error(sacp::Error::new(-32000, err))?;
                }
            }
        }

        Ok(())
    }
}
