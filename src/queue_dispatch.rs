//! # Module: queue_dispatch
//!
//! ## Spec
//! - Classifies orchestration items as `Prompt` or `Command` based on leading `/`.
//! - Commands are dispatched through three paths in priority order:
//!   1. Supervisor IPC — if a supervisor socket is active for the document session
//!   2. tmux send-keys — if the document's harness pane is alive
//!   3. Inline execution — for commands that don't need the harness (`/model`, `/compact`)
//! - Prompts pass through to the existing agent orchestration pipeline unchanged.
//! - Ordering is strict: each item must complete before the next starts.
//! - Command failure halts the queue; remaining items stay unprocessed.
//!
//! ## Agentic Contracts
//! - Commands never enter the preflight → agent → finalize cycle.
//! - `/model <tier>` modifies the orchestrate-local model override for subsequent
//!   prompts; it does not persist to frontmatter.
//! - `/compact <file>` delegates to `agent-doc compact` as a subprocess.
//! - Unknown commands that cannot be dispatched via supervisor or tmux fail
//!   immediately with a descriptive error.
//!
//! ## Evals
//! - classify_prompt_item
//! - classify_command_item
//! - classify_command_with_args
//! - dispatch_inline_model_updates_override
//! - dispatch_inline_compact_runs_subprocess
//! - dispatch_tmux_sends_command_text
//! - dispatch_supervisor_injects_command
//! - dispatch_priority_prefers_supervisor_over_tmux
//! - unknown_command_without_dispatch_path_fails

use std::path::Path;

use anyhow::{Context, Result};

use crate::frontmatter;
use agent_doc_orchestration::sessions;
use agent_doc_orchestration::snapshot;
use agent_doc_orchestration::supervisor::ipc as supervisor_ipc;

/// Classification of a queue/orchestration item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueItemKind {
    Prompt,
    Command,
}

/// A classified queue item with its raw text and parsed components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub kind: QueueItemKind,
    pub raw: String,
    /// For commands: the command name without leading `/` (e.g., "clear", "model").
    /// For prompts: same as `raw`.
    pub command: Option<String>,
    /// For commands: arguments after the command name. Empty for prompts.
    pub args: Vec<String>,
}

/// Classify a text item as a prompt or command.
pub fn classify(text: &str) -> QueueItem {
    let trimmed = text.trim();
    if let Some(command) = agent_doc_orchestration::queue_command::classify(trimmed) {
        return QueueItem {
            kind: QueueItemKind::Command,
            raw: command.raw,
            command: Some(command.name),
            args: command.args,
        };
    }
    if let Some(without_slash) = trimmed.strip_prefix('/') {
        let mut parts = without_slash.split_whitespace();
        let command = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(String::from).collect();
        QueueItem {
            kind: QueueItemKind::Command,
            raw: trimmed.to_string(),
            command: Some(command),
            args,
        }
    } else {
        QueueItem {
            kind: QueueItemKind::Prompt,
            raw: trimmed.to_string(),
            command: None,
            args: Vec::new(),
        }
    }
}

/// Commands that can be executed inline without a harness session.
const INLINE_COMMANDS: &[&str] = &["model", "compact"];

/// Result of dispatching a command.
#[derive(Debug)]
pub enum DispatchResult {
    /// Command completed successfully.
    Ok,
    /// `/model` command: the model override was updated.
    ModelOverride(String),
}

/// Context for command dispatch — provides the paths and state needed to
/// locate supervisor sockets, tmux panes, and the document session.
pub struct DispatchContext {
    pub file: std::path::PathBuf,
    pub project_root: Option<std::path::PathBuf>,
    pub session_uuid: Option<String>,
    pub pane_id: Option<String>,
    pub harness: String,
}

impl DispatchContext {
    /// Build dispatch context from a document file path.
    pub fn from_file(file: &Path) -> Result<Self> {
        let doc = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (fm, _) = frontmatter::parse(&doc)?;
        let project_root = snapshot::find_project_root(file);

        let pane_id = if let (Some(root), Some(session)) = (&project_root, &fm.session) {
            lookup_pane(root, session)
        } else {
            None
        };

        Ok(Self {
            file: file.to_path_buf(),
            project_root,
            session_uuid: fm.session,
            pane_id,
            harness: fm.agent.unwrap_or_else(|| "claude".to_string()),
        })
    }
}

fn sanitize_progress_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '%' | '=') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn item_fingerprint(item: &QueueItem) -> String {
    format!(
        "command={} bytes={} sha256={}",
        sanitize_progress_field(item.command.as_deref().unwrap_or("prompt")),
        item.raw.len(),
        agent_doc_orchestration::ops_log::content_hash(&item.raw)
    )
}

