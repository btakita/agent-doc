//! Pending response state adapter.
//!
//! The lazily state backbone is the authority for pending responses. The
//! `.agent-doc/pending/` file is a crash-recovery projection only: write it
//! best-effort, never let it block normal closeout, and read it only as a
//! fallback import source when the authoritative state is unavailable.

use anyhow::{Context, Result};
use std::path::Path;

/// Save a response to the pending store before attempting write-back.
/// This makes the response durable across context compaction.
pub fn save_pending(file: &Path, response: &str) -> Result<()> {
    let response = agent_doc_template_io::canonicalize_response_for_capture(file, response)?;
    let capture = agent_doc_capture_io::capture_response(file, &response)?;
    append_pending_response_captured(file, &capture)?;
    if let Err(err) = write_pending_projection_file(file, &response) {
        eprintln!("[repair] warning: failed to update pending response backup projection: {err}");
    }
    Ok(())
}

/// Remove the pending projection after a successful write-back.
pub fn clear_pending(file: &Path) -> Result<()> {
    if let Err(e) = append_pending_response_cleared(file, "write_applied") {
        eprintln!(
            "[repair] warning: failed to clear pending response state: {}",
            e
        );
    }
    if let Err(e) = remove_pending_projection_file(file) {
        eprintln!(
            "[repair] warning: failed to delete pending response backup projection: {}",
            e
        );
    }
    // Also clean up the pre-response snapshot (saved before write for undo support).
    // Without this, pre-response files accumulate indefinitely after successful writes.
    if let Err(e) = agent_doc_snapshot_io::delete_pre_response(file) {
        eprintln!("[repair] warning: failed to delete pre-response: {}", e);
    }
    if let Err(e) = agent_doc_capture_io::mark_write_applied(file) {
        eprintln!("[repair] warning: failed to update capture state: {}", e);
    }
    Ok(())
}

/// Load the active pending response from the lazily state-backbone projection.
pub fn load_active_pending_response(file: &Path) -> Result<Option<String>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(document_hash) = agent_doc_fs::document_state_hash(&canonical).ok() else {
        return Ok(None);
    };
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    let ledger = load_state_event_ledger(&project_root).with_context(|| {
        format!(
            "failed to load pending response state for {}",
            canonical.display()
        )
    })?;
    Ok(ledger
        .project_document(&document_hash)
        .and_then(|projection| projection.closeout.pending_response)
        .map(|pending| pending.response_body))
}

/// Load the crash-recovery backup projection. Do not call this from the normal
/// closeout hot path; it exists only to import a response when backbone/capture
/// state is missing.
pub fn load_pending_projection_file(file: &Path) -> Result<Option<String>> {
    let pending_path = agent_doc_fs::pending_response_path_for(file)?;
    agent_doc_fs::read_optional_text(&pending_path).with_context(|| {
        format!(
            "failed to read pending response backup {}",
            pending_path.display()
        )
    })
}

fn append_pending_response_captured(
    file: &Path,
    capture: &agent_doc_capture_io::CaptureRecord,
) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_fs::document_state_hash(&canonical)?;
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    let event_id = format!(
        "pending-response-captured:{document_hash}:{}:{}",
        capture.capture_id, capture.response_sha256
    );
    let event = agent_doc_state_backbone::StateEvent::new(
        event_id,
        agent_doc_state_backbone::StateFact::PendingResponseCaptured {
            document_hash,
            cycle_id: capture.cycle_id.clone(),
            capture_id: capture.capture_id.clone(),
            response_sha256: capture.response_sha256.clone(),
            response_body: capture.response_body.clone(),
        },
    );
    append_state_event(&project_root, &event).with_context(|| {
        format!(
            "failed to append pending response state for {}",
            canonical.display()
        )
    })?;
    Ok(())
}

fn append_pending_response_cleared(file: &Path, reason: &str) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_fs::document_state_hash(&canonical)?;
    let cycle_state = agent_doc_cycle_state_io::load(&canonical)?;
    let active_capture = agent_doc_capture_io::load_active(&canonical)?;
    let cycle_id = active_capture
        .as_ref()
        .map(|capture| capture.cycle_id.clone())
        .or_else(|| cycle_state.as_ref().map(|state| state.cycle_id.clone()));
    let Some(cycle_id) = cycle_id else {
        return Ok(());
    };
    let capture_id = active_capture
        .as_ref()
        .map(|capture| capture.capture_id.clone())
        .or_else(|| cycle_state.and_then(|state| state.capture_id));
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    let event_id = format!(
        "pending-response-cleared:{document_hash}:{cycle_id}:{}:{reason}",
        capture_id.as_deref().unwrap_or("any")
    );
    let event = agent_doc_state_backbone::StateEvent::new(
        event_id,
        agent_doc_state_backbone::StateFact::PendingResponseCleared {
            document_hash,
            cycle_id,
            capture_id,
            reason: reason.to_string(),
        },
    );
    append_state_event(&project_root, &event)?;
    Ok(())
}

fn append_state_event(
    project_root: &Path,
    event: &agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let payload_json = serde_json::to_string(event).context("serialize state backbone event")?;
    agent_doc_sqlite::state_store::insert_state_event_in_db(
        &conn,
        &agent_doc_sqlite::state_store::StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    )
}

fn load_state_event_ledger(project_root: &Path) -> Result<agent_doc_state_backbone::EventLedger> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    for row in agent_doc_sqlite::state_store::load_state_events_from_db(&conn, None)? {
        let event: agent_doc_state_backbone::StateEvent = serde_json::from_str(&row.payload_json)
            .with_context(|| {
            format!(
                "parse state backbone event {} from controller state",
                row.event_id
            )
        })?;
        ledger.append(event);
    }
    Ok(ledger)
}

fn write_pending_projection_file(file: &Path, response: &str) -> Result<()> {
    let pending_path = agent_doc_fs::pending_response_path_for(file)?;
    if let Some(parent) = pending_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pending_path, response)
        .with_context(|| format!("failed to save pending response {}", pending_path.display()))
}

fn remove_pending_projection_file(file: &Path) -> Result<()> {
    let pending_path = agent_doc_fs::pending_response_path_for(file)?;
    if pending_path.exists() {
        std::fs::remove_file(&pending_path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_clears_pending_response_state_with_projection_backup() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        save_pending(&doc, "response text").unwrap();
        assert_eq!(
            load_active_pending_response(&doc).unwrap().as_deref(),
            Some("response text")
        );
        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        assert!(pending.exists());
        assert_eq!(std::fs::read_to_string(&pending).unwrap(), "response text");

        clear_pending(&doc).unwrap();
        assert_eq!(load_active_pending_response(&doc).unwrap(), None);
        assert!(!pending.exists());
    }

    #[test]
    fn broken_pending_projection_backup_does_not_block_state_capture() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        std::fs::create_dir_all(pending.parent().unwrap()).unwrap();
        std::fs::create_dir(&pending).unwrap();

        save_pending(&doc, "response text").unwrap();

        assert_eq!(
            load_active_pending_response(&doc).unwrap().as_deref(),
            Some("response text")
        );
        assert!(
            pending.is_dir(),
            "projection path was intentionally broken; state remains authoritative"
        );
    }
}
