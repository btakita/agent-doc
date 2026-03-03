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
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bridge = dir.join("junie-bridge.sh");
            if bridge.exists() {
                return bridge.to_string_lossy().to_string();
            }
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
