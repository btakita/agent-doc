use anyhow::Result;
use std::process::Command;

use super::{Agent, AgentResponse};

pub struct Claude {
    command: String,
    base_args: Vec<String>,
}

impl Claude {
    pub fn new(command: Option<String>, base_args: Option<Vec<String>>) -> Self {
        Self {
            command: command.unwrap_or_else(|| "claude".to_string()),
            base_args: base_args.unwrap_or_else(|| {
                vec![
                    "-p".to_string(),
                    "--output-format".to_string(),
                    "json".to_string(),
                    "--permission-mode".to_string(),
                    "acceptEdits".to_string(),
                ]
            }),
        }
    }
}

impl Agent for Claude {
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

        args.push("--append-system-prompt".to_string());
        args.push(
            "You are responding inside an interactive session document. \
             The user edits the document and submits diffs to you. \
             Respond concisely in markdown. Address inline annotations \
             (blockquotes, comments) as well as new ## User blocks."
                .to_string(),
        );

        let output = Command::new(&self.command)
            .args(&args)
            .env_remove("CLAUDECODE")
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
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("claude command failed: {}", stderr);
        }

        let raw = String::from_utf8_lossy(&output.stdout);
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
            anyhow::bail!("Claude returned an error: {}", result);
        }
        if result.is_empty() {
            anyhow::bail!("Empty response from Claude");
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
