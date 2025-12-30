//! ACP Component implementation for Ferris
//!
//! This module provides the Component trait implementation that allows Ferris
//! to run as an ACP proxy, providing its tools through the conductor chain.

use std::path::PathBuf;

use anyhow::Result;
use sacp::ProxyToConductor;
use sacp::component::Component;
use sacp::link::ConductorToProxy;

use crate::Ferris;

/// Ferris ACP Component that provides Rust development tools via proxy.
///
/// This component wraps the Ferris MCP server and provides it to sessions
/// that pass through the conductor chain.
pub struct FerrisComponent {
    config: Ferris,
}

impl FerrisComponent {
    /// Create a new FerrisComponent with the given configuration.
    pub fn new(config: Ferris) -> Self {
        Self { config }
    }
}

impl Default for FerrisComponent {
    fn default() -> Self {
        Self::new(Ferris::default())
    }
}

impl Component<ProxyToConductor> for FerrisComponent {
    async fn serve(self, client: impl Component<ConductorToProxy>) -> Result<(), sacp::Error> {
        tracing::info!("Ferris ACP proxy starting");

        // Get the cwd for the MCP server - for now use current directory
        // In the future, this could be passed through session context
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        ProxyToConductor::builder()
            .name("ferris-proxy")
            .with_mcp_server(self.config.into_mcp_server(cwd))
            .serve(client)
            .await
    }
}
