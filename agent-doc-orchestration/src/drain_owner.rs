//! Drain-owner lease (#kp5z / #qflood).
//!
//! When the Claude Code `/loop` auto-loop drives an `agent:queue` drain, it
//! re-invokes `agent-doc <FILE>` from the harness itself. The supervisor's
//! `idle_queue_watch_drain` (see [`crate::start`]) *also* injects an
//! `agent-doc <FILE>` trigger on the busy→idle transition. With two drain
//! owners the triggers pile up in the live Claude Code input queue and the
//! operator has to delete them by hand.
//!
//! Spec `specs/07-session-tmux-commands.md` states the supervisor idle-queue
//! watch owns the drain payload, but it must *defer* when a self-driving
//! harness loop is the active owner. This module is that single-owner tie-break:
//! the `/loop` path refreshes a short-TTL lease (`agent-doc drain-claim <FILE>`,
//! written just before invoking `/loop`); the supervisor reads it and defers
//! while it is fresh. A stale or absent lease hands ownership back to the
//! supervisor, so non-`/loop` harnesses (e.g. the Codex `Stop`-hook loop, which
//! never writes a lease) keep getting supervisor drive exactly as before.
//!
//! The lease deliberately keys on the *document* path (one drain owner per doc),
//! and the TTL is short so a crashed loop returns ownership quickly.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document drain-owner
/// leases. Mirrors the sibling `.agent-doc/live-buffer` sidecar layout.
const DRAIN_OWNER_DIR: &str = ".agent-doc/drain-owner";

/// Default lease freshness window. Long enough to span the `/loop` inter-cycle
/// re-invoke gap (the only window in which the supervisor would otherwise
/// double-drive), short enough that a crashed loop hands ownership back fast.
const DEFAULT_DRAIN_OWNER_TTL_SECS: u64 = 90;
const DRAIN_OWNER_TTL_SECS_ENV: &str = "AGENT_DOC_DRAIN_OWNER_TTL_SECS";

/// Canonical owner tag for the Claude Code `/loop` auto-loop.
pub const DRAIN_OWNER_CLAUDE_LOOP: &str = "claude_loop";

/// Persisted drain-owner lease body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainOwnerLease {
    /// Who owns the drain (e.g. [`DRAIN_OWNER_CLAUDE_LOOP`]).
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

/// Pure freshness predicate: a lease is fresh while its heartbeat is within
/// `ttl` of `now`. Side-effect free for deterministic unit tests.
pub fn drain_owner_lease_is_fresh(heartbeat_secs: u64, now: u64, ttl: Duration) -> bool {
    now.saturating_sub(heartbeat_secs) <= ttl.as_secs()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the drain-owner lease path for a document. Mirrors
/// `live_buffer_snapshot_path`: hash the document path and land the sidecar in
/// the nearest ancestor `.agent-doc/` directory.
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

/// Read the raw drain-owner lease for `file` (regardless of freshness).
pub fn read_drain_owner_lease(file: &str) -> Option<DrainOwnerLease> {
    let path = drain_owner_lease_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Return the drain-owner lease iff a self-driving loop *currently* owns the
/// drain (the lease exists and is fresh against `now`). `None` means the
/// supervisor should drain as usual.
pub fn fresh_drain_owner_lease(file: &str, now: u64) -> Option<DrainOwnerLease> {
    let lease = read_drain_owner_lease(file)?;
    drain_owner_lease_is_fresh(lease.heartbeat_secs, now, drain_owner_ttl()).then_some(lease)
}

/// Best-effort release of the drain-owner lease (e.g. when the loop terminates).
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
    fn freshness_predicate_uses_ttl_window() {
        let ttl = Duration::from_secs(90);
        assert!(
            drain_owner_lease_is_fresh(1_000, 1_000, ttl),
            "same instant"
        );
        assert!(
            drain_owner_lease_is_fresh(1_000, 1_090, ttl),
            "at the ttl edge"
        );
        assert!(
            !drain_owner_lease_is_fresh(1_000, 1_091, ttl),
            "past the ttl"
        );
        // Clock skew (heartbeat in the future) saturates to fresh.
        assert!(drain_owner_lease_is_fresh(2_000, 1_000, ttl));
    }

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

        // Fresh against a `now` near the heartbeat...
        assert!(fresh_drain_owner_lease(&file, lease.heartbeat_secs).is_some());
        // ...stale against a `now` far past the TTL.
        assert!(
            fresh_drain_owner_lease(&file, lease.heartbeat_secs + 10_000).is_none(),
            "an old heartbeat must hand ownership back to the supervisor"
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
        // Idempotent: clearing an absent lease must not panic or warn-fail.
        clear_drain_owner_lease(&file);
    }
}
