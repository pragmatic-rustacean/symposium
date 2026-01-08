//! User configuration for Symposium.
//!
//! Reads configuration from `~/.symposium/config.jsonc`.

use anyhow::Result;
use sacp::schema::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, SessionId,
    SessionNotification, SessionUpdate, StopReason, TextContent,
};
use sacp::{AgentToClient, Component, JrConnectionCx, JrRequestCx};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// User configuration for Symposium.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SymposiumUserConfig {
    /// Downstream agent command (shell words, e.g., "npx -y @anthropic-ai/claude-code-acp")
    pub agent: String,

    /// Proxy extensions to enable
    pub proxies: Vec<ProxyEntry>,
}

/// A proxy extension entry in the configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyEntry {
    /// Proxy name (e.g., "sparkle", "ferris", "cargo")
    pub name: String,

    /// Whether this proxy is enabled
    pub enabled: bool,
}

impl SymposiumUserConfig {
    /// Get the config directory path: ~/.symposium/
    pub fn dir() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        Ok(home.join(".symposium"))
    }

    /// Get the config file path: ~/.symposium/config.jsonc
    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.jsonc"))
    }

    /// Load config from the default path, returning None if it doesn't exist.
    pub fn load() -> Result<Option<Self>> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = serde_jsonc::from_str(&content)?;
        Ok(Some(config))
    }

    /// Save config to the default path.
    pub fn save(&self) -> Result<()> {
        let dir = Self::dir()?;
        std::fs::create_dir_all(&dir)?;

        let path = Self::path()?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Get the list of enabled proxy names.
    pub fn enabled_proxies(&self) -> Vec<String> {
        self.proxies
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.name.clone())
            .collect()
    }

    /// Parse the agent string into command arguments (shell words).
    pub fn agent_args(&self) -> Result<Vec<String>> {
        shell_words::split(&self.agent)
            .map_err(|e| anyhow::anyhow!("Failed to parse agent command: {}", e))
    }

    /// Create a default config with all proxies enabled.
    pub fn with_agent(agent: impl Into<String>) -> Self {
        Self {
            agent: agent.into(),
            proxies: vec![
                ProxyEntry {
                    name: "sparkle".to_string(),
                    enabled: true,
                },
                ProxyEntry {
                    name: "ferris".to_string(),
                    enabled: true,
                },
                ProxyEntry {
                    name: "cargo".to_string(),
                    enabled: true,
                },
            ],
        }
    }
}

/// Known downstream agents that can be configured.
pub struct KnownAgent {
    pub name: &'static str,
    pub command: &'static str,
}

/// List of known agents for the configuration wizard.
pub const KNOWN_AGENTS: &[KnownAgent] = &[
    KnownAgent {
        name: "Claude Code",
        command: "npx -y @zed-industries/claude-code-acp",
    },
    KnownAgent {
        name: "Gemini CLI",
        command: "npx -y -- @google/gemini-cli@latest --experimental-acp",
    },
    KnownAgent {
        name: "Codex",
        command: "npx -y @zed-industries/codex-acp",
    },
    KnownAgent {
        name: "Kiro CLI",
        command: "kiro-cli-chat acp",
    },
];

// ============================================================================
// Configuration Agent
// ============================================================================

/// State for a configuration session.
#[derive(Debug, Clone)]
enum ConfigState {
    /// Waiting for agent selection (1-N)
    SelectAgent,
    /// Configuration complete, waiting for restart
    Done,
}

/// Session data for the configuration agent.
#[derive(Clone)]
struct ConfigSessionData {
    state: ConfigState,
}

/// A simple agent that walks users through initial Symposium configuration.
///
/// This agent presents numbered options and expects the user to type a number.
/// It creates `~/.symposium/config.jsonc` with their choices.
#[derive(Clone)]
pub struct ConfigurationAgent {
    sessions: Arc<Mutex<HashMap<SessionId, ConfigSessionData>>>,
}