fn log_dispatch_progress(ctx: &DispatchContext, event: String) {
    agent_doc_orchestration::ops_log::log_op(&ctx.file, &event);
    if agent_doc_orchestration::input_diag::verbose_enabled() {
        eprintln!("[queue_dispatch] {event}");
    }
}

/// Dispatch a command item through the available paths.
///
/// Returns `DispatchResult::ModelOverride` for `/model` commands so the caller
/// can update its local state. All other successful commands return `DispatchResult::Ok`.
pub fn dispatch_command(item: &QueueItem, ctx: &DispatchContext) -> Result<DispatchResult> {
    let command = item.command.as_deref().unwrap_or("");

    // Try inline execution first for eligible commands
    if INLINE_COMMANDS.contains(&command) {
        return dispatch_inline(item, ctx);
    }

    if command == "clear" {
        return dispatch_clear(ctx);
    }

    // Try supervisor IPC
    if let Some(result) = try_supervisor_dispatch(item, ctx)? {
        return Ok(result);
    }

    // Try tmux send-keys
    if let Some(result) = try_tmux_dispatch(item, ctx)? {
        return Ok(result);
    }

    anyhow::bail!(
        "cannot dispatch command `{}`: no supervisor socket or tmux pane available",
        item.raw
    );
}

fn dispatch_clear(ctx: &DispatchContext) -> Result<DispatchResult> {
    log_dispatch_progress(
        ctx,
        "queue_dispatch_progress transport=session_clear command=clear".to_string(),
    );
    crate::session_actor_cmd::clear(&ctx.file).with_context(|| {
        format!(
            "failed to dispatch guarded /clear for {}",
            ctx.file.display()
        )
    })?;
    Ok(DispatchResult::Ok)
}

/// Execute an inline-eligible command.
fn dispatch_inline(item: &QueueItem, ctx: &DispatchContext) -> Result<DispatchResult> {
    let command = item.command.as_deref().unwrap_or("");
    match command {
        "model" => {
            let tier = item
                .args
                .first()
                .ok_or_else(|| anyhow::anyhow!("/model requires a tier argument"))?;
            log_dispatch_progress(
                ctx,
                format!(
                    "queue_dispatch_progress transport=inline_model tier={} {}",
                    sanitize_progress_field(tier),
                    item_fingerprint(item)
                ),
            );
            Ok(DispatchResult::ModelOverride(tier.clone()))
        }
        "compact" => {
            let default_file = ctx.file.to_string_lossy();
            let file_arg = item
                .args
                .first()
                .map(|s| s.as_str())
                .unwrap_or(&default_file);
            log_dispatch_progress(
                ctx,
                format!(
                    "queue_dispatch_progress transport=inline_compact target={} {}",
                    sanitize_progress_field(file_arg),
                    item_fingerprint(item)
                ),
            );
            let exe =
                std::env::current_exe().context("failed to resolve current agent-doc binary")?;
            let status = std::process::Command::new(&exe)
                .args(["compact", file_arg])
                .status()
                .context("failed to run agent-doc compact")?;
            if !status.success() {
                anyhow::bail!(
                    "/compact exited with status {}",
                    status.code().unwrap_or(-1)
                );
            }
            // Also commit after compact (as per normal compact workflow)
            let commit_status = std::process::Command::new(&exe)
                .args(["commit", file_arg])
                .status()
                .context("failed to run agent-doc commit after compact")?;
            if !commit_status.success() {
                log_dispatch_progress(
                    ctx,
                    format!(
                        "queue_dispatch_warning transport=inline_compact phase=commit_after_compact status={}",
                        commit_status.code().unwrap_or(-1)
                    ),
                );
            }
            Ok(DispatchResult::Ok)
        }
        _ => anyhow::bail!("command `/{command}` is not inline-executable"),
    }
}

/// Try dispatching via supervisor IPC. Returns `None` if no supervisor is available.
fn try_supervisor_dispatch(
    item: &QueueItem,
    ctx: &DispatchContext,
) -> Result<Option<DispatchResult>> {
    let (project_root, session_uuid) = match (&ctx.project_root, &ctx.session_uuid) {
        (Some(root), Some(uuid)) => (root.as_path(), uuid.as_str()),
        _ => return Ok(None),
    };

    let sock = supervisor_ipc::socket_path(project_root, session_uuid);
    if !sock.exists() {
        return Ok(None);
    }

    log_dispatch_progress(
        ctx,
        format!(
            "queue_dispatch_progress transport=supervisor_ipc destination={} {}",
            sanitize_progress_field(&format!("socket:{}", sock.display())),
            item_fingerprint(item)
        ),
    );

    // Use `inject` method to send the command text to the harness stdin.
    // The harness interprets `/command` lines natively.
    let bytes = supervisor_ipc::normalize_submit_text(&item.raw);
    agent_doc_orchestration::input_diag::log_text_submit(
        Some(&ctx.file),
        "queue_dispatch.supervisor_ipc",
        &format!("socket:{}", sock.display()),
        &bytes,
        None,
        "supervisor_ipc_inject",
        "Inject",
    );
    let method = supervisor_ipc::IpcMethod::Inject { bytes };
    let resp =
        supervisor_ipc::send_command(&sock, &method).context("supervisor IPC dispatch failed")?;

    if !resp.ok {
        let msg = resp.error.unwrap_or_else(|| "unknown error".to_string());
        anyhow::bail!("supervisor rejected command `{}`: {}", item.raw, msg);
    }

    Ok(Some(DispatchResult::Ok))
}

