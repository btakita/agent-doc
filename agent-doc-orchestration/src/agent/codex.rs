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
//!   `thread_id` from `thread.started` and response text from the final `item.completed`
//!   (type=agent_message) before `turn.completed`.
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
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::io::BufRead;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::streaming::StreamingAgent;
use super::{Agent, AgentResponse};
use agent_doc_frontmatter::frontmatter::{CodexNetworkAccess, Frontmatter};
use agent_doc_turn_executor::agent_stream::{StreamChunk, parse_codex_line};

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

const CODEX_CHILD_NETWORK_PROBE_MARKER: &str = "AGENT_DOC_NETWORK_PROBE_OK";
const CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER: &str = "AGENT_DOC_WRITABLE_ROOT_PROBE_OK";

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

fn is_codex_marketplace_manifest_noise(line: &str) -> bool {
    let trimmed = line.trim();
    let is_external_plugin_manifest = trimmed.contains("/.codex/.tmp/plugins/plugins/");
    let is_prompt_warning = (trimmed.contains("codex_core_plugins::manifest:")
        || trimmed.contains("codex_core::plugins::manifest:"))
        && trimmed.contains("ignoring interface.defaultPrompt:");
    let is_skill_icon_warning = trimmed.contains("codex_core_skills::loader:")
        && (trimmed.contains("ignoring interface.icon_small: icon path must not contain '..'")
            || trimmed.contains("ignoring interface.icon_large: icon path must not contain '..'"));

    (is_external_plugin_manifest && is_prompt_warning) || is_skill_icon_warning
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexStderrNoiseReport {
    filtered: String,
    suppressed_marketplace_manifest_warnings: usize,
}

fn filter_codex_stderr_noise(stderr: &str) -> String {
    codex_stderr_noise_report(stderr).filtered
}

fn codex_stderr_noise_report(stderr: &str) -> CodexStderrNoiseReport {
    let mut suppressed_marketplace_manifest_warnings = 0;
    let mut kept = Vec::new();
    for line in stderr.lines() {
        if is_codex_marketplace_manifest_noise(line) {
            suppressed_marketplace_manifest_warnings += 1;
        } else {
            kept.push(line);
        }
    }
    let mut filtered = kept.join("\n");
    if !filtered.is_empty() && stderr.ends_with('\n') {
        filtered.push('\n');
    }
    CodexStderrNoiseReport {
        filtered,
        suppressed_marketplace_manifest_warnings,
    }
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

fn looks_like_codex_transport_403_429(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let ws_403 = lower.contains("403")
        && (lower.contains("websocket") || lower.contains("handshake") || lower.contains("wss://"));
    let https_429 = lower.contains("429")
        && (lower.contains("too many requests") || lower.contains("rate limit"));
    ws_403 || https_429
}

fn format_transport_403_429_diagnostic(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    let is_ws_403 = lower.contains("403")
        && (lower.contains("websocket") || lower.contains("handshake") || lower.contains("wss://"));
    let is_https_429 = lower.contains("429")
        && (lower.contains("too many requests") || lower.contains("rate limit"));
    let primary = if is_ws_403 && is_https_429 {
        "Codex transport rejection: 403 Forbidden on WebSocket handshake, then 429 Too Many Requests on HTTPS fallback"
    } else if is_ws_403 {
        "Codex transport rejection: 403 Forbidden on WebSocket handshake"
    } else {
        "Codex transport rejection: 429 Too Many Requests on HTTPS fallback"
    };
    format!(
        "{primary}. Possible causes: per-session rate limit, Cloudflare edge block, \
         or session-specific token/auth state. Suggestions: (1) wait a few minutes and \
         retry, (2) restart the codex session, or (3) check Codex service status. \
         Original error: {}",
        stderr.trim()
    )
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

fn line_proves_ssh_failure_context(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    lower.starts_with("ssh:")
        || lower.starts_with("ssh.exe:")
        || lower.starts_with("kex_exchange_identification:")
        || lower.starts_with("connection closed by ")
}

fn format_required_ssh_command_failure(command: &str, line: &str) -> String {
    let trimmed_command = command.trim();
    if trimmed_command.is_empty() {
        line.to_string()
    } else {
        format!("command `{trimmed_command}`: {line}")
    }
}

fn command_execution_required_ssh_failure(
    command: &str,
    aggregated_output: &str,
    lowered_terms: &[String],
) -> Option<String> {
    if looks_like_local_browser_cdp_permission_denied(aggregated_output) {
        return None;
    }

    let command_proves_required_ssh =
        looks_like_ssh_command(command) && has_required_ssh_match_term(command, lowered_terms);
    if command_proves_required_ssh {
        return first_required_ssh_failure_line(aggregated_output, lowered_terms, false)
            .map(|line| format_required_ssh_command_failure(command, line));
    }

    first_required_ssh_failure_line(aggregated_output, lowered_terms, true)
        .filter(|line| line_proves_ssh_failure_context(line))
        .map(str::to_string)
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

fn add_dirs_from_args(args: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--add-dir" {
            if let Some(dir) = iter.next() {
                dirs.push(PathBuf::from(dir));
            }
            continue;
        }
        if let Some(dir) = arg.strip_prefix("--add-dir=") {
            dirs.push(PathBuf::from(dir));
        }
    }
    dirs
}

fn resolved_codex_agent_args_for_contract(
    fm: &Frontmatter,
    global_config: &crate::config::Config,
) -> Option<String> {
    fm.agent_args
        .clone()
        .or_else(|| fm.codex_args.clone())
        .or_else(|| global_config.agent_args.clone())
        .or_else(|| global_config.codex_args.clone())
}

fn normalized_writable_root_strings(roots: &[PathBuf]) -> Vec<String> {
    let mut normalized = BTreeSet::new();
    for root in roots {
        let path = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        normalized.insert(path.to_string_lossy().into_owned());
    }
    normalized.into_iter().collect()
}

pub fn writable_root_contract_id(roots: &[PathBuf]) -> Option<String> {
    let normalized = normalized_writable_root_strings(roots);
    if normalized.is_empty() {
        return None;
    }
    Some(crate::snapshot::doc_hash_from_str(&normalized.join("\n")))
}

pub fn managed_writable_roots_for_doc(
    file: &Path,
    fm: &Frontmatter,
    global_config: &crate::config::Config,
) -> Vec<PathBuf> {
    let mut args = Vec::new();
    if let Some(raw_args) = resolved_codex_agent_args_for_contract(fm, global_config) {
        args.extend(raw_args.split_whitespace().map(String::from));
    }
    crate::agent::append_workspace_access_args("codex", &mut args, file);
    add_dirs_from_args(&args)
}

pub fn managed_writable_root_contract_id_for_doc(
    file: &Path,
    fm: &Frontmatter,
    global_config: &crate::config::Config,
) -> Option<String> {
    writable_root_contract_id(&managed_writable_roots_for_doc(file, fm, global_config))
}

fn proof_status_label(required: bool, proven: bool) -> &'static str {
    match (required, proven) {
        (false, _) => "not_required",
        (true, true) => "proven",
        (true, false) => "failed",
    }
}

#[derive(Debug, Clone, Default)]
struct ManagedCapabilityProofTimings {
    network_host_dns: Option<Duration>,
    network_child: Option<Duration>,
    ssh: Option<Duration>,
    writable_launcher: Option<Duration>,
    writable_child: Option<Duration>,
    total: Duration,
}

fn proof_timing_ms(duration: Option<Duration>) -> String {
    duration
        .map(|value| value.as_millis().to_string())
        .unwrap_or_else(|| "not_required".to_string())
}

impl ManagedCapabilityProofTimings {
    fn event_fields(&self) -> String {
        format!(
            "timings_ms=network_host_dns:{},network_child:{},ssh:{},writable_launcher:{},writable_child:{},total:{}",
            proof_timing_ms(self.network_host_dns),
            proof_timing_ms(self.network_child),
            proof_timing_ms(self.ssh),
            proof_timing_ms(self.writable_launcher),
            proof_timing_ms(self.writable_child),
            self.total.as_millis()
        )
    }
}

static MANAGED_NETWORK_CHILD_PROOF_CACHE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn managed_network_child_proof_cache() -> &'static Mutex<HashSet<String>> {
    MANAGED_NETWORK_CHILD_PROOF_CACHE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn managed_network_child_proof_cache_key(
    command: &str,
    args: &[String],
    env: &std::collections::HashMap<String, String>,
    harness: &str,
) -> String {
    let mut env_pairs: Vec<_> = env.iter().collect();
    env_pairs.sort_by_key(|(left, _)| *left);
    let raw = serde_json::json!({
        "harness": harness,
        "command": command,
        "probe_args": codex_exec_args_for_probe(args),
        "env": env_pairs,
    })
    .to_string();
    agent_doc_hash::content_hash(&raw)
}

fn managed_network_child_proof_is_cached(key: &str) -> bool {
    managed_network_child_proof_cache()
        .lock()
        .map(|cache| cache.contains(key))
        .unwrap_or(false)
}

fn remember_managed_network_child_proof(key: String) {
    if let Ok(mut cache) = managed_network_child_proof_cache().lock() {
        cache.insert(key);
    }
}

fn env_map_as_overrides(
    env: &std::collections::HashMap<String, String>,
) -> Vec<(String, Option<String>)> {
    env.iter()
        .map(|(key, value)| (key.clone(), Some(value.clone())))
        .collect()
}

fn prove_dns_resolution() -> Result<()> {
    ("github.com", 443)
        .to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("DNS probe for github.com failed: {e}"))?
        .next()
        .ok_or_else(|| anyhow::anyhow!("DNS probe for github.com returned no addresses"))?;
    Ok(())
}

