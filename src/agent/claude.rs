//! # Module: agent::claude
//!
//! ## Spec
//! - Wraps the `claude` CLI binary as an `Agent` and `StreamingAgent` backend.
//! - Default invocation: `claude -p --output-format json --permission-mode acceptEdits`.
//! - Session resumption: appends `--resume <id>` when `session_id` is provided.
//! - Session forking: appends `--continue --fork-session` when `fork = true` and no session ID.
//! - Model override: appends `--model <m>` when `model` is provided.
//! - Injects a fixed `--append-system-prompt` that contextualises the agent within an
//!   interactive session document (respond to diffs, blockquotes, and `## User` blocks).
//! - `CLAUDECODE` env var is removed from the child process to prevent recursive detection.
//! - Non-streaming: spawns child, writes prompt to stdin, waits for full output, parses JSON
//!   `{result, is_error, session_id}`.
//! - Streaming (`StreamingAgent`): uses `--output-format stream-json`, closes stdin, yields
//!   `StreamChunk` items via `StreamIterator` line-by-line until the process exits.
//!
//! ## Agentic Contracts
//! - `Agent::send` blocks until the child exits; errors propagate via `anyhow::Result`.
//! - Returns `Err` if the process exits non-zero, `is_error` is true, or `result` is empty.
//! - `session_id` on the returned `AgentResponse` is taken from the JSON `session_id` field.
//! - `StreamingAgent::send_streaming` returns an iterator immediately; the child runs concurrently
//!   and is held alive by `StreamIterator` until the iterator is exhausted or dropped.
//! - Final chunk (`is_final = true`) carries the complete response text and `session_id`.
//!
//! ## Evals
//! - send_success: valid JSON `{result: "ok", session_id: "abc"}` stdout → `AgentResponse { text: "ok", session_id: Some("abc") }`
//! - send_is_error: JSON `{is_error: true, result: "boom"}` → `Err("Claude returned an error: boom")`
//! - send_empty_result: JSON `{result: ""}` → `Err("Empty response from Claude")`
//! - send_nonzero_exit: child exits 1 with stderr "fail" → `Err("claude command failed: fail")`
//! - streaming_chunks: stream-json lines emitted incrementally → iterator yields partial then final chunk
//! - streaming_session_id: final `{type:"result", session_id:"xyz"}` line → last chunk has `session_id = Some("xyz")`

use anyhow::Result;
use std::io::BufRead;
use std::process::Command;

use super::streaming::{StreamChunk, StreamingAgent, parse_stream_line};
use super::{Agent, AgentResponse};

pub struct Claude {
    command: String,
    base_args: Vec<String>,
    env: Vec<(String, Option<String>)>,
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
            env: Vec::new(),
        }
    }

    /// Set environment variables to apply to the spawned `claude` child process.
    /// Values must already be expanded — pass the output of `env::expand_values()`.
    /// A `None` value means "unset this key in the child env" (translated to
    /// `Command::env_remove`).
    pub fn with_env(mut self, env: Vec<(String, Option<String>)>) -> Self {
        self.env = env;
        self
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
             Respond concisely in markdown. Classify prompt-bearing inline edits \
             as prompt targets vs content edits, and address new ## User blocks \
             as well as prompt-bearing changes inside prior responses."
                .to_string(),
        );

        let mut cmd = Command::new(&self.command);
        cmd.args(&args).env_remove("CLAUDECODE");
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
        let output = cmd
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

impl StreamingAgent for Claude {
    fn send_streaming(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamChunk>>>> {
        // Build streaming args: replace json output with stream-json
        let mut args: Vec<String> = vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--permission-mode".to_string(),
            "acceptEdits".to_string(),
        ];

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
             Respond concisely in markdown. Classify prompt-bearing inline edits \
             as prompt targets vs content edits, and address new ## User blocks \
             as well as prompt-bearing changes inside prior responses."
                .to_string(),
        );

        let mut cmd = Command::new(&self.command);
        cmd.args(&args).env_remove("CLAUDECODE");
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
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        // Write prompt to stdin and close it
        {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(prompt.as_bytes())?;
            }
            child.stdin.take(); // Close stdin
        }

        // Read stdout line by line via BufReader (blocking)
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdout"))?;
        let reader = std::io::BufReader::new(stdout);

        Ok(Box::new(StreamIterator {
            lines: reader.lines(),
            _child: child,
        }))
    }
}

/// Iterator that reads stream-json lines from a child process.
struct StreamIterator {
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    _child: std::process::Child,
}

impl Iterator for StreamIterator {
    type Item = Result<StreamChunk>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.lines.next()? {
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    return Some(parse_stream_line(&line));
                }
                Err(e) => return Some(Err(e.into())),
            }
        }
    }
}
