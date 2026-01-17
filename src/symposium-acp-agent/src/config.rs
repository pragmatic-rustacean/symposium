//! Configuration Agent for first-time setup.
//!
//! This module contains the legacy ConfigurationAgent used for first-time setup.
//! It will eventually be replaced by the new ConfigAgent in the library.

use futures::future::{BoxFuture, Shared};
use futures::FutureExt;
use sacp::schema::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, SessionId,
    SessionNotification, SessionUpdate, StopReason, TextContent,
};
use sacp::{AgentToClient, Component, JrConnectionCx, JrRequestCx};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// Re-export the shared config types
pub use symposium_acp_agent::user_config::{ProxyEntry, SymposiumUserConfig};

/// An agent available for configuration.
#[derive(Debug, Clone)]
pub struct AvailableAgent {
    pub id: String,
    pub name: String,
    pub command: String,
}

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
    /// Shared future that resolves available agents (fetched from registry + built-ins)
    agents: Shared<BoxFuture<'static, Arc<Vec<AvailableAgent>>>>,
    /// Custom config path for testing. If None, uses the default ~/.symposium/config.jsonc
    config_path: Option<PathBuf>,
}

impl ConfigurationAgent {
    /// Create a new ConfigurationAgent with agents from the registry.
    pub async fn new() -> Self {
        let agents_future = async move { Arc::new(Self::fetch_agents().await) }
            .boxed()
            .shared();
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            agents: agents_future,
            config_path: None,
        }
    }

    /// Create with a pre-set list of agents (for testing).
    pub fn with_agents(agents: Vec<AvailableAgent>) -> Self {
        let agents_future = futures::future::ready(Arc::new(agents)).boxed().shared();
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            agents: agents_future,
            config_path: None,
        }
    }

    /// Set a custom config path (for testing).
    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    /// Fetch available agents from the registry.
    async fn fetch_agents() -> Vec<AvailableAgent> {
        use crate::registry;

        match registry::list_agents().await {
            Ok(agents) => {
                let mut result = Vec::new();
                for agent in agents {
                    // Resolve each agent to get its command
                    match registry::resolve_agent(&agent.id).await {
                        Ok(server) => {
                            let command = Self::server_to_command(&server);
                            result.push(AvailableAgent {
                                id: agent.id,
                                name: agent.name,
                                command,
                            });
                        }
                        Err(e) => {
                            tracing::warn!("Failed to resolve agent {}: {}", agent.id, e);
                        }
                    }
                }
                result
            }
            Err(e) => {
                tracing::warn!("Failed to fetch registry, using fallback agents: {}", e);
                let mut result = Vec::new();

                // Try built-ins even if registry fetch failed
                if let Ok(built_ins) = registry::built_in_agents() {
                    for agent in built_ins {
                        match registry::resolve_agent(&agent.id).await {
                            Ok(server) => {
                                let command = Self::server_to_command(&server);
                                result.push(AvailableAgent {
                                    id: agent.id,
                                    name: agent.name,
                                    command,
                                });
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "Failed to resolve built-in agent {}: {}",
                                    agent.id,
                                    err
                                );
                            }
                        }
                    }
                }

                result.extend(Self::fallback_agents());
                result
            }
        }
    }

    /// Convert an McpServer to a shell command string.
    fn server_to_command(server: &sacp::schema::McpServer) -> String {
        match server {
            sacp::schema::McpServer::Stdio(stdio) => {
                let mut parts = vec![stdio.command.to_string_lossy().to_string()];
                parts.extend(stdio.args.iter().cloned());
                // Add env vars as prefix
                let env_prefix: Vec<String> = stdio
                    .env
                    .iter()
                    .map(|e| format!("{}={}", e.name, e.value))
                    .collect();
                if env_prefix.is_empty() {
                    shell_words::join(&parts)
                } else {
                    format!("{} {}", env_prefix.join(" "), shell_words::join(&parts))
                }
            }
            _ => String::new(),
        }
    }

    /// Fallback agents if registry fetch fails.
    fn fallback_agents() -> Vec<AvailableAgent> {
        vec![AvailableAgent {
            id: "gemini".to_string(),
            name: "Gemini CLI".to_string(),
            command: "npx -y @google/gemini-cli@latest --experimental-acp".to_string(),
        }]
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
    fn welcome_message(agents: &[AvailableAgent]) -> String {
        let mut msg = String::from(
            "Welcome to Symposium!\n\n\
             No configuration found. Let's set up your AI agent.\n\n\
             Which agent would you like to use?\n\n",
        );

        for (i, agent) in agents.iter().enumerate() {
            msg.push_str(&format!("  {}. {}\n", i + 1, agent.name));
        }

        msg.push_str("\nType a number (1-");
        msg.push_str(&agents.len().to_string());
        msg.push_str(") to select:");

        msg
    }

    /// Generate invalid input message.
    fn invalid_input_message(agents: &[AvailableAgent]) -> String {
        let mut msg = String::from("Invalid selection. Please type a number from 1 to ");
        msg.push_str(&agents.len().to_string());
        msg.push_str(".\n\n");

        for (i, agent) in agents.iter().enumerate() {
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
    fn process_input(
        &self,
        session_id: &SessionId,
        input: &str,
        agents: &[AvailableAgent],
    ) -> String {
        let state = match self.get_state(session_id) {
            Some(s) => s,
            None => return "Session not found. Please restart.".to_string(),
        };

        match state {
            ConfigState::SelectAgent => {
                // Parse input as number
                let trimmed = input.trim();

                let selected = trimmed
                    .parse::<usize>()
                    .ok()
                    .and_then(|num| agents.get(num.saturating_sub(1)))
                    .map(|agent| (agent.name.clone(), agent.command.clone()));

                if let Some((agent_name, agent_command)) = selected {
                    // Save configuration
                    let config = SymposiumUserConfig::with_agent(&agent_command);
                    let save_result = match &self.config_path {
                        Some(path) => config.save_to(path),
                        None => config.save(),
                    };
                    if let Err(e) = save_result {
                        return format!("Error saving configuration: {}", e);
                    }

                    self.set_state(session_id, ConfigState::Done);
                    return Self::success_message(&agent_name);
                }

                // Invalid input
                Self::invalid_input_message(&agents)
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

        let loading_message = "Hello, welcome to Symposium! I am fetching the list of agents from the [ACP Agent Registry](https://github.com/agentclientprotocol/registry). Give me a second...\n\n";
        cx.send_notification(SessionNotification::new(
            session_id.clone(),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(loading_message.into())),
        ))?;

        // Respond immediately so the client isn't blocked while we fetch agents
        request_cx.respond(NewSessionResponse::new(session_id.clone()))?;

        // Load agents (registry + built-ins), then send options
        let agents = self.agents.clone().await;

        cx.send_notification(SessionNotification::new(
            session_id.clone(),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                Self::welcome_message(&agents).into(),
            )),
        ))?;

        Ok(())
    }

    async fn handle_prompt(
        &self,
        request: PromptRequest,
        request_cx: JrRequestCx<PromptResponse>,
        cx: JrConnectionCx<AgentToClient>,
    ) -> Result<(), sacp::Error> {
        let session_id = request.session_id.clone();

        let agents = self.agents.clone().await;

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
        let response = self.process_input(&session_id, &input, &agents);

        // Send response
        cx.send_notification(SessionNotification::new(
            session_id,
            SessionUpdate::AgentMessageChunk(ContentChunk::new(response.into())),
        ))?;

        request_cx.respond(PromptResponse::new(StopReason::EndTurn))
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

#[cfg(test)]
mod tests {
    use super::*;
    use expect_test::expect;
    use sacp::link::ClientToAgent;
    use sacp::on_receive_notification;
    use sacp::schema::ProtocolVersion;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tempfile::TempDir;

    /// Test agents for unit tests (no network access needed)
    fn test_agents() -> Vec<AvailableAgent> {
        vec![
            AvailableAgent {
                id: "claude-code".to_string(),
                name: "Claude Code".to_string(),
                command: "npx -y @zed-industries/claude-code-acp".to_string(),
            },
            AvailableAgent {
                id: "gemini".to_string(),
                name: "Gemini CLI".to_string(),
                command: "npx -y @google/gemini-cli@latest --experimental-acp".to_string(),
            },
            AvailableAgent {
                id: "codex".to_string(),
                name: "Codex".to_string(),
                command: "npx -y @zed-industries/codex-acp".to_string(),
            },
            AvailableAgent {
                id: "kiro".to_string(),
                name: "Kiro CLI".to_string(),
                command: "kiro-cli-chat acp".to_string(),
            },
        ]
    }

    /// Extract text from a ContentBlock.
    fn content_block_text(block: &ContentBlock) -> Option<String> {
        match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        }
    }

    /// Collected session notifications from the configuration agent.
    #[derive(Debug, Default)]
    struct CollectedMessages {
        chunks: Vec<String>,
    }

    impl CollectedMessages {
        fn text(&self) -> String {
            self.chunks.join("")
        }

        fn clear(&mut self) {
            self.chunks.clear();
        }
    }

    #[tokio::test]
    async fn test_configuration_agent_welcome_message() -> Result<(), sacp::Error> {
        let messages = Arc::new(Mutex::new(CollectedMessages::default()));

        let messages_clone = messages.clone();
        ClientToAgent::builder()
            .on_receive_notification(
                async move |n: SessionNotification, _| {
                    if let SessionUpdate::AgentMessageChunk(chunk) = n.update {
                        if let Some(text) = content_block_text(&chunk.content) {
                            messages_clone.lock().unwrap().chunks.push(text);
                        }
                    }
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(ConfigurationAgent::with_agents(test_agents()))?
            .run_until(async |cx| {
                // Initialize the agent
                let init_response = cx
                    .send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                    .block_task()
                    .await?;
                assert_eq!(init_response.protocol_version, ProtocolVersion::LATEST);

                // Create a new session - this should trigger welcome message
                let _session_response = cx
                    .send_request(NewSessionRequest::new("."))
                    .block_task()
                    .await?;

                // Give a moment for the notification to arrive
                tokio::time::sleep(Duration::from_millis(50)).await;

                let text = messages.lock().unwrap().text();
                expect![[r#"
                    Hello, welcome to Symposium! I am fetching the list of agents from the [ACP Agent Registry](https://github.com/agentclientprotocol/registry). Give me a second...

                    Welcome to Symposium!

                    No configuration found. Let's set up your AI agent.

                    Which agent would you like to use?

                      1. Claude Code
                      2. Gemini CLI
                      3. Codex
                      4. Kiro CLI

                    Type a number (1-4) to select:"#]]
                .assert_eq(&text);

                Ok(())
            })
            .await
    }

    #[tokio::test]
    async fn test_configuration_agent_select_agent() -> Result<(), sacp::Error> {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.jsonc");

        let messages = Arc::new(Mutex::new(CollectedMessages::default()));

        let messages_clone = messages.clone();
        ClientToAgent::builder()
            .on_receive_notification(
                async move |n: SessionNotification, _| {
                    if let SessionUpdate::AgentMessageChunk(chunk) = n.update {
                        if let Some(text) = content_block_text(&chunk.content) {
                            messages_clone.lock().unwrap().chunks.push(text);
                        }
                    }
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(
                ConfigurationAgent::with_agents(test_agents()).with_config_path(&config_path),
            )?
            .run_until(async |cx| {
                // Initialize
                cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                    .block_task()
                    .await?;

                // Create session
                let session_response = cx
                    .send_request(NewSessionRequest::new("."))
                    .block_task()
                    .await?;
                let session_id = session_response.session_id;

                // Clear welcome message
                tokio::time::sleep(Duration::from_millis(50)).await;
                messages.lock().unwrap().clear();

                // Select Claude Code (option 1)
                let prompt_response = cx
                    .send_request(PromptRequest::new(
                        session_id.clone(),
                        vec![ContentBlock::Text(TextContent::new("1".to_string()))],
                    ))
                    .block_task()
                    .await?;

                assert_eq!(prompt_response.stop_reason, StopReason::EndTurn);

                tokio::time::sleep(Duration::from_millis(50)).await;

                let text = messages.lock().unwrap().text();
                expect![[r#"
                    Configuration saved!

                    Agent: Claude Code
                    Proxies: sparkle, ferris, cargo (all enabled)

                    Please restart your editor to start using Symposium with Claude Code."#]]
                .assert_eq(&text);

                // Verify config file was created
                assert!(config_path.exists(), "Config file should exist");
                let content = std::fs::read_to_string(&config_path).unwrap();
                let saved_config: SymposiumUserConfig = serde_json::from_str(&content).unwrap();
                assert_eq!(saved_config.agent, "npx -y @zed-industries/claude-code-acp");
                assert_eq!(saved_config.proxies.len(), 3);

                Ok(())
            })
            .await
    }

    #[tokio::test]
    async fn test_configuration_agent_invalid_input() -> Result<(), sacp::Error> {
        let messages = Arc::new(Mutex::new(CollectedMessages::default()));

        let messages_clone = messages.clone();
        ClientToAgent::builder()
            .on_receive_notification(
                async move |n: SessionNotification, _| {
                    if let SessionUpdate::AgentMessageChunk(chunk) = n.update {
                        if let Some(text) = content_block_text(&chunk.content) {
                            messages_clone.lock().unwrap().chunks.push(text);
                        }
                    }
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(ConfigurationAgent::with_agents(test_agents()))?
            .run_until(async |cx| {
                // Initialize
                cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                    .block_task()
                    .await?;

                // Create session
                let session_response = cx
                    .send_request(NewSessionRequest::new("."))
                    .block_task()
                    .await?;
                let session_id = session_response.session_id;

                // Clear welcome message
                tokio::time::sleep(Duration::from_millis(50)).await;
                messages.lock().unwrap().clear();

                // Send invalid input
                cx.send_request(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("invalid".to_string()))],
                ))
                .block_task()
                .await?;

                tokio::time::sleep(Duration::from_millis(50)).await;

                let text = messages.lock().unwrap().text();
                assert!(text.contains("Invalid selection"));
                assert!(text.contains("1 to 4"));

                Ok(())
            })
            .await
    }

    #[tokio::test]
    async fn test_configuration_agent_done_state() -> Result<(), sacp::Error> {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.jsonc");

        let messages = Arc::new(Mutex::new(CollectedMessages::default()));

        let messages_clone = messages.clone();
        ClientToAgent::builder()
            .on_receive_notification(
                async move |n: SessionNotification, _| {
                    if let SessionUpdate::AgentMessageChunk(chunk) = n.update {
                        if let Some(text) = content_block_text(&chunk.content) {
                            messages_clone.lock().unwrap().chunks.push(text);
                        }
                    }
                    Ok(())
                },
                on_receive_notification!(),
            )
            .connect_to(
                ConfigurationAgent::with_agents(test_agents()).with_config_path(&config_path),
            )?
            .run_until(async |cx| {
                // Initialize
                cx.send_request(InitializeRequest::new(ProtocolVersion::LATEST))
                    .block_task()
                    .await?;

                // Create session
                let session_response = cx
                    .send_request(NewSessionRequest::new("."))
                    .block_task()
                    .await?;
                let session_id = session_response.session_id;

                tokio::time::sleep(Duration::from_millis(50)).await;
                messages.lock().unwrap().clear();

                // Select an agent
                cx.send_request(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new("2".to_string()))],
                ))
                .block_task()
                .await?;

                tokio::time::sleep(Duration::from_millis(50)).await;
                messages.lock().unwrap().clear();

                // Try to send another prompt after done
                cx.send_request(PromptRequest::new(
                    session_id.clone(),
                    vec![ContentBlock::Text(TextContent::new(
                        "something else".to_string(),
                    ))],
                ))
                .block_task()
                .await?;

                tokio::time::sleep(Duration::from_millis(50)).await;

                let text = messages.lock().unwrap().text();
                expect!["Configuration is complete. Please restart your editor to use Symposium."]
                    .assert_eq(&text);

                Ok(())
            })
            .await
    }
}
