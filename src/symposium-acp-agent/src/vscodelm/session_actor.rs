//! Session actor for VS Code Language Model Provider
//!
//! Each session actor manages a single conversation with an ACP agent. It receives
//! messages from the HistoryActor and sends response parts back to it.

use elizacp::ElizaAgent;
use futures::channel::{mpsc, oneshot};
use futures::stream::Peekable;
use futures::{FutureExt, StreamExt, TryFutureExt};
use futures_concurrency::future::FutureExt as _;
use sacp::schema::{ToolCallUpdate, ToolCallUpdateFields};
use sacp::JrConnectionCx;
use sacp::{
    schema::{
        InitializeRequest, ProtocolVersion, RequestPermissionOutcome, RequestPermissionRequest,
        RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    },
    ClientToAgent, Component, MessageCx,
};
use sacp_tokio::AcpAgent;
use std::path::PathBuf;
use std::pin::Pin;
use uuid::Uuid;

use super::history_actor::{HistoryActorHandle, SessionToHistoryMessage};
use super::{ContentPart, Message, ROLE_USER, SYMPOSIUM_AGENT_ACTION};

/// Defines which agent backend to use for a session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDefinition {
    /// Use the in-process Eliza chatbot (for testing)
    Eliza {
        #[serde(default)]
        deterministic: bool,
    },
    /// Spawn an external ACP agent process
    McpServer(sacp::schema::McpServer),
}

/// Messages sent to SessionActor from HistoryActor.
#[derive(Debug)]
pub struct SessionRequest {
    /// New messages to process
    pub messages: Vec<Message>,
    /// Whether this request represents a cancellation of previous work
    pub canceled: bool,
    /// Cancelation channel for this request
    pub cancel_rx: oneshot::Receiver<()>,
}

/// Handle for communicating with a session actor.
pub struct SessionActor {
    /// Channel to send requests to the actor
    tx: mpsc::UnboundedSender<SessionRequest>,
    /// Unique identifier for this session
    session_id: Uuid,
}

impl SessionActor {
    /// Spawn a new session actor.
    pub fn spawn(
        history_handle: HistoryActorHandle,
        agent_definition: AgentDefinition,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::unbounded();
        let session_id = Uuid::new_v4();

        tracing::info!(%session_id, ?agent_definition, "spawning new session actor");

        // Spawn the actor task
        tokio::spawn(Self::run(rx, history_handle, agent_definition, session_id));

        Ok(Self { tx, session_id })
    }

    /// Returns the session ID.
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Send messages to the session actor.
    pub fn send_messages(
        &self,
        messages: Vec<Message>,
        canceled: bool,
        cancel_rx: oneshot::Receiver<()>,
    ) {
        let _ = self.tx.unbounded_send(SessionRequest {
            messages,
            canceled,
            cancel_rx,
        });
    }

    /// The actor's main run loop.
    async fn run(
        request_rx: mpsc::UnboundedReceiver<SessionRequest>,
        history_handle: HistoryActorHandle,
        agent_definition: AgentDefinition,
        session_id: Uuid,
    ) -> Result<(), sacp::Error> {
        tracing::debug!(%session_id, "session actor starting");

        let result = match agent_definition {
            AgentDefinition::Eliza { deterministic } => {
                let agent = ElizaAgent::new(deterministic);
                Self::run_with_agent(request_rx, history_handle.clone(), agent, session_id).await
            }
            AgentDefinition::McpServer(config) => {
                let agent = AcpAgent::new(config);
                Self::run_with_agent(request_rx, history_handle.clone(), agent, session_id).await
            }
        };

        if let Err(ref e) = result {
            history_handle
                .send_from_session(session_id, SessionToHistoryMessage::Error(e.to_string()))?;
        }

        result
    }

    /// Run the session with a specific agent component.
    async fn run_with_agent(
        request_rx: mpsc::UnboundedReceiver<SessionRequest>,
        history_handle: HistoryActorHandle,
        agent: impl Component<sacp::link::AgentToClient>,
        session_id: Uuid,
    ) -> Result<(), sacp::Error> {
        ClientToAgent::builder()
            .connect_to(agent)?
            .run_until(async |cx| {
                tracing::debug!(%session_id, "connected to agent, initializing");

                let _init_response = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                    .block_task()
                    .await?;

                tracing::debug!(%session_id, "agent initialized, creating session");

                Self::run_with_cx(request_rx, history_handle, cx, session_id).await
            })
            .await
    }

