//! # Module: agent::codex
//!
//! ## Spec
//! - Wraps the `codex` CLI binary as an `Agent` and `StreamingAgent` backend.
//! - Default invocation: `codex exec --json -s workspace-write`.
//! - Session resumption: uses `codex exec resume <id> --json` subcommand.
//! - Session fork: uses `codex exec resume --last --json` (no non-interactive fork in Codex).
//! - Model override: appends `-m <model>`.
//! - `CODEX_CLI` and `CODEX` env vars are removed from the child process to prevent recursive detection.
//! - Non-streaming: spawns child, writes prompt to stdin, collects JSONL output, extracts
//!   `thread_id` from `thread.started` and response text from `item.completed` (type=agent_message).
//! - Streaming (`StreamingAgent`): same JSONL, yields `StreamChunk` per line as events arrive.
//!
//! ## Codex JSONL Event Schema
//! ```jsonl
//! {"type":"thread.started","thread_id":"<uuid>"}
//! {"type":"turn.started"}
//! {"type":"item.started","item":{"id":"...","type":"command_execution",...}}
//! {"type":"item.completed","item":{"id":"...","type":"agent_message","text":"..."}}
//! {"type":"item.completed","item":{"id":"...","type":"command_execution","command":"...","aggregated_output":"...","exit_code":0,"status":"completed"}}
//! {"type":"turn.completed","usage":{"input_tokens":...,"output_tokens":...}}
//! ```
//!
//! ## Agentic Contracts
//! - `Agent::send` blocks until the child exits; errors propagate via `anyhow::Result`.
//! - Returns `Err` if the process exits non-zero or no agent_message text is found.
//! - `session_id` on the returned `AgentResponse` is taken from `thread.started.thread_id`.
//! - `StreamingAgent::send_streaming` returns an iterator immediately; JSONL lines are parsed
//!   as they arrive. `turn.completed` produces the final chunk.
//!
//! ## Evals
//! - parse_thread_started: extracts thread_id as session_id
//! - parse_agent_message: extracts text from item.completed agent_message
//! - parse_command_execution: command_execution items yield empty chunks
//! - parse_turn_completed: final chunk with is_final=true
//! - parse_turn_started: empty non-final chunk
//! - parse_unknown_event: unknown types yield empty chunks
//! - parse_malformed_json: invalid JSON produces Err
//! - send_builds_correct_args: verifies base command construction
//! - send_resume_args: verifies session resume command
//! - send_fork_args: verifies fork (resume --last) command

use anyhow::Result;
use std::io::BufRead;
use std::process::Command;

use super::streaming::{StreamChunk, StreamingAgent};
use super::{Agent, AgentResponse};

pub struct Codex {
    command: String,
    base_args: Vec<String>,
    env: Vec<(String, Option<String>)>,
}

impl Codex {
    pub fn new(command: Option<String>, base_args: Option<Vec<String>>) -> Self {
        Self {
            command: command.unwrap_or_else(|| "codex".to_string()),
            base_args: base_args.unwrap_or_else(|| {
                vec![
                    "exec".to_string(),
                    "--json".to_string(),
                    "-s".to_string(),
                    "workspace-write".to_string(),
                ]
            }),
            env: Vec::new(),
        }
    }

    pub fn with_env(mut self, env: Vec<(String, Option<String>)>) -> Self {
        self.env = env;
        self
    }

    fn build_command(
        &self,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Command {
        let mut cmd = Command::new(&self.command);

        if let Some(sid) = session_id {
            // codex exec resume <id> --json -s workspace-write
            cmd.arg("exec").arg("resume").arg(sid).arg("--json");
            // Add remaining base_args that aren't "exec" or "--json"
            for arg in &self.base_args {
                if arg != "exec" && arg != "--json" {
                    cmd.arg(arg);
                }
            }
        } else if fork {
            // codex exec resume --last --json -s workspace-write
            cmd.arg("exec")
                .arg("resume")
                .arg("--last")
                .arg("--json");
            for arg in &self.base_args {
                if arg != "exec" && arg != "--json" {
                    cmd.arg(arg);
                }
            }
        } else {
            cmd.args(&self.base_args);
        }

        if let Some(m) = model {
            cmd.arg("-m").arg(m);
        }

        cmd.env_remove("CODEX_CLI").env_remove("CODEX");
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

        cmd
    }
}

/// Parse a single JSONL line from Codex output into a StreamChunk.
pub fn parse_codex_line(line: &str) -> Result<StreamChunk> {
    let json: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| anyhow::anyhow!("failed to parse Codex JSONL: {}: {}", e, line))?;