/// Try dispatching via tmux send-keys. Returns `None` if no pane is available.
fn try_tmux_dispatch(item: &QueueItem, ctx: &DispatchContext) -> Result<Option<DispatchResult>> {
    let pane_id = match &ctx.pane_id {
        Some(id) => id.clone(),
        None => return Ok(None),
    };

    log_dispatch_progress(
        ctx,
        format!(
            "queue_dispatch_progress transport=tmux_send_keys destination={} {}",
            sanitize_progress_field(&format!("pane:{pane_id}")),
            item_fingerprint(item)
        ),
    );

    let tmux = sessions::Tmux::default();
    let profile = sessions::tmux_submit_profile_for_harness(&ctx.harness);

    // Send the command text through the canonical tmux submit path.
    agent_doc_orchestration::input_diag::log_text_submit(
        Some(&ctx.file),
        "queue_dispatch.tmux_send_keys",
        &format!("pane:{pane_id}"),
        &item.raw,
        Some(&ctx.harness),
        profile.transform(),
        profile.submit_key(),
    );
    sessions::send_submitted_text_for_harness(&tmux, &pane_id, &item.raw, &ctx.harness)?;

    // Poll for completion: wait until the command text disappears from the
    // pane's last few visible lines (same approach as route.rs send_command).
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(30);
    let poll_interval = std::time::Duration::from_millis(500);
    while start.elapsed() < timeout {
        std::thread::sleep(poll_interval);
        if let Ok(content) = sessions::capture_pane(&tmux, &pane_id) {
            let still_visible = content
                .lines()
                .rev()
                .take(5)
                .any(|line| line.contains(&item.raw));

            if !still_visible {
                log_dispatch_progress(
                    ctx,
                    format!(
                        "queue_dispatch_progress transport=tmux_send_keys outcome=accepted elapsed_ms={}",
                        start.elapsed().as_millis()
                    ),
                );
                return Ok(Some(DispatchResult::Ok));
            }
        }
    }

    log_dispatch_progress(
        ctx,
        format!(
            "queue_dispatch_warning transport=tmux_send_keys outcome=acceptance_unconfirmed elapsed_ms={}",
            start.elapsed().as_millis()
        ),
    );
    Ok(Some(DispatchResult::Ok))
}

