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
//! - When a document declares `required_ssh_targets`, the backend probes those targets before
//!   launch and treats target-specific SSH failures in resumed Codex sessions as capability drift:
//!   retry once with fresh `codex exec`, then fail closed if the capability still is not proven.
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
use std::collections::VecDeque;
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
    required_ssh_targets: Vec<String>,
}

struct ParsedCodexResponse {
    response: AgentResponse,
    saw_resume_capability_drift: bool,
    required_ssh_failure: Option<String>,
}

#[derive(Clone, Debug)]
struct RequiredSshCapability {
    match_terms: Vec<String>,
}

#[derive(Clone, Debug)]
struct SshProbeOutcome {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Clone, Debug)]
struct ResolvedSshTarget {
    direct_target: String,
    port: Option<String>,
    identity_file: Option<String>,
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

fn lower_trimmed_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|line| !line.is_empty())
}

fn append_isolated_ssh_probe_args(args: &mut Vec<String>) {
    args.extend(
        [
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
            "-o",
            "ClearAllForwardings=yes",
            "-o",
            "PermitLocalCommand=no",
        ]
        .into_iter()
        .map(str::to_string),
    );
}

fn looks_like_ssh_dns_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("could not resolve hostname")
        || lower.contains("name or service not known")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("nodename nor servname provided")
}

fn looks_like_ssh_network_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("operation not permitted")
        || lower.contains("network is unreachable")
        || lower.contains("no route to host")
        || lower.contains("connection timed out")
        || lower.contains("connection refused")
        || lower.contains("connect to host")
}

fn looks_like_ssh_auth_failure(text: &str) -> bool {
    text.to_ascii_lowercase().contains("permission denied")
}

fn looks_like_ssh_alias_config_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("bad configuration option")
        || lower.contains("terminating,")
        || lower.contains("could not include")
        || lower.contains("include ")
        || lower.contains("no such file or directory")
}

fn has_required_ssh_match_term(text: &str, lowered_terms: &[String]) -> bool {
    let lower = text.to_ascii_lowercase();
    lowered_terms.iter().any(|term| lower.contains(term))
}

fn looks_like_ssh_command(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let trimmed = token.trim_matches(|c: char| matches!(c, '"' | '\'' | '`'));
        let basename = trimmed.rsplit('/').next().unwrap_or(trimmed);
        basename.eq_ignore_ascii_case("ssh") || basename.eq_ignore_ascii_case("ssh.exe")
    })
}

fn first_required_ssh_failure_line<'a>(
    text: &'a str,
    lowered_terms: &[String],
    require_match_term: bool,
) -> Option<&'a str> {
    for line in lower_trimmed_lines(text) {
        let lower = line.to_ascii_lowercase();
        if require_match_term && !lowered_terms.iter().any(|term| lower.contains(term)) {
            continue;
        }
        if looks_like_ssh_dns_failure(&lower)
            || looks_like_ssh_network_failure(&lower)
            || looks_like_ssh_auth_failure(&lower)
            || looks_like_ssh_alias_config_failure(&lower)
        {
            return Some(line);
        }
    }
    None
}

fn command_execution_required_ssh_failure(
    command: &str,
    aggregated_output: &str,
    lowered_terms: &[String],
) -> Option<String> {
    if let Some(line) = first_required_ssh_failure_line(aggregated_output, lowered_terms, true) {
        return Some(line.to_string());
    }

    if !looks_like_ssh_command(command)
        || !has_required_ssh_match_term(command, lowered_terms)
        || looks_like_local_browser_cdp_permission_denied(aggregated_output)
    {
        return None;
    }

    first_required_ssh_failure_line(aggregated_output, lowered_terms, false).map(str::to_string)
}