impl ConfigurationAgent {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn create_session(&self, session_id: &SessionId) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(
            session_id.clone(),
            ConfigSessionData {
                state: ConfigState::SelectAgent,
            },
        );
    }

    fn get_state(&self, session_id: &SessionId) -> Option<ConfigState> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(session_id).map(|s| s.state.clone())
    }

    fn set_state(&self, session_id: &SessionId, state: ConfigState) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(session_id) {
            session.state = state;
        }
    }

    /// Generate the welcome message with agent options.
    fn welcome_message() -> String {
        let mut msg = String::from(
            "Welcome to Symposium!\n\n\
             No configuration found. Let's set up your AI agent.\n\n\
             Which agent would you like to use?\n\n",
        );

        for (i, agent) in KNOWN_AGENTS.iter().enumerate() {
            msg.push_str(&format!("  {}. {}\n", i + 1, agent.name));
        }

        msg.push_str("\nType a number (1-");
        msg.push_str(&KNOWN_AGENTS.len().to_string());
        msg.push_str(") to select:");

        msg
    }

    /// Generate invalid input message.
    fn invalid_input_message() -> String {
        let mut msg = String::from("Invalid selection. Please type a number from 1 to ");
        msg.push_str(&KNOWN_AGENTS.len().to_string());
        msg.push_str(".\n\n");

        for (i, agent) in KNOWN_AGENTS.iter().enumerate() {
            msg.push_str(&format!("  {}. {}\n", i + 1, agent.name));
        }

        msg
    }

    /// Generate success message.
    fn success_message(agent_name: &str) -> String {
        format!(
            "Configuration saved!\n\n\
             Agent: {}\n\
             Proxies: sparkle, ferris, cargo (all enabled)\n\n\
             Please restart your editor to start using Symposium with {}.",
            agent_name, agent_name
        )
    }

    /// Process user input and return response.
    fn process_input(&self, session_id: &SessionId, input: &str) -> String {
        let state = match self.get_state(session_id) {
            Some(s) => s,
            None => return "Session not found. Please restart.".to_string(),
        };

        match state {
            ConfigState::SelectAgent => {
                // Parse input as number
                let trimmed = input.trim();
                if let Ok(num) = trimmed.parse::<usize>() {
                    if num >= 1 && num <= KNOWN_AGENTS.len() {
                        let agent = &KNOWN_AGENTS[num - 1];

                        // Save configuration
                        let config = SymposiumUserConfig::with_agent(agent.command);
                        if let Err(e) = config.save() {
                            return format!("Error saving configuration: {}", e);
                        }

                        self.set_state(session_id, ConfigState::Done);
                        return Self::success_message(agent.name);
                    }
                }

                // Invalid input
                Self::invalid_input_message()
            }
            ConfigState::Done => {
                "Configuration is complete. Please restart your editor to use Symposium."
                    .to_string()
            }
        }
    }

    async fn handle_new_session(
        &self,
        _request: NewSessionRequest,
        request_cx: JrRequestCx<NewSessionResponse>,
        cx: JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        let session_id = SessionId::new(uuid::Uuid::new_v4().to_string());
        self.create_session(&session_id);

        // Send welcome message immediately
        cx.send_notification(SessionNotification::new(
            session_id.clone(),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(Self::welcome_message().into())),
        ))?;

        request_cx.respond(NewSessionResponse::new(session_id))
    }

    async fn handle_prompt(
        &self,
        request: PromptRequest,
        request_cx: JrRequestCx<PromptResponse>,
        cx: JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        let session_id = request.session_id.clone();

        // Extract text from prompt
        let input = request
            .prompt
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(TextContent { text, .. }) => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        // Process input and get response
        let response = self.process_input(&session_id, &input);

        // Send response
        cx.send_notification(SessionNotification::new(
            session_id,
            SessionUpdate::AgentMessageChunk(ContentChunk::new(response.into())),
        ))?;

        request_cx.respond(PromptResponse::new(StopReason::EndTurn))
    }
}

impl Default for ConfigurationAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Component<sacp::link::AgentToClient> for ConfigurationAgent {
    async fn serve(
        self,
        client: impl Component<sacp::link::ClientToAgent>,
    ) -> Result<(), sacp::Error> {
        AgentToClient::builder()
            .name("symposium-config")
            .on_receive_request(
                async |initialize: InitializeRequest, request_cx, _cx| {
                    request_cx.respond(
                        InitializeResponse::new(initialize.protocol_version)
                            .agent_capabilities(AgentCapabilities::new()),
                    )
                },
                sacp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = self.clone();
                    async move |request: NewSessionRequest, request_cx, cx| {
                        agent.handle_new_session(request, request_cx, cx).await
                    }
                },
                sacp::on_receive_request!(),
            )
            .on_receive_request(
                {
                    let agent = self.clone();
                    async move |request: PromptRequest, request_cx, cx| {
                        agent.handle_prompt(request, request_cx, cx).await
                    }
                },
                sacp::on_receive_request!(),
            )
            .connect_to(client)?
            .serve()
            .await
    }
}
