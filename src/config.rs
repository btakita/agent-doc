use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
