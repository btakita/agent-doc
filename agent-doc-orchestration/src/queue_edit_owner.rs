//! Queue-edit lease (#sqedit-race Phase 2).
//!
//! A direct, out-of-band queue edit (`agent-doc queue prune-noise` /
//! `agent-doc queue consume`) made while the route-owned supervisor and a live
//! JetBrains editor are running can be undone or amplified into corruption: the
//! supervisor idle-queue-watch and the per-cycle preflight queue maintenance
//! (#mirrorall mirror, backlog→queue sync, #7r2s pin, live-buffer converge) can
//! observe a *torn intermediate* queue and round-trip it, re-mangling entries
//! (the documented "noise heads are never auto-deleted — the live IPC supervisor
//! races on direct queue edits" hazard).
//!
//! This lease is the writer-side half of a single-writer guarantee for direct
//! queue edits. A direct queue-edit command acquires it for the duration of its
//! read-modify-write (via [`QueueEditGuard`]); the two *other* concurrent queue
//! writers — preflight queue maintenance and the supervisor idle-watch — read it
//! and defer while it is fresh and held by a **live, different** process. So an
//! out-of-band queue edit is atomic with respect to the supervisor + preflight
//! instead of racing them.
//!
//! Mirrors [`crate::drain_owner`]: a per-document filesystem lease with a short,
//! self-healing TTL. The TTL only has to cover a single read-modify-write plus
//! clock skew, so a crashed editor hands the queue back to the supervisor fast.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document queue-edit
/// leases. Mirrors the sibling `.agent-doc/drain-owner` sidecar layout.
const QUEUE_EDIT_OWNER_DIR: &str = ".agent-doc/queue-edit-owner";

/// Default lease freshness window. A direct queue edit is a single
/// read-modify-write that completes in well under a second; the TTL only spans
/// that edit plus clock skew so a crashed writer returns ownership quickly.
const DEFAULT_QUEUE_EDIT_OWNER_TTL_SECS: u64 = 15;
const QUEUE_EDIT_OWNER_TTL_SECS_ENV: &str = "AGENT_DOC_QUEUE_EDIT_OWNER_TTL_SECS";

/// Persisted queue-edit lease body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEditOwnerLease {
    /// PID of the process performing the direct queue edit.
    pub holder_pid: u32,
    /// Unix seconds of the last heartbeat / claim.
    pub heartbeat_secs: u64,
}

/// Resolve the lease TTL, honoring the `AGENT_DOC_QUEUE_EDIT_OWNER_TTL_SECS`
/// override.
pub fn queue_edit_owner_ttl() -> Duration {
    let secs = std::env::var(QUEUE_EDIT_OWNER_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_QUEUE_EDIT_OWNER_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

/// Pure freshness predicate: a lease is fresh while its heartbeat is within
/// `ttl` of `now`. Side-effect free for deterministic unit tests.
pub fn queue_edit_owner_lease_is_fresh(heartbeat_secs: u64, now: u64, ttl: Duration) -> bool {
    now.saturating_sub(heartbeat_secs) <= ttl.as_secs()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the queue-edit lease path for a document. Mirrors
/// [`crate::drain_owner`]: hash the document path and land the sidecar in the
/// nearest ancestor `.agent-doc/` directory.
fn queue_edit_owner_lease_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir
                .join(QUEUE_EDIT_OWNER_DIR)
                .join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(QUEUE_EDIT_OWNER_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

/// Claim or refresh the queue-edit lease for `file` held by `pid` with the
/// current heartbeat.
pub fn refresh_queue_edit_owner_lease(file: &str, pid: u32) -> Result<()> {
    let path = queue_edit_owner_lease_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create queue-edit-owner dir {}", parent.display())
        })?;
    }
    let lease = QueueEditOwnerLease {
        holder_pid: pid,
        heartbeat_secs: now_secs(),
    };
    let body = serde_json::to_string(&lease).context("failed to serialize queue-edit lease")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write queue-edit lease {}", path.display()))?;
    Ok(())
}

/// Read the raw queue-edit lease for `file` (regardless of freshness).
pub fn read_queue_edit_owner_lease(file: &str) -> Option<QueueEditOwnerLease> {
    let path = queue_edit_owner_lease_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Best-effort release of the queue-edit lease (e.g. when the edit completes).
pub fn clear_queue_edit_owner_lease(file: &str) {
    let path = queue_edit_owner_lease_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear queue-edit lease {}: {err}",
            path.display()
        );
    }
}

/// Testable core of [`foreign_queue_edit_in_flight`]: return the holder pid iff a
/// **different, live** process currently holds a fresh queue-edit lease. `None`
/// means no foreign edit is in flight (absent / stale / dead / self-held), so
/// other queue writers may proceed.
pub fn foreign_queue_edit_in_flight_with(
    lease: Option<&QueueEditOwnerLease>,
    self_pid: u32,
    now: u64,
    ttl: Duration,
    pid_is_live: impl Fn(u32) -> bool,
) -> Option<u32> {
    let lease = lease?;
    if lease.holder_pid == self_pid {
        return None;
    }
    if !queue_edit_owner_lease_is_fresh(lease.heartbeat_secs, now, ttl) {
        return None;
    }
    if !pid_is_live(lease.holder_pid) {
        return None;
    }
    Some(lease.holder_pid)
}

