//! Queue-drain ownership as a durable Lazily owner claim (`#implementlazilyowned`).
//!
//! Which process drains a document's queue (a `/loop`, or an explicit
//! `agent-doc drain-claim --owner`) is an owner claim
//! (`agent_doc_sqlite::owner_lease_claim`): each new owner is a Lazily epoch
//! that supersedes the previous owner's retained epoch, the same owner's
//! refresh renews its own grant, and a release completes only the current
//! owner's epoch. The `queue_drain` coordination-lease row in the project
//! `state.db` stays the backend lock and the heartbeat readers judge freshness
//! by; it is process coordination, not document state.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use agent_doc_lease::DRAIN_OWNER_SCOPE;
use agent_doc_sqlite::owner_lease_claim::{
    CoordinationLeaseBackend, acquire_owner_lease, observe_owner_lease, release_owner_lease,
};

pub const DRAIN_OWNER_CLAUDE_LOOP: &str = "claude_loop";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainOwnerLease {
    pub owner: String,
    pub heartbeat_secs: u64,
}

pub use agent_doc_lease::drain_owner_ttl;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// The project root and the drain-owner claim's backend for `file`.
fn drain_owner_claim(file: &str) -> Result<(std::path::PathBuf, CoordinationLeaseBackend)> {
    let file = Path::new(file);
    let root = agent_doc_fs::find_project_root_canonical(file)
        .or_else(|| agent_doc_fs::find_project_root(file))
        .with_context(|| format!("no .agent-doc project root for {}", file.display()))?;
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    Ok((
        root,
        CoordinationLeaseBackend::new(DRAIN_OWNER_SCOPE, document_hash),
    ))
}

/// Claim (or, for the current owner, renew) the document's queue drain for
/// `owner`. A different owner supersedes the previous owner's epoch.
pub fn refresh_drain_owner_lease(file: &str, owner: &str) -> Result<()> {
    let (project_root, backend) = drain_owner_claim(file)?;
    let mut conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let grant = agent_doc_sqlite::state_store::CoordinationLeaseRecord {
        scope_kind: backend.scope_kind.clone(),
        scope_id: backend.scope_id.clone(),
        holder: owner.to_string(),
        holder_pid: Some(std::process::id()),
        heartbeat_secs: now_secs(),
    };
    acquire_owner_lease(&mut conn, &backend, grant, |_| true).map(|_| ())
}

pub fn read_drain_owner_lease(file: &str) -> Option<DrainOwnerLease> {
    let (project_root, backend) = drain_owner_claim(file).ok()?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root).ok()?;
    let lease = observe_owner_lease(&conn, &backend).ok()??;
    Some(DrainOwnerLease {
        owner: lease.holder,
        heartbeat_secs: lease.heartbeat_secs,
    })
}

pub fn fresh_drain_owner_lease(file: &str, now: u64) -> Option<DrainOwnerLease> {
    let lease = read_drain_owner_lease(file)?;
    agent_doc_lease::timestamp_is_fresh(lease.heartbeat_secs, now, drain_owner_ttl())
        .then_some(lease)
}

pub fn fresh_loop_drain_owner_lease(file: &str, now: u64) -> Option<DrainOwnerLease> {
    fresh_drain_owner_lease(file, now).filter(|lease| lease.owner == DRAIN_OWNER_CLAUDE_LOOP)
}

/// A loop-owned lease still inside the STALL grace window.
///
/// `#queuestallloopfalsepositive`: use this — never [`fresh_loop_drain_owner_lease`]
/// — to decide whether a loop is continuing. Ownership expires in 90s so a
/// stopped loop hands the queue back promptly, but a healthy self-paced cycle
/// (verification suite, build/install, wakeup jitter) spans minutes and nothing
/// refreshes the lease in between. Judging continuation by the ownership TTL made
/// `queue_stall_detected` fire on three consecutive healthy cycles.
pub fn loop_drain_owner_lease_within_stall_grace(file: &str, now: u64) -> Option<DrainOwnerLease> {
    let lease = read_drain_owner_lease(file)?;
    (lease.owner == DRAIN_OWNER_CLAUDE_LOOP
        && agent_doc_lease::timestamp_is_fresh(
            lease.heartbeat_secs,
            now,
            agent_doc_lease::drain_stall_grace(),
        ))
    .then_some(lease)
}

fn release_drain_owner(file: &str, owner: Option<&str>) -> Result<bool> {
    let (project_root, backend) = drain_owner_claim(file)?;
    let mut conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    release_owner_lease(&mut conn, &backend, owner)
}

/// Release the document's queue drain, whoever owns it (`drain-claim --release`).
pub fn clear_drain_owner_lease(file: &str) {
    if let Err(err) = release_drain_owner(file, None) {
        eprintln!("[agent-doc] warning: failed to clear queue-drain lease: {err}");
    }
}

/// Release the document's queue drain only while `owner` still owns it. The
/// comparison and the release are one Lazily epoch, so an owner that claimed
/// the drain after the caller decided keeps it.
pub fn release_drain_owner_if(file: &str, owner: &str) -> bool {
    release_drain_owner(file, Some(owner)).unwrap_or_else(|err| {
        eprintln!("[agent-doc] warning: failed to release queue-drain owner {owner}: {err}");
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_db_lease_roundtrips_and_clears_without_sidecar() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        refresh_drain_owner_lease(&file, DRAIN_OWNER_CLAUDE_LOOP).unwrap();
        let lease = read_drain_owner_lease(&file).unwrap();
        assert_eq!(lease.owner, DRAIN_OWNER_CLAUDE_LOOP);
        assert!(!dir.path().join(".agent-doc/drain-owner").exists());

        clear_drain_owner_lease(&file);
        assert!(read_drain_owner_lease(&file).is_none());
    }

    #[test]
    fn a_conditional_release_keeps_an_owner_that_claimed_after_the_decision() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        refresh_drain_owner_lease(&file, DRAIN_OWNER_CLAUDE_LOOP).unwrap();
        // An explicit owner supersedes the loop between the caller's read and
        // its release: the loop-only release must leave the new owner alone.
        refresh_drain_owner_lease(&file, "operator").unwrap();
        assert!(!release_drain_owner_if(&file, DRAIN_OWNER_CLAUDE_LOOP));
        assert_eq!(read_drain_owner_lease(&file).unwrap().owner, "operator");
        assert!(release_drain_owner_if(&file, "operator"));
        assert!(read_drain_owner_lease(&file).is_none());
    }
}
