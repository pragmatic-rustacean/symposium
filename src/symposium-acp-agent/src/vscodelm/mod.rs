//! VS Code Language Model Provider backend
//!
//! This module implements the Rust backend for the VS Code `LanguageModelChatProvider` API.
//! It uses sacp's JSON-RPC infrastructure for communication with the TypeScript extension.

mod session_actor;

use anyhow::Result;
use sacp::{
    link::RemoteStyle, util::MatchMessage, Component, Handled, JrConnectionCx, JrLink,
    JrMessageHandler, JrNotification, JrPeer, JrRequest, JrResponsePayload, MessageCx,
};
use serde::{Deserialize, Serialize};
use session_actor::SessionActor;
use std::path::PathBuf;

// ============================================================================
// Peers
// ============================================================================

/// Peer representing the VS Code extension (TypeScript side).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VsCodePeer;

impl JrPeer for VsCodePeer {}

/// Peer representing the LM backend (Rust side).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LmBackendPeer;

impl JrPeer for LmBackendPeer {}

// ============================================================================
// Links
// ============================================================================

/// Link from the LM backend's perspective (talking to VS Code).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LmBackendToVsCode;

impl JrLink for LmBackendToVsCode {
    type ConnectsTo = VsCodeToLmBackend;
    type State = ();
}

impl sacp::HasDefaultPeer for LmBackendToVsCode {
    type DefaultPeer = VsCodePeer;
}

impl sacp::HasPeer<VsCodePeer> for LmBackendToVsCode {
    fn remote_style(_peer: VsCodePeer) -> RemoteStyle {
        RemoteStyle::Counterpart
    }
}

/// Link from VS Code's perspective (talking to the LM backend).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VsCodeToLmBackend;

impl JrLink for VsCodeToLmBackend {
    type ConnectsTo = LmBackendToVsCode;
    type State = ();
}

impl sacp::HasDefaultPeer for VsCodeToLmBackend {
    type DefaultPeer = LmBackendPeer;
}

impl sacp::HasPeer<LmBackendPeer> for VsCodeToLmBackend {
    fn remote_style(_peer: LmBackendPeer) -> RemoteStyle {
        RemoteStyle::Counterpart
    }
}

// ============================================================================
// Message Types
// ============================================================================

/// Message content part
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContentPart {
    Text { value: String },
}

/// A chat message
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentPart>,
}

impl Message {
    /// Extract text content from the message
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { value } => Some(value.as_str()),
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub family: String,
    pub version: String,
    pub max_input_tokens: u32,
    pub max_output_tokens: u32,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    #[serde(default)]
    pub tool_calling: bool,
}

// ----------------------------------------------------------------------------
// lm/provideLanguageModelChatInformation
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrRequest)]
#[request(method = "lm/provideLanguageModelChatInformation", response = ProvideInfoResponse)]
pub struct ProvideInfoRequest {
    #[serde(default)]
    pub silent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JrResponsePayload)]
pub struct ProvideInfoResponse {
    pub models: Vec<ModelInfo>,
}

// ----------------------------------------------------------------------------
// lm/provideLanguageModelChatResponse
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrRequest)]
#[request(method = "lm/provideLanguageModelChatResponse", response = ProvideResponseResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProvideResponseRequest {
    pub model_id: String,
    pub messages: Vec<Message>,
    pub agent: session_actor::AgentDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, JrResponsePayload)]
pub struct ProvideResponseResponse {}

// ----------------------------------------------------------------------------
// lm/responsePart (notification: backend -> vscode)
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrNotification)]
#[notification(method = "lm/responsePart")]
#[serde(rename_all = "camelCase")]
pub struct ResponsePartNotification {
    pub request_id: serde_json::Value,
    pub part: ResponsePart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResponsePart {
    Text { value: String },
}

// ----------------------------------------------------------------------------
// lm/responseComplete (notification: backend -> vscode)
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrNotification)]
#[notification(method = "lm/responseComplete")]
#[serde(rename_all = "camelCase")]
pub struct ResponseCompleteNotification {
    pub request_id: serde_json::Value,
}

// ----------------------------------------------------------------------------
// lm/cancel (notification: vscode -> backend)
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrNotification)]
#[notification(method = "lm/cancel")]
#[serde(rename_all = "camelCase")]
pub struct CancelNotification {
    pub request_id: serde_json::Value,
}

