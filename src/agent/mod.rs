//! # Module: agent
//!
//! ## Spec
//! - Defines the `Agent` trait: one method `send(prompt, session_id, fork, model)` → `AgentResponse`.
//! - `AgentResponse` carries the response text and an optional session ID for session resumption.
//! - `resolve(name, config)` maps a backend name (`"claude"`, `"codex"`, `"junie"`, or a config-defined name)
//!   to a boxed `Agent` implementation.
//! - When `config` is provided for an unknown name, falls back to the Claude backend using
//!   the configured command and args.
//! - Errors on unknown names with no config entry.
//!
//! ## Agentic Contracts
//! - `resolve` always returns a `Box<dyn Agent>` on success; callers need not know the concrete type.
//! - `Agent::send` is synchronous (no async); callers block until the full response is available.
//! - `session_id` threads conversation context across multiple `send` calls.
//! - `fork = true` continues the most recent session and branches it (only when `session_id` is `None`).
//!
//! ## Evals
//! - resolve_claude: `resolve("claude", None)` → returns Claude backend without error
//! - resolve_codex: `resolve("codex", None)` → returns Codex backend without error
//! - resolve_junie: `resolve("junie", None)` → returns Junie backend without error
//! - resolve_unknown_no_config: `resolve("other", None)` → `Err("Unknown agent backend: other")`
//! - resolve_unknown_with_config: `resolve("custom", Some(&cfg))` → returns Claude backend using cfg

pub mod claude;
pub mod codex;
pub mod junie;
pub mod streaming;

use anyhow::Result;

use crate::config::AgentConfig;

/// Response from an agent backend.
pub struct AgentResponse {
    pub text: String,
    pub session_id: Option<String>,
}

/// Agent backend trait — send a prompt, get a response.
pub trait Agent {
    fn send(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<AgentResponse>;
}

/// Resolve an agent backend by name. `env` is applied to the spawned child process
/// (currently only honored by the Claude backend).
pub fn resolve(
    name: &str,
    config: Option<&AgentConfig>,
    env: Vec<(String, Option<String>)>,
) -> Result<Box<dyn Agent>> {
    let (cmd, args) = match config {
        Some(ac) => (Some(ac.command.clone()), Some(ac.args.clone())),
        None => (None, None),
    };
    match name {
        "claude" => Ok(Box::new(claude::Claude::new(cmd, args).with_env(env))),
        "codex" => Ok(Box::new(codex::Codex::new(cmd, args).with_env(env))),
        "junie" => Ok(Box::new(junie::Junie::new(cmd, args))),
        other => {
            if config.is_some() {
                Ok(Box::new(claude::Claude::new(cmd, args).with_env(env)))
            } else {
                anyhow::bail!("Unknown agent backend: {}", other)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_claude() {
        let agent = resolve("claude", None, vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn resolve_codex() {
        let agent = resolve("codex", None, vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn resolve_junie() {
        let agent = resolve("junie", None, vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn resolve_unknown_no_config() {
        let agent = resolve("other", None, vec![]);
        assert!(agent.is_err());
        let err = agent.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("Unknown agent backend: other"));
    }

    #[test]
    fn resolve_unknown_with_config_falls_back() {
        let cfg = AgentConfig {
            command: "custom-binary".into(),
            args: vec!["--flag".into()],
            result_path: None,
            session_path: None,
        };
        let agent = resolve("custom", Some(&cfg), vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn multi_harness_resolve_all_backends_independently() {
        let claude = resolve("claude", None, vec![]);
        let codex = resolve("codex", None, vec![]);
        let junie = resolve("junie", None, vec![]);
        assert!(claude.is_ok());
        assert!(codex.is_ok());
        assert!(junie.is_ok());
    }

    #[test]
    fn multi_harness_resolve_with_env_isolation() {
        let claude_env = vec![("CLAUDECODE".into(), None)];
        let codex_env = vec![("CODEX_CLI".into(), None), ("CODEX".into(), None)];
        let claude = resolve("claude", None, claude_env);
        let codex = resolve("codex", None, codex_env);
        assert!(claude.is_ok());
        assert!(codex.is_ok());
    }
}
