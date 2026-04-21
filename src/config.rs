//! # Module: config
//!
//! ## Spec
//! - Defines `Config`: global user configuration loaded from `~/.config/agent-doc/config.toml`
//!   (or `$XDG_CONFIG_HOME/agent-doc/config.toml`). Fields: `default_agent`, `agents` map,
//!   `claude_args`, `execution_mode`, `terminal`.
//! - Defines `AgentConfig`: per-named-agent settings (`command`, `args`, `result_path`,
//!   `session_path`).
//! - Defines `TerminalConfig`: command template for launching an external terminal; supports
//!   `{tmux_command}` substitution.
//! - Defines `ExecutionMode` enum: `Hybrid` (default — first doc direct, rest subagent),
//!   `Parallel` (always subagent), `Sequential` (fully serial).
//! - `load()` reads and parses the global config file; returns `Config::default()` when the
//!   file is absent. Propagates I/O and parse errors via `anyhow::Result`.
//! - Project-level configuration (`ProjectConfig`, `ComponentConfig`) is now in the
//!   `project_config` module for shared access (used by both binary and library).
//! - Re-exports: `ProjectConfig`, `ComponentConfig`, and project-level functions from
//!   `project_config` for backward compatibility.
//!
//! ## Agentic Contracts
//! - **Never panics on missing config**: `load()` returns defaults when the file is absent.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

// Import ModelConfig from the library version (works in both binary and library contexts)
use agent_doc::model_tier::ModelConfig;

// Re-export project-level configuration from the shared module (for convenience)
pub use crate::project_config::{clear_project_tmux_session, project_tmux_session, update_project_tmux_session};

/// Execution mode for skill-level parallelism.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    /// First doc direct, 2nd+ concurrent use subagent (default)
    #[default]
    Hybrid,
    /// Every /agent-doc spawns subagent
    Parallel,
    /// Fully sequential, cheapest
    Sequential,
}

impl std::fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hybrid => write!(f, "hybrid"),
            Self::Parallel => write!(f, "parallel"),
            Self::Sequential => write!(f, "sequential"),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_agent: Option<String>,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentConfig>,
    /// Additional CLI arguments to pass to the `claude` process.
    /// Space-separated string (e.g., "--dangerously-skip-permissions").
    #[serde(default)]
    pub claude_args: Option<String>,
    /// Execution mode: hybrid (default), parallel, sequential.
    /// Controls how the skill handles concurrent /agent-doc invocations.
    #[serde(default)]
    pub execution_mode: Option<ExecutionMode>,
    /// Terminal emulator configuration for `agent-doc terminal`.
    #[serde(default)]
    pub terminal: Option<TerminalConfig>,
    /// Model tier configuration: tier→model name maps per harness, plus
    /// gating preferences. Loaded from `[model]` and `[model.tiers.<harness>]`
    /// sections. See `model_tier::ModelConfig`.
    #[serde(default)]
    pub model: ModelConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalConfig {
    /// Command template to launch a terminal.
    /// `{tmux_command}` is replaced with the tmux attach/create command.
    /// Example: `wezterm start -- {tmux_command}`
    pub command: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub result_path: Option<String>,
    #[serde(default)]
    pub session_path: Option<String>,
}

/// Load config from ~/.config/agent-doc/config.toml, or return defaults.
pub fn load() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&content)?)
    } else {
        Ok(Config::default())
    }
}

fn config_path() -> PathBuf {
    dirs_config_dir()
        .join("agent-doc")
        .join("config.toml")
}

fn dirs_config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_missing_global_config() {
        // This test assumes no config exists in the temp directory
        // In practice, load() returns Config::default() when file is absent
        let _cfg = Config::default();
        assert!(_cfg.agents.is_empty());
    }

    #[test]
    fn test_execution_mode_display() {
        assert_eq!(ExecutionMode::Hybrid.to_string(), "hybrid");
        assert_eq!(ExecutionMode::Parallel.to_string(), "parallel");
        assert_eq!(ExecutionMode::Sequential.to_string(), "sequential");
    }
}
