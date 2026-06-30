//! Drain-owner lease (#kp5z / #qflood).
//!
//! When the Claude Code `/loop` auto-loop drives an `agent:queue` drain, it
//! re-invokes `agent-doc <FILE>` from the harness itself. The supervisor's
//! idle-queue watcher can also inject an `agent-doc <FILE>` trigger on the
//! busy->idle transition. With two drain owners the triggers pile up in the
//! live Claude Code input queue and the operator has to delete them by hand.
//!
//! This module is the single-owner tie-break: the `/loop` path refreshes a
//! short-TTL lease (`agent-doc drain-claim <FILE>`, written just before invoking
//! `/loop`); the supervisor reads it and defers while it is fresh. A stale or
//! absent lease hands ownership back to the supervisor, so non-`/loop` harnesses
//! keep getting supervisor drive exactly as before.
//!
//! The lease deliberately keys on the document path, and the TTL is short so a
//! crashed loop returns ownership quickly.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document drain-owner
/// leases. Mirrors the sibling `.agent-doc/queue-edit-owner` sidecar layout.
const DRAIN_OWNER_DIR: &str = ".agent-doc/drain-owner";

/// Default lease freshness window. Long enough to span the `/loop` inter-cycle
/// re-invoke gap, short enough that a crashed loop hands ownership back fast.
const DEFAULT_DRAIN_OWNER_TTL_SECS: u64 = 90;
const DRAIN_OWNER_TTL_SECS_ENV: &str = "AGENT_DOC_DRAIN_OWNER_TTL_SECS";

/// Canonical owner tag for the Claude Code `/loop` auto-loop.
pub const DRAIN_OWNER_CLAUDE_LOOP: &str = "claude_loop";

/// Persisted drain-owner lease body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainOwnerLease {
    /// Who owns the drain, for example [`DRAIN_OWNER_CLAUDE_LOOP`].
    pub owner: String,
    /// Unix seconds of the last heartbeat / claim.
    pub heartbeat_secs: u64,
}

/// Resolve the lease TTL, honoring the `AGENT_DOC_DRAIN_OWNER_TTL_SECS` override.
pub fn drain_owner_ttl() -> Duration {
    let secs = std::env::var(DRAIN_OWNER_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_DRAIN_OWNER_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the drain-owner lease path for a document. Hash the document path
/// and land the sidecar in the nearest ancestor `.agent-doc/` directory.
fn drain_owner_lease_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(DRAIN_OWNER_DIR).join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(DRAIN_OWNER_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

/// Claim or refresh the drain-owner lease for `file` with the current heartbeat.
pub fn refresh_drain_owner_lease(file: &str, owner: &str) -> Result<()> {
    let path = drain_owner_lease_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create drain-owner dir {}", parent.display()))?;
    }
    let lease = DrainOwnerLease {
        owner: owner.to_string(),
        heartbeat_secs: now_secs(),
    };
    let body = serde_json::to_string(&lease).context("failed to serialize drain-owner lease")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write drain-owner lease {}", path.display()))?;
    Ok(())
}

/// Read the raw drain-owner lease for `file` regardless of freshness.
pub fn read_drain_owner_lease(file: &str) -> Option<DrainOwnerLease> {
    let path = drain_owner_lease_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Return the drain-owner lease iff a self-driving loop currently owns the
/// drain. `None` means the supervisor should drain as usual.
pub fn fresh_drain_owner_lease(file: &str, now: u64) -> Option<DrainOwnerLease> {
    let lease = read_drain_owner_lease(file)?;
    agent_doc_lease::timestamp_is_fresh(lease.heartbeat_secs, now, drain_owner_ttl())
        .then_some(lease)
}

/// Return the drain-owner lease iff a self-driving in-session loop currently
/// owns the drain. Supervisor-side fallback dispatches must not count here: the
/// lease gates only the external loop owner that can race the supervisor.
pub fn fresh_loop_drain_owner_lease(file: &str, now: u64) -> Option<DrainOwnerLease> {
    fresh_drain_owner_lease(file, now).filter(|lease| lease.owner == DRAIN_OWNER_CLAUDE_LOOP)
}

/// Best-effort release of the drain-owner lease, for example when the loop
/// terminates.
pub fn clear_drain_owner_lease(file: &str) {
    let path = drain_owner_lease_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear drain-owner lease {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_then_read_roundtrips_a_fresh_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        refresh_drain_owner_lease(&file, DRAIN_OWNER_CLAUDE_LOOP).unwrap();
        let lease = read_drain_owner_lease(&file).expect("lease present after refresh");
        assert_eq!(lease.owner, DRAIN_OWNER_CLAUDE_LOOP);

        assert!(fresh_drain_owner_lease(&file, lease.heartbeat_secs).is_some());
        assert!(
            fresh_drain_owner_lease(&file, lease.heartbeat_secs + 10_000).is_none(),
            "an old heartbeat must hand ownership back to the supervisor"
        );
    }

    #[test]
    fn fresh_loop_lease_ignores_supervisor_failsafe_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        refresh_drain_owner_lease(&file, "supervisor-failsafe").unwrap();
        let lease = read_drain_owner_lease(&file).expect("lease present after refresh");
        assert!(
            fresh_drain_owner_lease(&file, lease.heartbeat_secs).is_some(),
            "raw freshness still sees the sidecar"
        );
        assert!(
            fresh_loop_drain_owner_lease(&file, lease.heartbeat_secs).is_none(),
            "a supervisor failsafe sidecar must not suppress later supervisor drains"
        );
    }

    #[test]
    fn absent_lease_reads_as_none() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md").to_string_lossy().to_string();
        assert!(read_drain_owner_lease(&file).is_none());
        assert!(fresh_drain_owner_lease(&file, now_secs()).is_none());
    }

    #[test]
    fn clear_removes_the_lease_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        refresh_drain_owner_lease(&file, DRAIN_OWNER_CLAUDE_LOOP).unwrap();
        clear_drain_owner_lease(&file);
        assert!(read_drain_owner_lease(&file).is_none());
        clear_drain_owner_lease(&file);
    }
}
