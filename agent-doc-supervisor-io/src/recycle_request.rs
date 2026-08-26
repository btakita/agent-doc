//! Cross-supervisor recycle-request state.
//!
//! Install fan-out records one typed fact per document served by an open supervisor in the
//! project `state.db`; the owning supervisor consumes it at its next idle boundary.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_state_backbone::{StateEvent, StateFact, SupervisorRecyclePhase};
use agent_doc_supervisor::recycle_request::{
    RecycleRequest, recycle_request, recycle_request_is_fresh,
};
use anyhow::Result;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Write (or refresh) a recycle-request for `file` with the current heartbeat.
pub fn request_recycle(file: &str, reason: &str) -> Result<()> {
    let Some(identity) = crate::state_events::document_state_identity(Path::new(file))? else {
        return Ok(());
    };
    let ledger = crate::state_events::load_document_ledger_shared(
        &identity.project_root,
        &identity.document_hash,
    )?;
    let recycle_epoch = ledger
        .document_epoch(&identity.document_hash)
        .saturating_add(1);
    let marked_secs = now_secs();
    let event = StateEvent::new(
        format!(
            "supervisor-recycle-requested:{}:epoch-{recycle_epoch}",
            identity.document_hash
        ),
        StateFact::SupervisorRecycleRequested {
            document_hash: identity.document_hash,
            reason: reason.to_string(),
            recycle_epoch,
            marked_secs,
        },
    );
    crate::state_events::append_event(&identity.project_root, &event)?;
    Ok(())
}

/// Write (or refresh) a recycle-request for a document path.
pub fn request_recycle_for_doc(file: &Path, reason: &str) -> Result<()> {
    request_recycle(&file.to_string_lossy(), reason)
}

/// Read the raw recycle-request for `file` regardless of freshness.
pub fn read_recycle_request(file: &str) -> Option<RecycleRequest> {
    let identity = crate::state_events::document_state_identity(Path::new(file)).ok()??;
    let ledger = crate::state_events::load_document_ledger_shared(
        &identity.project_root,
        &identity.document_hash,
    )
    .ok()?;
    let recycle = ledger
        .project_document(&identity.document_hash)?
        .supervisor
        .recycle;
    (recycle.phase == SupervisorRecyclePhase::Requested).then(|| {
        recycle_request(
            recycle.reason.as_deref().unwrap_or("unspecified"),
            recycle.marked_secs,
        )
    })
}

/// Return the recycle-request iff it exists and is fresh against `now`.
pub fn fresh_recycle_request(file: &str, now: u64) -> Option<RecycleRequest> {
    let request = read_recycle_request(file)?;
    recycle_request_is_fresh(&request, now).then_some(request)
}

/// Convenience boolean: is a fresh recycle-request pending for `file`?
pub fn recycle_request_pending(file: &Path) -> bool {
    fresh_recycle_request(&file.to_string_lossy(), now_secs()).is_some()
}

/// Best-effort clear of the recycle-request.
pub fn clear_recycle_request(file: &str) {
    if let Err(err) = clear_recycle_request_inner(file) {
        eprintln!("[agent-doc] warning: failed to clear recycle-request state: {err}");
    }
}

fn clear_recycle_request_inner(file: &str) -> Result<()> {
    let Some(identity) = crate::state_events::document_state_identity(Path::new(file))? else {
        return Ok(());
    };
    let ledger = crate::state_events::load_document_ledger_shared(
        &identity.project_root,
        &identity.document_hash,
    )?;
    let Some(recycle) = ledger
        .project_document(&identity.document_hash)
        .map(|projection| projection.supervisor.recycle)
        .filter(|recycle| recycle.phase == SupervisorRecyclePhase::Requested)
    else {
        return Ok(());
    };
    let recycle_epoch = ledger
        .document_epoch(&identity.document_hash)
        .saturating_add(1);
    let event = StateEvent::new(
        format!(
            "supervisor-recycle-settled:{}:epoch-{recycle_epoch}",
            identity.document_hash
        ),
        StateFact::SupervisorRecycleSettled {
            document_hash: identity.document_hash,
            reason: recycle
                .reason
                .unwrap_or_else(|| "request_consumed".to_string()),
            recycle_epoch,
            marked_secs: now_secs(),
        },
    );
    crate::state_events::append_event(&identity.project_root, &event)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_then_read_roundtrips_a_fresh_request() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        request_recycle(
            &file,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT,
        )
        .unwrap();
        let request = read_recycle_request(&file).expect("request present after write");
        assert_eq!(
            request.reason,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT
        );

        assert!(fresh_recycle_request(&file, request.requested_secs).is_some());
        assert!(
            fresh_recycle_request(&file, request.requested_secs + 10_000_000).is_none(),
            "an old request must not force a stale-forever recycle"
        );
    }

    #[test]
    fn absent_request_reads_as_none() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md").to_string_lossy().to_string();
        assert!(read_recycle_request(&file).is_none());
        assert!(fresh_recycle_request(&file, now_secs()).is_none());
    }

    #[test]
    fn clear_removes_the_request_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        request_recycle(
            &file,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT,
        )
        .unwrap();
        clear_recycle_request(&file);
        assert!(read_recycle_request(&file).is_none());
        clear_recycle_request(&file);
    }
}
