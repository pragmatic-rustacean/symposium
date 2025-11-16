//! User-facing MCP service for researching Rust crates.
//!
//! Provides the `rust_crate_query` tool which allows agents to request research
//! about Rust crate source code by describing what information they need.
//! The service coordinates with research_agent to spawn sub-sessions that
//! investigate crate sources and return synthesized findings.

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

/// Request to start a research session for a Rust crate
#[derive(Debug)]
pub struct ResearchRequest {
    /// Name of the Rust crate to research
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    pub crate_version: Option<String>,
    /// Research prompt describing what information is needed
    pub prompt: String,
    /// Channel to send the research findings back
    pub response_tx: oneshot::Sender<String>,
}

/// Parameters for the rust_crate_query tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RustCrateQueryParams {
    /// Name of the Rust crate to research
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    /// Defaults to latest version if not specified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_version: Option<String>,
    /// Research prompt describing what information you need about the crate.
    /// Examples:
    /// - "How do I use the derive macro for custom field names?"
    /// - "What are the signatures of all methods on tokio::runtime::Runtime?"
    /// - "Show me an example of using async-trait with associated types"
    pub prompt: String,
}

/// MCP service that provides the rust_crate_query tool
#[derive(Clone)]
pub struct CrateQueryService {
    tool_router: ToolRouter<CrateQueryService>,
    /// Channel to send research requests to the background task
    research_tx: mpsc::Sender<ResearchRequest>,
}

impl CrateQueryService {
    pub fn new(research_tx: mpsc::Sender<ResearchRequest>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            research_tx,
        }
    }
}

#[tool_router]
impl CrateQueryService {
    /// Research a Rust crate by spawning a dedicated sub-agent to investigate the source code
    #[tool(
        description = "Research a Rust crate's source code. Provide the crate name and describe what you want to know. A specialized research agent will examine the crate sources and return findings."
    )]
    async fn rust_crate_query(
        &self,
        Parameters(RustCrateQueryParams {
            crate_name,
            crate_version,
            prompt,
        }): Parameters<RustCrateQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!(
            "Received crate query for '{}' version: {:?}",
            crate_name,
            crate_version
        );
        tracing::debug!("Research prompt: {}", prompt);

        // Create oneshot channel for the response
        let (response_tx, response_rx) = oneshot::channel();

        // Send research request to background task
        let request = ResearchRequest {
            crate_name: crate_name.clone(),
            crate_version,
            prompt,
            response_tx,
        };

        self.research_tx.send(request).await.map_err(|_| {
            McpError::internal_error("Failed to send research request to background task", None)
        })?;

        tracing::debug!("Research request sent, awaiting response");

        // Wait for the response from the research session
        let response = response_rx.await.map_err(|_| {
            McpError::internal_error("Research session closed without sending response", None)
        })?;

        tracing::info!("Research complete for '{}'", crate_name);

        Ok(CallToolResult::success(vec![Content::text(response)]))
    }
}

#[tool_handler]
impl ServerHandler for CrateQueryService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rust-crate-query".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: Some(
                "Provides research capabilities for Rust crate source code via dedicated sub-agent sessions"
                    .to_string(),
            ),
        }
    }
}
