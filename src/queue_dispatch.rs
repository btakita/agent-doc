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
//! - Supervisor IPC and tmux both read the composer before typing
//!   (`#queuecommanddraftguard`). The decision is `auto_trigger_draft_guard`.
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
use agent_doc_supervisor::auto_trigger::{AutoTriggerDraftGuard, auto_trigger_draft_guard};
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
    /// Test-only composer snapshot. `Some` skips the pane capture so unit tests
    /// never touch the live tmux server. Production builds do not have this field.
    #[cfg(test)]
    pub composer_read: Option<TestQueueCommandComposerRead>,
}

/// One composer snapshot substituted for a pane capture (`#queuecommanddraftguard`).
#[cfg(test)]
pub struct TestQueueCommandComposerRead {
    /// `false` is a failed capture: unread, not an empty composer.
    pub captured: bool,
    pub content: String,
    pub cursor_y: Option<usize>,
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
            #[cfg(test)]
            composer_read: None,
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

/// What a queue command may do after the composer has been read
/// (`#queuecommanddraftguard`).
#[derive(Clone, Copy)]
enum QueueCommandTyping {
    /// The read composer can take this command as new text.
    Type,
    /// This exact command is already the idle draft. Press Enter; do not type it again.
    SubmitStranded,
}

/// Read the composer a queue command is about to type into.
///
/// No pane, and a failed ANSI capture, are unread. An empty screen is not
/// invented from a plain `capture-pane -p` (`#plaincaptureghost`).
fn read_queue_command_composer(ctx: &DispatchContext) -> (Option<String>, Option<usize>) {
    #[cfg(test)]
    if let Some(read) = &ctx.composer_read {
        return (read.captured.then(|| read.content.clone()), read.cursor_y);
    }
    let Some(pane_id) = ctx.pane_id.as_deref() else {
        return (None, None);
    };
    let tmux = tmux_router::Tmux::default();
    let content = agent_doc_tmux_io::capture_pane_with_ansi(&tmux, pane_id).ok();
    let cursor_y = content
        .as_ref()
        .and_then(|_| agent_doc_tmux_io::pane_cursor_y(&tmux, pane_id));
    (content, cursor_y)
}

/// `#queuecommanddraftguard`: the guard for a queue command that types through
/// supervisor IPC or tmux with no composer check of its own. A refusal types
/// nothing. The decision is the one idle-queue inject already uses.
fn guard_queue_command_before_typing(
    ctx: &DispatchContext,
    item: &QueueItem,
    command: &str,
) -> Result<QueueCommandTyping> {
    let harness = agent_doc_harness::HarnessConfig::from_agent_name(&ctx.harness);
    let (content, cursor_y) = read_queue_command_composer(ctx);
    let (facts, preview) = agent_doc_start_runtime_io::read_auto_trigger_draft(
        content.as_deref(),
        cursor_y,
        &harness,
        command,
    );
    match auto_trigger_draft_guard(facts) {
        AutoTriggerDraftGuard::Type => Ok(QueueCommandTyping::Type),
        AutoTriggerDraftGuard::SubmitStranded => Ok(QueueCommandTyping::SubmitStranded),
        AutoTriggerDraftGuard::Refuse(reason) => {
            let preview_text = preview.as_deref().unwrap_or("<unavailable>");
            log_dispatch_progress(
                ctx,
                format!(
                    "queue_dispatch_draft_refused reason={reason} draft_preview_bytes={} draft_preview_sha256={} #queuecommanddraftguard note=nothing was typed {}",
                    preview_text.len(),
                    agent_doc_hash::content_hash(preview_text),
                    item_fingerprint(item),
                ),
            );
            anyhow::bail!(
                "queue command dispatch refused to type into the composer ({reason}); nothing was typed. Submit or clear that draft, then retry. Redacted draft preview: {preview_text:?}. (#queuecommanddraftguard)"
            )
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

    // The harness interprets `/command` lines natively. The composer check is
    // first: `Inject` itself does not read the composer.
    let bytes = agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(&item.raw)
        .to_string();
    let typing = guard_queue_command_before_typing(ctx, item, &bytes)?;
    let (payload, source, diag) = match typing {
        QueueCommandTyping::Type => (bytes, "queue_dispatch.supervisor_ipc", "type"),
        QueueCommandTyping::SubmitStranded => (
            "\r".to_string(),
            "queue_dispatch.supervisor_ipc_stranded",
            "submit_stranded",
        ),
    };
    log_dispatch_progress(
        ctx,
        format!(
            "queue_dispatch_progress transport=supervisor_ipc composer={diag} destination={} {}",
            sanitize_progress_field(&format!("socket:{}", sock.display())),
            item_fingerprint(item)
        ),
    );
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(
            Some(&ctx.file),
            agent_doc_ops_log_io::log_op,
        ),
        source,
        &format!("socket:{}", sock.display()),
        &payload,
        None,
        "supervisor_ipc_inject",
        "Inject",
    );
    let method = IpcMethod::Inject { bytes: payload };
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

    let bytes = agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(&item.raw)
        .to_string();
    let typing = guard_queue_command_before_typing(ctx, item, &bytes)?;
    let (payload, source, diag) = match typing {
        QueueCommandTyping::Type => (bytes, "queue_dispatch.tmux_send_keys", "type"),
        QueueCommandTyping::SubmitStranded => (
            String::new(),
            "queue_dispatch.tmux_send_keys_stranded",
            "submit_stranded",
        ),
    };

    log_dispatch_progress(
        ctx,
        format!(
            "queue_dispatch_progress transport=tmux_send_keys composer={diag} destination={} {}",
            sanitize_progress_field(&format!("pane:{pane_id}")),
            item_fingerprint(item)
        ),
    );

    let tmux = tmux_router::Tmux::default();
    let profile = agent_doc_tmux_commands::tmux_submit_profile_for_harness(&ctx.harness);

    // Send the command text through the canonical tmux submit path. A stranded
    // draft sends no text: an empty payload is the harness submit key only.
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(
            Some(&ctx.file),
            agent_doc_ops_log_io::log_op,
        ),
        source,
        &format!("pane:{pane_id}"),
        &payload,
        Some(&ctx.harness),
        profile.transform(),
        profile.submit_key(),
    );
    agent_doc_tmux_io::send_submitted_text_for_harness_logged(
        &tmux,
        &pane_id,
        &payload,
        &ctx.harness,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text_for_harness",
    )?;
    if matches!(typing, QueueCommandTyping::SubmitStranded) {
        // The command was already visible. The acceptance poll treats that text
        // as "not yet consumed" and would wait out its budget on every repair.
        log_dispatch_progress(
            ctx,
            "queue_dispatch_progress transport=tmux_send_keys outcome=submit_stranded".to_string(),
        );
        return Ok(Some(DispatchResult::Ok));
    }

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
            composer_read: None,
        }
    }

    fn claude_composer(body: &str) -> String {
        format!(
            "────────────────────────────────────────\n\
             {body}\n\
             ────────────────────────────────────────\n\
             ⏵⏵ bypass permissions on (shift+tab to cycle)\n"
        )
    }

    fn composer_read(content: impl Into<String>) -> TestQueueCommandComposerRead {
        TestQueueCommandComposerRead {
            captured: true,
            content: content.into(),
            cursor_y: None,
        }
    }

    struct RecordingIpc {
        ipc: agent_doc_supervisor_io::ipc::SupervisorIpc,
        captured: std::sync::Arc<parking_lot::Mutex<Vec<String>>>,
    }

    fn start_recording_ipc(dir: &Path) -> RecordingIpc {
        let captured = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            dir,
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
        RecordingIpc { ipc, captured }
    }

    fn session_doc(dir: &Path) -> std::path::PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join("doc.md");
        std::fs::write(&doc, "---\nagent_doc_session: queue-session\n---\n").unwrap();
        doc
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
        let doc = session_doc(dir.path());
        let mut ipc = start_recording_ipc(dir.path());

        let item = classify("/doctor");
        let mut ctx = DispatchContext::from_file(&doc).unwrap();
        ctx.composer_read = Some(composer_read(claude_composer("❯")));
        let result = dispatch_command(&item, &ctx).unwrap();
        assert!(matches!(result, DispatchResult::Ok));
        assert_eq!(
            ipc.captured.lock().as_slice(),
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

        ipc.ipc.stop();
    }

    /// `#queuecommanddraftguard`: no pane and no capture is an unread composer.
    /// The supervisor socket is live and must still receive nothing.
    #[test]
    fn dispatch_supervisor_refuses_an_unread_composer_and_types_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session_doc(dir.path());
        let ipc = start_recording_ipc(dir.path());

        let item = classify("/doctor");
        let ctx = DispatchContext::from_file(&doc).unwrap();
        let err = dispatch_command(&item, &ctx).unwrap_err().to_string();
        assert!(
            err.contains("composer_unreadable"),
            "an unread composer must refuse: {err}"
        );
        assert!(err.contains("nothing was typed"), "{err}");
        assert!(
            ipc.captured.lock().is_empty(),
            "refusing must not inject: {:?}",
            ipc.captured.lock()
        );

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("queue_dispatch_draft_refused"), "{ops}");
        assert!(ops.contains("reason=composer_unreadable"), "{ops}");
        assert!(ops.contains("#queuecommanddraftguard"), "{ops}");
        assert!(
            !ops.contains("queue_dispatch_progress transport=supervisor_ipc"),
            "a refusal is not a typed dispatch: {ops}"
        );
        assert!(!ops.contains("/doctor"), "{ops}");
    }

    /// `#queuecommanddraftguard`: a different draft refuses on the supervisor
    /// path, idle or not. Nothing is injected.
    #[test]
    fn dispatch_supervisor_refuses_a_foreign_draft_and_types_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session_doc(dir.path());
        let ipc = start_recording_ipc(dir.path());

        let item = classify("/doctor");
        let mut ctx = DispatchContext::from_file(&doc).unwrap();
        ctx.composer_read = Some(composer_read(claude_composer("❯ keep going")));
        let err = dispatch_command(&item, &ctx).unwrap_err().to_string();
        assert!(err.contains("foreign_draft"), "{err}");
        assert!(err.contains("keep going"), "{err}");
        assert!(ipc.captured.lock().is_empty(), "{:?}", ipc.captured.lock());

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("reason=foreign_draft"), "{ops}");
        assert!(
            !ops.contains("keep going"),
            "ops must not contain the raw draft: {ops}"
        );
        assert!(!ops.contains("/doctor"), "{ops}");
    }

    /// `#queuecommanddraftguard`: the same command already sitting in an idle
    /// composer is submitted with Enter and is not typed again.
    #[test]
    fn dispatch_supervisor_submits_a_stranded_command_without_retyping() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session_doc(dir.path());
        let ipc = start_recording_ipc(dir.path());

        let item = classify("/doctor");
        let mut ctx = DispatchContext::from_file(&doc).unwrap();
        ctx.composer_read = Some(composer_read(claude_composer("❯ /doctor")));
        dispatch_command(&item, &ctx).unwrap();
        assert_eq!(ipc.captured.lock().as_slice(), &["\r".to_string()]);

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("composer=submit_stranded"), "{ops}");
        assert!(
            ops.contains("source=queue_dispatch.supervisor_ipc_stranded"),
            "{ops}"
        );
        assert!(!ops.contains("/doctor"), "{ops}");
    }

    /// `#queuecommanddraftguard`: this command held behind a running turn is
    /// not submitted and not appended to.
    #[test]
    fn dispatch_supervisor_refuses_a_command_behind_a_running_turn() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session_doc(dir.path());
        let ipc = start_recording_ipc(dir.path());

        let item = classify("/doctor");
        let mut ctx = DispatchContext::from_file(&doc).unwrap();
        ctx.composer_read = Some(composer_read(
            "✶ Generating… (3s · esc to interrupt)\n\
             ────────────────────────────────────────\n\
             ❯ /doctor\n\
             ────────────────────────────────────────\n\
             ⏵⏵ bypass permissions on · 1 shell\n",
        ));
        let err = dispatch_command(&item, &ctx).unwrap_err().to_string();
        assert!(err.contains("command_behind_running_turn"), "{err}");
        assert!(ipc.captured.lock().is_empty(), "{:?}", ipc.captured.lock());
    }

    /// `#queuecommanddraftguard`: a dim ghost of the command is not a draft.
    /// The command is typed.
    #[test]
    fn dispatch_supervisor_types_over_a_ghost_of_the_command() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session_doc(dir.path());
        let ipc = start_recording_ipc(dir.path());

        let item = classify("/doctor");
        let mut ctx = DispatchContext::from_file(&doc).unwrap();
        ctx.harness = "codex".to_string();
        ctx.composer_read = Some(composer_read(
            "\x1b[1m›\x1b[0m \x1b[2m/doctor\x1b[0m\n\
             gpt-5.5 high · ~/work/btakita/agent-loop · Context 40% used\n",
        ));
        dispatch_command(&item, &ctx).unwrap();
        assert_eq!(
            ipc.captured.lock().as_slice(),
            &[
                agent_doc_tmux_commands::submitted_text_without_trailing_line_endings("/doctor")
                    .to_string()
            ]
        );
    }

    /// `#queuecommanddraftguard`: the tmux path refuses before send-keys. The
    /// composer snapshot stands in for the pane, so this test never calls tmux.
    #[test]
    fn dispatch_tmux_refuses_a_foreign_draft_before_send_keys() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session_doc(dir.path());
        let item = classify("/doctor");
        let mut ctx = DispatchContext::from_file(&doc).unwrap();
        ctx.project_root = None;
        ctx.session_uuid = None;
        ctx.pane_id = Some("%queue-command-draft".to_string());
        ctx.composer_read = Some(composer_read(claude_composer("❯ keep going")));

        let err = dispatch_command(&item, &ctx).unwrap_err().to_string();
        assert!(err.contains("foreign_draft"), "{err}");
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("reason=foreign_draft"), "{ops}");
        assert!(
            !ops.contains("queue_dispatch_progress transport=tmux_send_keys"),
            "a refusal must not reach send-keys: {ops}"
        );
    }

    /// `#queuecommanddraftguard`: a failed capture is unread even when a pane
    /// id is known, and the tmux path still types nothing.
    #[test]
    fn dispatch_tmux_refuses_a_failed_capture() {
        let dir = tempfile::tempdir().unwrap();
        let doc = session_doc(dir.path());
        let item = classify("/doctor");
        let mut ctx = DispatchContext::from_file(&doc).unwrap();
        ctx.project_root = None;
        ctx.session_uuid = None;
        ctx.pane_id = Some("%queue-command-draft".to_string());
        ctx.composer_read = Some(TestQueueCommandComposerRead {
            captured: false,
            content: claude_composer("❯"),
            cursor_y: None,
        });

        let err = dispatch_command(&item, &ctx).unwrap_err().to_string();
        assert!(err.contains("composer_unreadable"), "{err}");
    }

    #[test]
    fn queue_command_paths_consult_the_composer_guard_before_typing() {
        let source = include_str!("queue_dispatch.rs");
        let supervisor = source
            .split_once("fn try_supervisor_dispatch(")
            .and_then(|(_, tail)| tail.split_once("\nfn try_tmux_dispatch("))
            .map(|(body, _)| body)
            .expect("supervisor dispatch");
        let tmux = source
            .split_once("fn try_tmux_dispatch(")
            .and_then(|(_, tail)| tail.split_once("\n/// Look up the tmux pane"))
            .map(|(body, _)| body)
            .expect("tmux dispatch");
        for (name, body) in [("supervisor", supervisor), ("tmux", tmux)] {
            let guard = body
                .find("guard_queue_command_before_typing(")
                .unwrap_or_else(|| panic!("{name} must call the composer guard"));
            let typed = body
                .find("IpcMethod::Inject")
                .or_else(|| body.find("send_submitted_text_for_harness_logged("))
                .unwrap_or_else(|| panic!("{name} must still type"));
            assert!(guard < typed, "{name} types before reading the composer");
        }
        let observe = source
            .split_once("fn read_queue_command_composer(")
            .and_then(|(_, tail)| tail.split_once("\nfn guard_queue_command_before_typing("))
            .map(|(body, _)| body)
            .expect("composer read");
        assert!(
            observe.contains("capture_pane_with_ansi("),
            "the composer read must use a styled capture"
        );
        assert!(
            !observe.contains("capture_pane("),
            "a plain capture cannot tell a ghost from a draft"
        );
        let clear = source
            .split_once("fn dispatch_clear(")
            .and_then(|(_, tail)| tail.split_once("\nfn dispatch_inline("))
            .map(|(body, _)| body)
            .expect("clear");
        assert!(
            !clear.contains("guard_queue_command_before_typing("),
            "session clear keeps its own guard"
        );
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
