//! Config mode actor - handles the interactive configuration "phone tree" UI.
//!
//! This actor is spawned when a user enters config mode via `/symposium:config`.
//! It owns the configuration state and processes user input through a simple
//! text-based menu system.

use super::ConfigAgentMessage;
use crate::registry::{self, AgentListEntry};
use crate::user_config::SymposiumUserConfig;
use futures::channel::mpsc::{self, UnboundedSender};
use futures::StreamExt;
use regex::Regex;
use sacp::link::AgentToClient;
use sacp::schema::SessionId;
use sacp::JrConnectionCx;
use std::sync::LazyLock;

/// Messages sent to the config mode actor.
pub enum ConfigModeInput {
    /// User sent a prompt (the text content).
    UserInput(String),
}

/// Messages sent from the config mode actor back to ConfigAgent.
pub enum ConfigModeOutput {
    /// Send this text to the user.
    SendMessage(String),

    /// Configuration is complete - save and exit.
    Done {
        /// The final configuration to save.
        config: SymposiumUserConfig,
    },

    /// User cancelled - exit without saving.
    Cancelled,
}

/// Handle to communicate with the config mode actor.
#[derive(Clone)]
pub struct ConfigModeHandle {
    tx: mpsc::Sender<ConfigModeInput>,
}

impl ConfigModeHandle {
    /// Spawn a new config mode actor.
    ///
    /// Returns a handle for sending input to the actor.
    pub fn spawn(
        config: SymposiumUserConfig,
        session_id: SessionId,
        config_agent_tx: UnboundedSender<ConfigAgentMessage>,
        cx: &JrConnectionCx<AgentToClient>,
    ) -> Result<Self, sacp::Error> {
        let (tx, rx) = mpsc::channel(32);
        let handle = Self { tx };

        cx.spawn(run_actor(config, session_id, config_agent_tx, rx))?;

        Ok(handle)
    }

    /// Send user input to the actor.
    pub async fn send_input(&self, text: String) -> Result<(), sacp::Error> {
        self.tx
            .clone()
            .try_send(ConfigModeInput::UserInput(text))
            .map_err(|_| sacp::util::internal_error("Config mode actor closed"))
    }
}

/// Context passed through the actor's async functions.
struct ActorContext {
    config: SymposiumUserConfig,
    session_id: SessionId,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    rx: mpsc::Receiver<ConfigModeInput>,
    available_agents: Vec<AgentListEntry>,
}

impl ActorContext {
    /// Wait for the next user input.
    async fn next_input(&mut self) -> Option<String> {
        match self.rx.next().await {
            Some(ConfigModeInput::UserInput(text)) => Some(text),
            None => None,
        }
    }

    /// Send a message to the user.
    fn send_message(&self, text: impl Into<String>) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::SendMessage(text.into()),
            ))
            .ok();
    }

    /// Signal that configuration is done (save and exit).
    fn done(&self) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::Done {
                    config: self.config.clone(),
                },
            ))
            .ok();
    }

    /// Signal that configuration was cancelled.
    fn cancelled(&self) {
        self.config_agent_tx
            .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                self.session_id.clone(),
                ConfigModeOutput::Cancelled,
            ))
            .ok();
    }
}

/// The main actor loop.
async fn run_actor(
    config: SymposiumUserConfig,
    session_id: SessionId,
    config_agent_tx: UnboundedSender<ConfigAgentMessage>,
    rx: mpsc::Receiver<ConfigModeInput>,
) -> Result<(), sacp::Error> {
    // Fetch available agents
    let available_agents = match registry::list_agents().await {
        Ok(agents) => agents,
        Err(e) => {
            config_agent_tx
                .unbounded_send(ConfigAgentMessage::ConfigModeOutput(
                    session_id.clone(),
                    ConfigModeOutput::SendMessage(format!(
                        "Warning: Failed to fetch registry: {}",
                        e
                    )),
                ))
                .ok();
            Vec::new()
        }
    };

    let mut ctx = ActorContext {
        config,
        session_id,
        config_agent_tx,
        rx,
        available_agents,
    };

    main_menu_loop(&mut ctx).await;

    Ok(())
}

/// Main menu loop.
async fn main_menu_loop(ctx: &mut ActorContext) {
    loop {
        show_main_menu(ctx);

        let Some(input) = ctx.next_input().await else {
            return;
        };

        if !handle_main_menu_input(ctx, &input).await {
            return;
        }
    }
}

