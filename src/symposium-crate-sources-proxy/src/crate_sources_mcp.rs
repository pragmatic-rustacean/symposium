//! MCP service for research sub-agent sessions.
//!
//! Provides tools that research agents use to investigate Rust crate sources:
//! - `get_rust_crate_source`: Locates and extracts crate sources from crates.io
//! - `return_response_to_user`: Sends research findings back to complete the query
//!
//! This service is attached to NewSessionRequest when spawning research sessions.

use crate::eg;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the get_rust_crate_source tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetRustCrateSourceParams {
    /// Name of the crate to search
    pub crate_name: String,
    /// Optional semver range (e.g., "1.0", "^1.2", "~1.2.3")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Parameters for the return_response_to_user tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ReturnResponseParams {
    /// The research findings to return to the user
    pub response: String,
}

/// MCP service that provides tools for sub-agent research sessions
#[derive(Clone)]
pub struct SubAgentService {
    tool_router: ToolRouter<SubAgentService>,
}

impl SubAgentService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl SubAgentService {
    /// Get Rust crate source location
    #[tool(
        description = "Locate and extract Rust crate sources from crates.io. Returns the local path where the crate sources are available for reading."
    )]
    async fn get_rust_crate_source(
        &self,
        Parameters(GetRustCrateSourceParams {
            crate_name,
            version,
        }): Parameters<GetRustCrateSourceParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::debug!(
            "Getting Rust crate source for '{}' version: {:?}",
            crate_name,
            version,
        );

        let mut search = eg::Eg::rust_crate(&crate_name);

        // Use version resolver for semver range support and project detection
        if let Some(version_spec) = version {
            search = search.version(&version_spec);
        }

        let search_result = search.search().await.map_err(|e| {
            let error_msg = format!("Search failed: {}", e);
            McpError::internal_error(error_msg, None)
        })?;

        // Format the result
        let result = serde_json::json!({
            "crate_name": crate_name,
            "version": search_result.version,
            "checkout_path": search_result.checkout_path.display().to_string(),
            "message": format!(
                "Crate '{}' version {} extracted to {}",
                crate_name,
                search_result.version,
                search_result.checkout_path.display()
            ),
        });

        let content_text = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(content_text)]))
    }

    /// Return research findings to the waiting user
    #[tool(
        description = "Return your research findings to complete the crate query. This ends the research session and delivers your response to the agent that initiated the query."
    )]
    async fn return_response_to_user(
        &self,
        Parameters(ReturnResponseParams { response }): Parameters<ReturnResponseParams>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!("Research complete, returning response");
        tracing::debug!("Response: {}", response);

        // TODO: Implementation steps:
        // 1. Look up current session's response channel from shared state
        // 2. Send response through the channel
        // 3. Return success to indicate the tool completed

        // Placeholder implementation
        Ok(CallToolResult::success(vec![Content::text(
            "Response recorded. Implementation pending.".to_string(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for SubAgentService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rust-crate-research".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                title: None,
                website_url: None,
            },
            instructions: Some(
                "Provides tools for researching Rust crate sources: get_rust_crate_source to locate crates, return_response_to_user to deliver findings"
                    .to_string(),
            ),
        }
    }
}
