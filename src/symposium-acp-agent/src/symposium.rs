//! Symposium proxy chain orchestration
//!
//! This module provides the core Symposium functionality - building and running
//! proxy chains that enrich agent capabilities.
//!
//! Two modes are supported:
//! - `Symposium`: Proxy mode - sits between editor and an existing agent
//! - `SymposiumAgent`: Agent mode - wraps a downstream agent

use sacp::link::{AgentToClient, ConductorToProxy, ProxyToConductor};
use sacp::{Component, DynComponent};
use sacp_conductor::{Conductor, McpBridgeMode, ProxiesAndAgent};
use std::path::PathBuf;

/// Shared configuration for Symposium proxy chains.
#[derive(Clone)]
pub struct SymposiumConfig {
    trace_dir: Option<PathBuf>,
}

impl SymposiumConfig {
    /// Create an empty config.
    pub fn new() -> Self {
        SymposiumConfig { trace_dir: None }
    }

    /// Set the trace directory.
    pub fn trace_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.trace_dir = Some(dir.into());
        self
    }

    /// Configure a conductor with tracing and other settings.
    fn configure_conductor<L: sacp_conductor::ConductorLink>(
        &self,
        conductor: Conductor<L>,
    ) -> Result<Conductor<L>, sacp::Error> {
        let Some(ref dir) = self.trace_dir else {
            return Ok(conductor);
        };

        std::fs::create_dir_all(dir).map_err(sacp::Error::into_internal_error)?;
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let trace_path = dir.join(format!("{}.jsons", timestamp));
        let conductor = conductor
            .trace_to_path(&trace_path)
            .map_err(sacp::Error::into_internal_error)?;
        tracing::info!("Tracing to {}", trace_path.display());

        Ok(conductor)
    }
}

/// Symposium in proxy mode - sits between an editor and an existing agent.
///
/// Use this when you want to add Symposium's capabilities to an existing
/// agent setup without Symposium managing the agent lifecycle.
pub struct Symposium {
    config: SymposiumConfig,
    proxies: Vec<DynComponent<ProxyToConductor>>,
}

impl Symposium {
    /// Create a new Symposium from configuration.
    pub fn new(config: SymposiumConfig, proxies: Vec<DynComponent<ProxyToConductor>>) -> Self {
        Symposium { config, proxies }
    }

    /// Pair the symposium proxy with an agent, producing a new composite agent
    pub fn with_agent(self, agent: impl Component<AgentToClient>) -> SymposiumAgent {
        let Symposium { config, proxies } = self;
        SymposiumAgent::new(config, proxies, agent)
    }
}

impl Component<ProxyToConductor> for Symposium {
    async fn serve(self, client: impl Component<ConductorToProxy>) -> Result<(), sacp::Error> {
        tracing::debug!("Symposium::serve starting (proxy mode)");
        let Self { config, proxies } = self;

        tracing::debug!("Creating conductor (proxy mode)");
        let conductor = Conductor::new_proxy("symposium", proxies, McpBridgeMode::default());

        let conductor = config.configure_conductor(conductor)?;

        tracing::debug!("Starting conductor.run()");
        conductor.run(client).await
    }
}

/// Symposium in agent mode - wraps a downstream agent.
///
/// Use this when Symposium should manage the agent lifecycle, e.g., when
/// building a standalone enriched agent binary.
pub struct SymposiumAgent {
    config: SymposiumConfig,
    proxies: Vec<DynComponent<ProxyToConductor>>,
    agent: DynComponent<AgentToClient>,
}

impl SymposiumAgent {
    fn new<C: Component<AgentToClient>>(
        config: SymposiumConfig,
        proxies: Vec<DynComponent<ProxyToConductor>>,
        agent: C,
    ) -> Self {
        SymposiumAgent {
            config,
            proxies,
            agent: DynComponent::new(agent),
        }
    }
}

impl Component<AgentToClient> for SymposiumAgent {
    async fn serve(
        self,
        client: impl Component<sacp::link::ClientToAgent>,
    ) -> Result<(), sacp::Error> {
        tracing::debug!("SymposiumAgent::serve starting (agent mode)");
        let Self {
            config,
            proxies,
            agent,
        } = self;

        tracing::debug!("Creating conductor (agent mode)");
        let conductor = Conductor::new_agent(
            "symposium",
            ProxiesAndAgent::new(agent).proxies(proxies),
            McpBridgeMode::default(),
        );

        let conductor = config.configure_conductor(conductor)?;

        tracing::debug!("Starting conductor.run()");
        conductor.run(client).await
    }
}
