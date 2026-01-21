//! User configuration types for Symposium.
//!
//! These types represent the user's configuration stored in `~/.symposium/config.jsonc`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User configuration for Symposium.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct SymposiumUserConfig {
    /// Downstream agent command (shell words, e.g., "npx -y @anthropic-ai/claude-code-acp")
    pub agent: String,

    /// Proxy extensions to enable
    pub proxies: Vec<ProxyEntry>,
}

/// A proxy extension entry in the configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ProxyEntry {
    /// Proxy name (e.g., "sparkle", "ferris", "cargo")
    pub name: String,

    /// Whether this proxy is enabled
    pub enabled: bool,
}

impl SymposiumUserConfig {
    /// Get the config directory path: ~/.symposium/
    pub fn dir() -> anyhow::Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        Ok(home.join(".symposium"))
    }

    /// Get the config file path: ~/.symposium/config.jsonc
    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(Self::dir()?.join("config.jsonc"))
    }

    /// Load config from the given path, or the default path if None.
    /// Returns None if the config file doesn't exist.
    pub fn load(path: Option<impl AsRef<std::path::Path>>) -> anyhow::Result<Option<Self>> {
        let path = match path {
            Some(p) => p.as_ref().to_path_buf(),
            None => Self::path()?,
        };
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = serde_jsonc::from_str(&content)?;
        Ok(Some(config))
    }

    /// Save config to the default path.
    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::path()?)
    }

    /// Save config to a specific path.
    pub fn save_to(&self, path: &PathBuf) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
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
    pub fn agent_args(&self) -> anyhow::Result<Vec<String>> {
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
