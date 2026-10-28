//! `#statussyncrevert`: the latest `status:sync` payload a status writer landed.
//!
//! A sync-managed `agent:status` block is derived state owned by its `sync`
//! instruction. The editor can still change it — an IntelliJ Ctrl+Z walks back
//! through agent edits because each one is an undoable command — and that
//! change arrives as operator text. To tell such a change from a sync result,
//! every status writer records the payload it landed here; the status-sync
//! checkpoints compare the document against it
//! ([`agent_doc_document::status_projection::status_sync_revert`]) and demand a
//! re-sync when they differ.
//!
//! The record lives in `state.db`'s `document_runtime_state` (kind
//! [`STATE_KIND`]), keyed by the document's state hash. It is a single
//! superseding row, not a ledger fact: older binaries replaying the document's
//! fact ledger never see it.

use agent_doc_sqlite::state_store::{self, DocumentRuntimeStateRecord};
use anyhow::{Context, Result};
use std::path::Path;

/// `document_runtime_state.state_kind` of the record.
pub const STATE_KIND: &str = "status_sync_synced_payload";

fn project_root(doc: &Path) -> Result<std::path::PathBuf> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    agent_doc_project_root_io::project_root_or_file_parent(&canonical)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// Record the managed payload of `landed` (the document a status writer just
/// landed) as the latest synced value. A document without a sync-managed
/// block records nothing. Returns the recorded payload's 12-hex hash.
pub fn record_status_sync_synced_payload(doc: &Path, landed: &str) -> Result<Option<String>> {
    let Some(payload) = agent_doc_document::status_projection::status_sync_managed_payload(landed)
    else {
        return Ok(None);
    };
    record_status_sync_synced_payload_text(doc, &payload)
}

/// Record `payload` (a managed block's body) as the latest synced value for
/// `doc`. A blank payload records nothing. Returns the recorded 12-hex hash.
pub fn record_status_sync_synced_payload_text(doc: &Path, payload: &str) -> Result<Option<String>> {
    let payload = payload.trim().to_string();
    if payload.is_empty() {
        return Ok(None);
    }
    let hash: String = agent_doc_hash::content_hash(&payload)
        .chars()
        .take(12)
        .collect();
    let root = project_root(doc)?;
    let conn = state_store::open_state_db(&root)?;
    state_store::upsert_document_runtime_state_in_db(
        &conn,
        &DocumentRuntimeStateRecord {
            document_hash: agent_doc_fs::document_state_hash(doc)?,
            state_kind: STATE_KIND.to_string(),
            canonical_path: doc
                .canonicalize()
                .unwrap_or_else(|_| doc.to_path_buf())
                .to_string_lossy()
                .into_owned(),
            payload_json: serde_json::json!({ "payload": payload, "payload_hash": hash })
                .to_string(),
            updated_at_ms: now_ms(),
        },
    )?;
    Ok(Some(hash))
}

/// The latest synced payload for `doc`, `None` when none was recorded (or the
/// row is unreadable: a missing record only means no re-sync is demanded).
pub fn load_status_sync_synced_payload(doc: &Path) -> Result<Option<String>> {
    let root = project_root(doc)?;
    if !state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = state_store::open_state_db(&root)?;
    let row = state_store::load_document_runtime_state_from_db(
        &conn,
        &agent_doc_fs::document_state_hash(doc)?,
        STATE_KIND,
    )
    .context("load status-sync synced payload")?;
    Ok(row.and_then(|row| {
        serde_json::from_str::<serde_json::Value>(&row.payload_json)
            .ok()?
            .get("payload")?
            .as_str()
            .map(str::to_string)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNCED: &str = "---\nagent_doc_session: t\n---\n\n<!-- agent:status sync=\"keep release current\" -->\nprose\n\n<!-- status:sync -->\nLatest release: v0.35.559\n<!-- /status:sync -->\n<!-- /agent:status -->\n";

    #[test]
    fn records_and_loads_the_landed_payload_superseding_the_previous_one() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, SYNCED).unwrap();
        assert_eq!(load_status_sync_synced_payload(&doc).unwrap(), None);

        let hash = record_status_sync_synced_payload(&doc, SYNCED)
            .unwrap()
            .unwrap();
        assert_eq!(hash.len(), 12);
        assert_eq!(
            load_status_sync_synced_payload(&doc).unwrap().as_deref(),
            Some("Latest release: v0.35.559")
        );

        let newer = SYNCED.replace("v0.35.559", "v0.35.560");
        record_status_sync_synced_payload(&doc, &newer).unwrap();
        assert_eq!(
            load_status_sync_synced_payload(&doc).unwrap().as_deref(),
            Some("Latest release: v0.35.560")
        );
    }

    #[test]
    fn a_status_without_sync_records_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        let plain = "<!-- agent:status -->\nWorking.\n<!-- /agent:status -->\n";
        std::fs::write(&doc, plain).unwrap();
        assert_eq!(
            record_status_sync_synced_payload(&doc, plain).unwrap(),
            None
        );
        assert_eq!(load_status_sync_synced_payload(&doc).unwrap(), None);
    }
}
