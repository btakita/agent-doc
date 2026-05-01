//! # Module: agent::codex
//!
//! ## Spec
//! - Wraps the `codex` CLI binary as an `Agent` and `StreamingAgent` backend.
//! - Default invocation: `codex exec --json -s workspace-write`.
//! - Session resumption: uses `codex exec resume <id> --json` subcommand.
//!   Legacy sandbox flags (`-s/--sandbox`) are translated to `-c sandbox_mode="..."`
//!   because current `codex exec resume` no longer accepts `-s` directly.
//! - Session fork: falls back to a fresh `codex exec` session because Codex has no
//!   non-interactive fork equivalent.
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
//! - Stderr is drained in a background thread. On non-zero exit, the iterator yields a final
//!   `Err` with the exit status and stderr content. On zero exit with non-empty stderr, the
//!   content is logged to the parent's stderr with an `[agent]` prefix.
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
use std::sync::{Arc, Mutex};

use super::streaming::{StreamChunk, StreamingAgent};
use super::{Agent, AgentResponse};

#[derive(Clone)]
pub struct Codex {
    command: String,
    base_args: Vec<String>,
    env: Vec<(String, Option<String>)>,
}

struct ParsedCodexResponse {
    response: AgentResponse,
    saw_resume_capability_drift: bool,
}

struct StreamProcess {
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    child: std::process::Child,
    stderr_handle: Option<std::thread::JoinHandle<()>>,
    stderr_buf: Arc<Mutex<String>>,
}

fn looks_like_local_browser_cdp_permission_denied(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let local_cdp = lower.contains("127.0.0.1:9222") || lower.contains("localhost:9222");
    let permission_denied = lower.contains("operation not permitted")
        || lower.contains("os error 1")
        || lower.contains("eperm");
    local_cdp && permission_denied
}

fn resume_capability_drift_notice() -> &'static str {
    "[agent] codex resume session hit a stale local browser/CDP capability drift \
     (`Operation not permitted` on 127.0.0.1:9222); retrying once with a fresh \
     `codex exec` session"
}

pub(crate) fn default_base_args() -> Vec<String> {
    vec![
        "exec".to_string(),
        "--json".to_string(),
        "-s".to_string(),
        "workspace-write".to_string(),
    ]
}

/// Structural minimum args required for non-interactive JSON communication.
/// Sandbox settings are intentionally excluded — callers supply those
/// from frontmatter or config.
pub(crate) fn structural_base_args() -> Vec<String> {
    vec!["exec".to_string(), "--json".to_string()]
}

impl Codex {
    pub fn new(command: Option<String>, base_args: Option<Vec<String>>) -> Self {
        Self {
            command: command.unwrap_or_else(|| "codex".to_string()),
            base_args: base_args.unwrap_or_else(default_base_args),
            env: Vec::new(),
        }
    }

    pub fn with_env(mut self, env: Vec<(String, Option<String>)>) -> Self {
        self.env = env;
        self
    }

