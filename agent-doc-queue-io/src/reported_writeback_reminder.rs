//! `#reportedwritebackloop`: the per-document record of the last Claude Stop
//! reminder for unpersisted reported worker results.
//!
//! The Stop gate is bounded within one stop by `stop_hook_active`, but nothing
//! bounded it ACROSS stops: when every closeout it prescribed was refused, it
//! re-emitted the identical reminder at the end of every turn (2026-11-03,
//! tasks/agent-doc/agent-doc-bugs.md, three released `☑️` rows). This record
//! lets the gate remind once per distinct debt set per harness session; a
//! changed debt set, a new session, or an urgent recovered-after-loss row
//! reminds again.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const STATE_KIND: &str = "reported_writeback_reminder";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportedWritebackReminder {
    /// Harness session that received the reminder.
    pub session_id: String,
    /// Order-independent digest of the reminded debts.
    pub debts_fingerprint: String,
    pub reminded_at_secs: u64,
}

/// Order-independent digest of `(item identity text, owner, generation)` rows.
pub fn debts_fingerprint<'a>(rows: impl IntoIterator<Item = (&'a str, &'a str, u64)>) -> String {
    let mut rows = rows
        .into_iter()
        .map(|(item, owner, generation)| format!("{}\u{1f}{owner}\u{1f}{generation}", item.trim()))
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

pub fn load(file: &Path) -> Result<Option<ReportedWritebackReminder>> {
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

/// True when `session_id` was already reminded about exactly this debt set.
pub fn already_reminded(file: &Path, session_id: &str, debts_fingerprint: &str) -> Result<bool> {
    Ok(load(file)?.is_some_and(|reminder| {
        reminder.session_id == session_id && reminder.debts_fingerprint == debts_fingerprint
    }))
}

pub fn record(file: &Path, session_id: &str, debts_fingerprint: &str) -> Result<()> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(());
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let reminder = ReportedWritebackReminder {
        session_id: session_id.to_string(),
        debts_fingerprint: debts_fingerprint.to_string(),
        reminded_at_secs: now,
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
        &conn,
        &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
            document_hash,
            state_kind: STATE_KIND.to_string(),
            canonical_path,
            payload_json: serde_json::to_string(&reminder)
                .context("serialize reported writeback reminder")?,
            updated_at_secs: now,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_order_independent_and_generation_sensitive() {
        let a = debts_fingerprint([("[#a] x", "subagent:a", 1), ("[#b] y", "subagent:b", 2)]);
        let b = debts_fingerprint([("[#b] y", "subagent:b", 2), ("[#a] x", "subagent:a", 1)]);
        let c = debts_fingerprint([("[#b] y", "subagent:b", 3), ("[#a] x", "subagent:a", 1)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn reminder_is_per_session_and_per_debt_set() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "x\n").unwrap();
        assert!(!already_reminded(&doc, "s1", "f1").unwrap());
        record(&doc, "s1", "f1").unwrap();
        assert!(already_reminded(&doc, "s1", "f1").unwrap());
        assert!(!already_reminded(&doc, "s2", "f1").unwrap());
        assert!(!already_reminded(&doc, "s1", "f2").unwrap());
    }
}
