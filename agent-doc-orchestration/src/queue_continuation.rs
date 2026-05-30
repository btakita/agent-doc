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
//! (`queue_active: true`, `agent:queue auto`, and a ready head) plus the durable
//! marker this module persists at successful closeout. The detector here is the
//! single shared source of truth; `session-check`, the `codex-stop` hook, and
//! the closeout paths all consult it instead of duplicating the activation
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

/// Detect whether `file` currently requires auto-queue continuation.
///
/// True only when: frontmatter `queue_active: true`, the `agent:queue` component
/// carries `auto`, [`crate::queue::resolve_activation`] is active, the head is a
/// real prompt (not a stop fence or future time gate), and the head was not
/// edited between the committed snapshot and the file. This mirrors the
/// codex-hook `active_auto_queue_prompt` logic in one shared, testable place.
pub fn detect(file: &Path) -> Result<Option<QueueContinuation>> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    detect_in_content(file, &content)
}

fn detect_in_content(file: &Path, content: &str) -> Result<Option<QueueContinuation>> {
    let (fm, _) = crate::frontmatter::parse_for_file(content, file)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = crate::component::parse(content)?;
    let Some(queue_component) = components.iter().find(|component| component.name == "queue") else {
        return Ok(None);
    };
    if !crate::queue::has_auto_attr(&queue_component.attrs) {
        return Ok(None);
    }

    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = crate::queue::parse(body).context("queue continuation: failed to parse queue")?;
    let activation = crate::queue::resolve_activation(&entries, true, false, true);
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
    Ok(Some(QueueContinuation {
        reason: "active `agent:queue auto` still has a ready head prompt after a clean closeout"
            .to_string(),
        head_id,
        head_prompt,
    }))
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
    prompt.split_whitespace().find_map(|token| {
        token.strip_prefix('#').map(|rest| {
            rest.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                .to_string()
        })
    }).filter(|id| !id.is_empty())
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
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
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
    let json = serde_json::to_string_pretty(&marker)
        .context("serialize continuation marker")?;
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
pub fn pending_marker_continuation_for_roots(
    roots: &[PathBuf],
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
        let doc = write_doc(dir.path(), &["do [#seopdp] next", "do [#third]"], true, true);
        let continuation = detect(&doc).unwrap().expect("ready auto-queue head");
        assert_eq!(continuation.head_prompt, "do [#seopdp] next");
        assert_eq!(continuation.head_id.as_deref(), Some("seopdp"));
    }

    #[test]
    fn detect_none_without_auto_attr() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#x]"], true, false);
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
        let content = std::fs::read_to_string(&doc).unwrap().replace(
            "- -- stop placeholder\n",
            "--- stop\n- do [#x]\n",
        );
        std::fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        // A stop fence at the head must not force continuation.
        assert!(detect(&doc).unwrap().is_none());
    }

    #[test]
    fn extract_head_id_handles_bracket_and_bare() {
        assert_eq!(extract_head_id("do [#abc] thing").as_deref(), Some("abc"));
        assert_eq!(extract_head_id("#bare-id do it").as_deref(), Some("bare-id"));
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
        let found = pending_marker_continuation_for_roots(&[root.clone()])
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
            pending_marker_continuation_for_roots(&[root.clone()])
                .unwrap()
                .is_none()
        );
        assert!(!path.exists(), "stale marker pruned during scan");
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