    let event_type = json
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match event_type {
        "thread.started" => {
            let session_id = json
                .get("thread_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok(StreamChunk {
                text: String::new(),
                thinking: None,
                is_final: false,
                session_id,
            })
        }
        "item.completed" => {
            let item = json.get("item");
            let item_type = item
                .and_then(|i| i.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if item_type == "agent_message" {
                let text = item
                    .and_then(|i| i.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(StreamChunk {
                    text,
                    thinking: None,
                    is_final: false,
                    session_id: None,
                })
            } else {
                Ok(StreamChunk {
                    text: String::new(),
                    thinking: None,
                    is_final: false,
                    session_id: None,
                })
            }
        }
        "turn.completed" => Ok(StreamChunk {
            text: String::new(),
            thinking: None,
            is_final: true,
            session_id: None,
        }),
        _ => Ok(StreamChunk {
            text: String::new(),
            thinking: None,
            is_final: false,
            session_id: None,
        }),
    }
}

impl Agent for Codex {
    fn send(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<AgentResponse> {
        let mut cmd = self.build_command(session_id, fork, model);
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
            anyhow::bail!("codex command failed: {}", stderr);
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let mut thread_id: Option<String> = None;
        let mut response_text = String::new();

        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let json: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let event_type = json
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match event_type {
                "thread.started" => {
                    thread_id = json
                        .get("thread_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
                "item.completed" => {
                    let item = json.get("item");
                    let item_type = item
                        .and_then(|i| i.get("type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if item_type == "agent_message"
                        && let Some(text) = item.and_then(|i| i.get("text")).and_then(|v| v.as_str())
                    {
                        if !response_text.is_empty() {
                            response_text.push('\n');
                        }
                        response_text.push_str(text);
                    }
                }
                _ => {}
            }
        }

        if response_text.is_empty() {
            anyhow::bail!("Empty response from Codex");
        }

        Ok(AgentResponse {
            text: response_text,
            session_id: thread_id,
        })
    }
}

impl StreamingAgent for Codex {
    fn send_streaming(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamChunk>>>> {
        let mut cmd = self.build_command(session_id, fork, model);
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

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdout"))?;
        let reader = std::io::BufReader::new(stdout);

        Ok(Box::new(CodexStreamIterator {
            lines: reader.lines(),
            _child: child,
            session_id: None,
        }))
    }
}

struct CodexStreamIterator {
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    _child: std::process::Child,
    session_id: Option<String>,
}

impl Iterator for CodexStreamIterator {
    type Item = Result<StreamChunk>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.lines.next()? {
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match parse_codex_line(&line) {
                        Ok(mut chunk) => {
                            // Capture session_id from thread.started, propagate on final chunk
                            if chunk.session_id.is_some() && !chunk.is_final {
                                self.session_id = chunk.session_id.take();
                            }
                            if chunk.is_final {
                                chunk.session_id = self.session_id.take();
                            }
                            // Skip empty non-final chunks (turn.started, command_execution, etc.)
                            if !chunk.is_final && chunk.text.is_empty() && chunk.thinking.is_none()
                                && chunk.session_id.is_none()
                            {
                                continue;
                            }
                            return Some(Ok(chunk));
                        }
                        Err(e) => return Some(Err(e)),
                    }
                }
                Err(e) => return Some(Err(e.into())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_thread_started() {
        let line = r#"{"type":"thread.started","thread_id":"019db613-e57b-77d2-844c-9e7dca83ad01"}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
        assert_eq!(
            chunk.session_id.as_deref(),
            Some("019db613-e57b-77d2-844c-9e7dca83ad01")
        );
    }

    #[test]
    fn parse_agent_message() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hello world"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "hello world");
        assert!(!chunk.is_final);
        assert!(chunk.session_id.is_none());
    }

    #[test]
    fn parse_command_execution() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"foo\n","exit_code":0,"status":"completed"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }

    #[test]
    fn parse_turn_completed() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":100,"output_tokens":20}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_turn_started() {
        let line = r#"{"type":"turn.started"}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_item_started() {
        let line = r#"{"type":"item.started","item":{"id":"item_0","type":"command_execution","command":"ls","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_unknown_event() {
        let line = r#"{"type":"some.future.event","data":42}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert!(!chunk.is_final);
        assert_eq!(chunk.text, "");
    }

    #[test]
    fn parse_malformed_json() {
        let result = parse_codex_line("not json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_agent_message_missing_text() {
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message"}}"#;
        let chunk = parse_codex_line(line).unwrap();
        assert_eq!(chunk.text, "");
        assert!(!chunk.is_final);
    }

    #[test]
    fn stream_iterator_propagates_session_id_to_final() {
        // Simulate the iterator behavior: session_id from thread.started
        // should appear on the final (turn.completed) chunk
        let lines = vec![
            r#"{"type":"thread.started","thread_id":"abc-123"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"hi"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}"#,
        ];

        // Parse individually and verify the propagation logic
        let mut session_id: Option<String> = None;
        let mut chunks: Vec<StreamChunk> = Vec::new();

        for line in &lines {
            let mut chunk = parse_codex_line(line).unwrap();
            if chunk.session_id.is_some() && !chunk.is_final {
                session_id = chunk.session_id.take();
            }
            if chunk.is_final {
                chunk.session_id = session_id.take();
            }
            // Filter same as iterator: skip empty non-final
            if !chunk.is_final
                && chunk.text.is_empty()
                && chunk.thinking.is_none()
                && chunk.session_id.is_none()
            {
                continue;
            }
            chunks.push(chunk);
        }

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "hi");
        assert!(!chunks[0].is_final);
        assert!(chunks[1].is_final);
        assert_eq!(chunks[1].session_id.as_deref(), Some("abc-123"));
    }
}
