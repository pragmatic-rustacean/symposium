//! MCP server implementation for Ferris tools.

use std::path::PathBuf;

use sacp::{ProxyToConductor, mcp_server::McpServer};

use crate::Ferris;

/// Build an MCP server with the configured Ferris tools.
pub fn build_server(
    config: Ferris,
    _cwd: PathBuf,
) -> McpServer<ProxyToConductor, impl sacp::JrResponder<ProxyToConductor>> {

}
