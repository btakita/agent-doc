//! # Module: queue_dispatch
//!
//! ## Spec
//! - Dispatches items classified by `agent-doc-queue` as prompts or commands.
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
//! - dispatch_inline_model_updates_override
//! - dispatch_inline_compact_runs_subprocess
//! - dispatch_tmux_sends_command_text
//! - dispatch_supervisor_injects_command
//! - dispatch_priority_prefers_supervisor_over_tmux
//! - unknown_command_without_dispatch_path_fails

use std::path::Path;

use anyhow::{Context, Result};

use agent_doc_frontmatter::frontmatter;
#[cfg(test)]
use agent_doc_queue::dispatch_item::classify;
use agent_doc_queue::dispatch_item::{
    InlineDispatchCommand, QueueItem, inline_dispatch_command, is_session_clear_command,
    item_fingerprint, sanitize_progress_field,
};
use agent_doc_supervisor::ipc_protocol::IpcMethod;
#[cfg(test)]
use agent_doc_supervisor::ipc_protocol::IpcResponse;
use agent_doc_supervisor_io::ipc as supervisor_ipc;

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
        let doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "queue_dispatch_command_document",
        )
        .with_context(|| format!("failed to resolve {}", file.display()))?;
        let (fm, _) = frontmatter::parse(&doc)?;
        let project_root = agent_doc_fs::find_project_root(file);

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

fn log_dispatch_progress(ctx: &DispatchContext, event: String) {
    agent_doc_ops_log_io::log_op(&ctx.file, &event);
    if agent_doc_tmux_commands::input_diag::verbose_enabled() {
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
    if let Some(inline_command) = inline_dispatch_command(command) {
        return dispatch_inline(inline_command, item, ctx);
    }

    if is_session_clear_command(command) {
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
fn dispatch_inline(
    command: InlineDispatchCommand,
    item: &QueueItem,
    ctx: &DispatchContext,
) -> Result<DispatchResult> {
    match command {
        InlineDispatchCommand::Model => {
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
        InlineDispatchCommand::Compact => {
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
    let bytes = agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(&item.raw)
        .to_string();
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(
            Some(&ctx.file),
            agent_doc_ops_log_io::log_op,
        ),
        "queue_dispatch.supervisor_ipc",
        &format!("socket:{}", sock.display()),
        &bytes,
        None,
        "supervisor_ipc_inject",
        "Inject",
    );
    let method = IpcMethod::Inject { bytes };
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

    let tmux = tmux_router::Tmux::default();
    let profile = agent_doc_tmux_commands::tmux_submit_profile_for_harness(&ctx.harness);

    // Send the command text through the canonical tmux submit path.
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(
            Some(&ctx.file),
            agent_doc_ops_log_io::log_op,
        ),
        "queue_dispatch.tmux_send_keys",
        &format!("pane:{pane_id}"),
        &item.raw,
        Some(&ctx.harness),
        profile.transform(),
        profile.submit_key(),
    );
    agent_doc_tmux_io::send_submitted_text_for_harness_logged(
        &tmux,
        &pane_id,
        &item.raw,
        &ctx.harness,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text_for_harness",
    )?;

    // Poll for completion: wait until the command text disappears from the
    // pane's last few visible lines (same approach as route.rs send_command).
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(30);
    let poll_interval = std::time::Duration::from_millis(500);
    while start.elapsed() < timeout {
        std::thread::sleep(poll_interval);
        if let Ok(content) = agent_doc_tmux_io::capture_pane(&tmux, &pane_id) {
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
    let registry = agent_doc_session_registry_io::load().ok()?;
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
    fn dispatch_inline_model_updates_override() {
        let item = classify("/model sonnet");
        let ctx = test_dispatch_context();
        match dispatch_inline(InlineDispatchCommand::Model, &item, &ctx).unwrap() {
            DispatchResult::ModelOverride(tier) => assert_eq!(tier, "sonnet"),
            _ => panic!("expected ModelOverride"),
        }
    }

    #[test]
    fn dispatch_inline_model_requires_arg() {
        let item = classify("/model");
        let ctx = test_dispatch_context();
        assert!(dispatch_inline(InlineDispatchCommand::Model, &item, &ctx).is_err());
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
            !agent_doc_tmux_commands::input_diag::verbose_enabled(),
            "non-verbose queue dispatch must not mirror progress into the foreground TUI"
        );

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "---\nagent_doc_session: queue-session\n---\n").unwrap();

        let captured = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let mut ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            dir.path(),
            "queue-session",
            move |method| match method {
                IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                    captured_for_ipc.lock().push(bytes);
                    IpcResponse::ok_empty()
                }
                IpcMethod::State
                | IpcMethod::Pid
                | IpcMethod::Restart { .. }
                | IpcMethod::Stop { .. }
                | IpcMethod::StopAgent { .. } => IpcResponse::ok_empty(),
            },
        )
        .unwrap();

        let item = classify("/doctor");
        let ctx = DispatchContext::from_file(&doc).unwrap();
        let result = dispatch_command(&item, &ctx).unwrap();
        assert!(matches!(result, DispatchResult::Ok));
        assert_eq!(
            captured.lock().as_slice(),
            &[
                agent_doc_tmux_commands::submitted_text_without_trailing_line_endings("/doctor")
                    .to_string()
            ]
        );

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_dispatch_progress transport=supervisor_ipc"),
            "{ops}"
        );
        assert!(ops.contains("command=doctor bytes=7"), "{ops}");
        assert!(
            ops.contains(&agent_doc_hash::content_hash("/doctor")),
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

        let captured = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let mut ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            dir.path(),
            "queue-session",
            move |method| match method {
                IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                    captured_for_ipc.lock().push(bytes);
                    IpcResponse::ok_empty()
                }
                IpcMethod::State
                | IpcMethod::Pid
                | IpcMethod::Restart { .. }
                | IpcMethod::Stop { .. }
                | IpcMethod::StopAgent { .. } => IpcResponse::ok_empty(),
            },
        )
        .unwrap();

        let item = classify("/clear");
        let ctx = DispatchContext::from_file(&doc).unwrap();
        let result = dispatch_command(&item, &ctx);

        assert!(result.is_err());
        assert!(
            captured.lock().is_empty(),
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
