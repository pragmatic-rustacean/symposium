//! Zed editor configuration

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// ACP agents available for Zed configuration
#[derive(Debug, Clone, Copy)]
pub enum ZedAgent {
    ClaudeCode,
    Codex,
    KiroCli,
    Gemini,
}

impl ZedAgent {
    /// All available agents
    pub const ALL: &[ZedAgent] = &[
        ZedAgent::ClaudeCode,
        ZedAgent::Codex,
        ZedAgent::KiroCli,
        ZedAgent::Gemini,
    ];

    /// Get the human-readable name for this agent
    fn display_name(&self) -> &str {
        match self {
            ZedAgent::ClaudeCode => "Claude Code",
            ZedAgent::Codex => "Codex",
            ZedAgent::KiroCli => "Kiro CLI",
            ZedAgent::Gemini => "Gemini",
        }
    }

    /// Get the agent server entry name in Zed config
    fn config_name(&self) -> String {
        format!("Symposium ({})", self.display_name())
    }

    /// Get the downstream agent command and args (what comes after `--`)
    fn downstream_args(&self) -> Vec<&str> {
        match self {
            ZedAgent::ClaudeCode => vec!["npx", "-y", "@zed-industries/claude-code-acp"],
            ZedAgent::Codex => vec!["npx", "-y", "@zed-industries/codex-acp"],
            ZedAgent::KiroCli => vec!["kiro-cli-chat", "acp"],
            ZedAgent::Gemini => vec![
                "npx",
                "-y",
                "--",
                "@google/gemini-cli@latest",
                "--experimental-acp",
            ],
        }
    }
}

/// Configure Zed with all supported agents
pub fn configure_zed(symposium_acp_agent_path: &Path, dry_run: bool) -> Result<()> {
    let zed_config_path = get_zed_config_path()?;

    if !zed_config_path.exists() {
        println!("⚠️  Zed settings.json not found, skipping Zed configuration");
        println!("   Expected path: {}", zed_config_path.display());
        return Ok(());
    }

    println!("🔧 Configuring Zed editor...");
    println!("   Config file: {}", zed_config_path.display());

    // Read existing configuration
    let contents =
        std::fs::read_to_string(&zed_config_path).context("Failed to read Zed settings.json")?;

    // Strip comments and parse JSON
    let mut config: Value = strip_comments_and_parse(&contents)?;

    // Ensure agent_servers map exists
    if !config.get("agent_servers").is_some() {
        config["agent_servers"] = json!({});
    }

    let agent_servers = config["agent_servers"]
        .as_object_mut()
        .context("agent_servers is not an object")?;

    // Add configuration for all agents
    for agent in ZedAgent::ALL {
        let config_name = agent.config_name();
        let agent_config = create_agent_config(symposium_acp_agent_path, agent);

        if dry_run {
            println!("   Would add configuration for: {}", config_name);
            println!(
                "   Config: {}",
                serde_json::to_string_pretty(&agent_config).unwrap()
            );
        } else {
            println!("   Adding configuration for: {}", config_name);
            agent_servers.insert(config_name, agent_config);
        }
    }

    if !dry_run {
        // Write back configuration
        let formatted =
            serde_json::to_string_pretty(&config).context("Failed to serialize config")?;

        std::fs::write(&zed_config_path, formatted).context("Failed to write Zed settings.json")?;

        println!(
            "✅ Zed configuration updated with {} agent(s)",
            ZedAgent::ALL.len()
        );
    }

    Ok(())
}

/// Create an agent server configuration entry
fn create_agent_config(symposium_acp_agent_path: &Path, agent: &ZedAgent) -> Value {
    // Build args: act-as-agent --proxy defaults -- <downstream args>
    let mut args: Vec<&str> = vec!["act-as-agent", "--proxy", "defaults", "--"];
    args.extend(agent.downstream_args());

    json!({
        "type": "custom",
        "command": symposium_acp_agent_path.to_string_lossy(),
        "args": args,
        "env": {}
    })
}

/// Get the path to Zed settings.json
fn get_zed_config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config/zed/settings.json"))
}

/// Strip JSON comments and parse
/// Zed's settings.json uses JSON with comments, but serde_json doesn't support them
fn strip_comments_and_parse(contents: &str) -> Result<Value> {
    let mut stripped = String::new();

    for line in contents.lines() {
        // Remove full-line comments
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }

        // Remove inline comments (simple approach - doesn't handle strings with //)
        if let Some(comment_pos) = line.find("//") {
            stripped.push_str(&line[..comment_pos]);
        } else {
            stripped.push_str(line);
        }
        stripped.push('\n');
    }

    serde_json::from_str(&stripped).context("Failed to parse Zed settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_comments() {
        let input = r#"{
  // This is a comment
  "key": "value", // inline comment
  "nested": {
    "field": 123
  }
}"#;

        let result = strip_comments_and_parse(input);
        assert!(result.is_ok());

        let json = result.unwrap();
        assert_eq!(json["key"], "value");
        assert_eq!(json["nested"]["field"], 123);
    }
}
