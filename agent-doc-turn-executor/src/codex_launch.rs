//! Pure Codex launch/restart argument policy.
//!
//! Orchestration chooses when to restart a Codex turn executor. This module owns
//! the harness-specific argument transformation for `codex resume`, which has a
//! narrower CLI surface than the original `codex exec` launch.

use agent_doc_frontmatter::frontmatter::CodexNetworkAccess;
use anyhow::Result;
use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

pub const CODEX_SANDBOX_NETWORK_DISABLED_ENV: &str = "CODEX_SANDBOX_NETWORK_DISABLED";
pub const CODEX_CHILD_NETWORK_PROBE_MARKER: &str = "AGENT_DOC_NETWORK_PROBE_OK";
pub const CODEX_CHILD_WRITABLE_ROOT_PROBE_MARKER: &str = "AGENT_DOC_WRITABLE_ROOT_PROBE_OK";
pub const OPENCODE_CHILD_SSH_PROBE_MARKER: &str = "AGENT_DOC_OPENCODE_SSH_PROBE_OK";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexResumeRestartArgsError {
    ConflictingSandboxModes { existing: String, requested: String },
    MissingSandboxMode { flag: String },
    MissingConfigValue { flag: String },
}

impl fmt::Display for CodexResumeRestartArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingSandboxModes {
                existing,
                requested,
            } => write!(
                f,
                "Codex launch policy mismatch: resume args contain conflicting sandbox modes \
                 `{existing}` and `{requested}`. Refusing to resume because this could silently \
                 downgrade the requested sandbox before task work starts."
            ),
            Self::MissingSandboxMode { flag } => write!(
                f,
                "Codex launch policy mismatch: `{flag}` was provided without a sandbox mode. \
                 Refusing to resume because the session could fall back to the Codex default \
                 sandbox."
            ),
            Self::MissingConfigValue { flag } => write!(
                f,
                "Codex launch policy mismatch: `{flag}` was provided without a value."
            ),
        }
    }
}

impl Error for CodexResumeRestartArgsError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexNetworkPolicyStatus {
    pub access: CodexNetworkAccess,
    pub parent_disabled: bool,
    pub effective_disabled: bool,
    pub sandbox_mode: Option<String>,
}

impl CodexNetworkPolicyStatus {
    pub fn summary(&self) -> String {
        let effective = if self.effective_disabled {
            "disabled"
        } else {
            "enabled"
        };
        let requested = match self.access {
            CodexNetworkAccess::Inherit => "inherit",
            CodexNetworkAccess::Enabled => "enabled",
            CodexNetworkAccess::Disabled => "disabled",
        };
        let detail = match self.access {
            CodexNetworkAccess::Inherit if self.parent_disabled => {
                "inherited CODEX_SANDBOX_NETWORK_DISABLED=1".to_string()
            }
            CodexNetworkAccess::Inherit => {
                "no inherited CODEX_SANDBOX_NETWORK_DISABLED override".to_string()
            }
            CodexNetworkAccess::Enabled if self.parent_disabled => {
                "agent-doc removed inherited CODEX_SANDBOX_NETWORK_DISABLED=1".to_string()
            }
            CodexNetworkAccess::Enabled => {
                "agent-doc forced CODEX_SANDBOX_NETWORK_DISABLED off".to_string()
            }
            CodexNetworkAccess::Disabled => {
                "agent-doc forced CODEX_SANDBOX_NETWORK_DISABLED=1".to_string()
            }
        };
        match self.sandbox_mode.as_deref() {
            Some(mode) => format!("{effective} (policy: {requested}, sandbox: {mode}; {detail})"),
            None => format!("{effective} (policy: {requested}; {detail})"),
        }
    }

