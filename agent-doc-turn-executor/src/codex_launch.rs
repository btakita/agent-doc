//! Pure Codex launch/restart argument policy.
//!
//! Orchestration chooses when to restart a Codex turn executor. This module owns
//! the harness-specific argument transformation for `codex resume`, which has a
//! narrower CLI surface than the original `codex exec` launch.

use std::error::Error;
use std::fmt;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
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