// ----------------------------------------------------------------------------
// lm/provideTokenCount
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JrRequest)]
#[request(method = "lm/provideTokenCount", response = ProvideTokenCountResponse)]
#[serde(rename_all = "camelCase")]
pub struct ProvideTokenCountRequest {
    pub model_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JrResponsePayload)]
pub struct ProvideTokenCountResponse {
    pub count: u32,
}

// ============================================================================
// Message Handler
// ============================================================================

use tokio::sync::oneshot;

/// A session with its current state.
struct SessionData {
    actor: SessionActor,
    state: SessionState,
}

/// State of a session from the handler's perspective.
enum SessionState {
    /// Session is idle, waiting for a prompt.
    Idle,
    /// Session is streaming a response.
    Streaming {
        /// The JSON-RPC request ID of the in-flight request.
        request_id: serde_json::Value,
        /// Send on this channel to cancel the streaming response.
        cancel_tx: oneshot::Sender<()>,
    },
}

impl SessionState {
    /// Cancel any in-progress streaming and transition to Idle.
    /// No-op if already Idle.
    fn cancel(&mut self) {
        let old_state = std::mem::replace(self, SessionState::Idle);
        if let SessionState::Streaming { cancel_tx, .. } = old_state {
            // Ignore send error - receiver may already be gone
            let _ = cancel_tx.send(());
        }
    }
}

impl SessionData {
    /// Check if incoming messages extend this session's history.
    fn prefix_match_len(&self, messages: &[Message]) -> Option<usize> {
        self.actor.prefix_match_len(messages)
    }

    /// Returns true if this session is streaming with the given request ID.
    fn is_streaming_request(&self, request_id: &serde_json::Value) -> bool {
        matches!(&self.state, SessionState::Streaming { request_id: rid, .. } if rid == request_id)
    }
}

/// Handler for LM backend messages
pub struct LmBackendHandler {
    /// Active sessions, searched linearly for prefix matches
    sessions: Vec<SessionData>,
}

impl LmBackendHandler {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }
}

/// JSON-RPC error code for request cancellation.
/// Using -32800 which is in the server error range (-32000 to -32099 reserved for implementation).
const ERROR_CODE_CANCELLED: i32 = -32800;