    pub fn mismatch_error(&self) -> Option<String> {
        match self.access {
            CodexNetworkAccess::Enabled if self.effective_disabled => Some(format!(
                "Codex launch policy mismatch: `codex_network_access: enabled` should remove \
                 `{}`, but the effective child env is still network-disabled.",
                CODEX_SANDBOX_NETWORK_DISABLED_ENV
            )),
            CodexNetworkAccess::Disabled if !self.effective_disabled => Some(format!(
                "Codex launch policy mismatch: `codex_network_access: disabled` should set \
                 `{}` to `1`, but the effective child env is still network-enabled.",
                CODEX_SANDBOX_NETWORK_DISABLED_ENV
            )),
            CodexNetworkAccess::Inherit
                if self.effective_disabled
                    && self.sandbox_mode.as_deref() == Some("danger-full-access") =>
            {
                Some(format!(
                    "Codex launch policy mismatch: sandbox is `danger-full-access`, but network \
                     is still disabled by inherited {}=1. Set `codex_network_access: enabled` \
                     (or `codex_network_access = \"enabled\"` in config) so agent-doc removes \
                     that launcher override, or remove the env var before starting the session.",
                    CODEX_SANDBOX_NETWORK_DISABLED_ENV
                ))
            }
            _ => None,
        }
    }
}

pub fn resolve_codex_network_access(
    frontmatter_access: Option<CodexNetworkAccess>,
    config_access: Option<CodexNetworkAccess>,
) -> CodexNetworkAccess {
    frontmatter_access.or(config_access).unwrap_or_default()
}

pub fn apply_codex_network_access_env_map(
    env: &mut HashMap<String, String>,
    access: CodexNetworkAccess,
) {
    match access {
        CodexNetworkAccess::Inherit => {}
        CodexNetworkAccess::Enabled => {
            env.remove(CODEX_SANDBOX_NETWORK_DISABLED_ENV);
        }
        CodexNetworkAccess::Disabled => {
            env.insert(
                CODEX_SANDBOX_NETWORK_DISABLED_ENV.to_string(),
                "1".to_string(),
            );
        }
    }
}

pub fn apply_codex_network_access_env_overrides(
    env: &mut Vec<(String, Option<String>)>,
    access: CodexNetworkAccess,
) {
    match access {
        CodexNetworkAccess::Inherit => {}
        CodexNetworkAccess::Enabled => {
            env.push((CODEX_SANDBOX_NETWORK_DISABLED_ENV.to_string(), None))
        }
        CodexNetworkAccess::Disabled => env.push((
            CODEX_SANDBOX_NETWORK_DISABLED_ENV.to_string(),
            Some("1".to_string()),
        )),
    }
}

pub fn codex_network_status_from_env_map(
    args: &[String],
    access: CodexNetworkAccess,
    parent_disabled: bool,
    env: &HashMap<String, String>,
) -> CodexNetworkPolicyStatus {
    CodexNetworkPolicyStatus {
        access,
        parent_disabled,
        effective_disabled: env
            .get(CODEX_SANDBOX_NETWORK_DISABLED_ENV)
            .is_some_and(|value| value == "1"),
        sandbox_mode: codex_sandbox_mode_from_args(args),
    }
}

pub fn codex_network_status_from_overrides(
    args: &[String],
    access: CodexNetworkAccess,
    parent_disabled: bool,
    overrides: &[(String, Option<String>)],
) -> CodexNetworkPolicyStatus {
    let mut effective_disabled = parent_disabled;
    for (key, value) in overrides {
        if key == CODEX_SANDBOX_NETWORK_DISABLED_ENV {
            effective_disabled = value.as_deref() == Some("1");
        }
    }
    CodexNetworkPolicyStatus {
        access,
        parent_disabled,
        effective_disabled,
        sandbox_mode: codex_sandbox_mode_from_args(args),
    }
}

fn parse_sandbox_mode_config(value: &str) -> Option<String> {
    let raw = value.trim();
    let mode = raw.strip_prefix("sandbox_mode=")?;
    let mode = mode.trim().trim_matches(|c| c == '"' || c == '\'');
    if mode.is_empty() {
        None
    } else {
        Some(mode.to_string())
    }
}

