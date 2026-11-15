//! `#followuprootprovenance`: the durable root-item ledger for follow-up rows.
//!
//! `queue report-complete` and `queue follow-up` record, for each row they
//! create, the tracked item it descends from
//! ([`agent_doc_queue::followup_provenance`]). The backlog→queue mirrors read
//! it so that an open item with a live follow-up row is already in the queue.
//!
//! The ledger is one typed `state.db` record per document, beside the
//! follow-up owner ledger ([`crate::followup_owner_ledger`]). It is written
//! only by the commands that create follow-up rows, which hold the queue
//! apply guard, so its read-modify-write never races another writer. Mirrors
//! only read it. A record whose row no queue holds any more is dropped on the
//! next write, so the ledger stays the size of the queue's follow-up history.
//!
//! The ledger is best-effort: when `state.db` cannot be read, no row has
//! provenance and every mirror behaves exactly as it did before the ledger
//! existed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_doc_queue::followup_provenance::{self, FollowupRootLedger};

const STATE_KIND: &str = "followup_root_item_provenance";

fn state_identity(doc: &Path) -> Option<(PathBuf, String, String)> {
    let canonical = doc.canonicalize().ok()?;
    let hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Some((project_root, hash, canonical.to_string_lossy().into_owned()))
}

/// The recorded follow-up root provenance of `doc`. Empty when the record is
/// absent or unreadable.
pub fn load(doc: &Path) -> FollowupRootLedger {
    let Some((project_root, document_hash, _)) = state_identity(doc) else {
        return FollowupRootLedger::default();
    };
    let Ok(conn) = agent_doc_sqlite::state_store::open_state_db(&project_root) else {
        return FollowupRootLedger::default();
    };
    let Ok(Some(record)) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        STATE_KIND,
    ) else {
        return FollowupRootLedger::default();
    };
    match serde_json::from_str::<FollowupRootLedger>(&record.payload_json) {
        Ok(ledger) => ledger,
        Err(e) => {
            eprintln!(
                "[queue] ignoring unreadable follow-up root provenance document_hash={document_hash}: {e}",
            );
            FollowupRootLedger::default()
        }
    }
}

/// Root item ids of every live follow-up row of `content` (the document the
/// mirror is about to change). Every backlog→queue mirror counts these as
/// already queued. Empty when the ledger is empty or unreadable.
pub fn live_root_item_ids(doc: &Path, content: &str) -> HashSet<String> {
    let ledger = load(doc);
    if ledger.is_empty() {
        return HashSet::new();
    }
    match followup_provenance::live_followup_root_ids(content, &ledger) {
        Ok(roots) => roots,
        Err(e) => {
            eprintln!("[queue] could not read follow-up rows for root provenance: {e}");
            HashSet::new()
        }
    }
}

/// Record the root of the rows `inserted` created from `origin_rows` in
/// `content` (the document as the creating command wrote it), and drop
/// records whose row is gone. Call while holding the queue apply guard.
/// Returns the root, if the origin has one. Best-effort: a failure is logged,
/// never fatal, because the rows already landed.
pub fn record_followup_roots(
    doc: &Path,
    content: &str,
    origin_rows: &[&str],
    inserted: &[String],
) -> Option<String> {
    if inserted.is_empty() {
        return None;
    }
    let previous = load(doc);
    let (next, root) = match followup_provenance::plan_followup_root_record(
        &previous,
        content,
        origin_rows,
        inserted,
    ) {
        Ok(Some(plan)) => plan,
        Ok(None) => {
            return origin_rows
                .iter()
                .find_map(|origin| followup_provenance::origin_root_item_id(origin, &previous));
        }
        Err(e) => {
            eprintln!("[queue] could not plan follow-up root provenance: {e}");
            return None;
        }
    };
    save(doc, &next);
    if let Some(root) = &root {
        agent_doc_ops_log_io::log_op(
            doc,
            &format!(
                "followup_root_recorded root=#{root} rows={} (#followuprootprovenance)",
                inserted.len()
            ),
        );
    }
    root
}

fn save(doc: &Path, ledger: &FollowupRootLedger) {
    let Some((project_root, document_hash, canonical_path)) = state_identity(doc) else {
        return;
    };
    let payload_json = match serde_json::to_string(ledger) {
        Ok(payload_json) => payload_json,
        Err(e) => {
            eprintln!("[queue] could not serialize follow-up root provenance: {e}");
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
        eprintln!("[queue] could not persist follow-up root provenance: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_text(run: &str) -> String {
        format!(
            "<!-- agent:run go -->\n{run}<!-- /agent:run -->\n\n\
             <!-- agent:backlog queue -->\n- [ ] [#alpha] a\n<!-- /agent:backlog -->\n"
        )
    }

    /// The ledger round-trips through `state.db`; a nested row inherits the
    /// root recorded for its origin; a struck row stops counting as live.
    #[test]
    fn roots_round_trip_and_nest_through_state_db() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "x").unwrap();
        assert!(load(&doc).is_empty());

        let first = "follow up on an action item from `alpha`: agent: do more".to_string();
        let content = doc_text(&format!("- 📥 {first}\n"));
        assert_eq!(
            record_followup_roots(
                &doc,
                &content,
                &["do [#alpha]"],
                std::slice::from_ref(&first)
            ),
            Some("alpha".to_string())
        );
        assert_eq!(
            live_root_item_ids(&doc, &content),
            HashSet::from(["alpha".to_string()])
        );

        let nested = "follow up on an action item from queue item “follow up on an action item from `a…”: agent: finish".to_string();
        let running = format!("🚧 [#subagent:a#1] {first} [#q-0123456789ab]");
        let content = doc_text(&format!("- {running}\n- {nested}\n"));
        assert_eq!(
            record_followup_roots(
                &doc,
                &content,
                &[&running, &first],
                std::slice::from_ref(&nested)
            ),
            Some("alpha".to_string())
        );
        let struck = doc_text(&format!("- ~~{first}~~\n- {nested}\n"));
        assert_eq!(
            live_root_item_ids(&doc, &struck),
            HashSet::from(["alpha".to_string()])
        );
        let all_struck = doc_text(&format!("- ~~{first}~~\n- ~~{nested}~~\n"));
        assert!(live_root_item_ids(&doc, &all_struck).is_empty());
        assert_eq!(load(&doc).rows.len(), 2);
    }
}
