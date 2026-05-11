//! # Module: agent::opencode
//!
//! ## Spec
//! - Wraps the `opencode` CLI binary as a non-streaming `Agent` backend.
//! - Default invocation: `opencode run`.
//! - Session resumption: appends `--session <id>` when `session_id` is provided.
//! - Session forking: appends `--continue --fork` when `fork = true` and no session ID.
//! - Model override: appends `--model <m>`.
//! - Prompt delivery: passes the full agent-doc prompt as the `opencode run` message argument.
//! - `OPENCODE_CLIENT` is removed from the child process to prevent recursive harness detection.
//!
//! ## Agentic Contracts
//! - `Agent::send` blocks until the child exits; errors propagate via `anyhow::Result`.
//! - Returns `Err` if the process exits non-zero or stdout is empty.
//! - OpenCode's plain `run` output does not currently expose a stable session id in default
//!   output, so `AgentResponse.session_id` is `None`.

use anyhow::Result;
use std::process::Command;

use super::{Agent, AgentResponse, run_agent_timeout, wait_with_output_timeout};

pub struct OpenCode {
    command: String,
    base_args: Vec<String>,
    env: Vec<(String, Option<String>)>,
}

pub(crate) fn default_base_args() -> Vec<String> {
    vec!["run".to_string()]
}

pub(crate) fn build_args(
    base_args: &[String],
    prompt: &str,
    session_id: Option<&str>,
    fork: bool,
    model: Option<&str>,
) -> Vec<String> {
    let mut args = base_args.to_vec();
    if let Some(sid) = session_id {
        args.push("--session".to_string());
        args.push(sid.to_string());
    } else if fork {
        args.push("--continue".to_string());
        args.push("--fork".to_string());
    }

    if let Some(m) = model {
        args.push("--model".to_string());
        args.push(m.to_string());
    }

    args.push(prompt.to_string());
    args
}

impl OpenCode {
    pub fn new(command: Option<String>, base_args: Option<Vec<String>>) -> Self {
        Self {
            command: command.unwrap_or_else(|| "opencode".to_string()),
            base_args: base_args.unwrap_or_else(default_base_args),
            env: Vec::new(),
        }
    }

    /// Set environment variables to apply to the spawned `opencode` child process.
    /// Values must already be expanded. A `None` value means "unset this key".
    pub fn with_env(mut self, env: Vec<(String, Option<String>)>) -> Self {
        self.env = env;
        self
    }
}

impl Agent for OpenCode {
    fn send(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<AgentResponse> {
        let args = build_args(&self.base_args, prompt, session_id, fork, model);
        let mut cmd = Command::new(&self.command);
        cmd.args(&args).env_remove("OPENCODE_CLIENT");
        for (k, v) in &self.env {
            match v {
                Some(val) => {
                    cmd.env(k, val);
                }
                None => {
                    cmd.env_remove(k);
                }
            }
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let output = wait_with_output_timeout(cmd.spawn()?, run_agent_timeout())?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            anyhow::bail!(
                "opencode command failed (status {}): {}",
                output.status,
                stderr.trim()
            );
        }

        let text = stdout.trim().to_string();
        if text.is_empty() {
            anyhow::bail!("Empty response from OpenCode");
        }

        if !stderr.trim().is_empty() {
            eprintln!("[agent] opencode stderr: {}", stderr.trim());
        }

        Ok(AgentResponse {
            text,
            session_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_fresh_model() {
        let args = build_args(
            &default_base_args(),
            "hello",
            None,
            false,
            Some("zai/glm-5"),
        );
        assert_eq!(args, vec!["run", "--model", "zai/glm-5", "hello"]);
    }

    #[test]
    fn build_args_session_resume() {
        let args = build_args(&default_base_args(), "hello", Some("sess-1"), false, None);
        assert_eq!(args, vec!["run", "--session", "sess-1", "hello"]);
    }

    #[test]
    fn build_args_fork_last_session() {
        let args = build_args(&default_base_args(), "hello", None, true, None);
        assert_eq!(args, vec!["run", "--continue", "--fork", "hello"]);
    }
}