    async fn run_with_cx(
        request_rx: mpsc::UnboundedReceiver<SessionRequest>,
        history_handle: HistoryActorHandle,
        cx: JrConnectionCx<ClientToAgent>,
        session_id: Uuid,
    ) -> Result<(), sacp::Error> {
        // Create a session
        let mut session = cx
            .build_session(PathBuf::from("."))
            .block_task()
            .start_session()
            .await?;

        tracing::debug!(%session_id, "session created, waiting for messages");

        let mut request_rx = request_rx.peekable();

        while let Some(request) = request_rx.next().await {
            let new_message_count = request.messages.len();
            tracing::debug!(%session_id, new_message_count, canceled = request.canceled, "received request");

            let SessionRequest {
                messages,
                canceled: _,
                mut cancel_rx,
            } = request;

            // Build prompt from messages
            let prompt_text: String = messages
                .iter()
                .filter(|m| m.role == ROLE_USER)
                .map(|m| m.text())
                .collect::<Vec<_>>()
                .join("\n");

            if prompt_text.is_empty() {
                tracing::debug!(%session_id, "no user messages, skipping");
                history_handle.send_from_session(session_id, SessionToHistoryMessage::Complete)?;
                continue;
            }

            tracing::debug!(%session_id, %prompt_text, "sending prompt to agent");
            session.send_prompt(&prompt_text)?;

            // Read updates from the agent
            let canceled = loop {
                // Wait for either an update or a cancellation
                let cancel_future = (&mut cancel_rx).map(|_| Ok(None));
                let update = session
                    .read_update()
                    .map_ok(Some)
                    .race(cancel_future)
                    .await?;

                let Some(update) = update else {
                    // Canceled
                    break true;
                };

                match update {
                    sacp::SessionMessage::SessionMessage(message) => {
                        let new_cancel_rx = Self::process_session_message(
                            message,
                            &history_handle,
                            &mut request_rx,
                            cancel_rx,
                            session_id,
                        )
                        .await?;

                        match new_cancel_rx {
                            Some(c) => cancel_rx = c,
                            None => break true,
                        }
                    }
                    sacp::SessionMessage::StopReason(stop_reason) => {
                        tracing::debug!(%session_id, ?stop_reason, "agent turn complete");
                        break false;
                    }
                    other => {
                        tracing::trace!(%session_id, ?other, "ignoring session message");
                    }
                }
            };

            if canceled {
                cx.send_notification(sacp::schema::CancelNotification::new(
                    session.session_id().clone(),
                ))?;
            } else {
                // Turn completed normally
                history_handle.send_from_session(session_id, SessionToHistoryMessage::Complete)?;
            }
        }

        tracing::debug!(%session_id, "session actor shutting down");
        Ok(())
    }