fn transcript_has_required_ssh_failure(text: &str, match_terms: &[String]) -> Option<String> {
    if match_terms.is_empty() {
        return None;
    }
    let lowered_terms: Vec<String> = match_terms
        .iter()
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| !term.is_empty())
        .collect();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        let item = json.get("item")?;
        let item_type = item.get("type").and_then(|v| v.as_str())?;
        if item_type != "command_execution" {
            return None;
        }

        let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let aggregated_output = item
            .get("aggregated_output")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        return command_execution_required_ssh_failure(command, aggregated_output, &lowered_terms);
    }

    first_required_ssh_failure_line(text, &lowered_terms, true).map(str::to_string)
}

fn transcript_proves_required_ssh_success(text: &str, match_terms: &[String]) -> bool {
    if match_terms.is_empty() {
        return false;
    }
    let lowered_terms: Vec<String> = match_terms
        .iter()
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| !term.is_empty())
        .collect();
    if lowered_terms.is_empty() {
        return false;
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let Some(item) = json.get("item") else {
        return false;
    };
    if item.get("type").and_then(|v| v.as_str()) != Some("command_execution") {
        return false;
    }

    let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if !looks_like_ssh_command(command) || !has_required_ssh_match_term(command, &lowered_terms) {
        return false;
    }

    item.get("exit_code").and_then(|v| v.as_i64()) == Some(0)
}

