//! # Module: queue_continuation
//!
//! Binary-owned "queue continuation required" final gate
//! (`#codex-auto-queue-stalled-final-gate`).
//!
//! Codex auto-queue continuation historically depended on the `codex-stop` hook
//! finding tracked in-memory session state and then calling
//! `active_auto_queue_prompt`. That is too fragile for the live failure mode:
//! the Stop hook can miss the document when `UserPromptSubmit` did not persist
//! state for the exact API/session/root shape, or when the turn closed through a
//! manual / recovery path after a recursive direct-invocation rejection. A clean
//! `session-check` is not enough either — a committed document can still owe an
//! auto-queue continuation.
//!
//! The only durable proof after closeout is the document itself
//! (`queue_active: true` and a ready head) plus the durable marker this module
//! persists at successful closeout. `auto` is only a *start* trigger; once a
//! queue is active, continuation is driven by `queue_active: true`, so a
//! persisted-active `agent:queue` (no `auto` attribute) is equally eligible
//! (`#active-queue-persisted-no-continue`). The detector here is the single
//! shared source of truth; `session-check`, the `codex-stop` hook, and the
//! closeout paths all consult it instead of duplicating the activation
//! reasoning.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A required auto-queue continuation: the document closed cleanly but a ready
/// `agent:queue auto` head remains, so a Codex-managed turn must continue with
/// `agent-doc <FILE>` instead of sending a final answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueContinuation {
    pub head_prompt: String,
    pub head_id: Option<String>,
    pub reason: String,
}

/// Detect whether `file` currently requires queue continuation.
///
/// True only when: frontmatter `queue_active: true`,
/// [`crate::queue::resolve_activation`] is active, the head is a real prompt
/// (not a stop fence or future time gate), and the head was not edited between
/// the committed snapshot and the file.
///
/// `auto` is a *start* trigger only; once a queue is active (`queue_active:
/// true`) continuation is driven by the active state, not the opening-tag
/// attribute, so a persisted-active `agent:queue` (no `auto`) is equally
/// eligible (`#active-queue-persisted-no-continue`). An inactive plain queue
/// never reaches here because the `queue_active` guard above fails first. This
/// mirrors the codex-hook `active_auto_queue_prompt` logic in one shared,
/// testable place.
pub fn detect(file: &Path) -> Result<Option<QueueContinuation>> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    detect_in_content(file, &content)
}

fn detect_in_content(file: &Path, content: &str) -> Result<Option<QueueContinuation>> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = crate::frontmatter::parse_for_file_with_context(content, file, &rc)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = crate::component::parse(content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(None);
    };
    // `auto` is a start trigger only — continuation is gated on `queue_active:
    // true` (checked above), so a persisted-active queue without `auto` still
    // continues (`#active-queue-persisted-no-continue`).
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);

    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body).context("queue continuation: failed to parse queue")?;
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active
        || crate::queue::has_stop_fence_at_head(&activation.entries_after)
        || crate::queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return Ok(None);
    }

    // A head edited between the committed snapshot and the file is not a clean
    // continuation — the operator changed the next prompt, so defer to the
    // normal preflight/halt path rather than forcing continuation.
    if let Some(snapshot_content) = crate::snapshot::load(file)?
        && let Ok(snapshot_components) = crate::component::parse(&snapshot_content)
        && let Some(snapshot_queue) = snapshot_components
            .iter()
            .find(|component| component.name == "queue")
    {
        let snapshot_body = &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
        if let Ok(snapshot_entries) = crate::queue::parse(snapshot_body) {
            let snapshot_has_auto = crate::queue::has_auto_attr(&snapshot_queue.attrs);
            let snapshot_activation =
                crate::queue::resolve_activation(&snapshot_entries, snapshot_has_auto, false, true);
            if crate::queue::detect_head_prompt_modified(
                &snapshot_activation.entries_after,
                &activation.entries_after,
            ) {
                return Ok(None);
            }
        }
    }

    let Some(head) = crate::queue::first_prompt(&activation.entries_after) else {
        return Ok(None);
    };
    let head_prompt = head.text.clone();
    let head_id = extract_head_id(&head_prompt);
    let reason = if has_auto {
        "active `agent:queue auto` still has a ready head prompt after a clean closeout"
    } else {
        "active persisted `agent:queue` (queue_active: true) still has a ready head prompt after a clean closeout"
    }
    .to_string();
    Ok(Some(QueueContinuation {
        reason,
        head_id,
        head_prompt,
    }))
}

