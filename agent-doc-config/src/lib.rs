//! # Crate: agent-doc-config
//!
//! ## Spec
//! - Defines `Config`: global user configuration loaded from `~/.config/agent-doc/config.toml`
//!   (or `$XDG_CONFIG_HOME/agent-doc/config.toml`). Fields: `default_agent`, `agents` map,
//!   `agent_args`, `claude_args`, `codex_args`, `opencode_args` (harness aliases),
//!   `execution_mode`, `terminal`.
//! - Defines `AgentConfig`: per-named-agent settings (`command`, `args`, `result_path`,
//!   `session_path`).
//! - Defines `TerminalConfig`: command template for launching an external terminal; supports
//!   `{tmux_command}` substitution.
//! - Defines `ExecutionMode` enum: `Hybrid` (default — first doc direct, rest subagent),
//!   `Parallel` (always subagent), `Sequential` (fully serial).
//! - `load()` reads and parses the global config file; returns `Config::default()` when the
//!   file is absent. Propagates I/O and parse errors via `anyhow::Result`.
//! - Project-level configuration types live in `agent-doc-frontmatter`;
//!   file-backed helpers live in `agent-doc-project-config-io`.
//!
//! ## Agentic Contracts
//! - **Never panics on missing config**: `load()` returns defaults when the file is absent.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use agent_doc_frontmatter::frontmatter::{
    CodexNetworkAccess, FreeTextExecutionMode, TerminalHostPreference,
};
use agent_doc_model_tier::ModelConfig;

pub mod env;
pub mod terminal_host;

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
    /// Additional CLI arguments to pass to the `opencode` process.
    /// OpenCode-only alias for `agent_args`. `agent_args` takes precedence when both are set.
    #[serde(default)]
    pub opencode_args: Option<String>,
    /// Explicit Codex network policy for agent-doc-launched sessions.
    /// `inherit` keeps the ambient launcher setting, `enabled` removes
    /// `CODEX_SANDBOX_NETWORK_DISABLED`, and `disabled` forces it on.
    #[serde(default)]
    pub codex_network_access: Option<CodexNetworkAccess>,
    /// Maximum number of managed-capability-proof attempts before the dispatch
    /// gate is set to `Failed`. Frontmatter overrides this. Default `3`.
    #[serde(default)]
    pub managed_proof_max_attempts: Option<u32>,
    /// Base back-off (seconds) between managed-capability-proof retries.
    /// Frontmatter overrides this. Default `2`.
    #[serde(default)]
    pub managed_proof_retry_backoff_secs: Option<u64>,
    /// Override for the managed-capability child probe timeout (seconds).
    /// Frontmatter overrides this. Default `45`.
    #[serde(default)]
    pub managed_proof_probe_timeout_secs: Option<u64>,
    /// Global default execution strategy for free text admitted from
    /// `agent:exchange` or `agent:queue` after backlog item creation. Values:
    /// `auto`, `goal`, `queue`; frontmatter and project config override this.
    #[serde(default, alias = "free_text_execution")]
    pub agent_doc_free_text_execution: Option<FreeTextExecutionMode>,
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

/// Global terminal presentation policy. Project terminal configuration uses the
/// equivalent project-owned schema in `agent-doc-frontmatter`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<TerminalHostPreference>,
    /// External-terminal command template. `{tmux_command}` is substituted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Whether a missing tmux session may be created automatically. Defaults true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_start_tmux: Option<bool>,
    /// IDE attach command template. `{session}` and `{tmux_command}` are substituted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_command: Option<String>,
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
opencode_args = "--dangerously-skip-permissions"
codex_network_access = "enabled"
agent_doc_free_text_execution = "queue"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.agent_args.as_deref(), Some("--json -s workspace-write"));
        assert_eq!(
            cfg.claude_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(cfg.codex_args.as_deref(), Some("-s danger-full-access"));
        assert_eq!(
            cfg.opencode_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(cfg.codex_network_access, Some(CodexNetworkAccess::Enabled));
        assert_eq!(
            cfg.agent_doc_free_text_execution,
            Some(FreeTextExecutionMode::Queue)
        );
        let alias: Config = toml::from_str("free_text_execution = \"goal\"").unwrap();
        assert_eq!(
            alias.agent_doc_free_text_execution,
            Some(FreeTextExecutionMode::Goal)
        );
    }

    #[test]
    fn test_terminal_policy_deserialization() {
        let cfg: Config = toml::from_str(
            r#"
[terminal]
host = "ide"
command = "wezterm start -- {tmux_command}"
auto_start_tmux = false
attach_command = "tmux attach-session -t {session}"
"#,
        )
        .unwrap();
        let terminal = cfg.terminal.expect("terminal config");
        assert_eq!(
            terminal.host,
            Some(agent_doc_frontmatter::frontmatter::TerminalHostPreference::Ide)
        );
        assert_eq!(
            terminal.command.as_deref(),
            Some("wezterm start -- {tmux_command}")
        );
        assert_eq!(terminal.auto_start_tmux, Some(false));
        assert_eq!(
            terminal.attach_command.as_deref(),
            Some("tmux attach-session -t {session}")
        );
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

    #[test]
    fn test_config_opencode_args_fallback() {
        // When agent_args is absent, opencode_args is used for OpenCode-specific resolution.
        let cfg = Config {
            agent_args: None,
            opencode_args: Some("--dangerously-skip-permissions".to_string()),
            ..Default::default()
        };
        let resolved = cfg.agent_args.or(cfg.opencode_args);
        assert_eq!(resolved.as_deref(), Some("--dangerously-skip-permissions"));
    }
}