fn codex_exec_args_for_probe(launch_args: &[String]) -> Vec<String> {
    if launch_args.first().is_some_and(|arg| arg == "exec") {
        let mut args = launch_args.to_vec();
        if !args.iter().any(|arg| arg == "--json") {
            args.insert(1, "--json".to_string());
        }
        return args;
    }

    let mut args = structural_base_args();
    args.extend(launch_args.iter().cloned());
    args
}

fn opencode_run_args_for_probe(launch_args: &[String], prompt: String) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ];
    let mut iter = launch_args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--model" | "-m" | "--agent" | "--log-level" | "--variant" | "--command" | "--file"
            | "-f" | "--title" | "--attach" | "--password" | "-p" | "--username" | "-u" => {
                args.push(arg.clone());
                if let Some(value) = iter.next() {
                    args.push(value.clone());
                }
            }
            "--pure" | "--print-logs" | "--dangerously-skip-permissions" | "--thinking" => {
                args.push(arg.clone());
            }
            "--session" | "-s" | "--dir" | "--port" | "--hostname" | "--mdns-domain" | "--cors" => {
                let _ = iter.next();
            }
            "--continue" | "-c" | "--fork" | "--interactive" | "-i" | "--mdns" => {}
            _ if arg.starts_with("--model=")
                || arg.starts_with("-m=")
                || arg.starts_with("--agent=")
                || arg.starts_with("--log-level=")
                || arg.starts_with("--variant=")
                || arg.starts_with("--command=")
                || arg.starts_with("--file=")
                || arg.starts_with("-f=")
                || arg.starts_with("--title=")
                || arg.starts_with("--attach=")
                || arg.starts_with("--password=")
                || arg.starts_with("-p=")
                || arg.starts_with("--username=")
                || arg.starts_with("-u=") =>
            {
                args.push(arg.clone());
            }
            _ => {}
        }
    }
    args.push(prompt);
    args
}

fn codex_child_network_probe_prompt() -> String {
    "Run exactly this command:\n\n\
         sh -lc 'set -eu; \
         if command -v getent >/dev/null 2>&1; then getent hosts github.com >/dev/null; \
         else python3 -c \"import socket; socket.getaddrinfo(\\\"github.com\\\", 443)\"; fi; \
         if command -v curl >/dev/null 2>&1; then curl -fsSIL --max-time 10 https://github.com >/dev/null; \
         else python3 -c \"import urllib.request; urllib.request.urlopen(\\\"https://github.com\\\", timeout=10).close()\"; fi; \
         printf \"%s%s\\n\" AGENT_DOC_NETWORK _PROBE_OK'"
        .to_string()
}

fn opencode_child_network_probe_prompt() -> String {
    "Run exactly this shell command. Return the command output only.\n\n\
         sh -lc 'set -eu; \
         if command -v getent >/dev/null 2>&1; then getent hosts github.com >/dev/null; \
         else python3 -c \"import socket; socket.getaddrinfo(\\\"github.com\\\", 443)\"; fi; \
         if command -v curl >/dev/null 2>&1; then curl -fsSIL --max-time 10 https://github.com >/dev/null; \
         else python3 -c \"import urllib.request; urllib.request.urlopen(\\\"https://github.com\\\", timeout=10).close()\"; fi; \
         printf \"%s%s\\n\" AGENT_DOC_NETWORK _PROBE_OK'"
        .to_string()
}

fn classify_child_network_probe_failure(detail: &str, harness: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if looks_like_opencode_usage_output(detail) {
        format!("{harness} child probe printed CLI usage/help instead of running")
    } else if lower.contains("could not resolve")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("name or service not known")
        || lower.contains("nodename nor servname provided")
        || lower.contains("getaddrinfo")
    {
        format!("DNS resolution failed inside the {harness} child")
    } else if lower.contains("operation not permitted")
        || lower.contains("network is unreachable")
        || lower.contains("permission denied")
        || lower.contains("eperm")
        || lower.contains("sandbox")
        || lower.contains("network disabled")
    {
        format!("{harness} sandbox/network capability denied outbound access")
    } else if lower.contains("connection refused") {
        format!("remote network service refused the {harness} child connection")
    } else if lower.contains("timed out") || lower.contains("timeout") {
        format!("{harness} child network probe timed out")
    } else {
        format!("{harness} child network probe failed")
    }
}

fn looks_like_opencode_usage_output(output: &str) -> bool {
    let lower = crate::prompt::strip_ansi(output).to_ascii_lowercase();
    (lower.contains("opencode run [message..]")
        && (lower.contains("positionals:") || lower.contains("options:")))
        || lower.contains("unknown argument")
        || lower.contains("unknown option")
}

fn is_text_file_busy(err: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        err.raw_os_error() == Some(libc::ETXTBSY)
    }
    #[cfg(not(unix))]
    {
        let _ = err;
        false
    }
}

