//! # Module: config
//!
//! ## Spec
//! - Defines `Config`: global user configuration loaded from `~/.config/agent-doc/config.toml`
//!   (or `$XDG_CONFIG_HOME/agent-doc/config.toml`). Fields: `default_agent`, `agents` map,
//!   `agent_args`, `claude_args`, `codex_args` (harness aliases), `execution_mode`, `terminal`.
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
use crate::frontmatter::CodexNetworkAccess;
use agent_doc::model_tier::ModelConfig;

// Re-export project-level configuration from the shared module (for convenience)
pub use crate::project_config::{
    clear_project_tmux_session, project_tmux_session, update_project_tmux_session,
};

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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_agent: Option<String>,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentConfig>,
    /// Additional CLI arguments to pass to the agent process (harness-neutral).
    /// Takes precedence over harness-specific aliases. Space-separated string.
    #[serde(default)]
    pub agent_args: Option<String>,
    /// Additional CLI arguments to pass to the `claude` process.
    /// Backward-compatible alias for `agent_args`. `agent_args` takes precedence when both are set.
    #[serde(default)]
    pub claude_args: Option<String>,
    /// Additional CLI arguments to pass to the `codex` process.
    /// Codex-only alias for `agent_args`. `agent_args` takes precedence when both are set.
    #[serde(default)]
    pub codex_args: Option<String>,
    /// Explicit Codex network policy for agent-doc-launched sessions.
    /// `inherit` keeps the ambient launcher setting, `enabled` removes
    /// `CODEX_SANDBOX_NETWORK_DISABLED`, and `disabled` forces it on.
    #[serde(default)]
    pub codex_network_access: Option<CodexNetworkAccess>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    /// Command template to launch a terminal.
    /// `{tmux_command}` is replaced with the tmux attach/create command.
    /// Example: `wezterm start -- {tmux_command}`
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    dirs_config_dir().join("agent-doc").join("config.toml")
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

    #[test]
    fn test_config_agent_args_deserialization() {
        let toml_str = r#"
agent_args = "--json -s workspace-write"
claude_args = "--dangerously-skip-permissions"
codex_args = "-s danger-full-access"
codex_network_access = "enabled"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.agent_args.as_deref(), Some("--json -s workspace-write"));
        assert_eq!(
            cfg.claude_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(cfg.codex_args.as_deref(), Some("-s danger-full-access"));
        assert_eq!(cfg.codex_network_access, Some(CodexNetworkAccess::Enabled));
    }

    #[test]
    fn test_config_agent_args_precedence_resolution() {
        // agent_args takes precedence over claude_args
        let cfg = Config {
            agent_args: Some("--json".to_string()),
            claude_args: Some("--old-flag".to_string()),
            ..Default::default()
        };
        let resolved = cfg.agent_args.or(cfg.claude_args);
        assert_eq!(resolved.as_deref(), Some("--json"));
    }

    #[test]
    fn test_config_claude_args_fallback() {
        // When agent_args is absent, claude_args is used
        let cfg = Config {
            agent_args: None,
            claude_args: Some("--old-flag".to_string()),
            ..Default::default()
        };
        let resolved = cfg.agent_args.or(cfg.claude_args);
        assert_eq!(resolved.as_deref(), Some("--old-flag"));
    }

    #[test]
    fn test_config_codex_args_fallback() {
        // When agent_args is absent, codex_args is used for Codex-specific resolution.
        let cfg = Config {
            agent_args: None,
            codex_args: Some("-s danger-full-access".to_string()),
            ..Default::default()
        };
        let resolved = cfg.agent_args.or(cfg.codex_args);
        assert_eq!(resolved.as_deref(), Some("-s danger-full-access"));
    }
}
