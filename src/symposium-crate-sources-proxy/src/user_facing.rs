//! User-facing MCP service that provides the rust_crate_query tool.
//!
//! This service spawns research sub-sessions to investigate Rust crate sources
//! and return synthesized findings to the agent.

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}

impl CrateQueryService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
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

        // TODO: Implementation steps:
        // 1. Create oneshot channel for response
        // 2. Send NewSessionRequest with sub-agent MCP server
        // 3. Register session_id in shared state
        // 4. Send PromptRequest with the research prompt
        // 5. Await response on channel
        // 6. Return response as tool result

        // Placeholder implementation
        let response = format!(
            "Research request received for crate '{}'. Implementation pending.",
            crate_name
        );

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
