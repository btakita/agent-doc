//! # Module: agent::junie
//!
//! ## Spec
//! - Wraps the Junie CLI (via `junie-bridge.sh` or `junie` on PATH) as an `Agent` backend.
//! - Command resolution order: (1) `junie` on PATH via `which`, (2) `junie-bridge.sh` next to
//!   the agent-doc binary, (3) `~/bin/junie-bridge.sh`, (4) `~/.local/bin/junie-bridge.sh`,
//!   (5) fallback to literal `"junie"` (produces a clear error at runtime if absent).
//! - Command and base args may be overridden via `AgentConfig` (from `config.toml`).
//! - Session resumption: appends `--resume <id>` when `session_id` is provided.
//! - Session forking: appends `--continue --fork-session` when `fork = true` and no session ID.
//! - Model override: appends `--model <m>` when `model` is provided.
//! - Injects a Junie-specific `--append-system-prompt` that contextualises the agent within
//!   an interactive session document (respond to git diffs, blockquotes, `## User` blocks).
//! - Expects Junie's CLI to emit the same JSON schema as Claude: `{result, is_error, session_id}`.
//! - Does NOT implement `StreamingAgent`; streaming is Claude-only.
//!
//! ## Agentic Contracts
//! - `Agent::send` blocks until the child exits; all errors propagate via `anyhow::Result`.
//! - Returns `Err` (with install hint) when the junie command cannot be spawned.
//! - Returns `Err` if the process exits non-zero, `is_error` is true, or `result` is empty.
//! - `session_id` on the returned `AgentResponse` mirrors the JSON `session_id` field.
//!
//! ## Evals
//! - resolve_path_junie: `junie` on PATH → `command = "junie"`
//! - resolve_bridge_next_to_binary: `junie-bridge.sh` in binary dir → uses that path
//! - resolve_home_bin: `~/bin/junie-bridge.sh` exists → uses that path
//! - resolve_fallback: none of the above → `command = "junie"` (deferred runtime error)
//! - send_success: JSON `{result: "ok", session_id: "j1"}` stdout → `AgentResponse { text: "ok", session_id: Some("j1") }`
//! - send_is_error: JSON `{is_error: true, result: "err"}` → `Err("Junie returned an error: err")`
//! - send_spawn_fail: junie binary absent → `Err` containing install hint with config path

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

use super::{Agent, AgentResponse};

pub struct Junie {
    command: String,
    base_args: Vec<String>,
}

impl Junie {
    pub fn new(command: Option<String>, base_args: Option<Vec<String>>) -> Self {
        Self {
            command: command.unwrap_or_else(resolve_junie_bridge),
            base_args: base_args.unwrap_or_default(),
        }
    }
}

/// Find the junie-bridge.sh script. Checks:
/// 1. `junie` on PATH
/// 2. `junie-bridge.sh` next to the agent-doc binary
/// 3. Common install locations
fn resolve_junie_bridge() -> String {
    // Check if `junie` exists on PATH
    if Command::new("which")
        .arg("junie")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return "junie".to_string();
    }

    // Check next to the agent-doc binary
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent() {
            let bridge = dir.join("junie-bridge.sh");
            if bridge.exists() {
                return bridge.to_string_lossy().to_string();
            }
        }

    // Check common locations
    if let Ok(home) = std::env::var("HOME") {
        let candidates = [
            PathBuf::from(&home).join("bin/junie-bridge.sh"),
            PathBuf::from(&home).join(".local/bin/junie-bridge.sh"),
        ];
        for path in &candidates {
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
    }

    // Fallback — will produce a clear error message
    "junie".to_string()
}

impl Agent for Junie {
    fn send(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<AgentResponse> {
        let mut args = self.base_args.clone();

        if let Some(sid) = session_id {
            args.push("--resume".to_string());
            args.push(sid.to_string());
        } else if fork {
            args.push("--continue".to_string());
            args.push("--fork-session".to_string());
        }

        if let Some(m) = model {
            args.push("--model".to_string());
            args.push(m.to_string());
        }

        // Add Junie-specific system prompt instructions
        args.push("--append-system-prompt".to_string());
        args.push(
            "You are responding inside an interactive session document. \
             The user edits the document and submits git diffs to you. \
             Use the provided diffs to understand the changes and respond concisely in markdown. \
             Address inline annotations (blockquotes, comments) as well as new ## User blocks. \
             You are acting as the Junie agent within this document."
                .to_string(),
        );

        let output = Command::new(&self.command)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(prompt.as_bytes())?;
                }
                child.wait_with_output()
            })
            .with_context(|| {
                format!(
                    "failed to run junie command '{}'. Install junie-bridge.sh to your PATH \
                     or configure [agents.junie] command in ~/.config/agent-doc/config.toml",
                    self.command
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("junie command failed: {}", stderr);
        }

        let raw = String::from_utf8_lossy(&output.stdout);

        // Assuming Junie's CLI follows the same JSON output format as Claude's for compatibility
        let json: serde_json::Value = serde_json::from_str(&raw)?;

        let is_error = json
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let result = json
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if is_error {
            anyhow::bail!("Junie returned an error: {}", result);
        }
        if result.is_empty() {
            anyhow::bail!("Empty response from Junie");
        }

        let session_id = json
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(AgentResponse {
            text: result,
            session_id,
        })
    }
}