    fn append_resume_args(cmd: &mut Command, base_args: &[String]) {
        let mut args = base_args.iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "exec" | "--json" => {}
                "-s" | "--sandbox" => {
                    if let Some(mode) = args.next() {
                        cmd.arg("-c").arg(format!("sandbox_mode={mode:?}"));
                    } else {
                        cmd.arg(arg);
                    }
                }
                "--add-dir" => {
                    // `codex exec resume` does not accept --add-dir; the resumed
                    // session inherits writable roots from the original exec.
                    let _ = args.next();
                }
                _ if arg.starts_with("--sandbox=") => {
                    let mode = &arg["--sandbox=".len()..];
                    cmd.arg("-c").arg(format!("sandbox_mode={mode:?}"));
                }
                _ if arg.starts_with("--add-dir=") => {
                    // Same as --add-dir <DIR> above — strip for resume.
                }
                _ => {
                    cmd.arg(arg);
                }
            }
        }
    }

    fn build_command(&self, session_id: Option<&str>, _fork: bool, model: Option<&str>) -> Command {
        let mut cmd = Command::new(&self.command);

        if let Some(sid) = session_id {
            // codex exec resume <id> --json -c sandbox_mode="workspace-write"
            cmd.arg("exec").arg("resume").arg(sid).arg("--json");
            Self::append_resume_args(&mut cmd, &self.base_args);
        } else {
            // Fresh exec is also the fallback for `fork=true`: Codex has no
            // non-interactive "fork latest session" equivalent.
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

    fn send_once(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<ParsedCodexResponse> {
        let mut cmd = self.build_command(session_id, false, model);
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
        let mut saw_resume_capability_drift = false;

        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if looks_like_local_browser_cdp_permission_denied(line) {
                saw_resume_capability_drift = true;
            }
            let json: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

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
                        && let Some(text) =
                            item.and_then(|i| i.get("text")).and_then(|v| v.as_str())
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

        Ok(ParsedCodexResponse {
            response: AgentResponse {
                text: response_text,
                session_id: thread_id,
            },
            saw_resume_capability_drift,
        })
    }

    fn spawn_stream_process(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<StreamProcess> {
        let mut cmd = self.build_command(session_id, false, model);
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

        Ok(StreamProcess {
            lines: reader.lines(),
            child,
            stderr_handle,
            stderr_buf,
        })
    }
}

/// Parse a single JSONL line from Codex output into a StreamChunk.
pub fn parse_codex_line(line: &str) -> Result<StreamChunk> {
    let json: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| anyhow::anyhow!("failed to parse Codex JSONL: {}: {}", e, line))?;

    let event_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");

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
        let _ = fork;
        let mut parsed = self.send_once(prompt, session_id, model)?;
        if session_id.is_some() && parsed.saw_resume_capability_drift {
            eprintln!("{}", resume_capability_drift_notice());
            parsed = self.send_once(prompt, None, model).map_err(|e| {
                anyhow::anyhow!("fresh Codex retry after resume capability drift failed: {e}")
            })?;
        }

        if parsed.response.text.is_empty() {
            anyhow::bail!("Empty response from Codex");
        }

        Ok(parsed.response)
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
        let _ = fork;
        let process = self.spawn_stream_process(prompt, session_id, model)?;

        Ok(Box::new(CodexStreamIterator {
            lines: process.lines,
            child: process.child,
            stderr_handle: process.stderr_handle,
            stderr_buf: process.stderr_buf,
            session_id: None,
            done: false,
            backend: self.clone(),
            prompt: prompt.to_string(),
            model: model.map(|m| m.to_string()),
            allow_resume_capability_retry: session_id.is_some(),
            retried_fresh: false,
            yielded_agent_content: false,
        }))
    }
}

struct CodexStreamIterator {
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    child: std::process::Child,
    stderr_handle: Option<std::thread::JoinHandle<()>>,
    stderr_buf: Arc<Mutex<String>>,
    session_id: Option<String>,
    done: bool,
    backend: Codex,
    prompt: String,
    model: Option<String>,
    allow_resume_capability_retry: bool,
    retried_fresh: bool,
    yielded_agent_content: bool,
}

impl CodexStreamIterator {
    fn collect_stderr(&mut self) -> String {
        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
        self.stderr_buf
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn restart_fresh_after_resume_capability_drift(&mut self) -> Result<()> {
        eprintln!("{}", resume_capability_drift_notice());
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.collect_stderr();
        let process = self
            .backend
            .spawn_stream_process(&self.prompt, None, self.model.as_deref())
            .map_err(|e| {
                anyhow::anyhow!("fresh Codex retry after resume capability drift failed: {e}")
            })?;
        self.lines = process.lines;
        self.child = process.child;
        self.stderr_handle = process.stderr_handle;
        self.stderr_buf = process.stderr_buf;
        self.session_id = None;
        self.allow_resume_capability_retry = false;
        self.retried_fresh = true;
        self.yielded_agent_content = false;
        Ok(())
    }
}

impl Iterator for CodexStreamIterator {
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
                    if self.allow_resume_capability_retry
                        && !self.retried_fresh
                        && !self.yielded_agent_content
                        && looks_like_local_browser_cdp_permission_denied(&line)
                    {
                        if let Err(e) = self.restart_fresh_after_resume_capability_drift() {
                            self.done = true;
                            return Some(Err(e));
                        }
                        continue;
                    }
                    match parse_codex_line(&line) {
                        Ok(mut chunk) => {
                            if chunk.session_id.is_some() && !chunk.is_final {
                                self.session_id = chunk.session_id.take();
                            }
                            if chunk.is_final {
                                chunk.session_id = self.session_id.take();
                            }
                            if !chunk.is_final
                                && chunk.text.is_empty()
                                && chunk.thinking.is_none()
                                && chunk.session_id.is_none()
                            {
                                continue;
                            }
                            if !chunk.is_final
                                && (!chunk.text.is_empty() || chunk.thinking.is_some())
                            {
                                self.yielded_agent_content = true;
                            }
                            return Some(Ok(chunk));
                        }
                        Err(e) => return Some(Err(e)),
                    }
                }
                Some(Err(e)) => {
                    self.done = true;
                    return Some(Err(e.into()));
                }
                None => {
                    let stderr = self.collect_stderr();
                    let exit_status = self.child.wait().ok();
                    if let Some(status) = exit_status
                        && !status.success()
                    {
                        if self.allow_resume_capability_retry
                            && !self.retried_fresh
                            && !self.yielded_agent_content
                            && looks_like_local_browser_cdp_permission_denied(&stderr)
                        {
                            if let Err(e) = self.restart_fresh_after_resume_capability_drift() {
                                self.done = true;
                                return Some(Err(e));
                            }
                            continue;
                        }
                        self.done = true;
                        let msg = if stderr.trim().is_empty() {
                            format!("codex subprocess exited with {status}")
                        } else {
                            format!("codex subprocess exited with {status}: {}", stderr.trim())
                        };
                        return Some(Err(anyhow::anyhow!(msg)));
                    }
                    self.done = true;
                    if !stderr.trim().is_empty() {
                        eprintln!("[agent] codex subprocess stderr: {}", stderr.trim());
                    }
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_fake_codex_script(script: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fake-codex.sh");
        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        (dir, path.to_string_lossy().into_owned())
    }

    #[test]
    fn streaming_stderr_surfaced_on_nonzero_exit() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo >&2 'sandbox violation'; exit 1".into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("sandbox violation"), "got: {msg}");
        assert!(msg.contains("codex subprocess exited with"), "got: {msg}");
    }

    #[test]
    fn streaming_stderr_logged_on_zero_exit() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                r#"cat >/dev/null; echo '{"type":"thread.started","thread_id":"t1"}'; echo '{"type":"turn.completed","usage":{}}'; echo >&2 'deprecation warning'"#
                    .into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<Result<StreamChunk>> = iter.collect();
        assert!(
            chunks.iter().all(|c| c.is_ok()),
            "expected no errors, got: {chunks:?}"
        );
        let final_chunk = chunks
            .iter()
            .find(|c| c.as_ref().map(|sc| sc.is_final).unwrap_or(false));
        assert!(final_chunk.is_some(), "expected final chunk");
    }

    #[test]
    fn streaming_stderr_empty_on_nonzero_exit() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec!["-c".into(), "cat >/dev/null; exit 42".into()]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("codex subprocess exited with"), "got: {msg}");
    }

    fn command_args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(OsStr::to_string_lossy)
            .map(|s| s.into_owned())
            .collect()
    }

    #[test]
    fn parse_thread_started() {
        let line =
            r#"{"type":"thread.started","thread_id":"019db613-e57b-77d2-844c-9e7dca83ad01"}"#;
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

    #[test]
    fn build_command_exec_preserves_default_sandbox_flag() {
        let codex = Codex::new(None, None);
        let cmd = codex.build_command(None, false, None);

        assert_eq!(
            command_args(&cmd),
            vec!["exec", "--json", "-s", "workspace-write"]
        );
    }

    #[test]
    fn build_command_resume_translates_short_sandbox_flag() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--skip-git-repo-check".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-123"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-123",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "--skip-git-repo-check",
            ]
        );
    }

    #[test]
    fn build_command_fork_starts_fresh_exec_session() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "--sandbox=danger-full-access".into(),
                "--ignore-user-config".into(),
            ]),
        );
        let cmd = codex.build_command(None, true, Some("gpt-5.4"));

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "--json",
                "--sandbox=danger-full-access",
                "--ignore-user-config",
                "-m",
                "gpt-5.4",
            ]
        );
    }

    #[test]
    fn build_command_exec_preserves_add_dir() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir".into(),
                "/home/user/.git/modules/sub".into(),
            ]),
        );
        let cmd = codex.build_command(None, false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "--json",
                "-s",
                "workspace-write",
                "--add-dir",
                "/home/user/.git/modules/sub",
            ]
        );
    }

    #[test]
    fn build_command_resume_strips_add_dir() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir".into(),
                "/home/user/.git/modules/sub".into(),
                "--skip-git-repo-check".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-456"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-456",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "--skip-git-repo-check",
            ]
        );
    }

    #[test]
    fn build_command_resume_strips_add_dir_equals_form() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir=/home/user/.git".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-789"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-789",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
            ]
        );
    }

    #[test]
    fn build_command_resume_strips_multiple_add_dirs() {
        let codex = Codex::new(
            Some("codex".into()),
            Some(vec![
                "exec".into(),
                "--json".into(),
                "-s".into(),
                "workspace-write".into(),
                "--add-dir".into(),
                "/home/user/.git/modules/sub".into(),
                "--add-dir".into(),
                "/home/user/.git".into(),
            ]),
        );
        let cmd = codex.build_command(Some("thread-abc"), false, None);

        assert_eq!(
            command_args(&cmd),
            vec![
                "exec",
                "resume",
                "thread-abc",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
            ]
        );
    }

    #[test]
    fn local_browser_cdp_permission_denied_matches_resume_capability_drift_signature() {
        assert!(looks_like_local_browser_cdp_permission_denied(
            "chromium-bridge check failed for 127.0.0.1:9222: Operation not permitted (os error 1)"
        ));
        assert!(!looks_like_local_browser_cdp_permission_denied(
            "chromium-bridge check failed for 127.0.0.1:9222: Connection refused"
        ));
    }

    #[test]
    fn send_retries_fresh_exec_after_resume_capability_drift_signal() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"chromium-bridge check failed for 127.0.0.1:9222: Operation not permitted (os error 1)","exit_code":1,"status":"completed"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"stale resume response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None);

        let response = codex
            .send("prompt", Some("resume-123"), false, None)
            .unwrap();

        assert_eq!(response.text, "fresh response");
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn streaming_retries_fresh_exec_before_yielding_stale_resume_response() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"chromium-bridge list failed for localhost:9222: Operation not permitted (os error 1)","exit_code":1,"status":"completed"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"stale resume response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "fresh response");
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("fresh-thread")
        );
    }
}
