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
use anyhow::{Context, Result};

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
    let recycle_epoch = next_recycle_epoch(&ledger, &identity.document_hash)?;
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
    anyhow::ensure!(
        crate::state_events::append_event(&identity.project_root, &event)?,
        "recycle request generation collision for {} at epoch {recycle_epoch}",
        identity.canonical_file.display(),
    );
    Ok(())
}

/// Compatibility ledger high-water, not the number of retained events. History
/// compaction can shrink `document_epoch`; it cannot unspend a recycle event ID.
fn next_recycle_epoch(
    ledger: &agent_doc_state_backbone::EventLedger,
    document_hash: &str,
) -> Result<u64> {
    ledger
        .events()
        .iter()
        .filter(|event| event.document_hash() == document_hash)
        .filter_map(|event| match &event.fact {
            StateFact::SupervisorRecycleRequested { recycle_epoch, .. }
            | StateFact::SupervisorRecycleStarted { recycle_epoch, .. }
            | StateFact::SupervisorRecycleSettled { recycle_epoch, .. } => Some(*recycle_epoch),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .max(ledger.document_epoch(document_hash))
        .checked_add(1)
        .context("supervisor recycle generation exhausted")
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
    // Consume only the request we observed. A later request has a greater epoch
    // and must survive this settlement even if its durable append wins the race.
    let recycle_epoch = recycle.recycle_epoch;
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
    fn compacted_history_cannot_reuse_or_regress_recycle_generations() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "body").unwrap();
        let identity = crate::state_events::document_state_identity(&file)
            .unwrap()
            .unwrap();
        // Only two history rows remain, but epoch 9000 has already been spent.
        for (id, fact) in [
            (
                "historical-request",
                StateFact::SupervisorRecycleRequested {
                    document_hash: identity.document_hash.clone(),
                    reason: "old_install".into(),
                    recycle_epoch: 9000,
                    marked_secs: 1,
                },
            ),
            (
                "historical-settle",
                StateFact::SupervisorRecycleSettled {
                    document_hash: identity.document_hash.clone(),
                    reason: "old_install".into(),
                    recycle_epoch: 9000,
                    marked_secs: 2,
                },
            ),
        ] {
            crate::state_events::append_event(dir.path(), &StateEvent::new(id, fact)).unwrap();
        }
        request_recycle_for_doc(&file, "new_install").unwrap();
        assert_eq!(
            read_recycle_request(file.to_str().unwrap()).unwrap().reason,
            "new_install"
        );
        let ledger =
            crate::state_events::load_document_ledger_shared(dir.path(), &identity.document_hash)
                .unwrap();
        assert_eq!(
            ledger
                .project_document(&identity.document_hash)
                .unwrap()
                .supervisor
                .recycle
                .recycle_epoch,
            9001
        );
        clear_recycle_request(file.to_str().unwrap());
        assert!(read_recycle_request(file.to_str().unwrap()).is_none());
        request_recycle_for_doc(&file, "next_install").unwrap();
        assert_eq!(
            read_recycle_request(file.to_str().unwrap()).unwrap().reason,
            "next_install"
        );
    }

    #[test]
    fn late_settlement_cannot_consume_a_newer_install_request() {
        let mut ledger = agent_doc_state_backbone::EventLedger::new();
        for epoch in [9001, 9002] {
            ledger.append(StateEvent::new(
                format!("request-{epoch}"),
                StateFact::SupervisorRecycleRequested {
                    document_hash: "doc".into(),
                    reason: "install".into(),
                    recycle_epoch: epoch,
                    marked_secs: epoch,
                },
            ));
        }
        ledger.append(StateEvent::new(
            "late-settle",
            StateFact::SupervisorRecycleSettled {
                document_hash: "doc".into(),
                reason: "consumed_old_request".into(),
                recycle_epoch: 9001,
                marked_secs: 9003,
            },
        ));
        let projection = ledger.project_document("doc").unwrap().supervisor.recycle;
        assert_eq!(projection.phase, SupervisorRecyclePhase::Requested);
        assert_eq!(projection.recycle_epoch, 9002);
    }

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
