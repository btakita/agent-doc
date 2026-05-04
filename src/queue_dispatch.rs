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
use crate::sessions;
use crate::snapshot;
use crate::supervisor::ipc as supervisor_ipc;

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
        })
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

/// Execute an inline-eligible command.
fn dispatch_inline(item: &QueueItem, ctx: &DispatchContext) -> Result<DispatchResult> {
    let command = item.command.as_deref().unwrap_or("");
    match command {
        "model" => {
            let tier = item
                .args
                .first()
                .ok_or_else(|| anyhow::anyhow!("/model requires a tier argument"))?;
            eprintln!("[queue_dispatch] /model {} — updating local override", tier);
            Ok(DispatchResult::ModelOverride(tier.clone()))
        }
        "compact" => {
            let default_file = ctx.file.to_string_lossy();
            let file_arg = item
                .args
                .first()
                .map(|s| s.as_str())
                .unwrap_or(&default_file);
            eprintln!(
                "[queue_dispatch] /compact {} — running subprocess",
                file_arg
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
                eprintln!(
                    "[queue_dispatch] warning: commit after compact exited with {}",
                    commit_status.code().unwrap_or(-1)
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

    eprintln!(
        "[queue_dispatch] dispatching `{}` via supervisor IPC",
        item.raw
    );

    // Use `inject` method to send the command text to the harness stdin.
    // The harness interprets `/command` lines natively.
    let bytes = supervisor_ipc::submit_bytes(&item.raw);
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

    eprintln!(
        "[queue_dispatch] dispatching `{}` via tmux send-keys to pane {}",
        item.raw, pane_id
    );

    let tmux = sessions::Tmux::default();

    // Send the command text + Enter to the pane
    tmux.send_keys(&pane_id, &item.raw)?;

    // Poll for completion: wait until the command text disappears from the
    // pane's last few visible lines (same approach as route.rs send_command).
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(30);
    let poll_interval = std::time::Duration::from_millis(500);
    let mut enter_retries = 0u32;

    while start.elapsed() < timeout {
        std::thread::sleep(poll_interval);
        if let Ok(content) = sessions::capture_pane(&tmux, &pane_id) {
            let still_visible = content
                .lines()
                .rev()
                .take(5)
                .any(|line| line.contains(&item.raw));

            if !still_visible {
                eprintln!(
                    "[queue_dispatch] command accepted ({:.1}s, {} Enter retries)",
                    start.elapsed().as_secs_f64(),
                    enter_retries
                );
                return Ok(Some(DispatchResult::Ok));
            }

            // Command text still in input — retry Enter
            enter_retries += 1;
            if let Err(e) = tmux.send_keys_raw(&pane_id, "Enter") {
                eprintln!("[queue_dispatch] warning: retry Enter failed: {}", e);
            }
        }
    }

    eprintln!(
        "[queue_dispatch] warning: command may not have been accepted after {:.1}s",
        start.elapsed().as_secs_f64()
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
        let ctx = DispatchContext {
            file: Path::new("test.md").to_path_buf(),
            project_root: None,
            session_uuid: None,
            pane_id: None,
        };
        match dispatch_inline(&item, &ctx).unwrap() {
            DispatchResult::ModelOverride(tier) => assert_eq!(tier, "sonnet"),
            _ => panic!("expected ModelOverride"),
        }
    }

    #[test]
    fn dispatch_inline_model_requires_arg() {
        let item = classify("/model");
        let ctx = DispatchContext {
            file: Path::new("test.md").to_path_buf(),
            project_root: None,
            session_uuid: None,
            pane_id: None,
        };
        assert!(dispatch_inline(&item, &ctx).is_err());
    }

    #[test]
    fn unknown_command_without_dispatch_path_fails() {
        let item = classify("/clear");
        let ctx = DispatchContext {
            file: Path::new("test.md").to_path_buf(),
            project_root: None,
            session_uuid: None,
            pane_id: None,
        };
        let result = dispatch_command(&item, &ctx);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no supervisor socket or tmux pane available"));
    }

    #[test]
    fn dispatch_supervisor_injects_command_with_shared_submit_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "---\nagent_doc_session: queue-session\n---\n").unwrap();

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured_for_ipc = captured.clone();
        let mut ipc = crate::supervisor::ipc::SupervisorIpc::start(
            dir.path(),
            "queue-session",
            move |method| match method {
                crate::supervisor::ipc::IpcMethod::Inject { bytes } => {
                    captured_for_ipc.lock().unwrap().push(bytes);
                    crate::supervisor::ipc::IpcResponse::ok_empty()
                }
                crate::supervisor::ipc::IpcMethod::State
                | crate::supervisor::ipc::IpcMethod::Pid
                | crate::supervisor::ipc::IpcMethod::Restart { .. }
                | crate::supervisor::ipc::IpcMethod::Stop { .. } => {
                    crate::supervisor::ipc::IpcResponse::ok_empty()
                }
            },
        )
        .unwrap();

        let item = classify("/clear");
        let ctx = DispatchContext::from_file(&doc).unwrap();
        let result = dispatch_command(&item, &ctx).unwrap();
        assert!(matches!(result, DispatchResult::Ok));
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[crate::supervisor::ipc::submit_bytes("/clear")]
        );

        ipc.stop();
    }
}