fn spawn_agent_command(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    const TEXT_FILE_BUSY_RETRIES: usize = 3;

    for attempt in 0..=TEXT_FILE_BUSY_RETRIES {
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(err) if is_text_file_busy(&err) && attempt < TEXT_FILE_BUSY_RETRIES => {
                std::thread::sleep(Duration::from_millis(25 * (attempt as u64 + 1)));
            }
            Err(err) => return Err(err),
        }
    }

    unreachable!("spawn loop returns on success or final error")
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
    probe_name: &str,
    harness: &str,
) -> Result<std::process::Output> {
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map_err(|e| {
                anyhow::anyhow!("failed to collect {harness} child probe output: {e}")
            });
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output().map_err(|e| {
                anyhow::anyhow!("failed to collect timed-out {harness} child probe output: {e}")
            })?;
            anyhow::bail!(
                "{harness} child {probe_name} probe timed out after {}s; stderr={}",
                timeout.as_secs(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn validate_codex_child_network_probe_output(
    stdout: &str,
    stderr: &str,
    harness: &str,
) -> Result<()> {
    let mut saw_command_execution = false;
    let mut failure_detail: Option<String> = None;

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(item) = json.get("item") else {
            continue;
        };
        if item.get("type").and_then(|value| value.as_str()) != Some("command_execution") {
            continue;
        }

        saw_command_execution = true;
        let command = item
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let output = item
            .get("aggregated_output")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let exit_code = item.get("exit_code").and_then(|value| value.as_i64());
        if exit_code == Some(0) && output.contains(CODEX_CHILD_NETWORK_PROBE_MARKER) {
            return Ok(());
        }

        failure_detail.get_or_insert_with(|| {
            format!(
                "command={command:?} exit_code={} output={}",
                exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                output.trim()
            )
        });
    }

    let detail = failure_detail.unwrap_or_else(|| {
        if saw_command_execution {
            format!("{harness} command execution did not emit the network probe success marker")
        } else {
            format!(
                "{harness} child did not run a command_execution event; stderr={}",
                stderr.trim()
            )
        }
    });
    let classification = classify_child_network_probe_failure(&detail, harness);
    anyhow::bail!("{classification}: {detail}");
}

fn collect_json_strings(value: &serde_json::Value, output: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                output.push(trimmed.to_string());
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_json_strings(value, output);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values() {
                collect_json_strings(value, output);
            }
        }
        _ => {}
    }
}

fn validate_opencode_child_probe_marker_output(
    stdout: &str,
    stderr: &str,
    marker: &str,
    probe_name: &str,
    harness: &str,
) -> Result<()> {
    let combined = format!("{stdout}\n{stderr}");
    if combined.contains(marker) {
        return Ok(());
    }
    if looks_like_opencode_usage_output(&combined) {
        anyhow::bail!(
            "{harness} child probe printed CLI usage/help instead of running the {probe_name} probe: {}",
            combined.trim()
        );
    }

    let mut extracted = Vec::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            collect_json_strings(&value, &mut extracted);
        }
    }
    let detail = if extracted.is_empty() {
        format!(
            "{harness} child did not emit the {probe_name} probe marker; stderr={}",
            stderr.trim()
        )
    } else {
        extracted.join("\n")
    };
    let classification = if probe_name == "network" {
        classify_child_network_probe_failure(&detail, harness)
    } else {
        classify_child_required_ssh_probe_failure(&detail, harness)
    };
    anyhow::bail!("{classification}: {detail}");
}

fn prove_codex_child_network_access(
    command: &str,
    launch_args: &[String],
    env: &std::collections::HashMap<String, String>,
    harness: &str,
    probe_timeout: Duration,
) -> Result<()> {
    let probe_args = codex_exec_args_for_probe(launch_args);
    let codex =
        Codex::new(Some(command.to_string()), Some(probe_args)).with_env(env_map_as_overrides(env));
    let mut cmd = codex.build_command(None, false, None);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = spawn_agent_command(&mut cmd)
        .map_err(|e| anyhow::anyhow!("failed to start {harness} child network probe: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        Codex::write_prompt_to_child(stdin, &codex_child_network_probe_prompt())?;
    }
    child.stdin.take();

    let output = wait_with_timeout(child, probe_timeout, "network", harness)?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let classification = classify_child_network_probe_failure(&detail, harness);
        anyhow::bail!(
            "{classification}: {harness} child probe command exited nonzero: {}",
            detail.trim()
        );
    }

    validate_codex_child_network_probe_output(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        harness,
    )
}

fn prove_opencode_child_network_access(
    command: &str,
    launch_args: &[String],
    env: &std::collections::HashMap<String, String>,
    harness: &str,
    probe_timeout: Duration,
) -> Result<()> {
    let probe_args =
        opencode_run_args_for_probe(launch_args, opencode_child_network_probe_prompt());
    let mut cmd = std::process::Command::new(command);
    cmd.args(&probe_args).env_remove("OPENCODE_CLIENT");
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let output = wait_with_timeout(
        spawn_agent_command(&mut cmd)
            .map_err(|e| anyhow::anyhow!("failed to start {harness} child network probe: {e}"))?,
        probe_timeout,
        "network",
        harness,
    )?;
    if !output.status.success() {
        let detail = format!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let classification = classify_child_network_probe_failure(&detail, harness);
        anyhow::bail!("{classification}: {harness} child probe command exited nonzero: {detail}");
    }
    validate_opencode_child_probe_marker_output(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        CODEX_CHILD_NETWORK_PROBE_MARKER,
        "network",
        harness,
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

const OPENCODE_CHILD_SSH_PROBE_MARKER: &str = "AGENT_DOC_OPENCODE_SSH_PROBE_OK";

fn opencode_child_required_ssh_probe_prompt(targets: &[String]) -> String {
    let targets = targets
        .iter()
        .map(|target| shell_single_quote(target))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "Run exactly this shell command. Return the command output only.\n\n\
         sh -lc 'set -eu; \
         for target do \
           ssh -o BatchMode=yes -o ConnectTimeout=5 -o ControlMaster=no -o ControlPath=none -o ClearAllForwardings=yes -o PermitLocalCommand=no \"$target\" true; \
         done; \
         printf \"%s%s\\n\" AGENT_DOC_OPENCODE_SSH _PROBE_OK' sh {}",
        targets
    )
}

fn classify_child_required_ssh_probe_failure(detail: &str, harness: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if looks_like_opencode_usage_output(detail) {
        format!("{harness} child SSH probe printed CLI usage/help instead of running")
    } else if lower.contains("operation not permitted")
        || lower.contains("socket:")
        || lower.contains("eperm")
        || lower.contains("permission denied")
        || lower.contains("network is unreachable")
        || lower.contains("sandbox")
    {
        format!("SSH unavailable inside managed {harness} pane")
    } else if lower.contains("could not resolve")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("name or service not known")
        || lower.contains("getaddrinfo")
    {
        format!("SSH target resolution failed inside managed {harness} pane")
    } else if lower.contains("timed out") || lower.contains("timeout") {
        format!("SSH probe timed out inside managed {harness} pane")
    } else {
        format!("SSH capability failed inside managed {harness} pane")
    }
}

fn prove_opencode_child_required_ssh(
    command: &str,
    launch_args: &[String],
    env: &std::collections::HashMap<String, String>,
    targets: &[String],
    harness: &str,
) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let probe_args = opencode_run_args_for_probe(
        launch_args,
        opencode_child_required_ssh_probe_prompt(targets),
    );
    let mut cmd = std::process::Command::new(command);
    cmd.args(&probe_args).env_remove("OPENCODE_CLIENT");
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let output = wait_with_timeout(
        spawn_agent_command(&mut cmd)
            .map_err(|e| anyhow::anyhow!("failed to start {harness} child SSH probe: {e}"))?,
        Duration::from_secs(30),
        "ssh",
        harness,
    )?;
    if !output.status.success() {
        let detail = format!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let classification = classify_child_required_ssh_probe_failure(&detail, harness);
        anyhow::bail!(
            "{classification}: {harness} child SSH probe command exited nonzero: {detail}"
        );
    }
    validate_opencode_child_probe_marker_output(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        OPENCODE_CHILD_SSH_PROBE_MARKER,
        "ssh",
        harness,
    )
}