/// Handle input in the main menu. Returns false if we should exit.
async fn handle_main_menu_input(ctx: &mut ActorContext, text: &str) -> bool {
    let text = text.trim();
    let text_upper = text.to_uppercase();

    // Exit commands
    if text_upper == "EXIT" || text_upper == "DONE" || text_upper == "QUIT" {
        ctx.done();
        return false;
    }

    // Cancel without saving
    if text_upper == "CANCEL" {
        ctx.cancelled();
        return false;
    }

    // Agent selection
    if text_upper == "A" || text_upper == "AGENT" {
        agent_selection_loop(ctx).await;
        return true;
    }

    // Toggle proxy by index
    if let Ok(index) = text.parse::<usize>() {
        if index < ctx.config.proxies.len() {
            ctx.config.proxies[index].enabled = !ctx.config.proxies[index].enabled;
            let proxy = &ctx.config.proxies[index];
            let status = if proxy.enabled { "enabled" } else { "disabled" };
            ctx.send_message(format!("Proxy `{}` is now {}.", proxy.name, status));
        } else {
            ctx.send_message(format!(
                "Invalid index. Please enter 0-{}.",
                ctx.config.proxies.len().saturating_sub(1)
            ));
        }
        return true;
    }

    // Move command: "move X to Y"
    static MOVE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^move\s+(\d+)\s+to\s+(\d+)$").unwrap());

    if let Some(caps) = MOVE_RE.captures(text) {
        let from: usize = caps[1].parse().unwrap();
        let to: usize = caps[2].parse().unwrap();

        if from < ctx.config.proxies.len() && to <= ctx.config.proxies.len() {
            let proxy = ctx.config.proxies.remove(from);
            let insert_at = if to > from { to - 1 } else { to };
            ctx.send_message(format!("Moved `{}` from {} to {}.", proxy.name, from, to));
            ctx.config
                .proxies
                .insert(insert_at.min(ctx.config.proxies.len()), proxy);
        } else {
            ctx.send_message("Invalid indices for move.");
        }
        return true;
    }

    // Unknown command
    ctx.send_message(format!("Unknown command: `{}`", text));
    true
}

/// Agent selection loop.
async fn agent_selection_loop(ctx: &mut ActorContext) {
    loop {
        show_agent_selection(ctx);

        let Some(input) = ctx.next_input().await else {
            return;
        };

        let text = input.trim();
        let text_upper = text.to_uppercase();

        // Back to main menu
        if text_upper == "BACK" || text_upper == "CANCEL" {
            return;
        }

        // Select by index
        if let Ok(index) = text.parse::<usize>() {
            if index < ctx.available_agents.len() {
                let agent = &ctx.available_agents[index];
                ctx.config.agent = agent.id.clone();
                ctx.send_message(format!("Agent set to `{}`.", agent.name));
                return;
            } else {
                ctx.send_message(format!(
                    "Invalid index. Please enter 0-{}.",
                    ctx.available_agents.len().saturating_sub(1)
                ));
            }
            continue;
        }

        ctx.send_message(format!(
            "Unknown input: `{}`. Enter a number or `back`.",
            text
        ));
    }
}

/// Show the main menu.
fn show_main_menu(ctx: &ActorContext) {
    let mut msg = String::new();
    msg.push_str("# Symposium Configuration\n\n");

    // Current agent
    msg.push_str("**Agent:** ");
    if ctx.config.agent.is_empty() {
        msg.push_str("(not configured)\n\n");
    } else {
        // Try to find the agent name
        let agent_name = ctx
            .available_agents
            .iter()
            .find(|a| a.id == ctx.config.agent)
            .map(|a| a.name.as_str())
            .unwrap_or(&ctx.config.agent);
        msg.push_str(&format!("`{}`\n\n", agent_name));
    }

    // Proxies
    msg.push_str("**Proxies:**\n");
    if ctx.config.proxies.is_empty() {
        msg.push_str("  (none configured)\n");
    } else {
        for (i, proxy) in ctx.config.proxies.iter().enumerate() {
            let status = if proxy.enabled { "✓" } else { "✗" };
            msg.push_str(&format!("  `{}` [{}] {}\n", i, status, proxy.name));
        }
    }
    msg.push('\n');

    // Commands
    msg.push_str("**Commands:**\n");
    msg.push_str("  `A` or `AGENT` - Select a different agent\n");
    msg.push_str("  `0`, `1`, ... - Toggle proxy enabled/disabled\n");
    msg.push_str("  `move X to Y` - Reorder proxies\n");
    msg.push_str("  `done` - Save and exit\n");
    msg.push_str("  `cancel` - Exit without saving\n");

    ctx.send_message(msg);
}

/// Show the agent selection menu.
fn show_agent_selection(ctx: &ActorContext) {
    let mut msg = String::new();
    msg.push_str("# Select Agent\n\n");

    if ctx.available_agents.is_empty() {
        msg.push_str("No agents available.\n\n");
    } else {
        for (i, agent) in ctx.available_agents.iter().enumerate() {
            msg.push_str(&format!("`{}` **{}**", i, agent.name));
            if let Some(desc) = &agent.description {
                msg.push_str(&format!(" - {}", desc));
            }
            msg.push('\n');
        }
        msg.push('\n');
    }

    msg.push_str("Enter a number to select, or `back` to return.\n");

    ctx.send_message(msg);
}
