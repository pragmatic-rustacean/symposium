//! Session actor for VS Code Language Model Provider
//!
//! Each session actor manages a single conversation with an LLM backend (currently Eliza,
//! eventually an ACP agent). The actor pattern isolates session state and enables clean
//! cancellation via channel closure.

use elizacp::eliza::Eliza;
use tokio::sync::mpsc;

use super::{LmBackendToVsCode, Message, ResponsePart};

/// Message sent to the session actor
struct SessionMessage {
    /// New messages to process (not the full history, just what's new)
    new_messages: Vec<Message>,
    /// Channel for streaming response parts back
    reply_tx: mpsc::UnboundedSender<ResponsePart>,
}

/// Handle for communicating with a session actor.
///
/// This follows the Tokio actor pattern: the handle owns a sender channel and provides
/// methods for interacting with the actor. The actor itself runs in a spawned task.
pub struct SessionActor {
    tx: mpsc::UnboundedSender<SessionMessage>,
    /// The message history this session has processed
    history: Vec<Message>,
}

impl SessionActor {
    /// Spawn a new session actor.
    ///
    /// Creates the actor's mailbox and spawns the run loop. Returns a handle
    /// for sending messages to the actor.
    ///
    /// If `deterministic` is true, uses deterministic Eliza responses (for testing).
    pub fn spawn(
        cx: &sacp::JrConnectionCx<LmBackendToVsCode>,
        deterministic: bool,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::unbounded_channel();
        let eliza = if deterministic {
            Eliza::new_deterministic()
        } else {
            Eliza::new()
        };
        cx.spawn(Self::run(rx, eliza))?;
        Ok(Self {
            tx,
            history: Vec::new(),
        })
    }

    /// Send new content to the actor, returns a receiver for streaming response.
    ///
    /// The caller should stream from the returned receiver until it closes,
    /// which signals that the actor has finished processing.
    ///
    /// To cancel the request, simply drop the receiver - the actor will see
    /// send failures and stop processing.
    pub fn send_prompt(
        &mut self,
        new_messages: Vec<Message>,
    ) -> mpsc::UnboundedReceiver<ResponsePart> {
        let (reply_tx, reply_rx) = mpsc::unbounded_channel();

        // Update our history with what we're sending
        self.history.extend(new_messages.clone());

        // Send to the actor (ignore errors - actor may have died)
        let _ = self.tx.send(SessionMessage {
            new_messages,
            reply_tx,
        });

        reply_rx
    }

    /// Check if incoming messages extend our history.
    ///
    /// Returns the number of matching prefix messages, or None if the incoming
    /// messages don't start with our history.
    pub fn prefix_match_len(&self, messages: &[Message]) -> Option<usize> {
        if messages.len() < self.history.len() {
            return None;
        }
        if self
            .history
            .iter()
            .zip(messages.iter())
            .all(|(a, b)| a == b)
        {
            Some(self.history.len())
        } else {
            None
        }
    }

    /// The actor's main run loop.
    async fn run(
        mut rx: mpsc::UnboundedReceiver<SessionMessage>,
        mut eliza: Eliza,
    ) -> Result<(), sacp::Error> {
        while let Some(msg) = rx.recv().await {
            // Process each new message
            for message in msg.new_messages {
                if message.role == "user" {
                    let response = eliza.respond(&message.text());

                    // Stream response in chunks
                    for chunk in response.chars().collect::<Vec<_>>().chunks(5) {
                        let text: String = chunk.iter().collect();
                        if msg
                            .reply_tx
                            .send(ResponsePart::Text { value: text })
                            .is_err()
                        {
                            // Channel closed = request was cancelled
                            // Break out of chunk loop, but continue to next message
                            // (or if this was the last message, wait for next SessionMessage)
                            break;
                        }
                    }
                }
            }
            // reply_tx drops here when msg goes out of scope, signaling completion
        }
        Ok(())
    }
}