/// The live auto-queue continuation head of a document **string**, independent
/// of any snapshot/sidecar. Returns `Some(head_id_or_prompt)` when `content` has
/// an active queue (`queue_active: true`) whose head is a ready prompt — not a
/// stop fence or a future time gate — else `None`.
///
/// Unlike [`detect`], this performs no snapshot-edit comparison: callers that
/// already hold two explicit document strings use it to compare continuation
/// state across snapshot / HEAD / working without a sidecar round-trip. It is the
/// authoritative-side signal for closeout metadata-drift recovery
/// (`#recovery-drift-authoritative-side`): a live continuation present in HEAD
/// but absent (or re-headed) in a metadata-only local drift means HEAD is
/// authoritative, because legitimate consumption of a queue head always shows up
/// as response/content drift, never as metadata-only drift.
pub fn live_continuation_head(file: &Path, content: &str) -> Option<String> {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = crate::frontmatter::parse_for_file_with_context(content, file, &rc).ok()?;
    if fm.queue_active != Some(true) {
        return None;
    }
    let components = crate::component::parse(content).ok()?;
    let queue_component = components.iter().find(|c| c.name == "queue")?;
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body).ok()?;
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active
        || crate::queue::has_stop_fence_at_head(&activation.entries_after)
        || crate::queue::time_gate_at_head(&activation.entries_after).is_some()
    {
        return None;
    }
    let head = crate::queue::first_prompt(&activation.entries_after)?;
    Some(extract_head_id(&head.text).unwrap_or_else(|| head.text.trim().to_string()))
}

/// Extract the backlog `#id` from a queue prompt like `do [#id] ...` or `#id ...`.
fn extract_head_id(prompt: &str) -> Option<String> {
    if let Some(start) = prompt.find("[#")
        && let Some(end) = prompt[start + 2..].find(']')
    {
        let id = prompt[start + 2..start + 2 + end].trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    prompt
        .split_whitespace()
        .find_map(|token| {
            token.strip_prefix('#').map(|rest| {
                rest.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                    .to_string()
            })
        })
        .filter(|id| !id.is_empty())
}

/// Durable on-disk proof that a closed-out document still owes an auto-queue
/// continuation. Survives missing Codex hook session state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContinuationMarker {
    pub file: String,
    pub head_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_id: Option<String>,
    pub created_at: u64,
    pub source_command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_head: Option<String>,
    /// The head prompt last surfaced to a Codex Stop hook as a continuation
    /// request. Lets the hook fail closed when a repeated stop sees the same,
    /// non-advancing head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_requested_head: Option<String>,
}

fn marker_path(file: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = crate::fs_util::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(
        root.join(".agent-doc/queue-continuations")
            .join(format!("{hash}.json")),
    ))
}

/// Reconcile the durable continuation marker for `file` after a successful
/// closeout: write it when a continuation is required, clear it otherwise
/// (queue drained, `auto` removed, `queue_active` false, or head advanced).
/// Best-effort and never fatal to closeout — a marker write/clear failure is
/// logged, not propagated.
pub fn reconcile_marker(file: &Path, source_command: &str) -> Option<QueueContinuation> {
    match detect(file) {
        Ok(Some(continuation)) => {
            if let Err(err) = write_marker(file, &continuation, source_command) {
                eprintln!(
                    "[queue-continuation] WARNING: failed to write continuation marker for {}: {}",
                    file.display(),
                    err
                );
            }
            Some(continuation)
        }
        Ok(None) => {
            if let Err(err) = clear_marker(file) {
                eprintln!(
                    "[queue-continuation] WARNING: failed to clear continuation marker for {}: {}",
                    file.display(),
                    err
                );
            }
            None
        }
        Err(err) => {
            eprintln!(
                "[queue-continuation] WARNING: continuation detect failed for {}: {}",
                file.display(),
                err
            );
            None
        }
    }
}

