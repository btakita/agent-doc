//! Pending response state adapter.
//!
//! Pending responses live only in the state backbone. Keeping a second file
//! projection made closeout depend on two independently advancing authorities
//! and allowed a cleared response to be resurrected after reconnect.

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingResponseState {
    Active(String),
    Cleared,
    Missing,
}

/// Save a response to the pending store before attempting write-back.
/// This makes the response durable across context compaction.
pub fn save_pending(file: &Path, response: &str) -> Result<()> {
    let canonical_response =
        agent_doc_template_io::canonicalize_response_for_capture(file, response)?;
    let capture =
        agent_doc_capture_io::capture_response_with_intent(file, &canonical_response, response)?;
    save_pending_after_capture(file, response, &capture)
}

pub fn save_pending_with_current_content(
    file: &Path,
    response: &str,
    current_content: &str,
) -> Result<()> {
    save_pending_with_current_content_and_plan(file, response, current_content, None)
}

pub fn save_pending_with_current_content_and_plan(
    file: &Path,
    response: &str,
    current_content: &str,
    mutation_plan_json: Option<&str>,
) -> Result<()> {
    let canonical_response =
        agent_doc_template_io::canonicalize_response_for_capture_with_current_content(
            file,
            response,
            current_content,
        )?;
    let capture = agent_doc_capture_io::capture_response_with_current_content_and_intent_and_plan(
        file,
        &canonical_response,
        current_content,
        Some(response),
        mutation_plan_json,
    )?;
    save_pending_after_capture(file, response, &capture)
}

fn save_pending_after_capture(
    file: &Path,
    intent: &str,
    capture: &agent_doc_capture_io::CaptureRecord,
) -> Result<()> {
    let expected = agent_doc_secret_redact::redact(intent);
    let actual = load_active_pending_response(file)?.with_context(|| {
        format!(
            "captured turn intent {} was not projected as active for {}",
            capture.capture_id,
            file.display()
        )
    })?;
    anyhow::ensure!(
        actual == expected,
        "captured turn intent {} was truncated before recovery for {}",
        capture.capture_id,
        file.display()
    );
    Ok(())
}

/// Clear the pending response after a successful write-back.
pub fn clear_pending(file: &Path) -> Result<()> {
    // Also clean up the undo checkpoint (saved before write for undo support).
    // Successful settlement clears the active undo checkpoint in the ledger.
    if let Err(e) = agent_doc_snapshot_io::clear_undo_content(file) {
        eprintln!("[repair] warning: failed to clear undo checkpoint: {}", e);
    }
    if let Err(e) = agent_doc_capture_io::mark_write_applied(file) {
        eprintln!("[repair] warning: failed to update capture state: {}", e);
    }
    Ok(())
}

/// Load the active pending response from the lazily state-backbone projection.
pub fn load_active_pending_response(file: &Path) -> Result<Option<String>> {
    Ok(match load_pending_response_state(file)? {
        PendingResponseState::Active(response) => Some(response),
        PendingResponseState::Cleared | PendingResponseState::Missing => None,
    })
}

pub(crate) fn load_pending_response_state(file: &Path) -> Result<PendingResponseState> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(document_hash) = agent_doc_fs::document_state_hash(&canonical).ok() else {
        return Ok(PendingResponseState::Missing);
    };
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    let ledger = load_state_event_ledger(&project_root, &document_hash).with_context(|| {
        format!(
            "failed to load pending response state for {}",
            canonical.display()
        )
    })?;
    let Some(projection) = ledger.project_document(&document_hash) else {
        return Ok(PendingResponseState::Missing);
    };
    if let Some(pending) = projection.closeout.pending_response {
        return Ok(PendingResponseState::Active(pending.response_body));
    }
    if projection.closeout.pending_response_clear_reason.is_some() {
        return Ok(PendingResponseState::Cleared);
    }
    Ok(PendingResponseState::Missing)
}

/// `#ledgerdocscope`: load only the target document's events.
///
/// This ran on the repair/closeout path and loaded EVERY event in the project --
/// all rows materialized into a `Vec` with their full `payload_json`, then each
/// parsed into a `StateEvent`, so peak memory was roughly 2x the whole ledger.
/// Measured on agent-loop: ~119MB of payload after retention GC (512MB before),
/// with individual `crdt_recovery_projection_checkpointed` rows around 736KB.
/// Several concurrent agent-doc processes each doing this is a real contributor
/// to memory pressure, and the caller only ever projects ONE document out of the
/// result. `load_state_events_from_db` already takes an indexed `document_hash`
/// filter.
fn load_state_event_ledger(
    project_root: &Path,
    document_hash: &str,
) -> Result<agent_doc_state_backbone::EventLedger> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    for row in agent_doc_sqlite::state_store::load_state_events_from_db(&conn, Some(document_hash))?
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_clears_pending_response_in_state_backbone() {
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
        clear_pending(&doc).unwrap();
        assert_eq!(load_active_pending_response(&doc).unwrap(), None);
        assert_eq!(
            load_pending_response_state(&doc).unwrap(),
            PendingResponseState::Cleared
        );
    }
}
