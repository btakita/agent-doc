//! Operator-delete tombstones for the backlog→queue mirror (#provauth2 slice 2).
//!
//! The go-mode / persisted-active backlog→queue mirror re-adds any active
//! `queue`-attr backlog id that is absent from the queue. Absence is ambiguous:
//! it cannot distinguish "never mirrored" from "the operator deleted it." So an
//! operator who deletes a `do [#id]` line from the queue sees it **reappear**
//! on the next preflight ("I deleted items from the queue but they reappeared").
//!
//! Provenance principle: an operator delete is an **authoritative** action and
//! must stick. This module records the ids the operator deleted (present-and-
//! active in the committed snapshot queue, now entirely gone from the live queue
//! — *not* merely struck/consumed) in a durable sidecar
//! `<project_root>/.agent-doc/queue-tombstones/<hash>.json`. The mirror then
//! skips re-adding tombstoned ids. A tombstone self-clears when the operator
//! re-adds the id (it reappears as an active head) — their re-add is just as
//! authoritative as their delete.
//!
//! Distinct from `agent:done` (#ynra) exclusion: that suppresses *completed*
//! work; this suppresses *operator-deleted* work that was never completed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_doc_queue::backlog_sync::reconcile_queue_tombstones;

const TOMBSTONE_DIR: &str = ".agent-doc/queue-tombstones";

/// `<project_root>/.agent-doc/queue-tombstones/<sha256_hash>.json`, mirroring the
/// snapshot/pending/crdt sidecar convention. Falls back to the document's parent
/// directory when no `.agent-doc/` project root is found.
pub(crate) fn tombstone_path_for(doc: &Path) -> Option<PathBuf> {
    let canonical = doc.canonicalize().ok()?;
    let hash = agent_doc_fs::document_state_hash_from_str(&canonical.to_string_lossy());
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Some(
        project_root
            .join(TOMBSTONE_DIR)
            .join(format!("{hash}.json")),
    )
}

/// Load the persisted operator-delete tombstone set (lowercased ids). Returns an
/// empty set when the sidecar is absent or unreadable — tombstones are advisory,
/// so a missing/corrupt file never blocks the mirror.
pub(crate) fn load(doc: &Path) -> HashSet<String> {
    let Some(path) = tombstone_path_for(doc) else {
        return HashSet::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return HashSet::new();
    };
    match serde_json::from_slice::<Vec<String>>(&bytes) {
        Ok(ids) => ids.into_iter().map(|id| id.to_ascii_lowercase()).collect(),
        Err(e) => {
            eprintln!(
                "[preflight] queue: ignoring unreadable tombstone sidecar {}: {e}",
                path.display()
            );
            HashSet::new()
        }
    }
}

/// Persist the tombstone set (sorted for stable diffs). Best-effort: a write
/// failure is logged but never fatal.
fn save(doc: &Path, ids: &HashSet<String>) {
    let Some(path) = tombstone_path_for(doc) else {
        return;
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "[preflight] queue: could not create tombstone dir {}: {e}",
            parent.display()
        );
        return;
    }
    let mut sorted: Vec<&String> = ids.iter().collect();
    sorted.sort();
    match serde_json::to_vec_pretty(&sorted) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                eprintln!(
                    "[preflight] queue: could not persist tombstones {}: {e}",
                    path.display()
                );
            }
        }
        Err(e) => eprintln!("[preflight] queue: could not serialize tombstones: {e}"),
    }
}

/// Reconcile the tombstone set against this cycle's queue evidence and return the
/// active set the mirror must skip.
///
/// - `snapshot_active_ids` — do-ids that were **active** in the committed
///   snapshot queue (the last persisted state).
/// - `current_all_ids` — every do-id in the live queue now (active **and**
///   struck/completed).
/// - `current_active_ids` — do-ids that are active heads in the live queue now.
///
/// An id active in the snapshot but entirely gone now (not merely struck) was
/// deleted by the operator → tombstone it. An id the operator re-added (active
/// now) clears its tombstone. The updated set is persisted and returned.
pub(crate) fn reconcile_for_file(
    doc: &Path,
    snapshot_active_ids: &HashSet<String>,
    current_all_ids: &HashSet<String>,
    current_active_ids: &HashSet<String>,
) -> HashSet<String> {
    let previous = load(doc);
    let tomb = reconcile_queue_tombstones(
        &previous,
        snapshot_active_ids,
        current_all_ids,
        current_active_ids,
    );
    if tomb != previous {
        save(doc, &tomb);
    }
    tomb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn deleted_active_id_is_tombstoned_then_cleared_on_readd() {
        let dir = tempfile::tempdir().unwrap();
        // create a fake project root with .agent-doc so find_project_root works
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "x").unwrap();

        // Snapshot had #a and #b active; live queue now lacks #a (deleted) but
        // keeps #b. #a must be tombstoned.
        let tomb = reconcile_for_file(&doc, &set(&["a", "b"]), &set(&["b"]), &set(&["b"]));
        assert!(tomb.contains("a"), "deleted active id must be tombstoned");
        assert!(!tomb.contains("b"));
        // Persisted.
        assert!(load(&doc).contains("a"));

        // Operator re-adds #a (active again) → tombstone clears.
        let tomb2 = reconcile_for_file(&doc, &set(&["b"]), &set(&["a", "b"]), &set(&["a", "b"]));
        assert!(!tomb2.contains("a"), "re-added id clears its tombstone");
        assert!(!load(&doc).contains("a"));
    }

    #[test]
    fn struck_id_is_not_treated_as_a_delete() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "x").unwrap();

        // #a was active in the snapshot; now it is struck (still in
        // current_all_ids, absent from current_active_ids). Consumption, not a
        // delete — must NOT be tombstoned.
        let tomb = reconcile_for_file(&doc, &set(&["a"]), &set(&["a"]), &set(&[]));
        assert!(
            !tomb.contains("a"),
            "a struck/consumed id is not an operator delete"
        );
    }
}