fn codex_sandbox_mode_from_args(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-s" | "--sandbox" => {
                if let Some(mode) = iter.next() {
                    return Some(mode.clone());
                }
            }
            "-c" | "--config" => {
                if let Some(value) = iter.next()
                    && let Some(mode) = parse_sandbox_mode_config(value)
                {
                    return Some(mode);
                }
            }
            _ => {
                if let Some(mode) = arg.strip_prefix("--sandbox=") {
                    return Some(mode.to_string());
                }
                if let Some(value) = arg.strip_prefix("--config=")
                    && let Some(mode) = parse_sandbox_mode_config(value)
                {
                    return Some(mode);
                }
            }
        }
    }
    None
}

fn record_codex_resume_sandbox_mode(
    seen: &mut Option<String>,
    mode: &str,
) -> Result<(), CodexResumeRestartArgsError> {
    if let Some(existing) = seen
        && existing != mode
    {
        return Err(CodexResumeRestartArgsError::ConflictingSandboxModes {
            existing: existing.clone(),
            requested: mode.to_string(),
        });
    }
    *seen = Some(mode.to_string());
    Ok(())
}

fn push_codex_resume_sandbox_config(
    args: &mut Vec<String>,
    seen_sandbox_mode: &mut Option<String>,
    mode: &str,
) -> Result<(), CodexResumeRestartArgsError> {
    record_codex_resume_sandbox_mode(seen_sandbox_mode, mode)?;
    args.push("-c".to_string());
    args.push(format!("sandbox_mode={mode:?}"));
    Ok(())
}

pub fn codex_resume_restart_args(
    prefix: &[String],
    base_args: &[String],
) -> Result<Vec<String>, CodexResumeRestartArgsError> {
    let mut args = prefix.to_vec();
    let mut base = base_args.iter().peekable();
    let mut seen_sandbox_mode: Option<String> = None;
    while let Some(arg) = base.next() {
        match arg.as_str() {
            "exec" | "--json" => {}
            "-s" | "--sandbox" => {
                let Some(mode) = base.next() else {
                    return Err(CodexResumeRestartArgsError::MissingSandboxMode {
                        flag: arg.clone(),
                    });
                };
                push_codex_resume_sandbox_config(&mut args, &mut seen_sandbox_mode, mode)?;
            }
            "--add-dir" => {
                // `codex resume` does not accept --add-dir. A resumed session must inherit
                // writable roots from the original fresh launch.
                let _ = base.next();
            }
            "-c" | "--config" => {
                let Some(value) = base.next() else {
                    return Err(CodexResumeRestartArgsError::MissingConfigValue {
                        flag: arg.clone(),
                    });
                };
                if let Some(mode) = parse_sandbox_mode_config(value) {
                    record_codex_resume_sandbox_mode(&mut seen_sandbox_mode, &mode)?;
                }
                args.push(arg.clone());
                args.push(value.clone());
            }
            _ if arg.starts_with("--sandbox=") => {
                let mode = &arg["--sandbox=".len()..];
                push_codex_resume_sandbox_config(&mut args, &mut seen_sandbox_mode, mode)?;
            }
            _ if arg.starts_with("--add-dir=") => {
                // Same as --add-dir <DIR> above.
            }
            _ if arg.starts_with("--config=") => {
                let value = &arg["--config=".len()..];
                if let Some(mode) = parse_sandbox_mode_config(value) {
                    record_codex_resume_sandbox_mode(&mut seen_sandbox_mode, &mode)?;
                }
                args.push(arg.clone());
            }
            _ => {
                args.push(arg.clone());
            }
        }
    }
    Ok(args)
}

pub fn codex_exec_args_for_probe(launch_args: &[String]) -> Vec<String> {
    if launch_args.first().is_some_and(|arg| arg == "exec") {
        let mut args = launch_args.to_vec();
        if !args.iter().any(|arg| arg == "--json") {
            args.insert(1, "--json".to_string());
        }
        return args;
    }

    let mut args = vec!["exec".to_string(), "--json".to_string()];
    args.extend(launch_args.iter().cloned());
    args
}