/// Return the holder pid iff a different, live process currently holds a fresh
/// queue-edit lease for `file`. Callers (preflight queue maintenance, supervisor
/// idle-watch) must defer their own queue mutation while this is `Some`.
pub fn foreign_queue_edit_in_flight(file: &str) -> Option<u32> {
    let lease = read_queue_edit_owner_lease(file)?;
    foreign_queue_edit_in_flight_with(
        Some(&lease),
        std::process::id(),
        now_secs(),
        queue_edit_owner_ttl(),
        crate::hooks::pid_is_live,
    )
}

/// RAII guard around a direct queue edit. Acquires (refreshes) the queue-edit
/// lease for the current process on construction and best-effort releases it on
/// drop, so the lease is cleared even when the edit early-returns or errors.
pub struct QueueEditGuard {
    file: String,
}

impl QueueEditGuard {
    /// Acquire the queue-edit lease for `file` held by the current process.
    /// Lease-write failure is non-fatal (best-effort single-writer coordination);
    /// it is logged and the edit proceeds — the worst case is the pre-existing
    /// race, never a blocked edit.
    pub fn acquire(file: &Path) -> Self {
        let file = file.to_string_lossy().to_string();
        if let Err(err) = refresh_queue_edit_owner_lease(&file, std::process::id()) {
            eprintln!("[agent-doc] warning: failed to acquire queue-edit lease: {err}");
        }
        Self { file }
    }
}

impl Drop for QueueEditGuard {
    fn drop(&mut self) {
        clear_queue_edit_owner_lease(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_predicate_uses_ttl_window() {
        let ttl = Duration::from_secs(15);
        assert!(
            queue_edit_owner_lease_is_fresh(1_000, 1_000, ttl),
            "same instant"
        );
        assert!(
            queue_edit_owner_lease_is_fresh(1_000, 1_015, ttl),
            "at the ttl edge"
        );
        assert!(
            !queue_edit_owner_lease_is_fresh(1_000, 1_016, ttl),
            "past the ttl"
        );
        // Clock skew (heartbeat in the future) saturates to fresh.
        assert!(queue_edit_owner_lease_is_fresh(2_000, 1_000, ttl));
    }

    #[test]
    fn refresh_then_read_roundtrips_a_fresh_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        refresh_queue_edit_owner_lease(&file, 4242).unwrap();
        let lease = read_queue_edit_owner_lease(&file).expect("lease present after refresh");
        assert_eq!(lease.holder_pid, 4242);
    }

    #[test]
    fn foreign_edit_detected_only_for_a_different_live_fresh_holder() {
        let ttl = Duration::from_secs(15);
        let live = |_pid: u32| true;
        let dead = |_pid: u32| false;
        let lease = QueueEditOwnerLease {
            holder_pid: 999,
            heartbeat_secs: 1_000,
        };

        // Different, live, fresh holder → defer.
        assert_eq!(
            foreign_queue_edit_in_flight_with(Some(&lease), 1, 1_005, ttl, live),
            Some(999)
        );
        // Self-held → never defer to our own edit.
        assert_eq!(
            foreign_queue_edit_in_flight_with(Some(&lease), 999, 1_005, ttl, live),
            None
        );
        // Stale heartbeat → hand the queue back to the supervisor/preflight.
        assert_eq!(
            foreign_queue_edit_in_flight_with(Some(&lease), 1, 9_999, ttl, live),
            None
        );
        // Dead holder pid → not a real in-flight edit.
        assert_eq!(
            foreign_queue_edit_in_flight_with(Some(&lease), 1, 1_005, ttl, dead),
            None
        );
        // Absent lease → no foreign edit.
        assert_eq!(
            foreign_queue_edit_in_flight_with(None, 1, 1_005, ttl, live),
            None
        );
    }

    #[test]
    fn guard_acquires_on_construction_and_releases_on_drop() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file_str = file.to_string_lossy().to_string();

        {
            let _guard = QueueEditGuard::acquire(&file);
            let lease = read_queue_edit_owner_lease(&file_str).expect("lease held inside guard");
            assert_eq!(lease.holder_pid, std::process::id());
        }
        // Dropped → lease cleared.
        assert!(
            read_queue_edit_owner_lease(&file_str).is_none(),
            "guard drop must clear the queue-edit lease"
        );
    }

    #[test]
    fn clear_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        refresh_queue_edit_owner_lease(&file, 7).unwrap();
        clear_queue_edit_owner_lease(&file);
        assert!(read_queue_edit_owner_lease(&file).is_none());
        // Clearing an absent lease must not panic or warn-fail.
        clear_queue_edit_owner_lease(&file);
    }
}
