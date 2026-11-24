//! `#stophookactiveguard`: the per-document record of the last
//! `#transientcloseout` refusal the Claude/Grok Stop hook issued.
//!
//! The transient-closeout gate refuses a turn end while live queue rows still
//! owe a transition. It never consulted `stop_hook_active`, so a debt set the
//! agent could not move in that turn was refused on every Stop until Claude
//! Code's own consecutive-block cap overrode the hook ("A hook blocked the turn
//! from ending 9 consecutive times — overriding and ending turn"). The harness
//! cap must not be the only guard.
//!
//! This record lets the gate refuse once per distinct debt set per turn. When
//! a recursive Stop (`stop_hook_active = true`) finds exactly the debt set it
//! refused last time for the same session, the agent already heard about it
//! and made no progress the hook can see; the gate allows the final answer and
//! leaves the rows live. A changed debt set (a row advanced, a new row
//! appeared) or a new session refuses again. The hook [`clear`]s the record on
//! every turn's first Stop, so a refusal from an earlier turn never satisfies
//! a later one. Each hook call is a new process, so the record lives in
//! `state.db`, not in memory.
//!
//! `#stoploopcap`: some debt sets keep refusing an unchanged recursive Stop
//! (`📥`, `🚧`, `⚠️🚧`, `☑️`). The record counts consecutive refusals of one
//! debt set for one session, and the gate ends the turn once that count
//! reaches [`UNCHANGED_REFUSAL_BOUND`], well before Claude Code's own cap.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const STATE_KIND: &str = "transient_closeout_refusal";

/// `#stoploopcap`: how many consecutive refusals of one unchanged debt set,
/// for one session in one turn, the Stop hook issues before it ends the turn
/// anyway. Claude Code overrides a hook after 9 consecutive blocks; the
/// harness cap must never be the only guard.
pub const UNCHANGED_REFUSAL_BOUND: u32 = 3;

fn one_refusal() -> u32 {
    1
}

/// The last transient-closeout refusal issued for a document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransientCloseoutRefusal {
    /// Harness session the refusal was issued to.
    pub session_id: String,
    /// Order-independent digest of the refused `(row, owed)` debts.
    pub debts_fingerprint: String,
    /// Number of refused debt rows, for diagnostics.
    pub debts: usize,
    pub refused_at_secs: u64,
    /// `#stoploopcap`: consecutive refusals of this exact debt set to this
    /// session since the record was last cleared or changed. A record written
    /// before this field existed counts as one refusal.
    #[serde(default = "one_refusal")]
    pub refusals: u32,
}

/// Order-independent digest of the refused `(row text, owed transition)` debts.
///
/// The row text carries its lifecycle prefix, so a row that advanced (for
/// example `🚧` to `☑️`) changes the fingerprint even when its body did not.
pub fn debts_fingerprint<'a>(rows: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut rows = rows
        .into_iter()
        .map(|(row, owed)| format!("{}\u{1f}{owed}", row.trim()))
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    agent_doc_hash::content_hash(&rows.join("\u{1e}"))
}

fn state_identity(file: &Path) -> Result<Option<(std::path::PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

pub fn load(file: &Path) -> Result<Option<TransientCloseoutRefusal>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(None);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        STATE_KIND,
    )?
    else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&record.payload_json).ok())
}

/// True when the immediately previous refusal for this document went to
/// `session_id` over exactly this debt set.
pub fn repeats_previous_refusal(
    file: &Path,
    session_id: &str,
    debts_fingerprint: &str,
) -> Result<bool> {
    Ok(previous_refusal_count(file, session_id, debts_fingerprint)? > 0)
}

/// `#stoploopcap`: how many consecutive refusals this document's record holds
/// for `session_id` over exactly this debt set; `0` when the previous refusal
/// went to another session or another debt set, or none is recorded.
pub fn previous_refusal_count(
    file: &Path,
    session_id: &str,
    debts_fingerprint: &str,
) -> Result<u32> {
    Ok(load(file)?
        .filter(|refusal| {
            refusal.session_id == session_id && refusal.debts_fingerprint == debts_fingerprint
        })
        .map_or(0, |refusal| refusal.refusals.max(1)))
}

/// Forget this document's recorded refusal. `Ok(true)` when one was present.
///
/// The record is turn-scoped: the Stop hook clears it on every turn's first
/// Stop (`stop_hook_active = false`), so only a refusal issued since that Stop
/// can satisfy [`repeats_previous_refusal`]. Without this, a record left by an
/// earlier turn of the same session would release a later turn's unchanged
/// debts whenever an earlier gate (a queue or steering continuation, a
/// reported-writeback hold, the final status-sync checkpoint) answered that
/// turn's first Stop, and the debts would never be named in that turn.
pub fn clear(file: &Path) -> Result<bool> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(false);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    // `#claudestopbudget`: only the Stop gate reads this row, so clearing it
    // does not end the Stop decision's queue-projection read snapshot.
    agent_doc_sqlite::state_store::clear_queue_document_bookkeeping_in_db(
        &conn,
        &document_hash,
        STATE_KIND,
    )
}

