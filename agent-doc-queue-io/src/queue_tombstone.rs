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
//! — *not* merely struck/consumed) in the project's durable state ledger. The mirror then
//! skips re-adding tombstoned ids. A tombstone self-clears when the operator
//! re-adds the id (it reappears as an active head) — their re-add is just as
//! authoritative as their delete.
//!
//! Distinct from `agent:done` (#ynra) exclusion: that suppresses *completed*
//! work; this suppresses *operator-deleted* work that was never completed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_doc_queue::backlog_sync::reconcile_queue_tombstones;
use serde::{Deserialize, Serialize};

const TOMBSTONE_STATE_KIND: &str = "operator_delete_tombstones";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct QueueTombstoneState {
    #[serde(default)]
    tombstones: Vec<String>,
    /// Last editor-authoritative active-id frontier observed after queue
    /// maintenance. This closes the snapshot gap: a head introduced after the
    /// last committed snapshot can still be recognized as operator-deleted on
    /// the next pass.
    #[serde(default)]
    observed_active_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LoadedQueueTombstones {
    tombstones: HashSet<String>,
    observed_active_ids: HashSet<String>,
}

fn state_identity(doc: &Path) -> Option<(PathBuf, String, String)> {
    let canonical = doc.canonicalize().ok()?;
    let hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Some((project_root, hash, canonical.to_string_lossy().into_owned()))
}

/// Load the persisted operator-delete tombstone set (lowercased ids). Returns an
/// empty set when the ledger row is absent or unreadable.
fn load(doc: &Path) -> LoadedQueueTombstones {
    let Some((project_root, document_hash, _)) = state_identity(doc) else {
        return LoadedQueueTombstones::default();
    };
    let Ok(conn) = agent_doc_sqlite::state_store::open_state_db(&project_root) else {
        return LoadedQueueTombstones::default();
    };
    let Ok(Some(record)) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        TOMBSTONE_STATE_KIND,
    ) else {
        return LoadedQueueTombstones::default();
    };
    match serde_json::from_str::<QueueTombstoneState>(&record.payload_json) {
        Ok(state) => LoadedQueueTombstones {
            tombstones: state
                .tombstones
                .into_iter()
                .map(|id| id.to_ascii_lowercase())
                .collect(),
            observed_active_ids: state
                .observed_active_ids
                .into_iter()
                .map(|id| id.to_ascii_lowercase())
                .collect(),
        },
        Err(e) => {
            eprintln!(
                "[preflight] queue: ignoring unreadable tombstone state document_hash={document_hash}: {e}",
            );
            LoadedQueueTombstones::default()
        }
    }
}

/// Persist the tombstone set (sorted for stable diffs). Best-effort: a write
/// failure is logged but never fatal.
fn save(doc: &Path, state: &LoadedQueueTombstones) {
    let Some((project_root, document_hash, canonical_path)) = state_identity(doc) else {
        return;
    };
    let mut tombstones: Vec<String> = state.tombstones.iter().cloned().collect();
    tombstones.sort();
    let mut observed_active_ids: Vec<String> = state.observed_active_ids.iter().cloned().collect();
    observed_active_ids.sort();
    let stored = QueueTombstoneState {
        tombstones,
        observed_active_ids,
    };
    match serde_json::to_string(&stored) {
        Ok(payload_json) => match agent_doc_sqlite::state_store::open_state_db(&project_root)
            .and_then(|conn| {
                agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                    &conn,
                    &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                        document_hash,
                        state_kind: TOMBSTONE_STATE_KIND.to_string(),
                        canonical_path,
                        payload_json,
                        updated_at_secs: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    },
                )
            }) {
            Ok(()) => {}
            Err(e) => eprintln!("[preflight] queue: could not persist tombstones: {e}"),
        },
        Err(e) => eprintln!("[preflight] queue: could not serialize tombstones: {e}"),
    }
}

/// Load the persisted operator-delete tombstone set (lowercased ids) without
/// reconciling. Callers that need the authoritative deletion set at a point in
/// the cycle after the reconcile already ran use this; it returns an empty set
/// when the ledger is absent or unreadable.
pub fn current_tombstones(doc: &Path) -> HashSet<String> {
    load(doc).tombstones
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
pub fn reconcile_for_file(
    doc: &Path,
    snapshot_active_ids: &HashSet<String>,
    current_all_ids: &HashSet<String>,
    current_active_ids: &HashSet<String>,
) -> HashSet<String> {
    let previous = load(doc);
    let mut deletion_anchors = snapshot_active_ids.clone();
    deletion_anchors.extend(previous.observed_active_ids.iter().cloned());
    let tomb = reconcile_queue_tombstones(
        &previous.tombstones,
        &deletion_anchors,
        current_all_ids,
        current_active_ids,
    );
    let next = LoadedQueueTombstones {
        tombstones: tomb.clone(),
        observed_active_ids: current_active_ids.clone(),
    };
    if next != previous {
        save(doc, &next);
    }
    tomb
}

/// Persist the final editor-authoritative active-id frontier after automatic
/// queue maintenance has finished. The next preflight can then distinguish a
/// genuine operator deletion from an id that simply never appeared in the last
/// committed snapshot.
pub fn record_observed_active_ids(doc: &Path, current_active_ids: &HashSet<String>) {
    let mut state = load(doc);
    let normalized = current_active_ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    if state.observed_active_ids != normalized {
        state.observed_active_ids = normalized;
        save(doc, &state);
    }
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
        assert!(load(&doc).tombstones.contains("a"));

        // Operator re-adds #a (active again) → tombstone clears.
        let tomb2 = reconcile_for_file(&doc, &set(&["b"]), &set(&["a", "b"]), &set(&["a", "b"]));
        assert!(!tomb2.contains("a"), "re-added id clears its tombstone");
        assert!(!load(&doc).tombstones.contains("a"));
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

    #[test]
    fn observed_frontier_detects_delete_missing_from_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "x").unwrap();

        record_observed_active_ids(&doc, &set(&["late"]));
        let tomb = reconcile_for_file(&doc, &set(&[]), &set(&[]), &set(&[]));
        assert!(
            tomb.contains("late"),
            "an editor-deleted head remains provable even when the snapshot lagged its addition"
        );
    }
}
