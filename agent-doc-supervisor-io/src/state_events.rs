//! Typed supervisor coordination facts stored in the project `state.db`.

use std::path::{Path, PathBuf};

use agent_doc_sqlite::state_store::{
    StateEventInsert, insert_state_event_in_db, load_state_events_from_db, open_state_db,
};
use agent_doc_state_backbone::{EventLedger, StateEvent};
use anyhow::{Context, Result};

pub(crate) struct DocumentStateIdentity {
    pub project_root: PathBuf,
    pub canonical_file: PathBuf,
    pub document_hash: String,
}

pub(crate) fn document_state_identity(file: &Path) -> Result<Option<DocumentStateIdentity>> {
    let canonical_file = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical_file) else {
        return Ok(None);
    };
    let document_hash = agent_doc_fs::document_state_hash(&canonical_file)?;
    Ok(Some(DocumentStateIdentity {
        project_root,
        canonical_file,
        document_hash,
    }))
}

pub(crate) fn load_ledger(project_root: &Path) -> Result<EventLedger> {
    let conn = open_state_db(project_root)?;
    let mut ledger = EventLedger::new();
    for row in load_state_events_from_db(&conn, None)? {
        let event: StateEvent = serde_json::from_str(&row.payload_json).with_context(|| {
            format!(
                "parse supervisor state event {} from state.db",
                row.event_id
            )
        })?;
        ledger.append(event);
    }
    Ok(ledger)
}

pub(crate) fn append_event(project_root: &Path, event: &StateEvent) -> Result<bool> {
    let conn = open_state_db(project_root)?;
    let payload_json = serde_json::to_string(event).context("serialize supervisor state event")?;
    insert_state_event_in_db(
        &conn,
        &StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    )
}
