//! Shared durable state machine for editor transport health.
//!
//! Delivery transports report typed outcomes here after their bounded send
//! attempts. This crate owns the one SQLite transition so refusal accounting,
//! degradation, recycle latching, endpoint unregistration and recovery cannot
//! drift between socket, CRDT-replica and native-save paths.

use std::path::Path;

use agent_doc_ipc_protocol::SocketDeliveryFailure;
use anyhow::Result;

/// Prior durable state consumed by one pure failure transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTransportHealthState {
    pub consecutive_failures: u64,
    pub consecutive_rejections: u64,
    pub degraded: bool,
    pub recycle_attempted: bool,
    pub updated_at_secs: Option<u64>,
}

/// Pure next-state projection for one failed editor delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTransportFailureTransition {
    pub consecutive_failures: u64,
    pub consecutive_rejections: u64,
    pub degraded: bool,
    pub recycle_attempted: bool,
    pub updated_at_secs: u64,
    pub endpoint_unregistered: bool,
}

/// Whether the current session's editor endpoint is temporarily excluded from
/// delivery after a trailing run of explicit refusals.
///
/// This is the shared admission read for every targeted editor transport. The
/// durable health row owns the refusal run; individual transports must not
/// reimplement (or omit) the bounded probe policy.
pub fn endpoint_unregistered(project_root: &Path, file: &Path) -> Result<bool> {
    let session_id =
        agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_else(|| "-".to_string());
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let Some(health) =
        agent_doc_sqlite::state_store::load_editor_transport_health_from_db(&conn, &document_hash)?
            .filter(|health| health.session_id == session_id)
    else {
        return Ok(false);
    };
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    Ok(agent_doc_ipc_protocol::editor_endpoint_unregistered(
        health.consecutive_rejections,
        now_secs.saturating_sub(health.updated_at_secs),
    ))
}

pub fn failure_transition(
    prior: EditorTransportHealthState,
    failure: SocketDeliveryFailure,
    now_secs: u64,
    degraded_after: impl FnOnce(u64) -> bool,
) -> EditorTransportFailureTransition {
    let consecutive_failures = prior.consecutive_failures.saturating_add(1);
    let consecutive_rejections = failure.next_refusal_run(prior.consecutive_rejections);
    let endpoint_unregistered = consecutive_rejections
        >= agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD;
    let degraded = prior.degraded || endpoint_unregistered || degraded_after(consecutive_failures);
    let updated_at_secs = match (failure.refusal_run_effect(), prior.updated_at_secs) {
        (agent_doc_ipc_protocol::RefusalRunEffect::Preserve, Some(prior_at))
            if consecutive_rejections > 0 =>
        {
            prior_at
        }
        _ => now_secs,
    };
    EditorTransportFailureTransition {
        consecutive_failures,
        consecutive_rejections,
        degraded,
        recycle_attempted: prior.recycle_attempted,
        updated_at_secs,
        endpoint_unregistered,
    }
}

/// Persist one typed delivery failure through the shared transition. The
/// caller supplies the pure degradation predicate; this keeps supervisor
/// lifecycle policy out of the state-store boundary.
pub fn record_failure(
    project_root: &Path,
    file: &Path,
    delivery_id: Option<&str>,
    transport: &str,
    failure: SocketDeliveryFailure,
    degraded_after: impl FnOnce(u64) -> bool,
) -> Result<bool> {
    let session_id =
        agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_else(|| "-".to_string());
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let prior =
        agent_doc_sqlite::state_store::load_editor_transport_health_from_db(&conn, &document_hash)?
            .filter(|health| health.session_id == session_id);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let next = failure_transition(
        EditorTransportHealthState {
            consecutive_failures: prior
                .as_ref()
                .map(|health| health.consecutive_timeouts)
                .unwrap_or(0),
            consecutive_rejections: prior
                .as_ref()
                .map(|health| health.consecutive_rejections)
                .unwrap_or(0),
            degraded: prior.as_ref().is_some_and(|health| health.degraded),
            recycle_attempted: prior
                .as_ref()
                .is_some_and(|health| health.recycle_attempted),
            updated_at_secs: prior.as_ref().map(|health| health.updated_at_secs),
        },
        failure,
        now_secs,
        degraded_after,
    );
    agent_doc_sqlite::state_store::upsert_editor_transport_health_in_db(
        &conn,
        &agent_doc_sqlite::state_store::EditorTransportHealthRecord {
            document_hash,
            session_id,
            consecutive_timeouts: next.consecutive_failures,
            degraded: next.degraded,
            recycle_attempted: next.recycle_attempted,
            last_delivery_id: delivery_id.map(str::to_string),
            last_transport: transport.to_string(),
            updated_at_secs: next.updated_at_secs,
            consecutive_rejections: next.consecutive_rejections,
        },
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_ack_failure_recorded file={} transport={} kind={} patch_id={} consecutive_failures={} consecutive_rejections={} degraded={} (#rejectioncountswedge)",
            file.display(),
            transport,
            failure.as_str(),
            delivery_id.unwrap_or("-"),
            next.consecutive_failures,
            next.consecutive_rejections,
            next.degraded,
        ),
    );
    if next.endpoint_unregistered
        && prior
            .as_ref()
            .map(|health| health.consecutive_rejections)
            .unwrap_or(0)
            < next.consecutive_rejections
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "editor_endpoint_unregistered_after_refusals file={} transport={} kind={} consecutive_rejections={} threshold={} probe_after_secs={} action=route_through_document_authority (#gh131nonipc)",
                file.display(),
                transport,
                failure.as_str(),
                next.consecutive_rejections,
                agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD,
                agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_PROBE_AFTER_SECS,
            ),
        );
    }
    Ok(next.degraded)
}

