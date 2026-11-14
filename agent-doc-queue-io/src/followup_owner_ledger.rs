//! `#humanfollowupowner`: the follow-up rows preflight has already classified.
//!
//! Preflight queue maintenance tags an unstarted human-owned
//! `follow up on an action item` row with `[operator-verify]`
//! ([`agent_doc_queue::followup_owner`]). Absence of the tag is ambiguous: it
//! cannot distinguish "written before the tag existed" from "the operator
//! deleted it to hand the row back to an agent". So, as with the backlog
//! mirror's operator-delete tombstones ([`crate::queue_tombstone`]), the
//! operator's edit must stick: this ledger records the
//! [`agent_doc_queue::followup_owner::followup_owner_key`] of every row whose
//! owner has been decided, and preflight classifies only rows it has not
//! recorded. A key leaves the ledger once its row is no longer live (struck or
//! removed), so the ledger stays the size of the live queue.
//!
//! The ledger is best-effort: when the project `state.db` cannot be read,
//! every row is unseen and preflight falls back to tagging every untagged
//! human-owned row, which is the behaviour before the ledger existed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const STATE_KIND: &str = "human_followup_owner_classified";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ClassifiedFollowupState {
    #[serde(default)]
    classified: Vec<String>,
}

fn state_identity(doc: &Path) -> Option<(PathBuf, String, String)> {
    let canonical = doc.canonicalize().ok()?;
    let hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Some((project_root, hash, canonical.to_string_lossy().into_owned()))
}

/// The follow-up row keys preflight has classified for `doc`. Empty when the
/// ledger row is absent or unreadable.
pub fn classified_followup_keys(doc: &Path) -> HashSet<String> {
    let Some((project_root, document_hash, _)) = state_identity(doc) else {
        return HashSet::new();
    };
    let Ok(conn) = agent_doc_sqlite::state_store::open_state_db(&project_root) else {
        return HashSet::new();
    };
    let Ok(Some(record)) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        STATE_KIND,
    ) else {
        return HashSet::new();
    };
    match serde_json::from_str::<ClassifiedFollowupState>(&record.payload_json) {
        Ok(state) => state.classified.into_iter().collect(),
        Err(e) => {
            eprintln!(
                "[preflight] queue: ignoring unreadable follow-up owner ledger document_hash={document_hash}: {e}",
            );
            HashSet::new()
        }
    }
}

/// Record the follow-up rows of `content` (the document as written) whose
/// owner is now decided, and drop recorded keys whose row is no longer live.
/// Call after the document write, so a pass that never wrote its tags leaves
/// those rows unseen. Best-effort: a write failure is logged, never fatal.
pub fn record_classified_followups(doc: &Path, content: &str) {
    let keys = match agent_doc_queue::followup_owner::followup_row_keys(content) {
        Ok(keys) => keys,
        Err(e) => {
            eprintln!("[preflight] queue: could not read follow-up rows for the owner ledger: {e}");
            return;
        }
    };
    let previous = classified_followup_keys(doc);
    let next = next_classified(&previous, &keys);
    if next == previous {
        return;
    }
    save(doc, &next);
}

fn next_classified(
    previous: &HashSet<String>,
    keys: &agent_doc_queue::followup_owner::FollowupRowKeys,
) -> HashSet<String> {
    previous
        .intersection(&keys.live)
        .cloned()
        .chain(keys.classified.iter().cloned())
        .collect()
}

fn save(doc: &Path, classified: &HashSet<String>) {
    let Some((project_root, document_hash, canonical_path)) = state_identity(doc) else {
        return;
    };
    let mut classified: Vec<String> = classified.iter().cloned().collect();
    classified.sort();
    let payload_json = match serde_json::to_string(&ClassifiedFollowupState { classified }) {
        Ok(payload_json) => payload_json,
        Err(e) => {
            eprintln!("[preflight] queue: could not serialize the follow-up owner ledger: {e}");
            return;
        }
    };
    let result = agent_doc_sqlite::state_store::open_state_db(&project_root).and_then(|conn| {
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash,
                state_kind: STATE_KIND.to_string(),
                canonical_path,
                payload_json,
                updated_at_secs: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        )
    });
    if let Err(e) = result {
        eprintln!("[preflight] queue: could not persist the follow-up owner ledger: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ledger round-trips through `state.db`, keeps a key while its row is
    /// live in any lifecycle, and drops it once the row is struck.
    #[test]
    fn ledger_records_classified_rows_and_drops_struck_ones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "x").unwrap();
        let queue = |rows: &str| format!("<!-- agent:queue go -->\n{rows}<!-- /agent:queue -->\n");
        let row = "follow up on an action item from `x`: A non-author approves PR 9 [operator-verify] [#q-0123456789ab]";
        let key = agent_doc_queue::followup_owner::followup_owner_key(row).unwrap();

        assert!(classified_followup_keys(&doc).is_empty());
        record_classified_followups(&doc, &queue(&format!("- ⏳ {row}\n")));
        assert_eq!(classified_followup_keys(&doc), HashSet::from([key.clone()]));

        // The operator deletes the tag and a worker claims the row: still live,
        // so the key is kept.
        let claimed = row.replace(" [operator-verify]", "");
        record_classified_followups(&doc, &queue(&format!("- 🟡 [#subagent:a#1] {claimed}\n")));
        assert_eq!(classified_followup_keys(&doc), HashSet::from([key]));

        // Struck: the key is dropped.
        record_classified_followups(&doc, &queue(&format!("- ~~{claimed}~~\n")));
        assert!(classified_followup_keys(&doc).is_empty());
    }
}