/// Stream response parts from the session actor, with cancellation support.
///
/// This function races between:
/// - Receiving response parts from the actor
/// - Receiving a cancellation signal
///
/// On normal completion, sends `lm/responseComplete` and responds to the request.
/// On cancellation, responds with a cancellation error.
async fn stream_response(
    cx: JrConnectionCx<LmBackendToVsCode>,
    request_id: serde_json::Value,
    request_cx: sacp::JrRequestCx<ProvideResponseResponse>,
    mut reply_rx: tokio::sync::mpsc::UnboundedReceiver<ResponsePart>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<(), sacp::Error> {
    use futures_concurrency::future::Race;

    loop {
        // Race between receiving a part and receiving cancellation
        enum Outcome {
            Part(Option<ResponsePart>),
            Cancelled,
        }

        let outcome = (async { Outcome::Part(reply_rx.recv().await) }, async {
            let _ = (&mut cancel_rx).await;
            Outcome::Cancelled
        })
            .race()
            .await;

        match outcome {
            Outcome::Part(Some(part)) => {
                cx.send_notification(ResponsePartNotification {
                    request_id: request_id.clone(),
                    part,
                })?;
            }
            Outcome::Part(None) => {
                // Stream complete - send completion notification and respond
                cx.send_notification(ResponseCompleteNotification {
                    request_id: request_id.clone(),
                })?;
                request_cx.respond(ProvideResponseResponse {})?;
                break;
            }
            Outcome::Cancelled => {
                // Cancelled - respond with error
                tracing::debug!(?request_id, "streaming cancelled");
                request_cx.respond_with_error(sacp::Error::new(
                    ERROR_CODE_CANCELLED,
                    "Request cancelled",
                ))?;
                break;
            }
        }
    }

    Ok(())
}

impl JrMessageHandler for LmBackendHandler {
    type Link = LmBackendToVsCode;

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "LmBackendHandler"
    }

    async fn handle_message(
        &mut self,
        message: MessageCx,
        cx: JrConnectionCx<Self::Link>,
    ) -> Result<Handled<MessageCx>, sacp::Error> {
        tracing::trace!(?message, "handle_message");
        MatchMessage::new(message)
            .if_request(async |_req: ProvideInfoRequest, request_cx| {
                let response = ProvideInfoResponse {
                    models: vec![ModelInfo {
                        id: "symposium-eliza".to_string(),
                        name: "Symposium (Eliza)".to_string(),
                        family: "symposium".to_string(),
                        version: "1.0.0".to_string(),
                        max_input_tokens: 100000,
                        max_output_tokens: 100000,
                        capabilities: ModelCapabilities { tool_calling: true },
                    }],
                };
                request_cx.respond(response)
            })
            .await
            .if_request(async |req: ProvideTokenCountRequest, request_cx| {
                // Simple heuristic: 1 token ≈ 4 characters
                let count = (req.text.len() / 4).max(1) as u32;
                request_cx.respond(ProvideTokenCountResponse { count })
            })
            .await
            .if_request(async |req: ProvideResponseRequest, request_cx| {
                tracing::debug!(?req, "ProvideResponseRequest");

                // Get the request ID from the request context for notifications
                let request_id = request_cx.id().clone();

                // Find session with longest matching prefix
                let (session_idx, prefix_len) = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| s.prefix_match_len(&req.messages).map(|len| (i, len)))
                    .max_by_key(|(_, len)| *len)
                    .unwrap_or((usize::MAX, 0));

                // Get or create session
                let session_data = if session_idx < self.sessions.len() {
                    let session_data = &mut self.sessions[session_idx];
                    tracing::debug!(
                        session_id = %session_data.actor.session_id(),
                        prefix_len,
                        "continuing existing session"
                    );
                    session_data
                } else {
                    let actor = SessionActor::spawn(&cx, req.agent.clone())?;
                    self.sessions.push(SessionData {
                        actor,
                        state: SessionState::Idle,
                    });
                    self.sessions.last_mut().unwrap()
                };

                // If session is currently streaming, cancel it first
                if !matches!(session_data.state, SessionState::Idle) {
                    tracing::debug!(
                        session_id = %session_data.actor.session_id(),
                        "cancelling previous streaming before starting new request"
                    );
                    session_data.state.cancel();
                }

                // Compute new messages (everything after the matched prefix)
                let new_messages = req.messages[prefix_len..].to_vec();
                tracing::debug!(
                    session_id = %session_data.actor.session_id(),
                    new_message_count = new_messages.len(),
                    "sending new messages to session"
                );

                // Create cancellation channel
                let (cancel_tx, cancel_rx) = oneshot::channel();

                // Send prompt to actor
                let reply_rx = session_data.actor.send_prompt(new_messages);

                // Transition to Streaming state
                session_data.state = SessionState::Streaming {
                    request_id: request_id.clone(),
                    cancel_tx,
                };

                // Spawn task to stream response (non-blocking)
                cx.spawn(stream_response(
                    cx.clone(),
                    request_id,
                    request_cx,
                    reply_rx,
                    cancel_rx,
                ))?;

                Ok(())
            })
            .await
            .if_notification(async |notification: CancelNotification| {
                tracing::debug!(?notification, "CancelNotification");

                // Find the session streaming this request
                if let Some(session_data) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.is_streaming_request(&notification.request_id))
                {
                    session_data.state.cancel();
                    tracing::debug!(
                        session_id = %session_data.actor.session_id(),
                        "cancelled streaming response"
                    );
                } else {
                    tracing::warn!(
                        request_id = ?notification.request_id,
                        "cancel notification for unknown request"
                    );
                }

                Ok(())
            })
            .await
            .otherwise(async |message| match message {
                MessageCx::Request(request, request_cx) => {
                    tracing::warn!("unknown request method: {}", request.method());
                    request_cx.respond_with_error(sacp::Error::method_not_found())
                }
                MessageCx::Notification(notif) => {
                    tracing::warn!("unexpected notification: {}", notif.method());
                    Ok(())
                }
            })
            .await?;

        Ok(Handled::Yes)
    }
}

// ============================================================================
// Component Implementation
// ============================================================================

/// The LM backend component that can be used with sacp's Component infrastructure.
pub struct LmBackend {
    handler: LmBackendHandler,
}

impl LmBackend {
    pub fn new() -> Self {
        Self {
            handler: LmBackendHandler::new(),
        }
    }
}