fn format_required_ssh_failure(targets: &[String], detail: &str) -> String {
    format!(
        "required SSH capability failed for target(s) {}: {}",
        targets.join(", "),
        detail.trim()
    )
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
            required_ssh_targets: Vec::new(),
        }
    }

    pub fn with_env(mut self, env: Vec<(String, Option<String>)>) -> Self {
        self.env = env;
        self
    }

    pub fn with_required_ssh_targets(mut self, targets: Vec<String>) -> Self {
        let mut normalized = Vec::new();
        for target in targets {
            let trimmed = target.trim();
            if !trimmed.is_empty() && !normalized.iter().any(|existing| existing == trimmed) {
                normalized.push(trimmed.to_string());
            }
        }
        self.required_ssh_targets = normalized;
        self
    }

    fn apply_env_overrides(&self, cmd: &mut Command) {
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

    fn write_prompt_to_child(
        stdin: &mut std::process::ChildStdin,
        prompt: &str,
    ) -> std::io::Result<()> {
        use std::io::Write;

        match stdin.write_all(prompt.as_bytes()) {
            Ok(()) => Ok(()),
            // Tiny fake codex scripts used in unit tests can finish and close
            // stdin before the parent writes the prompt. Treat that as
            // non-fatal and let the child exit/output determine the result.
            Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            Err(err) => Err(err),
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

        self.apply_env_overrides(&mut cmd);

        cmd
    }

    fn run_ssh_probe(&self, args: &[String]) -> Result<SshProbeOutcome> {
        let mut cmd = Command::new("ssh");
        cmd.args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        self.apply_env_overrides(&mut cmd);
        let output = cmd
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run ssh probe: {e}"))?;
        Ok(SshProbeOutcome {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn resolve_direct_target(&self, target: &str) -> Result<Option<ResolvedSshTarget>> {
        let outcome = self.run_ssh_probe(&["-G".to_string(), target.to_string()])?;
        if !outcome.success {
            return Ok(None);
        }

        let mut host: Option<String> = None;
        let mut user: Option<String> = None;
        let mut port: Option<String> = None;
        let mut identity_file: Option<String> = None;
        for line in lower_trimmed_lines(&outcome.stdout) {
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or_default();
            let value = parts.collect::<Vec<_>>().join(" ");
            if value.is_empty() {
                continue;
            }
            match key {
                "hostname" => host = Some(value),
                "user" => user = Some(value),
                "port" => port = Some(value),
                "identityfile" => {
                    if identity_file.is_none() {
                        identity_file = Some(value);
                    }
                }
                _ => {}
            }
        }

        let host = match host {
            Some(host) => host,
            None => return Ok(None),
        };
        let direct_target = match user {
            Some(user) if !user.is_empty() => format!("{user}@{host}"),
            _ => host,
        };
        Ok(Some(ResolvedSshTarget {
            direct_target,
            port,
            identity_file,
        }))
    }

    fn prove_required_ssh_capability(&self) -> Result<Option<RequiredSshCapability>> {
        if self.required_ssh_targets.is_empty() {
            return Ok(None);
        }

        let mut match_terms = Vec::new();
        for target in &self.required_ssh_targets {
            match_terms.push(target.clone());

            let mut alias_args = Vec::new();
            append_isolated_ssh_probe_args(&mut alias_args);
            alias_args.push(target.clone());
            alias_args.push("true".to_string());
            let alias = self.run_ssh_probe(&alias_args)?;
            let resolved = self.resolve_direct_target(target)?;
            let direct = if let Some(resolved) = &resolved {
                let mut args = vec!["-F".to_string(), "/dev/null".to_string()];
                append_isolated_ssh_probe_args(&mut args);
                if let Some(port) = &resolved.port {
                    args.push("-p".to_string());
                    args.push(port.clone());
                }
                if let Some(identity_file) = &resolved.identity_file {
                    args.push("-i".to_string());
                    args.push(identity_file.clone());
                    args.push("-o".to_string());
                    args.push("IdentitiesOnly=yes".to_string());
                }
                args.push(resolved.direct_target.clone());
                args.push("true".to_string());
                match_terms.push(resolved.direct_target.clone());
                if let Some((_user, host)) = resolved.direct_target.split_once('@') {
                    match_terms.push(host.to_string());
                }
                Some((resolved.direct_target.clone(), self.run_ssh_probe(&args)?))
            } else {
                None
            };

            if alias.success {
                continue;
            }

            let alias_detail = if !alias.stderr.trim().is_empty() {
                alias.stderr.trim()
            } else if !alias.stdout.trim().is_empty() {
                alias.stdout.trim()
            } else {
                "unknown ssh alias failure"
            };
            if let Some((direct_target, direct_outcome)) = &direct
                && direct_outcome.success
            {
                anyhow::bail!(
                    "required SSH capability failed for target `{}`: alias/config path is degraded (`{}`), \
                     but isolated direct host probe `{}` succeeded. Fix the SSH alias/config before trusting this Codex session.",
                    target,
                    alias_detail,
                    direct_target
                );
            }

            let detail = if let Some((direct_target, direct_outcome)) = &direct {
                let direct_text = if !direct_outcome.stderr.trim().is_empty() {
                    direct_outcome.stderr.trim()
                } else if !direct_outcome.stdout.trim().is_empty() {
                    direct_outcome.stdout.trim()
                } else {
                    "unknown direct ssh failure"
                };
                let classification = if looks_like_ssh_dns_failure(direct_text) {
                    "DNS resolution failed"
                } else if looks_like_ssh_network_failure(direct_text) {
                    "outbound SSH/network access failed"
                } else if looks_like_ssh_auth_failure(direct_text) {
                    "SSH authentication failed"
                } else if looks_like_ssh_alias_config_failure(alias_detail) {
                    "SSH alias/config resolution failed"
                } else {
                    "SSH capability could not be proven"
                };
                format!(
                    "{classification} for `{}` via isolated direct probe `{}`: {}",
                    target, direct_target, direct_text
                )
            } else if looks_like_ssh_alias_config_failure(alias_detail)
                || looks_like_ssh_dns_failure(alias_detail)
            {
                format!(
                    "SSH alias/config resolution failed for `{}` during isolated pre-launch probe: {}",
                    target, alias_detail
                )
            } else {
                format!(
                    "SSH capability could not be proven for `{}` during isolated pre-launch probe: {}",
                    target, alias_detail
                )
            };
            anyhow::bail!("{detail}");
        }

        Ok(Some(RequiredSshCapability { match_terms }))
    }

    fn send_once(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        model: Option<&str>,
        required_ssh_match_terms: &[String],
    ) -> Result<ParsedCodexResponse> {
        let mut cmd = self.build_command(session_id, false, model);
        let output = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(ref mut stdin) = child.stdin {
                    Self::write_prompt_to_child(stdin, prompt)?;
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
        let mut required_ssh_failure = None;

        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if looks_like_local_browser_cdp_permission_denied(line) {
                saw_resume_capability_drift = true;
            }
            if required_ssh_failure.is_none() {
                required_ssh_failure =
                    transcript_has_required_ssh_failure(line, required_ssh_match_terms);
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
            required_ssh_failure,
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
            if let Some(ref mut stdin) = child.stdin {
                Self::write_prompt_to_child(stdin, prompt)?;
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
        let required_ssh = self.prove_required_ssh_capability()?;
        let required_ssh_match_terms = required_ssh
            .as_ref()
            .map(|capability| capability.match_terms.as_slice())
            .unwrap_or(&[]);
        let mut parsed = self.send_once(prompt, session_id, model, required_ssh_match_terms)?;
        if session_id.is_some()
            && (parsed.saw_resume_capability_drift || parsed.required_ssh_failure.is_some())
        {
            if parsed.saw_resume_capability_drift {
                eprintln!("{}", resume_capability_drift_notice());
            } else if let Some(detail) = parsed.required_ssh_failure.as_deref() {
                eprintln!(
                    "[agent] codex resume session lost required SSH capability for {} ({}); retrying once with a fresh `codex exec` session",
                    self.required_ssh_targets.join(", "),
                    detail
                );
            }
            parsed = self
                .send_once(prompt, None, model, required_ssh_match_terms)
                .map_err(|e| {
                    anyhow::anyhow!("fresh Codex retry after resume capability drift failed: {e}")
                })?;
        }
        if let Some(detail) = parsed.required_ssh_failure.as_deref() {
            anyhow::bail!(format_required_ssh_failure(
                &self.required_ssh_targets,
                detail
            ));
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
        let required_ssh = self.prove_required_ssh_capability()?;
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
            saw_final_chunk: false,
            buffered_chunks: VecDeque::new(),
            buffer_required_ssh_chunks: session_id.is_some() && required_ssh.is_some(),
            required_ssh_match_terms: required_ssh
                .map(|capability| capability.match_terms)
                .unwrap_or_default(),
            required_ssh_targets: self.required_ssh_targets.clone(),
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
    saw_final_chunk: bool,
    buffered_chunks: VecDeque<StreamChunk>,
    buffer_required_ssh_chunks: bool,
    required_ssh_match_terms: Vec<String>,
    required_ssh_targets: Vec<String>,
}

impl CodexStreamIterator {
    fn pop_buffered_chunk(&mut self) -> Option<Result<StreamChunk>> {
        self.buffered_chunks.pop_front().map(Ok)
    }

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
        self.saw_final_chunk = false;
        self.buffered_chunks.clear();
        self.buffer_required_ssh_chunks = false;
        Ok(())
    }

    fn restart_fresh_after_required_ssh_drift(&mut self, detail: &str) -> Result<()> {
        eprintln!(
            "[agent] codex resume session lost required SSH capability for {} ({}); retrying once with a fresh `codex exec` session",
            self.required_ssh_targets.join(", "),
            detail
        );
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
        self.saw_final_chunk = false;
        self.buffered_chunks.clear();
        self.buffer_required_ssh_chunks = false;
        Ok(())
    }

    fn release_required_ssh_buffer(&mut self) -> Option<Result<StreamChunk>> {
        self.buffer_required_ssh_chunks = false;
        self.pop_buffered_chunk()
    }
}

impl Iterator for CodexStreamIterator {
    type Item = Result<StreamChunk>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if let Some(chunk) = self.pop_buffered_chunk() {
            return Some(chunk);
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
                    if let Some(detail) =
                        transcript_has_required_ssh_failure(&line, &self.required_ssh_match_terms)
                    {
                        if self.allow_resume_capability_retry && !self.retried_fresh {
                            if let Err(e) = self.restart_fresh_after_required_ssh_drift(&detail) {
                                self.done = true;
                                return Some(Err(e));
                            }
                            continue;
                        }
                        self.done = true;
                        return Some(Err(anyhow::anyhow!(format_required_ssh_failure(
                            &self.required_ssh_targets,
                            &detail
                        ))));
                    }
                    if self.buffer_required_ssh_chunks
                        && transcript_proves_required_ssh_success(
                            &line,
                            &self.required_ssh_match_terms,
                        )
                        && let Some(chunk) = self.release_required_ssh_buffer()
                    {
                        return Some(chunk);
                    }
                    match parse_codex_line(&line) {
                        Ok(mut chunk) => {
                            if chunk.session_id.is_some() && !chunk.is_final {
                                self.session_id = chunk.session_id.take();
                            }
                            if chunk.is_final {
                                self.saw_final_chunk = true;
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
                                if self.buffer_required_ssh_chunks {
                                    self.buffered_chunks.push_back(chunk);
                                    continue;
                                }
                            }
                            if chunk.is_final && self.buffer_required_ssh_chunks {
                                self.buffered_chunks.push_back(chunk);
                                if let Some(buffered) = self.release_required_ssh_buffer() {
                                    return Some(buffered);
                                }
                                continue;
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
                        if let Some(detail) = transcript_has_required_ssh_failure(
                            &stderr,
                            &self.required_ssh_match_terms,
                        ) {
                            if self.allow_resume_capability_retry && !self.retried_fresh {
                                if let Err(e) = self.restart_fresh_after_required_ssh_drift(&detail)
                                {
                                    self.done = true;
                                    return Some(Err(e));
                                }
                                continue;
                            }
                            self.done = true;
                            return Some(Err(anyhow::anyhow!(format_required_ssh_failure(
                                &self.required_ssh_targets,
                                &detail
                            ))));
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
                    if !self.buffered_chunks.is_empty() {
                        if self.yielded_agent_content && !self.saw_final_chunk {
                            self.buffered_chunks.push_back(StreamChunk {
                                text: String::new(),
                                thinking: None,
                                is_final: true,
                                session_id: self.session_id.take(),
                            });
                        }
                        self.buffer_required_ssh_chunks = false;
                        return self.pop_buffered_chunk();
                    }
                    if self.yielded_agent_content && !self.saw_final_chunk {
                        return Some(Ok(StreamChunk {
                            text: String::new(),
                            thinking: None,
                            is_final: true,
                            session_id: self.session_id.take(),
                        }));
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

    fn write_fake_ssh_script(script: &str) -> (TempDir, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ssh");
        let dir_path = dir.path().to_string_lossy().into_owned();
        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        (dir, dir_path)
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
    fn streaming_synthesizes_final_chunk_when_successful_eof_follows_agent_message() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo '{\"type\":\"thread.started\",\"thread_id\":\"t1\"}'; echo '{\"type\":\"item.completed\",\"item\":{\"id\":\"msg-1\",\"type\":\"agent_message\",\"text\":\"hello\"}}'".into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<Result<StreamChunk>> = iter.collect();
        assert!(
            chunks.iter().all(|c| c.is_ok()),
            "expected no errors, got: {chunks:?}"
        );
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "hello");
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("t1")
        );
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
    fn required_ssh_capability_reports_alias_config_failure_when_direct_path_still_works() {
        let (_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
case "$*" in
  *monsterrodholders-server*)
    echo "ssh: Could not resolve hostname monsterrodholders-server: Name or service not known" >&2
    exit 255
    ;;
  *50.28.2.199*)
    exit 0
    ;;
esac
exit 0
"#,
        );
        let codex = Codex::new(None, None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let err = codex
            .prove_required_ssh_capability()
            .unwrap_err()
            .to_string();
        assert!(err.contains("monsterrodholders-server"), "got: {err}");
        assert!(err.contains("isolated direct host probe"), "got: {err}");
    }

    #[test]
    fn required_ssh_probes_disable_shared_control_socket_state() {
        let (dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$SSH_LOG"
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let log_path = dir.path().join("ssh-probe.log");
        fs::write(&log_path, "").unwrap();
        let codex = Codex::new(None, None)
            .with_env(vec![
                ("PATH".to_string(), Some(path_dir)),
                (
                    "SSH_LOG".to_string(),
                    Some(log_path.to_string_lossy().into_owned()),
                ),
            ])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        codex.prove_required_ssh_capability().unwrap();

        let log = fs::read_to_string(&log_path).unwrap();
        let connect_lines: Vec<_> = log
            .lines()
            .filter(|line| !line.starts_with("-G "))
            .collect();
        assert_eq!(connect_lines.len(), 2, "got log: {log}");
        for line in connect_lines {
            assert!(line.contains("-o BatchMode=yes"), "got: {line}");
            assert!(line.contains("-o ConnectTimeout=5"), "got: {line}");
            assert!(line.contains("-o ControlMaster=no"), "got: {line}");
            assert!(line.contains("-o ControlPath=none"), "got: {line}");
            assert!(line.contains("-o ClearAllForwardings=yes"), "got: {line}");
            assert!(line.contains("-o PermitLocalCommand=no"), "got: {line}");
        }
    }

    #[test]
    fn required_ssh_failure_detects_bare_socket_eperm_when_command_proves_ssh_context() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["monsterrodholders-server".to_string()]),
            Some("socket: Operation not permitted".to_string())
        );
    }

    #[test]
    fn required_ssh_failure_ignores_bare_socket_eperm_without_ssh_command_context() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"chromium-bridge list","aggregated_output":"socket: Operation not permitted","exit_code":1,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["monsterrodholders-server".to_string()]),
            None
        );
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
    fn send_retries_fresh_exec_after_bare_socket_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let response = codex
            .send("prompt", Some("resume-123"), false, None)
            .unwrap();

        assert_eq!(response.text, "fresh response");
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn send_retries_fresh_exec_after_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"ssh: connect to host 50.28.2.199 port 22: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

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

    #[test]
    fn streaming_retries_fresh_exec_after_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"ssh: connect to host 50.28.2.199 port 22: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

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

    #[test]
    fn streaming_retries_fresh_exec_after_bare_socket_required_ssh_resume_drift_signal() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

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

    #[test]
    fn streaming_required_ssh_retry_discards_buffered_resumed_prelude_text() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"stale-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"I am retrying the SSH step now.\n"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-2","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"","exit_code":0,"status":"completed"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().text, "fresh response");
        assert!(
            !chunks[0]
                .as_ref()
                .unwrap()
                .text
                .contains("I am retrying the SSH step now."),
            "stale resumed prelude should be discarded"
        );
        assert!(chunks[1].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[1].as_ref().unwrap().session_id.as_deref(),
            Some("fresh-thread")
        );
    }

    #[test]
    fn streaming_required_ssh_success_releases_buffered_chunks() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname 50.28.2.199"
  echo "port 22"
  echo "identityfile /tmp/id_ed25519"
  exit 0
fi
exit 0
"#,
        );
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"resume-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"I am checking SSH first.\n"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh monsterrodholders-server true","aggregated_output":"","exit_code":0,"status":"completed"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"SSH worked."}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["monsterrodholders-server".to_string()]);

        let chunks: Vec<_> = codex
            .send_streaming("prompt", Some("resume-123"), false, None)
            .unwrap()
            .collect();

        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[0].as_ref().unwrap().text,
            "I am checking SSH first.\n"
        );
        assert_eq!(chunks[1].as_ref().unwrap().text, "SSH worked.");
        assert!(chunks[2].as_ref().unwrap().is_final);
        assert_eq!(
            chunks[2].as_ref().unwrap().session_id.as_deref(),
            Some("resume-thread")
        );
    }
}