fn codex_child_writable_roots_probe_prompt(roots: &[PathBuf]) -> String {
    let roots = roots
        .iter()
        .map(|root| shell_single_quote(&root.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "Run exactly this command:\n\n\
         sh -lc 'set -eu; \
         for dir do \
           test -d \"$dir\"; \
           probe=\"$dir/.agent-doc-write-probe-$$\"; \
           printf \"%s\" agent-doc > \"$probe\"; \
           rm -f \"$probe\"; \
           if test -f \"$dir/HEAD\" || test -f \"$dir/commondir\" || test -d \"$dir/objects\"; then \
             lock=\"$dir/index.lock\"; \
             set -C; : > \"$lock\"; set +C; \
             rm -f \"$lock\"; \
           fi; \
         done; \
         printf \"{}\\n\"' sh {roots}",
        CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER
    )
}

fn classify_child_writable_root_probe_failure(detail: &str, harness: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("read-only file system")
        || lower.contains("operation not permitted")
        || lower.contains("permission denied")
        || lower.contains("eperm")
        || lower.contains("sandbox")
    {
        format!("{harness} sandbox/write capability denied git metadata access")
    } else if lower.contains("index.lock") || lower.contains("file exists") {
        format!("{harness} child could not create required git metadata lock")
    } else if lower.contains("not a directory") || lower.contains("no such file or directory") {
        format!("{harness} writable-root probe target is missing")
    } else {
        format!("{harness} child writable-root probe failed")
    }
}

fn validate_codex_child_writable_root_probe_output(
    stdout: &str,
    stderr: &str,
    harness: &str,
) -> Result<()> {
    let mut saw_command_execution = false;
    let mut failure_detail: Option<String> = None;

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(item) = json.get("item") else {
            continue;
        };
        if item.get("type").and_then(|value| value.as_str()) != Some("command_execution") {
            continue;
        }

        saw_command_execution = true;
        let command = item
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let output = item
            .get("aggregated_output")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let exit_code = item.get("exit_code").and_then(|value| value.as_i64());
        if exit_code == Some(0) && output.contains(CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER) {
            return Ok(());
        }

        failure_detail.get_or_insert_with(|| {
            format!(
                "command={command:?} exit_code={} output={}",
                exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                output.trim()
            )
        });
    }

    let detail = failure_detail.unwrap_or_else(|| {
        if saw_command_execution {
            format!(
                "{harness} command execution did not emit the writable-root probe success marker"
            )
        } else {
            format!(
                "{harness} child did not run a command_execution event; stderr={}",
                stderr.trim()
            )
        }
    });
    let classification = classify_child_writable_root_probe_failure(&detail, harness);
    anyhow::bail!("{classification}: {detail}");
}

fn prove_codex_child_writable_roots(
    command: &str,
    launch_args: &[String],
    env: &std::collections::HashMap<String, String>,
    roots: &[PathBuf],
    harness: &str,
    probe_timeout: Duration,
) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    let probe_args = codex_exec_args_for_probe(launch_args);
    let codex =
        Codex::new(Some(command.to_string()), Some(probe_args)).with_env(env_map_as_overrides(env));
    let mut cmd = codex.build_command(None, false, None);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = spawn_agent_command(&mut cmd)
        .map_err(|e| anyhow::anyhow!("failed to start {harness} child writable-root probe: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        Codex::write_prompt_to_child(stdin, &codex_child_writable_roots_probe_prompt(roots))?;
    }
    child.stdin.take();

    let output = wait_with_timeout(child, probe_timeout, "writable-root", harness)?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let classification = classify_child_writable_root_probe_failure(&detail, harness);
        anyhow::bail!(
            "{classification}: {harness} child writable-root probe command exited nonzero: {}",
            detail.trim()
        );
    }

    validate_codex_child_writable_root_probe_output(
        &String::from_utf8_lossy(&output.stdout),
        &String::from_utf8_lossy(&output.stderr),
        harness,
    )
}

fn prove_writable_root(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path).map_err(|e| {
        anyhow::anyhow!("writable-root probe could not stat {}: {e}", path.display())
    })?;
    if !metadata.is_dir() {
        anyhow::bail!(
            "writable-root probe expected a directory at {}",
            path.display()
        );
    }
    let probe = path.join(format!(
        ".agent-doc-write-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&probe, b"agent-doc write probe").map_err(|e| {
        anyhow::anyhow!(
            "writable-root probe could not write {}: {e}",
            probe.display()
        )
    })?;
    std::fs::remove_file(&probe).map_err(|e| {
        anyhow::anyhow!(
            "writable-root probe wrote but could not remove {}: {e}",
            probe.display()
        )
    })?;
    if path.join("HEAD").is_file()
        || path.join("commondir").is_file()
        || path.join("objects").is_dir()
    {
        let lock = path.join("index.lock");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|e| {
                anyhow::anyhow!(
                    "git metadata probe could not create {}: {e}",
                    lock.display()
                )
            })?;
        drop(file);
        std::fs::remove_file(&lock).map_err(|e| {
            anyhow::anyhow!(
                "git metadata probe created but could not remove {}: {e}",
                lock.display()
            )
        })?;
    }
    Ok(())
}

pub fn managed_capability_contract_required_for_doc_and_harness(
    file: &Path,
    fm: &Frontmatter,
    global_config: &crate::config::Config,
    harness: &str,
) -> bool {
    if harness == "opencode" {
        return super::resolve_codex_network_access(fm, global_config)
            == CodexNetworkAccess::Enabled
            || !fm.required_ssh_targets.is_empty();
    }
    if harness != "codex" {
        return false;
    }
    super::resolve_codex_network_access(fm, global_config) == CodexNetworkAccess::Enabled
        || !fm.required_ssh_targets.is_empty()
        || !crate::git::workspace_access_dirs_for_doc(file).is_empty()
        || fm.agent_args.as_deref().is_some_and(args_contain_add_dir)
        || fm.codex_args.as_deref().is_some_and(args_contain_add_dir)
        || global_config
            .agent_args
            .as_deref()
            .is_some_and(args_contain_add_dir)
        || global_config
            .codex_args
            .as_deref()
            .is_some_and(args_contain_add_dir)
}