    /// Process a single session message from the ACP agent.
    /// This will end the turn on the vscode side, so we consume the `cancel_rx`.
    /// Returns `Some` with a new `cancel_rx` if tool use was approved (and sends that response to the agent).
    /// Returns `None` if tool use was declined; the outer loop should await a new prompt.
    async fn process_session_message(
        message: MessageCx,
        history_handle: &HistoryActorHandle,
        request_rx: &mut Peekable<mpsc::UnboundedReceiver<SessionRequest>>,
        cancel_rx: oneshot::Receiver<()>,
        session_id: Uuid,
    ) -> Result<Option<oneshot::Receiver<()>>, sacp::Error> {
        use sacp::util::MatchMessage;

        let mut return_value = Some(cancel_rx);

        MatchMessage::new(message)
            .if_notification(async |notif: SessionNotification| {
                if let SessionUpdate::AgentMessageChunk(chunk) = notif.update {
                    let text = content_block_to_string(&chunk.content);
                    if !text.is_empty() {
                        history_handle.send_from_session(
                            session_id,
                            SessionToHistoryMessage::Part(ContentPart::Text { value: text }),
                        )?;
                    }
                }
                Ok(())
            })
            .await
            .if_request(async |perm_request: RequestPermissionRequest, request_cx| {
                tracing::debug!(%session_id, ?perm_request, "received permission request");

                let RequestPermissionRequest {
                    session_id: _,
                    tool_call:
                        ToolCallUpdate {
                            tool_call_id,
                            fields:
                                ToolCallUpdateFields {
                                    kind,
                                    status: _,
                                    title,
                                    content: _,
                                    locations: _,
                                    raw_input,
                                    raw_output: _,
                                    ..
                                },
                            meta: _,
                            ..
                        },
                    options,
                    meta: _,
                    ..
                } = perm_request;

                let tool_call_id_str = tool_call_id.to_string();

                let tool_call = ContentPart::ToolCall {
                    tool_call_id: tool_call_id_str.clone(),
                    tool_name: SYMPOSIUM_AGENT_ACTION.to_string(),
                    parameters: serde_json::json!({
                        "kind": kind,
                        "title": title,
                        "raw_input": raw_input,
                    }),
                };

                // Send tool call to history actor (which forwards to VS Code)
                history_handle.send_from_session(
                    session_id,
                    SessionToHistoryMessage::Part(tool_call),
                )?;

                // Signal completion so VS Code shows the confirmation UI
                history_handle.send_from_session(session_id, SessionToHistoryMessage::Complete)?;

                // Drop the cancel_rx because we just signaled completion.
                return_value = None;

                // Wait for the next request (which will have the tool result if approved)
                let Some(next_request) = Pin::new(&mut *request_rx).peek().await else {
                    request_cx.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ))?;
                    return Ok(());
                };

                // Check if canceled (history mismatch = rejection) or does not contain expected tool-use result
                if next_request.canceled || !next_request.messages[0].has_just_tool_result(&tool_call_id_str) {
                    tracing::debug!(%session_id, ?next_request, "permission denied, did not receive approval");
                    request_cx.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ))?;
                    return Ok(());
                }

                // Permission approved - find allow-once option and send.
                // If there is no such option, just cancel.
                let approve_once_outcome = options
                    .into_iter()
                    .find(|option| {
                        matches!(option.kind, sacp::schema::PermissionOptionKind::AllowOnce)
                    })
                    .map(|option| {
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                            option.option_id,
                        ))
                    });

                match approve_once_outcome {
                    Some(o) => request_cx.respond(RequestPermissionResponse::new(o))?,
                    None => {
                        request_cx.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ))?;
                        return Ok(());
                    }
                }

                // Consume the request
                let SessionRequest { messages, canceled, cancel_rx } = request_rx.next().await.expect("message is waiting");
                assert_eq!(canceled, false);
                assert_eq!(messages.len(), 1);
                return_value = Some(cancel_rx);

                Ok(())
            })
            .await
            .otherwise(async |message| {
                match message {
                    MessageCx::Request(req, request_cx) => {
                        tracing::warn!(%session_id, method = req.method(), "unknown request");
                        request_cx.respond_with_error(sacp::util::internal_error("unknown request"))?;
                    }
                    MessageCx::Notification(notif) => {
                        tracing::trace!(%session_id, method = notif.method(), "ignoring notification");
                    }
                }
                Ok(())
            })
            .await?;

        Ok(return_value)
    }
}

/// Convert a content block to a string representation
fn content_block_to_string(block: &sacp::schema::ContentBlock) -> String {
    use sacp::schema::{ContentBlock, EmbeddedResourceResource};
    match block {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(img) => format!("[Image: {}]", img.mime_type),
        ContentBlock::Audio(audio) => format!("[Audio: {}]", audio.mime_type),
        ContentBlock::ResourceLink(link) => link.uri.clone(),
        ContentBlock::Resource(resource) => match &resource.resource {
            EmbeddedResourceResource::TextResourceContents(text) => text.uri.clone(),
            EmbeddedResourceResource::BlobResourceContents(blob) => blob.uri.clone(),
            _ => "[Unknown resource type]".to_string(),
        },
        _ => "[Unknown content type]".to_string(),
    }
}

// TODO: request_response module is currently unused after refactoring to HistoryActor pattern.
// It may be useful later for a cleaner tool-call API, but needs to be updated for the new architecture.
// mod request_response;