/// Proven delivery recovers the whole health episode. Repeating the delete is
/// idempotent; an ACK without visible delivery proof must not call this.
pub fn clear_after_proven_delivery(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    if agent_doc_sqlite::state_store::clear_editor_transport_health_in_db(&conn, &document_hash)? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_socket_ack_timeouts_cleared file={} reason={}",
                file.display(),
                reason
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_is_monotone_through_refusal_thresholds() {
        let mut state = EditorTransportHealthState {
            consecutive_failures: 0,
            consecutive_rejections: 0,
            degraded: false,
            recycle_attempted: true,
            updated_at_secs: None,
        };
        for attempt in 1..=agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD {
            let next =
                failure_transition(state, SocketDeliveryFailure::Rejected, attempt, |count| {
                    count >= agent_doc_ipc_protocol::EDITOR_TRANSPORT_DEGRADE_FAILURE_THRESHOLD
                });
            assert_eq!(next.consecutive_rejections, attempt);
            assert_eq!(
                next.degraded,
                attempt >= agent_doc_ipc_protocol::EDITOR_TRANSPORT_DEGRADE_FAILURE_THRESHOLD
            );
            assert_eq!(
                next.endpoint_unregistered,
                attempt >= agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD
            );
            assert!(next.recycle_attempted);
            state = EditorTransportHealthState {
                consecutive_failures: next.consecutive_failures,
                consecutive_rejections: next.consecutive_rejections,
                degraded: next.degraded,
                recycle_attempted: next.recycle_attempted,
                updated_at_secs: Some(next.updated_at_secs),
            };
        }

        let timed_out = failure_transition(state, SocketDeliveryFailure::Timeout, 99, |_| true);
        assert_eq!(timed_out.consecutive_rejections, 0);
        assert!(!timed_out.endpoint_unregistered);
        assert!(timed_out.degraded);
        assert!(timed_out.recycle_attempted);
    }

    #[test]
    fn proven_delivery_clear_is_idempotent_and_recovers_all_health_votes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\nsession: health-test\n---\n\n# Session\n").unwrap();
        for attempt in 0..agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD {
            record_failure(
                dir.path(),
                &file,
                Some(&format!("r{attempt}")),
                "crdt_replica_notify",
                SocketDeliveryFailure::Rejected,
                |count| count >= agent_doc_ipc_protocol::EDITOR_TRANSPORT_DEGRADE_FAILURE_THRESHOLD,
            )
            .unwrap();
        }
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let document_hash = agent_doc_fs::document_state_hash(&file).unwrap();
        let health = agent_doc_sqlite::state_store::load_editor_transport_health_from_db(
            &conn,
            &document_hash,
        )
        .unwrap()
        .unwrap();
        assert!(health.degraded);
        assert_eq!(
            health.consecutive_rejections,
            agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD
        );

        clear_after_proven_delivery(dir.path(), &file, "crdt_replica_notify").unwrap();
        clear_after_proven_delivery(dir.path(), &file, "duplicate_success").unwrap();
        assert!(
            agent_doc_sqlite::state_store::load_editor_transport_health_from_db(
                &conn,
                &document_hash,
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn shared_admission_suppresses_the_current_sessions_refusing_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\nsession: health-admission\n---\n\n# Session\n").unwrap();

        for attempt in 0..agent_doc_ipc_protocol::EDITOR_ENDPOINT_UNREGISTER_REFUSAL_THRESHOLD {
            record_failure(
                dir.path(),
                &file,
                Some(&format!("r{attempt}")),
                "native_editor_save_request",
                SocketDeliveryFailure::Rejected,
                |_| false,
            )
            .unwrap();
        }
        assert!(endpoint_unregistered(dir.path(), &file).unwrap());

        std::fs::write(
            &file,
            "---\nsession: replacement-session\n---\n\n# Session\n",
        )
        .unwrap();
        assert!(
            !endpoint_unregistered(dir.path(), &file).unwrap(),
            "a prior session's refusal run must not fence a replacement endpoint"
        );
    }
}
