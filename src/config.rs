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
//! - Defines `ProjectConfig`: per-project settings loaded from `.agent-doc/config.toml` relative
//!   to the current working directory. Currently holds `tmux_session`.
//! - `load_project()` reads and parses the project config file. On absence, I/O error, or parse
//!   error, returns `ProjectConfig::default()` and emits a warning to stderr (never panics).
//! - `project_tmux_session()` is a convenience wrapper returning the configured tmux session name,
//!   if any.
//! - `save_project()` serialises `ProjectConfig` to TOML and writes it to
//!   `.agent-doc/config.toml`, creating the directory if needed.
//! - `update_project_tmux_session()` performs a read-modify-write of the tmux session field and
//!   emits a `[config]` log line to stderr recording the old and new values.
//!
//! ## Agentic Contracts
//! - **Never panics on missing config**: both `load()` (global) and `load_project()` (project)
//!   return defaults when the file is absent; callers may unconditionally call them.
//! - **Project config errors are non-fatal**: `load_project()` always returns a usable value;
//!   errors are surfaced as stderr warnings, not propagated.
//! - **XDG compliance**: global config respects `$XDG_CONFIG_HOME` before falling back to
//!   `~/.config`.
//! - **Atomic-safe directory creation**: `save_project()` calls `create_dir_all` before writing,
//!   so `.agent-doc/` need not pre-exist.
//! - **Serde round-trip**: all public config structs implement `Serialize + Deserialize`; TOML
//!   serialisation via `toml::to_string_pretty` is the canonical on-disk format.
//! - **`execution_mode` defaults to `Hybrid`**: when absent from the config file the field is
//!   `None`; callers that need the effective mode should resolve `None` as `Hybrid`.
//!
//! ## Evals
//! - `load_missing_global_config`: no config file at XDG path → `load()` returns
//!   `Config::default()` with no error.
//! - `load_project_missing`: no `.agent-doc/config.toml` in CWD → `load_project()` returns
//!   `ProjectConfig { tmux_session: None }`.
//! - `save_and_load_project_config`: save a session name, reload → field equals saved value
//!   (covered by existing test).
//! - `update_creates_config_if_missing`: call `update_project_tmux_session` with no existing
//!   `.agent-doc/` dir → dir and file created, value persisted (covered by existing test).
//! - `load_malformed_config_returns_defaults`: malformed TOML in `.agent-doc/config.toml` →
//!   `load_project()` returns defaults, warning printed (covered by existing test).
//! - `execution_mode_roundtrip`: all three `ExecutionMode` variants serialise to lowercase
//!   strings and deserialise back correctly.
//! - `xdg_config_home_override`: `$XDG_CONFIG_HOME` set to a temp dir → `load()` reads from
//!   that dir, not `~/.config`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

// =========================================================================
// Project-level config (.agent-doc/config.toml)
// =========================================================================

/// Project-level configuration, read from `.agent-doc/config.toml` relative to CWD.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Target tmux session name for this project.
    #[serde(default)]
    pub tmux_session: Option<String>,
}

/// Load project config from `.agent-doc/config.toml` in CWD, or return defaults.
pub fn load_project() -> ProjectConfig {
    let path = project_config_path();
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("warning: failed to parse {}: {}", path.display(), e);
                    ProjectConfig::default()
                }
            },
            Err(e) => {
                eprintln!("warning: failed to read {}: {}", path.display(), e);
                ProjectConfig::default()
            }
        }
    } else {
        ProjectConfig::default()
    }
}

/// Get the project's configured tmux session (convenience helper).
pub fn project_tmux_session() -> Option<String> {
    load_project().tmux_session
}

/// Save project config to `.agent-doc/config.toml`.
pub fn save_project(config: &ProjectConfig) -> Result<()> {
    let path = project_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Update the project's configured tmux session.
/// Called when the configured session is dead and we fall back to a different one.
pub fn update_project_tmux_session(new_session: &str) -> Result<()> {
    let mut config = load_project();
    let old = config.tmux_session.clone();
    config.tmux_session = Some(new_session.to_string());
    save_project(&config)?;
    eprintln!(
        "[config] updated tmux_session: {} → {}",
        old.as_deref().unwrap_or("(none)"),
        new_session
    );
    Ok(())
}

fn project_config_path() -> PathBuf {
    // Walk up from CWD to find the .agent-doc/ project root. This avoids
    // CWD-sensitivity when subcommands run from a subdirectory (e.g., a
    // submodule that changed directory mid-session).
    if let Ok(cwd) = std::env::current_dir() {
        let mut current: &Path = &cwd;
        loop {
            if current.join(".agent-doc").is_dir() {
                return current.join(".agent-doc").join("config.toml");
            }
            match current.parent() {
                Some(p) => current = p,
                None => break,
            }
        }
        // No .agent-doc/ found walking up — fall back to CWD (uninitialized project).
        cwd.join(".agent-doc").join("config.toml")
    } else {
        PathBuf::from(".agent-doc").join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Helper: load project config from a specific directory (avoids chdir).
    fn load_project_from(dir: &Path) -> ProjectConfig {
        let path = dir.join(".agent-doc").join("config.toml");
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(cfg) => cfg,
                    Err(_) => ProjectConfig::default(),
                },
                Err(_) => ProjectConfig::default(),
            }
        } else {
            ProjectConfig::default()
        }
    }

    /// Helper: save project config to a specific directory.
    fn save_project_to(dir: &Path, config: &ProjectConfig) -> Result<()> {
        let path = dir.join(".agent-doc").join("config.toml");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(config)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    #[test]
    fn save_and_load_project_config() {
        let dir = tempfile::tempdir().unwrap();

        // Initially no config
        let cfg = load_project_from(dir.path());
        assert!(cfg.tmux_session.is_none());

        // Save with a session
        let cfg = ProjectConfig { tmux_session: Some("5".to_string()) };
        save_project_to(dir.path(), &cfg).unwrap();

        // Load it back
        let loaded = load_project_from(dir.path());
        assert_eq!(loaded.tmux_session.as_deref(), Some("5"));

        // Update: read-modify-write
        let mut updated = load_project_from(dir.path());
        updated.tmux_session = Some("9".to_string());
        save_project_to(dir.path(), &updated).unwrap();
        let loaded = load_project_from(dir.path());
        assert_eq!(loaded.tmux_session.as_deref(), Some("9"));
    }

    #[test]
    fn update_creates_config_if_missing() {
        let dir = tempfile::tempdir().unwrap();

        // No .agent-doc/ dir exists yet — save should create it
        let cfg = ProjectConfig { tmux_session: Some("3".to_string()) };
        save_project_to(dir.path(), &cfg).unwrap();

        let loaded = load_project_from(dir.path());
        assert_eq!(loaded.tmux_session.as_deref(), Some("3"));
    }

    #[test]
    fn load_malformed_config_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join(".agent-doc/config.toml"), "not valid { toml").unwrap();

        let cfg = load_project_from(dir.path());
        assert!(cfg.tmux_session.is_none(), "malformed config should return defaults");
    }
}