impl Default for LmBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl sacp::Component<LmBackendToVsCode> for LmBackend {
    async fn serve(
        self,
        client: impl sacp::Component<VsCodeToLmBackend>,
    ) -> Result<(), sacp::Error> {
        LmBackendToVsCode::builder()
            .with_handler(self.handler)
            .serve(client)
            .await
    }
}

// ============================================================================
// Server (for CLI usage)
// ============================================================================

/// Run the LM backend on stdio
pub async fn serve_stdio(trace_dir: Option<PathBuf>) -> Result<()> {
    let stdio = if let Some(dir) = trace_dir {
        std::fs::create_dir_all(&dir)?;
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let trace_path = dir.join(format!("vscodelm-{}.log", timestamp));
        let file = std::sync::Arc::new(std::sync::Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&trace_path)?,
        ));
        tracing::info!(?trace_path, "Logging vscodelm messages");

        sacp_tokio::Stdio::new().with_debug(move |line, direction| {
            use std::io::Write;
            let dir_str = match direction {
                sacp_tokio::LineDirection::Stdin => "recv",
                sacp_tokio::LineDirection::Stdout => "send",
                sacp_tokio::LineDirection::Stderr => "stderr",
            };
            if let Ok(mut f) = file.lock() {
                let _ = writeln!(
                    f,
                    "[{}] {}: {}",
                    chrono::Utc::now().to_rfc3339(),
                    dir_str,
                    line
                );
                let _ = f.flush();
            }
        })
    } else {
        sacp_tokio::Stdio::new()
    };

    LmBackend::new().serve(stdio).await?;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;

    #[tokio::test]
    async fn test_provide_info() -> Result<(), sacp::Error> {
        VsCodeToLmBackend::builder()
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                let response = cx
                    .send_request(ProvideInfoRequest { silent: false })
                    .block_task()
                    .await?;

                expect![[r#"
                    ProvideInfoResponse {
                        models: [
                            ModelInfo {
                                id: "symposium-eliza",
                                name: "Symposium (Eliza)",
                                family: "symposium",
                                version: "1.0.0",
                                max_input_tokens: 100000,
                                max_output_tokens: 100000,
                                capabilities: ModelCapabilities {
                                    tool_calling: true,
                                },
                            },
                        ],
                    }
                "#]]
                .assert_debug_eq(&response);

                Ok(())
            })
            .await
    }

    #[tokio::test]
    async fn test_provide_token_count() -> Result<(), sacp::Error> {
        VsCodeToLmBackend::builder()
            .connect_to(LmBackend::new())?
            .run_until(async |cx| {
                let response = cx
                    .send_request(ProvideTokenCountRequest {
                        model_id: "symposium-eliza".to_string(),
                        text: "Hello, world!".to_string(),
                    })
                    .block_task()
                    .await?;

                expect![[r#"
                    ProvideTokenCountResponse {
                        count: 3,
                    }
                "#]]
                .assert_debug_eq(&response);

                Ok(())
            })
            .await
    }

    // TODO: Add integration tests that spawn a real agent process
    // The chat_response and session_continuation tests have been removed
    // because they relied on the old in-process Eliza implementation.
    // With the new architecture, the session actor spawns an external
    // ACP agent process, which requires different test infrastructure.

    #[test]
    fn test_agent_definition_eliza_serialization() {
        use super::session_actor::AgentDefinition;

        let agent = AgentDefinition::Eliza {
            deterministic: true,
        };
        let json = serde_json::to_string_pretty(&agent).unwrap();
        println!("Eliza:\n{}", json);

        // Should serialize as {"eliza": {"deterministic": true}}
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("eliza").is_some());
        assert_eq!(parsed["eliza"]["deterministic"], true);
    }

    #[test]
    fn test_agent_definition_mcp_server_serialization() {
        use super::session_actor::AgentDefinition;
        use sacp::schema::{McpServer, McpServerStdio};

        let server = McpServer::Stdio(McpServerStdio::new("test", "echo"));
        let agent = AgentDefinition::McpServer(server);
        let json = serde_json::to_string_pretty(&agent).unwrap();
        println!("McpServer:\n{}", json);

        // Should serialize as {"mcp_server": {name, command, args, env}}
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("mcp_server").is_some());
        assert_eq!(parsed["mcp_server"]["name"], "test");
        assert_eq!(parsed["mcp_server"]["command"], "echo");
    }
}