fn args_contain_add_dir(args: &str) -> bool {
    args.split_whitespace()
        .any(|arg| arg == "--add-dir" || arg.starts_with("--add-dir="))
}

pub fn managed_capability_contract_required(
    args: &[String],
    fm: &Frontmatter,
    global_config: &crate::config::Config,
    harness: &str,
) -> bool {
    if harness == "opencode" {
        return super::resolve_codex_network_access(fm, global_config)
            == CodexNetworkAccess::Enabled
            || !fm.required_ssh_targets.is_empty();
    }
    if harness != "codex" {
        return false;
    }
    super::resolve_codex_network_access(fm, global_config) == CodexNetworkAccess::Enabled
        || !fm.required_ssh_targets.is_empty()
        || !add_dirs_from_args(args).is_empty()
}

pub fn prove_managed_session_capabilities(
    command: &str,
    args: &[String],
    env: &std::collections::HashMap<String, String>,
    fm: &Frontmatter,
    global_config: &crate::config::Config,
    harness: &str,
    probe_timeout: Duration,
) -> Result<Option<String>> {
    if !managed_capability_contract_required(args, fm, global_config, harness) {
        return Ok(None);
    }

    let total_start = Instant::now();
    let mut timings = ManagedCapabilityProofTimings::default();
    let network_required =
        super::resolve_codex_network_access(fm, global_config) == CodexNetworkAccess::Enabled;
    let mut network_probe = "not_required";
    if network_required {
        let phase_start = Instant::now();
        prove_dns_resolution()?;
        timings.network_host_dns = Some(phase_start.elapsed());

        let phase_start = Instant::now();
        let cache_key = managed_network_child_proof_cache_key(command, args, env, harness);
        if managed_network_child_proof_is_cached(&cache_key) {
            network_probe = "child_dns_https_cached";
        } else {
            if harness == "opencode" {
                prove_opencode_child_network_access(command, args, env, harness, probe_timeout)?;
            } else {
                prove_codex_child_network_access(command, args, env, harness, probe_timeout)?;
            }
            remember_managed_network_child_proof(cache_key);
            network_probe = "child_dns_https";
        }
        timings.network_child = Some(phase_start.elapsed());
    }

    if !fm.required_ssh_targets.is_empty() {
        let phase_start = Instant::now();
        let codex_env = env_map_as_overrides(env);
        Codex::new(None, None)
            .with_env(codex_env)
            .with_required_ssh_targets(fm.required_ssh_targets.clone())
            .prove_required_ssh_capability()?;
        if harness == "opencode" {
            prove_opencode_child_required_ssh(
                command,
                args,
                env,
                &fm.required_ssh_targets,
                harness,
            )?;
        }
        timings.ssh = Some(phase_start.elapsed());
    }

    let writable_roots = if harness == "codex" {
        add_dirs_from_args(args)
    } else {
        Vec::new()
    };
    let writable_root_contract = writable_root_contract_id(&writable_roots);
    let phase_start = Instant::now();
    for root in &writable_roots {
        prove_writable_root(root)?;
    }
    if !writable_roots.is_empty() {
        timings.writable_launcher = Some(phase_start.elapsed());
    }
    let phase_start = Instant::now();
    prove_codex_child_writable_roots(command, args, env, &writable_roots, harness, probe_timeout)?;
    if !writable_roots.is_empty() {
        timings.writable_child = Some(phase_start.elapsed());
    }
    timings.total = total_start.elapsed();

    Ok(Some(format!(
        "{}_capability_proof status=proven network={} network_probe={} ssh_targets={} writable_roots={}{} {}",
        harness,
        proof_status_label(network_required, network_required),
        network_probe,
        fm.required_ssh_targets.len(),
        normalized_writable_root_strings(&writable_roots).len(),
        writable_root_contract
            .map(|contract| format!(" writable_root_contract={contract}"))
            .unwrap_or_default(),
        timings.event_fields()
    )))
}

