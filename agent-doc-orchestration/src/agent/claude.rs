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
//! - Stderr is drained in a background thread. On non-zero exit, the iterator yields a final
//!   `Err` with the exit status and stderr content. On zero exit with non-empty stderr, the
//!   content is logged to the parent's stderr with an `[agent]` prefix.
//! - Final chunk (`is_final = true`) carries the complete response text and `session_id`.
//!
//! ## Evals
//! - send_success: valid JSON `{result: "ok", session_id: "abc"}` stdout → `AgentResponse { text: "ok", session_id: Some("abc") }`
//! - send_is_error: JSON `{is_error: true, result: "boom"}` → `Err("Claude returned an error: boom")`
//! - send_empty_result: JSON `{result: ""}` → `Err("Empty response from Claude")`
//! - send_nonzero_exit: child exits 1 with stderr "fail" → `Err("claude command failed: fail")`
//! - streaming_chunks: stream-json lines emitted incrementally → iterator yields partial then final chunk
//! - streaming_session_id: final `{type:"result", session_id:"xyz"}` line → last chunk has `session_id = Some("xyz")`
//! - streaming_stderr_on_failure: subprocess exits non-zero with stderr "fail" → iterator yields `Err("claude subprocess exited with ...: fail")`
//! - streaming_stderr_on_success: subprocess exits zero with stderr warnings → logged to parent stderr, no error

use anyhow::Result;
use std::io::BufRead;
use std::process::Command;
use std::sync::{Arc, Mutex};

use super::streaming::StreamingAgent;
use super::{Agent, AgentResponse};
use agent_doc_turn_executor::agent_stream::{StreamChunk, parse_stream_line};
use agent_doc_turn_executor::claude_launch::{claude_json_args, claude_streaming_args};

pub struct Claude {
    command: String,
    base_args: Vec<String>,
    env: Vec<(String, Option<String>)>,
}

pub fn default_base_args() -> Vec<String> {
    vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--permission-mode".to_string(),
        "acceptEdits".to_string(),
    ]
}

/// Structural minimum args required for non-interactive JSON communication.
/// Permission settings are intentionally excluded — callers supply those
/// from frontmatter or config.
pub fn structural_base_args() -> Vec<String> {
    vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
    ]
}

impl Claude {
    pub fn new(command: Option<String>, base_args: Option<Vec<String>>) -> Self {
        Self {
            command: command.unwrap_or_else(|| "claude".to_string()),
            base_args: base_args.unwrap_or_else(default_base_args),
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
        let args = claude_json_args(&self.base_args, session_id, fork, model);

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
        {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(prompt.as_bytes())?;
            }
            child.stdin.take();
        }
        let output = super::wait_with_output_timeout(child, super::run_agent_timeout())?;

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
        let args = claude_streaming_args(&self.base_args, session_id, fork, model);

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
            if let Some(ref mut stdin) = child.stdin
                && let Err(err) = stdin.write_all(prompt.as_bytes())
                && err.kind() != std::io::ErrorKind::BrokenPipe
            {
                return Err(err.into());
            }
            child.stdin.take(); // Close stdin
        }

        // Read stdout line by line via BufReader (blocking)
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdout"))?;
        let reader = std::io::BufReader::new(stdout);

        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let stderr_handle = if let Some(stderr) = child.stderr.take() {
            let buf = Arc::clone(&stderr_buf);
            Some(std::thread::spawn(move || {
                use std::io::Read;
                let mut reader = std::io::BufReader::new(stderr);
                let mut content = String::new();
                let _ = reader.read_to_string(&mut content);
                if let Ok(mut guard) = buf.lock() {
                    *guard = content;
                }
            }))
        } else {
            None
        };

        Ok(Box::new(StreamIterator {
            lines: reader.lines(),
            child,
            stderr_handle,
            stderr_buf,
            done: false,
        }))
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn streaming_stderr_surfaced_on_nonzero_exit() {
        let claude = Claude::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "echo >&2 'permission denied'; exit 1".into(),
            ]),
        );
        let iter = claude.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("permission denied"), "got: {msg}");
        assert!(msg.contains("claude subprocess exited with"), "got: {msg}");
    }

    #[test]
    fn streaming_stderr_logged_on_zero_exit() {
        let claude = Claude::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                r#"echo '{"type":"result","result":"ok"}'; echo >&2 'deprecation warning'"#.into(),
            ]),
        );
        let iter = claude.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<Result<StreamChunk>> = iter.collect();
        assert!(
            chunks.iter().all(|c| c.is_ok()),
            "expected no errors, got: {chunks:?}"
        );
        let final_chunk = chunks.iter().find(|c| {
            c.as_ref()
                .map(|sc| sc.is_final && sc.text == "ok")
                .unwrap_or(false)
        });
        assert!(final_chunk.is_some(), "expected final chunk with text 'ok'");
    }

    #[test]
    fn streaming_stderr_empty_on_nonzero_exit() {
        let claude = Claude::new(
            Some("bash".into()),
            Some(vec!["-c".into(), "exit 42".into()]),
        );
        let iter = claude.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("claude subprocess exited with"), "got: {msg}");
    }
}

/// Iterator that reads stream-json lines from a child process.
/// Drains stderr in a background thread and surfaces it when the
/// subprocess exits non-zero.
struct StreamIterator {
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    child: std::process::Child,
    stderr_handle: Option<std::thread::JoinHandle<()>>,
    stderr_buf: Arc<Mutex<String>>,
    done: bool,
}

impl StreamIterator {
    fn collect_stderr(&mut self) -> String {
        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
        self.stderr_buf
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

impl Iterator for StreamIterator {
    type Item = Result<StreamChunk>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        loop {
            match self.lines.next() {
                Some(Ok(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    return Some(parse_stream_line(&line));
                }
                Some(Err(e)) => {
                    self.done = true;
                    return Some(Err(e.into()));
                }
                None => {
                    self.done = true;
                    let stderr = self.collect_stderr();
                    let exit_status = self.child.wait().ok();
                    if let Some(status) = exit_status
                        && !status.success()
                    {
                        let msg = if stderr.trim().is_empty() {
                            format!("claude subprocess exited with {status}")
                        } else {
                            format!("claude subprocess exited with {status}: {}", stderr.trim())
                        };
                        return Some(Err(anyhow::anyhow!(msg)));
                    }
                    if !stderr.trim().is_empty() {
                        eprintln!("[agent] claude subprocess stderr: {}", stderr.trim());
                    }
                    return None;
                }
            }
        }
    }
}