pub fn opencode_run_args_for_probe(launch_args: &[String], prompt: String) -> Vec<String> {
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

pub fn codex_child_network_probe_prompt() -> String {
    "Run exactly this command:\n\n\
         sh -lc 'set -eu; \
         if command -v getent >/dev/null 2>&1; then getent hosts github.com >/dev/null; \
         else python3 -c \"import socket; socket.getaddrinfo(\\\"github.com\\\", 443)\"; fi; \
         if command -v curl >/dev/null 2>&1; then curl -fsSIL --max-time 10 https://github.com >/dev/null; \
         else python3 -c \"import urllib.request; urllib.request.urlopen(\\\"https://github.com\\\", timeout=10).close()\"; fi; \
         printf \"%s%s\\n\" AGENT_DOC_NETWORK _PROBE_OK'"
        .to_string()
}

pub fn opencode_child_network_probe_prompt() -> String {
    "Run exactly this shell command. Return the command output only.\n\n\
         sh -lc 'set -eu; \
         if command -v getent >/dev/null 2>&1; then getent hosts github.com >/dev/null; \
         else python3 -c \"import socket; socket.getaddrinfo(\\\"github.com\\\", 443)\"; fi; \
         if command -v curl >/dev/null 2>&1; then curl -fsSIL --max-time 10 https://github.com >/dev/null; \
         else python3 -c \"import urllib.request; urllib.request.urlopen(\\\"https://github.com\\\", timeout=10).close()\"; fi; \
         printf \"%s%s\\n\" AGENT_DOC_NETWORK _PROBE_OK'"
        .to_string()
}

fn strip_ansi_for_probe_output(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == 0x1b && bytes.get(idx + 1) == Some(&b'[') {
            idx += 2;
            while idx < bytes.len() {
                let byte = bytes[idx];
                idx += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
        } else {
            out.push(bytes[idx] as char);
            idx += 1;
        }
    }
    out
}

pub fn looks_like_opencode_usage_output(output: &str) -> bool {
    let lower = strip_ansi_for_probe_output(output).to_ascii_lowercase();
    (lower.contains("opencode run [message..]")
        && (lower.contains("positionals:") || lower.contains("options:")))
        || lower.contains("unknown argument")
        || lower.contains("unknown option")
}

pub fn validate_codex_child_network_probe_output(
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
    let classification = classify_child_network_probe_failure(&detail, harness, false);
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

pub fn validate_opencode_child_probe_marker_output(
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
        classify_child_network_probe_failure(&detail, harness, false)
    } else {
        classify_child_required_ssh_probe_failure(&detail, harness, false)
    };
    anyhow::bail!("{classification}: {detail}");
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn opencode_child_required_ssh_probe_prompt(targets: &[String]) -> String {
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

pub fn codex_child_writable_roots_probe_prompt(roots: &[PathBuf]) -> String {
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

pub fn validate_codex_child_writable_root_probe_output(
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

pub fn looks_like_codex_transport_403_429(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let ws_403 = lower.contains("403")
        && (lower.contains("websocket") || lower.contains("handshake") || lower.contains("wss://"));
    let https_429 = lower.contains("429")
        && (lower.contains("too many requests") || lower.contains("rate limit"));
    ws_403 || https_429
}

pub fn codex_transport_403_429_diagnostic(stderr: &str) -> String {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexStderrNoiseReport {
    pub filtered: String,
    pub suppressed_marketplace_manifest_warnings: usize,
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

pub fn codex_stderr_noise_report(stderr: &str) -> CodexStderrNoiseReport {
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

pub fn filter_codex_stderr_noise(stderr: &str) -> String {
    codex_stderr_noise_report(stderr).filtered
}

pub fn looks_like_local_browser_cdp_permission_denied(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let local_cdp = lower.contains("127.0.0.1:9222") || lower.contains("localhost:9222");
    let permission_denied = lower.contains("operation not permitted")
        || lower.contains("os error 1")
        || lower.contains("eperm");
    local_cdp && permission_denied
}

pub fn resume_capability_drift_notice() -> &'static str {
    "[agent] codex resume session hit a stale local browser/CDP capability drift \
     (`Operation not permitted` on 127.0.0.1:9222); retrying once with a fresh \
     `codex exec` session"
}

pub fn looks_like_ssh_dns_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("could not resolve hostname")
        || lower.contains("name or service not known")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("nodename nor servname provided")
}

pub fn looks_like_ssh_network_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("operation not permitted")
        || lower.contains("network is unreachable")
        || lower.contains("no route to host")
        || lower.contains("connection timed out")
        || lower.contains("connection refused")
        || lower.contains("connect to host")
}

pub fn looks_like_ssh_auth_failure(text: &str) -> bool {
    text.to_ascii_lowercase().contains("permission denied")
}

pub fn looks_like_ssh_alias_config_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("bad configuration option")
        || lower.contains("terminating,")
        || lower.contains("could not include")
        || lower.contains("include ")
        || lower.contains("no such file or directory")
}

fn lower_trimmed_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(str::trim).filter(|line| !line.is_empty())
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

pub fn transcript_has_required_ssh_failure(text: &str, match_terms: &[String]) -> Option<String> {
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

pub fn transcript_proves_required_ssh_success(text: &str, match_terms: &[String]) -> bool {
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

pub fn format_required_ssh_failure(targets: &[String], detail: &str) -> String {
    format!(
        "required SSH capability failed for target(s) {}: {}",
        targets.join(", "),
        detail.trim()
    )
}

pub fn classify_child_network_probe_failure(
    detail: &str,
    harness: &str,
    usage_output: bool,
) -> String {
    let lower = detail.to_ascii_lowercase();
    if usage_output {
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

pub fn classify_child_required_ssh_probe_failure(
    detail: &str,
    harness: &str,
    usage_output: bool,
) -> String {
    let lower = detail.to_ascii_lowercase();
    if usage_output {
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

pub fn classify_child_writable_root_probe_failure(detail: &str, harness: &str) -> String {
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

pub fn add_dirs_from_args(args: &[String]) -> Vec<PathBuf> {
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

pub fn args_contain_add_dir(args: &str) -> bool {
    args.split_whitespace()
        .any(|arg| arg == "--add-dir" || arg.starts_with("--add-dir="))
}

pub fn normalized_writable_root_strings(roots: &[PathBuf]) -> Vec<String> {
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
    Some(agent_doc_hash::path_string_hash(&normalized.join("\n")))
}

pub fn proof_status_label(required: bool, proven: bool) -> &'static str {
    match (required, proven) {
        (false, _) => "not_required",
        (true, true) => "proven",
        (true, false) => "failed",
    }
}

pub fn proof_timing_ms(duration: Option<Duration>) -> String {
    duration
        .map(|value| value.as_millis().to_string())
        .unwrap_or_else(|| "not_required".to_string())
}

#[derive(Debug, Clone, Default)]
pub struct ManagedCapabilityProofTimings {
    pub network_host_dns: Option<Duration>,
    pub network_child: Option<Duration>,
    pub ssh: Option<Duration>,
    pub writable_launcher: Option<Duration>,
    pub writable_child: Option<Duration>,
    pub total: Duration,
}

impl ManagedCapabilityProofTimings {
    pub fn event_fields(&self) -> String {
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

pub fn managed_network_child_proof_cache_key(
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_frontmatter::frontmatter::CodexNetworkAccess;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn resolve_codex_network_access_prefers_frontmatter_over_config() {
        assert_eq!(
            resolve_codex_network_access(
                Some(CodexNetworkAccess::Enabled),
                Some(CodexNetworkAccess::Disabled),
            ),
            CodexNetworkAccess::Enabled
        );
    }

    #[test]
    fn codex_network_status_detects_inherited_disable_mismatch() {
        let args = vec!["-s".to_string(), "danger-full-access".to_string()];
        let status = codex_network_status_from_overrides(
            &args,
            CodexNetworkAccess::Inherit,
            true,
            &Vec::new(),
        );
        assert!(status.effective_disabled);
        assert!(status.mismatch_error().is_some());
    }

    #[test]
    fn codex_network_status_reads_config_sandbox_mode() {
        let args = vec![
            "-c".to_string(),
            "sandbox_mode=\"danger-full-access\"".to_string(),
        ];
        let status = codex_network_status_from_overrides(
            &args,
            CodexNetworkAccess::Inherit,
            true,
            &Vec::new(),
        );
        assert_eq!(status.sandbox_mode.as_deref(), Some("danger-full-access"));
        assert!(status.mismatch_error().is_some());
    }

    #[test]
    fn codex_network_status_clears_inherited_disable_when_enabled() {
        let args = vec!["-s".to_string(), "danger-full-access".to_string()];
        let mut overrides = Vec::new();
        apply_codex_network_access_env_overrides(&mut overrides, CodexNetworkAccess::Enabled);
        let status = codex_network_status_from_overrides(
            &args,
            CodexNetworkAccess::Enabled,
            true,
            &overrides,
        );
        assert!(!status.effective_disabled);
        assert!(status.mismatch_error().is_none());
    }

    #[test]
    fn codex_network_status_env_map_matches_override_policy() {
        let args = vec!["--sandbox=danger-full-access".to_string()];
        let mut env_map = HashMap::new();
        env_map.insert(
            CODEX_SANDBOX_NETWORK_DISABLED_ENV.to_string(),
            "1".to_string(),
        );
        apply_codex_network_access_env_map(&mut env_map, CodexNetworkAccess::Enabled);

        let mut overrides = Vec::new();
        apply_codex_network_access_env_overrides(&mut overrides, CodexNetworkAccess::Enabled);

        let map_status =
            codex_network_status_from_env_map(&args, CodexNetworkAccess::Enabled, true, &env_map);
        let override_status = codex_network_status_from_overrides(
            &args,
            CodexNetworkAccess::Enabled,
            true,
            &overrides,
        );

        assert_eq!(map_status, override_status);
        assert!(!map_status.effective_disabled);
    }

    #[test]
    fn codex_resume_restart_args_translates_sandbox_for_resume() {
        let args = codex_resume_restart_args(
            &strings(&["resume", "--last"]),
            &strings(&["-s", "danger-full-access", "--model", "gpt-5"]),
        )
        .unwrap();

        assert_eq!(
            args,
            strings(&[
                "resume",
                "--last",
                "-c",
                "sandbox_mode=\"danger-full-access\"",
                "--model",
                "gpt-5",
            ])
        );
    }

    #[test]
    fn codex_resume_restart_args_strips_add_dir_for_resume() {
        let args = codex_resume_restart_args(
            &strings(&["resume", "--last"]),
            &strings(&[
                "-s",
                "danger-full-access",
                "--add-dir",
                "/tmp/project/.git/modules/sub",
                "--add-dir=/tmp/project",
            ]),
        )
        .unwrap();

        assert_eq!(
            args,
            strings(&[
                "resume",
                "--last",
                "-c",
                "sandbox_mode=\"danger-full-access\"",
            ])
        );
    }

    #[test]
    fn codex_resume_restart_args_rejects_conflicting_sandbox_modes() {
        let err = codex_resume_restart_args(
            &strings(&["resume", "--last"]),
            &strings(&[
                "-s",
                "danger-full-access",
                "-c",
                "sandbox_mode=\"workspace-write\"",
            ]),
        )
        .unwrap_err();

        assert_eq!(
            err,
            CodexResumeRestartArgsError::ConflictingSandboxModes {
                existing: "danger-full-access".to_string(),
                requested: "workspace-write".to_string(),
            }
        );
        assert!(err.to_string().contains("conflicting sandbox modes"));
    }

    #[test]
    fn codex_resume_restart_args_rejects_missing_sandbox_value() {
        let err = codex_resume_restart_args(&strings(&["resume", "--last"]), &strings(&["-s"]))
            .unwrap_err();

        assert_eq!(
            err,
            CodexResumeRestartArgsError::MissingSandboxMode {
                flag: "-s".to_string(),
            }
        );
        assert!(err.to_string().contains("provided without a sandbox mode"));
    }

    #[test]
    fn codex_resume_restart_args_rejects_missing_config_value() {
        let err =
            codex_resume_restart_args(&strings(&["resume", "--last"]), &strings(&["--config"]))
                .unwrap_err();

        assert_eq!(
            err,
            CodexResumeRestartArgsError::MissingConfigValue {
                flag: "--config".to_string(),
            }
        );
        assert!(err.to_string().contains("provided without a value"));
    }

    #[test]
    fn looks_like_codex_transport_403_429_detects_ws_403() {
        assert!(looks_like_codex_transport_403_429(
            "WebSocket handshake failed: 403"
        ));
        assert!(looks_like_codex_transport_403_429(
            "wss://example.com returned 403 Forbidden"
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
    fn codex_transport_403_429_diagnostic_names_both_rejections() {
        let msg = codex_transport_403_429_diagnostic(
            "403 on wss://example.com then 429 Too Many Requests",
        );
        assert!(msg.contains("403 Forbidden on WebSocket"));
        assert!(msg.contains("429 Too Many Requests"));
        assert!(msg.contains("restart the codex session"));
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
    fn local_browser_cdp_permission_denied_matches_resume_capability_drift_signature() {
        assert!(looks_like_local_browser_cdp_permission_denied(
            "chromium-bridge check failed for 127.0.0.1:9222: Operation not permitted (os error 1)"
        ));
        assert!(!looks_like_local_browser_cdp_permission_denied(
            "chromium-bridge check failed for 127.0.0.1:9222: Connection refused"
        ));
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
    fn child_network_probe_failure_classifies_known_failures() {
        assert_eq!(
            classify_child_network_probe_failure("Could not resolve host", "Codex", false),
            "DNS resolution failed inside the Codex child"
        );
        assert_eq!(
            classify_child_network_probe_failure("Operation not permitted", "Codex", false),
            "Codex sandbox/network capability denied outbound access"
        );
        assert_eq!(
            classify_child_network_probe_failure("connection refused", "Codex", false),
            "remote network service refused the Codex child connection"
        );
        assert_eq!(
            classify_child_network_probe_failure("timed out", "Codex", false),
            "Codex child network probe timed out"
        );
        assert_eq!(
            classify_child_network_probe_failure("opencode usage", "OpenCode", true),
            "OpenCode child probe printed CLI usage/help instead of running"
        );
    }

    #[test]
    fn child_required_ssh_probe_failure_classifies_known_failures() {
        assert_eq!(
            classify_child_required_ssh_probe_failure(
                "socket: Operation not permitted",
                "OpenCode",
                false
            ),
            "SSH unavailable inside managed OpenCode pane"
        );
        assert_eq!(
            classify_child_required_ssh_probe_failure(
                "name or service not known",
                "OpenCode",
                false
            ),
            "SSH target resolution failed inside managed OpenCode pane"
        );
        assert_eq!(
            classify_child_required_ssh_probe_failure("timeout", "OpenCode", false),
            "SSH probe timed out inside managed OpenCode pane"
        );
        assert_eq!(
            classify_child_required_ssh_probe_failure("opencode usage", "OpenCode", true),
            "OpenCode child SSH probe printed CLI usage/help instead of running"
        );
    }

    #[test]
    fn child_writable_root_probe_failure_classifies_known_failures() {
        assert_eq!(
            classify_child_writable_root_probe_failure("read-only file system", "Codex"),
            "Codex sandbox/write capability denied git metadata access"
        );
        assert_eq!(
            classify_child_writable_root_probe_failure("index.lock exists", "Codex"),
            "Codex child could not create required git metadata lock"
        );
        assert_eq!(
            classify_child_writable_root_probe_failure("no such file or directory", "Codex"),
            "Codex writable-root probe target is missing"
        );
    }
}