pub fn default_base_args() -> Vec<String> {
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
pub fn structural_base_args() -> Vec<String> {
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

    fn should_fresh_start_resume_for_writable_roots(&self, session_id: Option<&str>) -> bool {
        session_id.is_some() && !add_dirs_from_args(&self.base_args).is_empty()
    }

    fn log_writable_root_resume_fresh_start(&self) {
        let count = normalized_writable_root_strings(&add_dirs_from_args(&self.base_args)).len();
        eprintln!(
            "[agent] codex resume session cannot accept the current writable-root contract ({} root{}); starting a fresh `codex exec` session with the full --add-dir set",
            count,
            if count == 1 { "" } else { "s" }
        );
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
                "identityfile" if identity_file.is_none() => identity_file = Some(value),
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
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = spawn_agent_command(&mut cmd)?;
        {
            if let Some(ref mut stdin) = child.stdin {
                Self::write_prompt_to_child(stdin, prompt)?;
            }
            child.stdin.take();
        }
        let output = super::wait_with_output_timeout(child, super::run_agent_timeout())?;

        if !output.status.success() {
            let stderr = filter_codex_stderr_noise(&String::from_utf8_lossy(&output.stderr));
            if looks_like_codex_transport_403_429(&stderr) {
                anyhow::bail!("{}", format_transport_403_429_diagnostic(&stderr));
            }
            anyhow::bail!("codex command failed: {}", stderr.trim());
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let mut thread_id: Option<String> = None;
        let mut response_text: Option<String> = None;
        let mut agent_message_count = 0usize;
        let mut saw_turn_completed = false;
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
                        agent_message_count += 1;
                        response_text = Some(text.to_string());
                    }
                }
                "turn.completed" => {
                    saw_turn_completed = true;
                }
                _ => {}
            }
        }

        if agent_message_count > 1 && !saw_turn_completed {
            anyhow::bail!(
                "ambiguous Codex response: saw {agent_message_count} agent_message items without a turn.completed boundary"
            );
        }

        Ok(ParsedCodexResponse {
            response: AgentResponse {
                text: response_text.unwrap_or_default(),
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
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = spawn_agent_command(&mut cmd)?;

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
        let effective_session_id = if self.should_fresh_start_resume_for_writable_roots(session_id)
        {
            self.log_writable_root_resume_fresh_start();
            None
        } else {
            session_id
        };
        let mut parsed = self.send_once(
            prompt,
            effective_session_id,
            model,
            required_ssh_match_terms,
        )?;
        if session_id.is_some()
            && effective_session_id.is_some()
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
        let effective_session_id = if self.should_fresh_start_resume_for_writable_roots(session_id)
        {
            self.log_writable_root_resume_fresh_start();
            None
        } else {
            session_id
        };
        let process = self.spawn_stream_process(prompt, effective_session_id, model)?;

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
            allow_resume_capability_retry: effective_session_id.is_some(),
            retried_fresh: false,
            yielded_agent_content: false,
            saw_final_chunk: false,
            buffered_chunks: VecDeque::new(),
            buffer_required_ssh_chunks: effective_session_id.is_some() && required_ssh.is_some(),
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
                    let stderr = filter_codex_stderr_noise(&self.collect_stderr());
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
                        if looks_like_codex_transport_403_429(&stderr) {
                            self.done = true;
                            return Some(Err(anyhow::anyhow!(
                                "{}",
                                format_transport_403_429_diagnostic(&stderr)
                            )));
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
    fn text_file_busy_detection_matches_unix_os_error() {
        #[cfg(unix)]
        assert!(is_text_file_busy(&std::io::Error::from_raw_os_error(
            libc::ETXTBSY
        )));
        assert!(!is_text_file_busy(&std::io::Error::from(
            std::io::ErrorKind::NotFound
        )));
    }

    fn init_repo(root: &Path) {
        let init = Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        for (key, value) in [("user.email", "test@test.com"), ("user.name", "Test")] {
            let config = Command::new("git")
                .current_dir(root)
                .args(["config", key, value])
                .output()
                .unwrap();
            assert!(
                config.status.success(),
                "git config failed: {}",
                String::from_utf8_lossy(&config.stderr)
            );
        }
    }

    fn commit_file(root: &Path, rel: &str, content: &str, message: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        let add = Command::new("git")
            .current_dir(root)
            .args(["add", rel])
            .output()
            .unwrap();
        assert!(
            add.status.success(),
            "git add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        );
        let commit = Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", message, "--no-verify"])
            .output()
            .unwrap();
        assert!(
            commit.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
    }

    fn add_submodule(repo: &Path, origin: &Path, target: &str) {
        let url = format!("file://{}", origin.display());
        let add = Command::new("git")
            .current_dir(repo)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &url,
                target,
            ])
            .output()
            .unwrap();
        assert!(
            add.status.success(),
            "git submodule add failed: {}",
            String::from_utf8_lossy(&add.stderr)
        );
        let commit = Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();
        assert!(
            commit.status.success(),
            "git commit submodule failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
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
    fn codex_stderr_filter_drops_marketplace_manifest_noise_only() {
        let stderr = "\
2026-05-04T02:58:49Z WARN codex_core_plugins::manifest: ignoring interface.defaultPrompt: prompt must be at most 128 characters path=/home/brian/.codex/.tmp/plugins/plugins/build-ios-apps/.codex-plugin/plugin.json
2026-05-04T02:58:49Z WARN codex_core_skills::loader: ignoring interface.icon_small: icon path must not contain '..'
2026-05-04T02:58:49Z WARN codex_core_skills::loader: ignoring interface.icon_large: icon path must not contain '..'
real stderr
";

        let filtered = filter_codex_stderr_noise(stderr);
        let report = codex_stderr_noise_report(stderr);

        assert_eq!(filtered, "real stderr\n");
        assert_eq!(report.filtered, "real stderr\n");
        assert_eq!(report.suppressed_marketplace_manifest_warnings, 3);
    }

    #[test]
    fn codex_stderr_filter_keeps_local_plugin_manifest_warnings() {
        let stderr = "WARN codex_core_plugins::manifest: ignoring interface.defaultPrompt: prompt must be at most 128 characters path=/home/brian/work/btakita/agent-loop/src/agent-doc/.codex-plugin/plugin.json\n";

        let filtered = filter_codex_stderr_noise(stderr);
        let report = codex_stderr_noise_report(stderr);

        assert_eq!(filtered, stderr);
        assert_eq!(report.suppressed_marketplace_manifest_warnings, 0);
    }

    #[test]
    fn looks_like_codex_transport_403_429_detects_ws_403() {
        assert!(looks_like_codex_transport_403_429(
            "403 Forbidden on wss://chatgpt.com/backend-api/codex/responses"
        ));
        assert!(looks_like_codex_transport_403_429(
            "WebSocket handshake failed: 403"
        ));
    }

    #[test]
    fn looks_like_codex_transport_403_429_detects_https_429() {
        assert!(looks_like_codex_transport_403_429("429 Too Many Requests"));
        assert!(looks_like_codex_transport_403_429(
            "rate limit exceeded 429"
        ));
    }

    #[test]
    fn looks_like_codex_transport_403_429_rejects_unrelated() {
        assert!(!looks_like_codex_transport_403_429("sandbox violation"));
        assert!(!looks_like_codex_transport_403_429("permission denied"));
    }

    #[test]
    fn format_transport_403_429_diagnostic_names_both_rejections() {
        let msg = format_transport_403_429_diagnostic(
            "403 on wss://example.com then 429 Too Many Requests",
        );
        assert!(msg.contains("403 Forbidden on WebSocket"));
        assert!(msg.contains("429 Too Many Requests"));
        assert!(msg.contains("restart the codex session"));
    }

    #[test]
    fn send_surfaces_transport_403_429_diagnostic() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo >&2 '403 Forbidden on wss://chatgpt.com/backend-api/codex/responses'; exit 1".into(),
            ]),
        );
        let result = codex.send("ignored", None, false, None);
        assert!(
            result.is_err(),
            "expected error for 403 transport rejection"
        );
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("transport rejection"), "got: {msg}");
        assert!(msg.contains("403 Forbidden"), "got: {msg}");
    }

    #[test]
    fn streaming_surfaces_transport_403_429_diagnostic() {
        let codex = Codex::new(
            Some("bash".into()),
            Some(vec![
                "-c".into(),
                "cat >/dev/null; echo >&2 '429 Too Many Requests'; exit 1".into(),
            ]),
        );
        let iter = codex.send_streaming("ignored", None, false, None).unwrap();
        let chunks: Vec<_> = iter.collect();
        assert_eq!(chunks.len(), 1);
        let err = chunks[0].as_ref().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("transport rejection"), "got: {msg}");
        assert!(msg.contains("429"), "got: {msg}");
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
    fn managed_capability_contract_requires_network_ssh_or_writable_roots() {
        let config = crate::config::Config::default();
        let mut fm = Frontmatter::default();
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(!managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));

        fm.codex_network_access = Some(CodexNetworkAccess::Enabled);
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));

        fm.codex_network_access = None;
        fm.required_ssh_targets = vec!["example-host".to_string()];
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "codex"
        ));
        assert!(managed_capability_contract_required(
            &[],
            &fm,
            &config,
            "opencode"
        ));

        fm.required_ssh_targets.clear();
        assert!(managed_capability_contract_required(
            &[
                "exec".to_string(),
                "--json".to_string(),
                "--add-dir".to_string(),
                "/tmp/example".to_string()
            ],
            &fm,
            &config,
            "codex"
        ));
        assert!(!managed_capability_contract_required(
            &[
                "exec".to_string(),
                "--json".to_string(),
                "--add-dir".to_string(),
                "/tmp/example".to_string()
            ],
            &fm,
            &config,
            "opencode"
        ));
    }

    #[test]
    fn managed_network_child_proof_cache_key_includes_environment() {
        let mut first_env = std::collections::HashMap::new();
        first_env.insert("HTTPS_PROXY".to_string(), "http://proxy-a".to_string());
        let mut second_env = std::collections::HashMap::new();
        second_env.insert("HTTPS_PROXY".to_string(), "http://proxy-b".to_string());

        let args = vec!["exec".to_string(), "--json".to_string()];
        let first = managed_network_child_proof_cache_key("codex", &args, &first_env, "codex");
        let second = managed_network_child_proof_cache_key("codex", &args, &second_env, "codex");

        assert_ne!(first, second);
        assert!(!managed_network_child_proof_is_cached(&first));
        remember_managed_network_child_proof(first.clone());
        assert!(managed_network_child_proof_is_cached(&first));
    }

    #[test]
    fn opencode_probe_args_use_run_format_json_and_preserve_safe_flags() {
        let args = opencode_run_args_for_probe(
            &[
                "--model".to_string(),
                "zai/glm-5".to_string(),
                "--continue".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--dir".to_string(),
                "/tmp/project".to_string(),
            ],
            "probe".to_string(),
        );

        assert_eq!(
            args,
            vec![
                "run",
                "--format",
                "json",
                "--model",
                "zai/glm-5",
                "--dangerously-skip-permissions",
                "probe"
            ]
        );
    }

    #[test]
    fn opencode_probe_args_drop_tui_only_prompt_flag_and_preserve_run_command_flag() {
        let args = opencode_run_args_for_probe(
            &[
                "--prompt".to_string(),
                "tui-only".to_string(),
                "--command".to_string(),
                "session".to_string(),
                "--file".to_string(),
                "SPEC.md".to_string(),
            ],
            "probe".to_string(),
        );

        assert_eq!(
            args,
            vec![
                "run",
                "--format",
                "json",
                "--command",
                "session",
                "--file",
                "SPEC.md",
                "probe"
            ]
        );
    }

    #[test]
    fn opencode_child_probe_classifies_cli_usage_separately_from_network_failure() {
        let err = validate_opencode_child_probe_marker_output(
            "opencode run [message..]\n\nPositionals:\n  message\n\nOptions:\n  --format\n",
            "",
            CODEX_CHILD_NETWORK_PROBE_MARKER,
            "network",
            "OpenCode",
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(
            message.contains("printed CLI usage/help instead of running the network probe"),
            "{message}"
        );
        assert!(
            !message.contains("sandbox/network capability denied outbound access"),
            "{message}"
        );
    }

    #[test]
    fn opencode_child_probe_classifies_socket_eperm_as_ssh_sandbox_denial() {
        let err = validate_opencode_child_probe_marker_output(
            r#"{"type":"message","text":"ssh sampleorders-server true\nsocket: Operation not permitted"}"#,
            "",
            OPENCODE_CHILD_SSH_PROBE_MARKER,
            "ssh",
            "OpenCode",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("SSH unavailable inside managed OpenCode pane"),
            "{err}"
        );
    }

    #[test]
    fn managed_capability_proof_runs_opencode_child_ssh_probe() {
        let (_ssh_dir, path_dir) = write_fake_ssh_script(
            r#"#!/bin/sh
if [ "$1" = "-G" ]; then
  echo "user root"
  echo "hostname $2"
  echo "port 22"
  exit 0
fi
exit 0
"#,
        );
        let (_opencode_dir, opencode) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
printf '%s\n' '{{"type":"message","text":"{}\n"}}'
"#,
            OPENCODE_CHILD_SSH_PROBE_MARKER
        ));
        let mut env = std::collections::HashMap::new();
        let old_path = std::env::var("PATH").unwrap_or_default();
        env.insert("PATH".to_string(), format!("{path_dir}:{old_path}"));
        let mut fm = Frontmatter::default();
        fm.required_ssh_targets = vec!["sampleorders-server".to_string()];

        let event = prove_managed_session_capabilities(
            &opencode,
            &[
                "--model".to_string(),
                "zai/glm-5".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
            &env,
            &fm,
            &crate::config::Config::default(),
            "opencode",
            agent_doc_turn_executor::capability_proof::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap()
        .unwrap();

        assert!(event.contains("opencode_capability_proof status=proven"));
        assert!(event.contains("ssh_targets=1"));
        assert!(event.contains("writable_roots=0"));
    }

    #[test]
    fn managed_capability_proof_checks_writable_add_dirs() {
        let dir = TempDir::new().unwrap();
        let (_script_dir, script) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{{"type":"thread.started","thread_id":"probe-thread"}}'
printf '%s\n' '{{"type":"item.completed","item":{{"type":"command_execution","command":"sh -lc probe","aggregated_output":"{}\n","exit_code":0}}}}'
printf '%s\n' '{{"type":"turn.completed","usage":{{}}}}'
"#,
            CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER
        ));
        let args = vec![
            "exec".to_string(),
            "--json".to_string(),
            "--add-dir".to_string(),
            dir.path().to_string_lossy().into_owned(),
        ];
        let fm = Frontmatter::default();
        let env = std::collections::HashMap::new();

        let event = prove_managed_session_capabilities(
            &script,
            &args,
            &env,
            &fm,
            &crate::config::Config::default(),
            "codex",
            agent_doc_turn_executor::capability_proof::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap()
        .unwrap();

        assert!(event.contains("codex_capability_proof status=proven"));
        assert!(event.contains("writable_roots=1"));
        assert!(event.contains("timings_ms="), "{event}");
        assert!(event.contains("writable_launcher:"), "{event}");
        assert!(event.contains("writable_child:"), "{event}");
    }

    #[test]
    fn managed_capability_proof_records_writable_root_contract() {
        let dir = TempDir::new().unwrap();
        let (_script_dir, script) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{{"type":"thread.started","thread_id":"probe-thread"}}'
printf '%s\n' '{{"type":"item.completed","item":{{"type":"command_execution","command":"sh -lc probe","aggregated_output":"{}\n","exit_code":0}}}}'
printf '%s\n' '{{"type":"turn.completed","usage":{{}}}}'
"#,
            CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER
        ));
        let root = dir.path().canonicalize().unwrap();
        let args = vec![
            "exec".to_string(),
            "--json".to_string(),
            "--add-dir".to_string(),
            root.to_string_lossy().into_owned(),
        ];
        let fm = Frontmatter::default();
        let env = std::collections::HashMap::new();
        let expected_contract = writable_root_contract_id(&[root]).unwrap();

        let event = prove_managed_session_capabilities(
            &script,
            &args,
            &env,
            &fm,
            &crate::config::Config::default(),
            "codex",
            agent_doc_turn_executor::capability_proof::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap()
        .unwrap();

        assert!(
            event.contains(&format!("writable_root_contract={expected_contract}")),
            "{event}"
        );
    }

    #[test]
    fn send_fresh_exec_when_resume_has_writable_add_dirs() {
        let dir = TempDir::new().unwrap();
        let (_script_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
if [ "${2:-}" = "resume" ]; then
  printf '%s\n' 'resume path must not be used when --add-dir roots are required' >&2
  exit 44
fi
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"fresh response"}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#,
        );
        let codex = Codex::new(
            Some(script),
            Some(vec![
                "exec".to_string(),
                "--json".to_string(),
                "--add-dir".to_string(),
                dir.path().to_string_lossy().into_owned(),
            ]),
        );

        let response = codex
            .send("prompt", Some("resume-123"), false, None)
            .unwrap();

        assert_eq!(response.text, "fresh response");
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn send_selects_last_agent_message_as_final_closeout() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"I am checking the document now.\n"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"make check","aggregated_output":"ok\n","exit_code":0,"status":"completed"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"<!-- patch:exchange -->\n### Re: final — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->"}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#,
        );
        let codex = Codex::new(Some(script), None);

        let response = codex.send("prompt", None, false, None).unwrap();

        assert_eq!(
            response.text,
            "<!-- patch:exchange -->\n### Re: final — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->"
        );
        assert!(
            !response.text.contains("I am checking"),
            "progress chatter must not be captured in patchback: {}",
            response.text
        );
        assert_eq!(response.session_id.as_deref(), Some("fresh-thread"));
    }

    #[test]
    fn send_rejects_multiple_agent_messages_without_final_boundary() {
        let (_dir, script) = write_fake_codex_script(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-1","type":"agent_message","text":"progress"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"possible final"}}'
"#,
        );
        let codex = Codex::new(Some(script), None);

        let err = match codex.send("prompt", None, false, None) {
            Ok(response) => panic!("expected ambiguous response error, got {}", response.text),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("ambiguous Codex response"),
            "got: {err}"
        );
    }

    #[test]
    fn managed_capability_contract_for_doc_requires_auto_submodule_gitdirs() {
        let outer_dir = TempDir::new().unwrap();
        let outer = outer_dir.path();
        init_repo(outer);
        commit_file(outer, "README.md", "# outer\n", "init outer");

        let sub_origin_dir = TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        init_repo(sub_origin);
        commit_file(sub_origin, "README.md", "# sub\n", "init sub");

        add_submodule(outer, sub_origin, "src/sub");
        let doc = outer.join("src/sub/tasks/session.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, "test\n").unwrap();

        let fm = Frontmatter::default();
        assert!(managed_capability_contract_required_for_doc_and_harness(
            &doc,
            &fm,
            &crate::config::Config::default(),
            "codex"
        ));
    }

    #[test]
    fn codex_child_writable_probe_classifies_read_only_gitdir_lock() {
        let stdout = "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"sh: cannot create /repo/.git/modules/sub/index.lock: Read-only file system\\n\",\"exit_code\":1}}\n";

        let err = validate_codex_child_writable_root_probe_output(stdout, "", "Codex").unwrap_err();
        assert!(
            err.to_string()
                .contains("Codex sandbox/write capability denied git metadata access"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn codex_network_probe_exec_args_wrap_interactive_launch_args() {
        let args = codex_exec_args_for_probe(&[
            "-s".to_string(),
            "danger-full-access".to_string(),
            "--add-dir".to_string(),
            "/tmp/repo".to_string(),
        ]);

        assert_eq!(
            args,
            vec![
                "exec",
                "--json",
                "-s",
                "danger-full-access",
                "--add-dir",
                "/tmp/repo",
            ]
        );
    }

    #[test]
    fn codex_child_network_probe_requires_command_marker() {
        let stdout = format!(
            "{{\"type\":\"item.completed\",\"item\":{{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"{}\\n\",\"exit_code\":0}}}}\n",
            CODEX_CHILD_NETWORK_PROBE_MARKER
        );

        validate_codex_child_network_probe_output(&stdout, "", "codex").unwrap();
    }

    #[test]
    fn codex_child_network_probe_classifies_sandbox_denial() {
        let stdout = "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"socket: Operation not permitted\\n\",\"exit_code\":1}}\n";

        let err = validate_codex_child_network_probe_output(stdout, "", "Codex").unwrap_err();
        assert!(
            err.to_string()
                .contains("Codex sandbox/network capability denied outbound access"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn opencode_child_network_probe_classifies_sandbox_denial() {
        let stdout = "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"sh -lc probe\",\"aggregated_output\":\"socket: Operation not permitted\\n\",\"exit_code\":1}}\n";

        let err = validate_codex_child_network_probe_output(stdout, "", "OpenCode").unwrap_err();
        assert!(
            err.to_string()
                .contains("OpenCode sandbox/network capability denied outbound access"),
            "unexpected error: {err:#}"
        );
        assert!(
            !err.to_string().contains("Codex"),
            "should not contain Codex: {err:#}"
        );
    }

    #[test]
    fn prove_codex_child_network_access_runs_fake_codex_child_probe() {
        let (_dir, script) = write_fake_codex_script(&format!(
            r#"#!/bin/sh
cat >/dev/null
printf '%s\n' '{{"type":"thread.started","thread_id":"probe-thread"}}'
printf '%s\n' '{{"type":"item.completed","item":{{"type":"command_execution","command":"sh -lc probe","aggregated_output":"{}\n","exit_code":0}}}}'
printf '%s\n' '{{"type":"turn.completed","usage":{{}}}}'
"#,
            CODEX_CHILD_NETWORK_PROBE_MARKER
        ));
        let env = std::collections::HashMap::new();

        prove_codex_child_network_access(
            &script,
            &["-s".to_string(), "danger-full-access".to_string()],
            &env,
            "codex",
            agent_doc_turn_executor::capability_proof::DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        )
        .unwrap();
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
  *sampleorders-server*)
    echo "ssh: Could not resolve hostname sampleorders-server: Name or service not known" >&2
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
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

        let err = codex
            .prove_required_ssh_capability()
            .unwrap_err()
            .to_string();
        assert!(err.contains("sampleorders-server"), "got: {err}");
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
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

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
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh sampleorders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["sampleorders-server".to_string()]),
            Some(
                "command `ssh sampleorders-server true`: socket: Operation not permitted"
                    .to_string()
            )
        );
    }

    #[test]
    fn required_ssh_failure_ignores_bare_socket_eperm_without_ssh_command_context() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"chromium-bridge list","aggregated_output":"socket: Operation not permitted","exit_code":1,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["sampleorders-server".to_string()]),
            None
        );
    }

    #[test]
    fn required_ssh_failure_ignores_historical_capture_grep_output() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"rg 'Operation not permitted' .agent-doc/captures","aggregated_output":".agent-doc/captures/old/cycle.json:16: \"response_body\": \"required SSH capability failed for target(s) sampleorders-server: socket: Operation not permitted\"","exit_code":0,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(line, &["sampleorders-server".to_string()]),
            None
        );
    }

    #[test]
    fn required_ssh_failure_detects_direct_ssh_diagnostic_without_command_field() {
        let line = r#"{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","aggregated_output":"ssh: connect to host 50.28.2.199 port 22: Operation not permitted","exit_code":255,"status":"completed"}}"#;

        assert_eq!(
            transcript_has_required_ssh_failure(
                line,
                &["sampleorders-server".to_string(), "50.28.2.199".to_string()]
            ),
            Some("ssh: connect to host 50.28.2.199 port 22: Operation not permitted".to_string())
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
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh sampleorders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
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
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

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
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

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
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

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
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh sampleorders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
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
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

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
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh sampleorders-server true","aggregated_output":"socket: Operation not permitted","exit_code":255,"status":"completed"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
else
  printf '%s\n' '{"type":"thread.started","thread_id":"fresh-thread"}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-2","type":"command_execution","command":"ssh sampleorders-server true","aggregated_output":"","exit_code":0,"status":"completed"}}'
  printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"fresh response"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{}}'
fi
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

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
printf '%s\n' '{"type":"item.completed","item":{"id":"cmd-1","type":"command_execution","command":"ssh sampleorders-server true","aggregated_output":"","exit_code":0,"status":"completed"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"msg-2","type":"agent_message","text":"SSH worked."}}'
printf '%s\n' '{"type":"turn.completed","usage":{}}'
"#,
        );
        let codex = Codex::new(Some(script), None)
            .with_env(vec![("PATH".to_string(), Some(path_dir))])
            .with_required_ssh_targets(vec!["sampleorders-server".to_string()]);

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
