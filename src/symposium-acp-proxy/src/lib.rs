//! Symposium ACP Proxy
//!
//! This crate provides the Symposium proxy functionality. It sits between an
//! editor and an agent, using sacp-conductor to orchestrate a dynamic chain
//! of component proxies that enrich the agent's capabilities.
//!
//! Architecture:
//! 1. Receive Initialize request from editor
//! 2. Examine capabilities to determine what components are needed
//! 3. Build proxy chain dynamically using conductor's lazy initialization
//! 4. Forward Initialize through the chain
//! 5. Bidirectionally forward all subsequent messages

use anyhow::Result;
use sacp::{Component, DynComponent};
use sacp_conductor::{Conductor, McpBridgeMode};
use std::path::PathBuf;

pub struct Symposium {
    crate_sources_proxy: bool,
    sparkle: bool,
    trace_dir: Option<PathBuf>,
    agent: Option<DynComponent>,
}

impl Symposium {
    pub fn new() -> Self {
        Symposium {
            sparkle: true,
            crate_sources_proxy: true,
            trace_dir: None,
            agent: None,
        }
    }

    pub fn sparkle(mut self, enable: bool) -> Self {
        self.sparkle = enable;
        self
    }

    pub fn crate_sources_proxy(mut self, enable: bool) -> Self {
        self.crate_sources_proxy = enable;
        self
    }

    pub fn agent<C: Component>(mut self, agent: C) -> Self {
        self.agent = Some(DynComponent::new(agent));
        self
    }

    /// Enable trace logging to a directory.
    /// Traces will be written as `<timestamp>.jsons` files.
    pub fn trace_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.trace_dir = Some(dir.into());
        self
    }
}

impl sacp::Component for Symposium {
    async fn serve(self, client: impl Component) -> Result<(), sacp::Error> {
        tracing::debug!("Symposium::serve starting");
        let Self {
            crate_sources_proxy,
            sparkle,
            trace_dir,
            agent,
        } = self;

        tracing::debug!("Creating conductor");
        let mut conductor = Conductor::new(
            "symposium".to_string(),
            move |init_req| async move {
                tracing::info!("Building proxy chain based on capabilities");

                // TODO: Examine init_req.capabilities to determine what's needed

                let mut components = vec![];

                if crate_sources_proxy {
                    components.push(sacp::DynComponent::new(
                        symposium_crate_sources_proxy::CrateSourcesProxy {},
                    ));
                }

                if sparkle {
                    components.push(sacp::DynComponent::new(sparkle::SparkleComponent::new()));
                }

                if let Some(agent) = agent {
                    components.push(agent);
                }

                // TODO: Add more components based on capabilities
                // - Check for IDE operation capabilities
                // - Spawn ide-ops adapter if missing
                // - Spawn ide-ops component to provide MCP tools

                Ok((init_req, components))
            },
            McpBridgeMode::default(),
        );

        // Enable tracing if a directory was specified
        if let Some(dir) = trace_dir {
            std::fs::create_dir_all(&dir).map_err(sacp::Error::into_internal_error)?;
            let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            let trace_path = dir.join(format!("{}.jsons", timestamp));
            conductor = conductor
                .trace_to_path(&trace_path)
                .map_err(sacp::Error::into_internal_error)?;
            tracing::info!("Tracing to {}", trace_path.display());
        }

        tracing::debug!("Starting conductor.run()");
        conductor.run(client).await
    }
}
