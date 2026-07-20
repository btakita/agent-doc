//! Queue-drain ownership coordinated through the project `state.db`.
//!
//! This is process coordination, not document state. Keeping it in the typed
//! controller database prevents a filesystem lease from becoming a second hot
//! authority beside Lazily.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use agent_doc_lease::DRAIN_OWNER_SCOPE;

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

fn state_location(file: &str) -> Result<(std::path::PathBuf, String)> {
    let file = Path::new(file);
    let root = agent_doc_fs::find_project_root_canonical(file)
        .or_else(|| agent_doc_fs::find_project_root(file))
        .with_context(|| format!("no .agent-doc project root for {}", file.display()))?;
    let document_hash = agent_doc_fs::document_state_hash(file)?;
    Ok((root, document_hash))
}

pub fn refresh_drain_owner_lease(file: &str, owner: &str) -> Result<()> {
    let (project_root, document_hash) = state_location(file)?;
    agent_doc_controller_io::project_controller::upsert_coordination_lease(
        &project_root,
        &agent_doc_sqlite::state_store::CoordinationLeaseRecord {
            scope_kind: DRAIN_OWNER_SCOPE.to_string(),
            scope_id: document_hash,
            holder: owner.to_string(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: now_secs(),
        },
    )
}

pub fn read_drain_owner_lease(file: &str) -> Option<DrainOwnerLease> {
    let (project_root, document_hash) = state_location(file).ok()?;
    let lease = agent_doc_controller_io::project_controller::load_coordination_lease(
        &project_root,
        DRAIN_OWNER_SCOPE,
        &document_hash,
    )
    .ok()??;
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

pub fn clear_drain_owner_lease(file: &str) {
    let result = state_location(file).and_then(|(project_root, document_hash)| {
        agent_doc_controller_io::project_controller::clear_coordination_lease(
            &project_root,
            DRAIN_OWNER_SCOPE,
            &document_hash,
        )
        .map(|_| ())
    });
    if let Err(err) = result {
        eprintln!("[agent-doc] warning: failed to clear queue-drain lease: {err}");
    }
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
}