pub fn write_marker(
    file: &Path,
    continuation: &QueueContinuation,
    source_command: &str,
) -> Result<()> {
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    // Preserve the last continuation request across reconciles so the Stop-hook
    // non-advancing-head guard still works after a re-detect.
    let last_requested_head = load_marker(file)?.and_then(|marker| marker.last_requested_head);
    let marker = ContinuationMarker {
        file: file.display().to_string(),
        head_prompt: continuation.head_prompt.clone(),
        head_id: continuation.head_id.clone(),
        created_at: now_secs(),
        source_command: source_command.to_string(),
        commit_head: head_oid(file),
        last_requested_head,
    };
    let json = serde_json::to_string_pretty(&marker).context("serialize continuation marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn clear_marker(file: &Path) -> Result<()> {
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

fn cooldown_marker_path(file: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = crate::fs_util::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(
        root.join(".agent-doc/queue-cooldowns")
            .join(format!("{hash}.json")),
    ))
}

pub fn write_clear_cooldown(file: &Path) -> Result<()> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let payload = serde_json::json!({
        "file": file.to_string_lossy(),
        "written_at": now_secs(),
    });
    let json = serde_json::to_string_pretty(&payload)
        .context("serialize cooldown marker")?;
    std::fs::write(&path, json)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn clear_cooldown_marker(file: &Path) -> Result<()> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

pub fn clear_cooldown_active(file: &Path) -> Result<bool> {
    let Some(path) = cooldown_marker_path(file)? else {
        return Ok(false);
    };
    match std::fs::read_to_string(&path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

pub fn load_marker(file: &Path) -> Result<Option<ContinuationMarker>> {
    let Some(path) = marker_path(file)? else {
        return Ok(None);
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(serde_json::from_str(&content).ok()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

/// Record that the head prompt was surfaced to a Codex Stop hook as a
/// continuation request, so a subsequent stop with the same head can fail
/// closed instead of looping. No-op when no marker exists.
pub fn record_requested_head(file: &Path, head_prompt: &str) -> Result<()> {
    let Some(mut marker) = load_marker(file)? else {
        return Ok(());
    };
    marker.last_requested_head = Some(head_prompt.to_string());
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    let json = serde_json::to_string_pretty(&marker).context("serialize continuation marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Scan every project root for a durable continuation marker whose document
/// still requires continuation. Used by the Codex Stop hook when no tracked
/// in-memory session state is available. Returns the first still-valid
/// `(file, continuation, marker)`.
/// `#codex-stop-cross-doc-queue-continuation`: a durable Stop-hook marker is
/// owned by *some* document; the fallback must not instruct the current Codex
/// pane to run a document owned by ANOTHER live actor. A marker doc is foreign
/// when it has a live (non-Closed) authoritative actor bound to a pane other
/// than `current_pane`. Unknown / closed / unowned ownership is NOT foreign
/// (allowed — covers the safe-claim and same-session cases). `current_pane` of
/// `None` (no tmux context) disables the gate and preserves prior behavior.
fn is_foreign_owned_marker(root: &Path, doc: &Path, current_pane: &str) -> bool {
    match crate::project_controller::authoritative_actor_binding(root, doc) {
        Ok(Some(record))
            if record.state != crate::session_actor::ActorState::Closed
                && !record.pane_id.trim().is_empty() =>
        {
            record.pane_id != current_pane
        }
        _ => false,
    }
}

/// Find the first still-valid durable `agent:queue auto` continuation marker
/// across `roots`. `current_pane` is the tmux pane of the Codex session whose
/// Stop hook is asking; markers owned by a different live pane are skipped (the
/// scan continues) so the hook never tells pane A to run document B while B has
/// its own live owner (`#codex-stop-cross-doc-queue-continuation`).
pub fn pending_marker_continuation_for_roots(
    roots: &[PathBuf],
    current_pane: Option<&str>,
) -> Result<Option<(PathBuf, QueueContinuation, ContinuationMarker)>> {
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        let dir = root.join(".agent-doc/queue-continuations");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err).with_context(|| format!("read {}", dir.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(marker) = serde_json::from_str::<ContinuationMarker>(&content) else {
                continue;
            };
            let doc = PathBuf::from(&marker.file);
            if !seen.insert(doc.clone()) {
                continue;
            }
            // `#codex-stop-cross-doc-queue-continuation`: skip a marker owned by
            // another live actor (different pane) and keep scanning, so this
            // Codex pane is never told to run a foreign-owned document. Does NOT
            // remove the marker — it stays for that document's own owner.
            if let Some(current) = current_pane
                && is_foreign_owned_marker(root, &doc, current)
            {
                crate::ops_log::log_op(
                    &doc,
                    &format!(
                        "codex_stop_foreign_queue_marker_skip file={} current_pane={}",
                        doc.display(),
                        current
                    ),
                );
                continue;
            }
            // The marker is durable but advisory — re-confirm against the live
            // document so a stale marker (queue since drained / edited) never
            // forces a spurious continuation.
            match detect(&doc)? {
                Some(continuation) => return Ok(Some((doc, continuation, marker))),
                None => {
                    // Stale marker — clean it up so it cannot mislead later.
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
    Ok(None)
}

fn head_oid(file: &Path) -> Option<String> {
    let dir = file.parent()?;
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!oid.is_empty()).then_some(oid)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_doc(dir: &Path, prompts: &[&str], queue_active: bool, has_auto: bool) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/snapshots")).unwrap();
        let doc = dir.join("task.md");
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        let auto = if has_auto { " auto" } else { "" };
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: {queue_active}\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue{auto} -->\n{queue}<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        doc
    }

    #[test]
    fn detect_returns_head_for_active_auto_queue() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#seopdp] next", "do [#third]"],
            true,
            true,
        );
        let continuation = detect(&doc).unwrap().expect("ready auto-queue head");
        assert_eq!(continuation.head_prompt, "do [#seopdp] next");
        assert_eq!(continuation.head_id.as_deref(), Some("seopdp"));
    }

    #[test]
    fn detect_returns_head_for_persisted_active_queue_without_auto() {
        // `#active-queue-persisted-no-continue`: a persisted-active queue
        // (queue_active: true) without the `auto` attribute still owes
        // continuation — `auto` is a start trigger only.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["do [#persisted] next", "do [#third]"],
            true,
            false,
        );
        let continuation = detect(&doc).unwrap().expect("ready persisted-active head");
        assert_eq!(continuation.head_prompt, "do [#persisted] next");
        assert_eq!(continuation.head_id.as_deref(), Some("persisted"));
        assert!(
            continuation.reason.contains("persisted"),
            "persisted-active reason should name the persisted trigger, got: {}",
            continuation.reason
        );
    }

    #[test]
    fn detect_none_when_inactive_plain_queue() {
        // A queue without `auto` AND without `queue_active: true` must never
        // self-start — the `queue_active` guard fails first.
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]"], false, false);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_queue_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]"], false, true);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_queue_drained() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &[], true, true);
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn detect_none_when_stop_fence_at_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["-- stop placeholder"], true, true);
        // Replace the queue body with a real stop fence at the head.
        let content = std::fs::read_to_string(&doc)
            .unwrap()
            .replace("- -- stop placeholder\n", "--- stop\n- do [#x]\n");
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        // A stop fence at the head must not force continuation.
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn extract_head_id_handles_bracket_and_bare() {
        assert_eq!(extract_head_id("do [#abc] thing").as_deref(), Some("abc"));
        assert_eq!(
            extract_head_id("#bare-id do it").as_deref(),
            Some("bare-id")
        );
        assert_eq!(extract_head_id("no id here"), None);
    }

    #[test]
    fn reconcile_marker_writes_then_clears() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#seopdp]"], true, true);

        // Active continuation → marker written.
        let continuation = reconcile_marker(&doc, "commit").expect("continuation required");
        assert_eq!(continuation.head_prompt, "do [#seopdp]");
        let marker = load_marker(&doc).unwrap().expect("marker persisted");
        assert_eq!(marker.head_prompt, "do [#seopdp]");
        assert_eq!(marker.source_command, "commit");

        // Drain the queue (queue_active flips false) → marker cleared.
        let _ = write_doc(dir.path(), &["do [#seopdp]"], false, true);
        assert!(reconcile_marker(&doc, "commit").is_none());
        assert!(load_marker(&doc).unwrap().is_none());
    }

    #[test]
    fn pending_marker_for_roots_finds_then_prunes_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let doc = write_doc(&root, &["do [#seopdp]"], true, true);
        reconcile_marker(&doc, "commit").expect("marker written");

        // The marker is found and re-confirmed against the live document.
        let found = pending_marker_continuation_for_roots(&[root.clone()], None)
            .unwrap()
            .expect("durable continuation found");
        assert_eq!(found.0, doc);
        assert_eq!(found.1.head_prompt, "do [#seopdp]");

        // Drain the queue but leave the marker file on disk (stale).
        let _ = write_doc(&root, &["do [#seopdp]"], false, true);
        let path = marker_path(&doc).unwrap().unwrap();
        assert!(path.exists(), "stale marker still on disk before scan");
        // Scan re-confirms against the document, finds it no longer owes
        // continuation, returns None, and prunes the stale marker.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], None)
                .unwrap()
                .is_none()
        );
        assert!(!path.exists(), "stale marker pruned during scan");
    }

    // `#codex-stop-cross-doc-queue-continuation`: a marker for a document owned
    // by another live actor's pane must be skipped (not driven) when the current
    // Codex pane differs; a same-pane / unowned marker is still returned.
    #[test]
    fn pending_marker_skips_foreign_owned_then_finds_same_pane() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // Foreign doc: owned by a live actor on pane %70.
        let foreign = write_doc(&root, &["do [#foreign]"], true, true);
        reconcile_marker(&foreign, "commit").expect("foreign marker written");
        crate::session_actor::project_binding_in(
            &root,
            &foreign.to_string_lossy(),
            "foreign-session",
            "%70",
            "@1",
            "test",
            "foreign_owner",
        )
        .unwrap();

        // From pane %74, the foreign-owned marker must be skipped → None.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], Some("%74"))
                .unwrap()
                .is_none(),
            "foreign-owned marker (pane %70) must be skipped from pane %74"
        );
        // The foreign marker must NOT be pruned — it belongs to its own owner.
        assert!(
            marker_path(&foreign).unwrap().unwrap().exists(),
            "foreign marker must survive the skip (not stale)"
        );

        // The foreign doc's OWN pane (%70) still drives its marker.
        let owned = pending_marker_continuation_for_roots(&[root.clone()], Some("%70"))
            .unwrap()
            .expect("same-pane owner drives its own marker");
        assert_eq!(owned.0, foreign);

        // Unknown pane context (None) preserves prior behavior — returns it.
        assert!(
            pending_marker_continuation_for_roots(&[root.clone()], None)
                .unwrap()
                .is_some(),
            "None current_pane disables the gate (prior behavior)"
        );
    }

    // `#codex-stop-cross-doc-queue-continuation` (scan ordering): a foreign-owned
    // marker scanned BEFORE a valid same-pane marker must be skipped while the
    // scan continues to return the later valid marker — never the foreign one.
    #[test]
    fn pending_marker_scan_continues_past_foreign_to_valid() {
        let foreign_dir = tempfile::tempdir().unwrap();
        let valid_dir = tempfile::tempdir().unwrap();
        let foreign_root = foreign_dir.path().to_path_buf();
        let valid_root = valid_dir.path().to_path_buf();

        // Foreign doc (scanned first): owned by a live actor on pane %70.
        let foreign = write_doc(&foreign_root, &["do [#foreign]"], true, true);
        reconcile_marker(&foreign, "commit").expect("foreign marker written");
        crate::session_actor::project_binding_in(
            &foreign_root,
            &foreign.to_string_lossy(),
            "foreign-session",
            "%70",
            "@1",
            "test",
            "foreign_owner",
        )
        .unwrap();

        // Valid doc (scanned second): owned by the current pane %74.
        let valid = write_doc(&valid_root, &["do [#valid]"], true, true);
        reconcile_marker(&valid, "commit").expect("valid marker written");
        crate::session_actor::project_binding_in(
            &valid_root,
            &valid.to_string_lossy(),
            "current-session",
            "%74",
            "@1",
            "test",
            "current_owner",
        )
        .unwrap();

        // From pane %74, the foreign root is scanned first; its %70-owned marker
        // is skipped and the scan continues to the %74-owned valid marker.
        let found = pending_marker_continuation_for_roots(
            &[foreign_root.clone(), valid_root.clone()],
            Some("%74"),
        )
        .unwrap()
        .expect("scan must continue past foreign marker to the valid one");
        assert_eq!(found.0, valid, "must return the same-pane valid doc, not foreign");
        assert_eq!(found.1.head_prompt, "do [#valid]");

        // The skipped foreign marker must survive for its own owner.
        assert!(
            marker_path(&foreign).unwrap().unwrap().exists(),
            "foreign marker must survive the skip (belongs to its own owner)"
        );
    }

    #[test]
    fn record_requested_head_persists_for_nonadvancing_guard() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#seopdp]"], true, true);
        reconcile_marker(&doc, "commit").expect("marker written");
        record_requested_head(&doc, "do [#seopdp]").unwrap();
        let marker = load_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.last_requested_head.as_deref(), Some("do [#seopdp]"));
        // A re-detect/reconcile preserves the requested head.
        reconcile_marker(&doc, "commit");
        let marker = load_marker(&doc).unwrap().unwrap();
        assert_eq!(marker.last_requested_head.as_deref(), Some("do [#seopdp]"));
    }
}