/// Look up the tmux pane for a document session from the registry.
fn lookup_pane(_project_root: &Path, session_uuid: &str) -> Option<String> {
    let registry = sessions::load().ok()?;
    registry
        .values()
        .find(|entry| entry.session_id == session_uuid)
        .map(|entry| entry.pane.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn remove(key: &'static str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn test_dispatch_context() -> DispatchContext {
        DispatchContext {
            file: Path::new("test.md").to_path_buf(),
            project_root: None,
            session_uuid: None,
            pane_id: None,
            harness: "codex".to_string(),
        }
    }

    #[test]
    fn classify_prompt_item() {
        let item = classify("do #fix1");
        assert_eq!(item.kind, QueueItemKind::Prompt);
        assert_eq!(item.raw, "do #fix1");
        assert!(item.command.is_none());
        assert!(item.args.is_empty());
    }

    #[test]
    fn classify_command_item() {
        let item = classify("/clear");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.raw, "/clear");
        assert_eq!(item.command.as_deref(), Some("clear"));
        assert!(item.args.is_empty());
    }

    #[test]
    fn classify_command_with_args() {
        let item = classify("/model sonnet");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.command.as_deref(), Some("model"));
        assert_eq!(item.args, vec!["sonnet"]);
    }

    #[test]
    fn classify_command_with_multiple_args() {
        let item = classify("/compact tasks/agent-doc/agent-doc-bugs.md");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.command.as_deref(), Some("compact"));
        assert_eq!(item.args, vec!["tasks/agent-doc/agent-doc-bugs.md"]);
    }

    #[test]
    fn classify_trims_whitespace() {
        let item = classify("  /clear  ");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.raw, "/clear");
    }

    #[test]
    fn classify_empty_slash() {
        let item = classify("/");
        assert_eq!(item.kind, QueueItemKind::Command);
        assert_eq!(item.command.as_deref(), Some(""));
    }

    #[test]
    fn classify_review_prompt() {
        let item = classify("Review the pending items");
        assert_eq!(item.kind, QueueItemKind::Prompt);
    }

    #[test]
    fn dispatch_inline_model_updates_override() {
        let item = classify("/model sonnet");
        let ctx = test_dispatch_context();
        match dispatch_inline(&item, &ctx).unwrap() {
            DispatchResult::ModelOverride(tier) => assert_eq!(tier, "sonnet"),
            _ => panic!("expected ModelOverride"),
        }
    }

    #[test]
    fn dispatch_inline_model_requires_arg() {
        let item = classify("/model");
        let ctx = test_dispatch_context();
        assert!(dispatch_inline(&item, &ctx).is_err());
    }

    #[test]
    fn unknown_command_without_dispatch_path_fails() {
        let item = classify("/unknown");
        let ctx = test_dispatch_context();
        let result = dispatch_command(&item, &ctx);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no supervisor socket or tmux pane available"));
    }

    #[test]
    fn dispatch_supervisor_injects_command_and_logs_redacted_progress_without_foreground_diag() {
        let _diag_guard = EnvGuard::remove("AGENT_DOC_TMUX_INPUT_DIAG");
        let _stdin_guard = EnvGuard::remove("AGENT_DOC_DEBUG_STDIN");
        assert!(
            !agent_doc_orchestration::input_diag::verbose_enabled(),
            "non-verbose queue dispatch must not mirror progress into the foreground TUI"
        );

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "---\nagent_doc_session: queue-session\n---\n").unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let mut ipc = agent_doc_orchestration::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "queue-session",
            move |method| match method {
                agent_doc_orchestration::supervisor::ipc::IpcMethod::Inject { bytes }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Clear { bytes } => {
                    captured_for_ipc.lock().unwrap().push(bytes);
                    agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty()
                }
                agent_doc_orchestration::supervisor::ipc::IpcMethod::State
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Pid
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Restart { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Stop { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::StopAgent { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaRegister { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaDeregister {
                    ..
                }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaUpdate { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaPull { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaAck { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaAwareness {
                    ..
                } => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty(),
            },
        )
        .unwrap();

        let item = classify("/doctor");
        let ctx = DispatchContext::from_file(&doc).unwrap();
        let result = dispatch_command(&item, &ctx).unwrap();
        assert!(matches!(result, DispatchResult::Ok));
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[agent_doc_orchestration::supervisor::ipc::normalize_submit_text("/doctor")]
        );

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_dispatch_progress transport=supervisor_ipc"),
            "{ops}"
        );
        assert!(ops.contains("command=doctor bytes=7"), "{ops}");
        assert!(
            ops.contains(&agent_doc_orchestration::ops_log::content_hash("/doctor")),
            "{ops}"
        );
        assert!(
            ops.contains("tmux_input_event source=queue_dispatch.supervisor_ipc"),
            "{ops}"
        );
        assert!(
            !ops.contains("/doctor"),
            "ops progress must not contain raw queue command text:\n{ops}"
        );

        ipc.stop();
    }

    #[test]
    fn dispatch_clear_uses_session_clear_guard_instead_of_raw_supervisor_inject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "---\nagent_doc_session: queue-session\n---\n").unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let mut ipc = agent_doc_orchestration::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "queue-session",
            move |method| match method {
                agent_doc_orchestration::supervisor::ipc::IpcMethod::Inject { bytes }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Clear { bytes } => {
                    captured_for_ipc.lock().unwrap().push(bytes);
                    agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty()
                }
                agent_doc_orchestration::supervisor::ipc::IpcMethod::State
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Pid
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Restart { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::Stop { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::StopAgent { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaRegister { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaDeregister {
                    ..
                }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaUpdate { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaPull { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaAck { .. }
                | agent_doc_orchestration::supervisor::ipc::IpcMethod::ReplicaAwareness {
                    ..
                } => agent_doc_orchestration::supervisor::ipc::IpcResponse::ok_empty(),
            },
        )
        .unwrap();

        let item = classify("/clear");
        let ctx = DispatchContext::from_file(&doc).unwrap();
        let result = dispatch_command(&item, &ctx);

        assert!(result.is_err());
        assert!(
            captured.lock().unwrap().is_empty(),
            "queued /clear must go through session clear guards, not raw supervisor/tmux injection"
        );

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_dispatch_progress transport=session_clear command=clear"),
            "{ops}"
        );
        assert!(
            !ops.contains("queue_dispatch_progress transport=supervisor_ipc"),
            "{ops}"
        );

        ipc.stop();
    }
}