/// Record a refusal of `debts` rows with `debts_fingerprint` to `session_id`.
///
/// `#stoploopcap`: a refusal of the same debt set to the same session as the
/// recorded one increments its consecutive count; any other starts at one.
pub fn record(file: &Path, session_id: &str, debts_fingerprint: &str, debts: usize) -> Result<()> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(());
    };
    let refusals = previous_refusal_count(file, session_id, debts_fingerprint)?.saturating_add(1);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let refusal = TransientCloseoutRefusal {
        session_id: session_id.to_string(),
        debts_fingerprint: debts_fingerprint.to_string(),
        debts,
        refused_at_secs: now,
        refusals,
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    // `#claudestopbudget`: a bookkeeping row (see `clear`).
    agent_doc_sqlite::state_store::upsert_queue_document_bookkeeping_in_db(
        &conn,
        &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
            document_hash,
            state_kind: STATE_KIND.to_string(),
            canonical_path,
            payload_json: serde_json::to_string(&refusal)
                .context("serialize transient closeout refusal")?,
            updated_at_secs: now,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_order_independent_and_marker_sensitive() {
        let a = debts_fingerprint([("🚧 do [#a]", "finish"), ("📥 do [#b]", "claim")]);
        let b = debts_fingerprint([("📥 do [#b]", "claim"), ("🚧 do [#a]", "finish")]);
        let advanced = debts_fingerprint([("☑️ do [#a]", "report"), ("📥 do [#b]", "claim")]);
        let fewer = debts_fingerprint([("📥 do [#b]", "claim")]);
        assert_eq!(a, b);
        assert_ne!(a, advanced);
        assert_ne!(a, fewer);
    }

    #[test]
    fn previous_refusal_is_per_session_and_per_debt_set_and_last_write_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "x\n").unwrap();
        assert!(!repeats_previous_refusal(&doc, "s1", "f1").unwrap());
        record(&doc, "s1", "f1", 2).unwrap();
        assert!(repeats_previous_refusal(&doc, "s1", "f1").unwrap());
        assert!(!repeats_previous_refusal(&doc, "s2", "f1").unwrap());
        assert!(!repeats_previous_refusal(&doc, "s1", "f2").unwrap());
        record(&doc, "s1", "f2", 1).unwrap();
        assert!(!repeats_previous_refusal(&doc, "s1", "f1").unwrap());
        assert!(repeats_previous_refusal(&doc, "s1", "f2").unwrap());
        assert_eq!(load(&doc).unwrap().unwrap().debts, 1);
    }

    #[test]
    fn clear_forgets_the_refusal_and_reports_whether_one_existed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        let other = dir.path().join("other.md");
        std::fs::write(&doc, "x\n").unwrap();
        std::fs::write(&other, "y\n").unwrap();
        assert!(!clear(&doc).unwrap(), "nothing recorded yet");
        record(&doc, "s1", "f1", 1).unwrap();
        record(&other, "s1", "f1", 1).unwrap();
        assert!(clear(&doc).unwrap());
        assert!(load(&doc).unwrap().is_none());
        assert!(!repeats_previous_refusal(&doc, "s1", "f1").unwrap());
        assert!(!clear(&doc).unwrap(), "delete-if-present");
        assert!(
            repeats_previous_refusal(&other, "s1", "f1").unwrap(),
            "clearing one document leaves another document's record"
        );
    }

    /// `#stoploopcap`: the record counts consecutive refusals of one debt set
    /// for one session, so the gate can bound them; any change restarts it.
    #[test]
    fn consecutive_refusals_of_one_debt_set_are_counted_and_a_change_restarts_the_count() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "x\n").unwrap();
        assert_eq!(previous_refusal_count(&doc, "s1", "f1").unwrap(), 0);
        record(&doc, "s1", "f1", 1).unwrap();
        assert_eq!(previous_refusal_count(&doc, "s1", "f1").unwrap(), 1);
        record(&doc, "s1", "f1", 1).unwrap();
        record(&doc, "s1", "f1", 1).unwrap();
        assert_eq!(previous_refusal_count(&doc, "s1", "f1").unwrap(), 3);
        assert_eq!(load(&doc).unwrap().unwrap().refusals, 3);
        // A changed debt set starts over, and the old set no longer repeats.
        record(&doc, "s1", "f2", 2).unwrap();
        assert_eq!(previous_refusal_count(&doc, "s1", "f2").unwrap(), 1);
        assert_eq!(previous_refusal_count(&doc, "s1", "f1").unwrap(), 0);
        // So does another session over the same set.
        record(&doc, "s2", "f2", 2).unwrap();
        assert_eq!(previous_refusal_count(&doc, "s2", "f2").unwrap(), 1);
        assert_eq!(previous_refusal_count(&doc, "s1", "f2").unwrap(), 0);
        // A turn's first Stop clears the count.
        assert!(clear(&doc).unwrap());
        assert_eq!(previous_refusal_count(&doc, "s2", "f2").unwrap(), 0);
        // A record written before the count existed reads as one refusal.
        let legacy: TransientCloseoutRefusal = serde_json::from_str(
            r#"{"session_id":"s1","debts_fingerprint":"f1","debts":1,"refused_at_secs":0}"#,
        )
        .unwrap();
        assert_eq!(legacy.refusals, 1);
    }
}
