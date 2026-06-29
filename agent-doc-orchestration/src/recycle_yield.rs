//! Stale-binary recycle-yield request (`#wd40` / `#staleloop-recycle-restart`).
//!
//! The supervisor hot-reloads onto a freshly-installed binary only at a turn
//! boundary (`prompt_visible && !turn_active`) — see
//! [`agent_doc_supervisor::lifecycle::supervisor_recycle_action`]. A continuously
//! self-draining Claude Code `/loop` holds a fresh drain-owner lease AND keeps
//! the harness `turn_active` back-to-back, so the supervisor never reaches that
//! boundary and a stale binary persists for the whole session (the root of the
//! `content_ours` finalize drift + `#rt83` phantom-pin flood this session).
//!
//! This module is the automation that closes that gap. When the supervisor
//! idle-watch detects its own binary is stale AND a self-driving loop owns the
//! drain AND the boundary is not currently reachable, it writes a short-TTL
//! *recycle-yield request* sidecar for the document. The attended in-session
//! loop reads the request at its next inter-item boundary (via
//! [`crate::queue_continuation::detect`] / `session-check` / preflight), yields
//! one boundary instead of re-triggering, and the resulting idle turn lets the
//! `execve` recycle fire on its own. After the recycle the fresh (no-longer-stale)
//! supervisor clears the request and the drain resumes on the new binary.
//!
//! Mid-turn `execve` stays OUT of scope (the supervisor owns the in-flight
//! cycle CRDT / write-queue / IPC state, rebuilt fresh after re-exec): the yield
//! is exactly what produces a clean boundary without a mid-write swap.
//!
//! Mirrors the [`crate::drain_owner`] sidecar layout: keyed on the *document*
//! path, short TTL so an abandoned request self-clears.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document recycle-yield
/// requests. Mirrors the sibling `.agent-doc/drain-owner` sidecar layout.
const RECYCLE_YIELD_DIR: &str = ".agent-doc/recycle-yield";

/// Default request freshness window. Long enough to span the `/loop` inter-cycle
/// re-invoke gap so the request is still live when the loop reaches its next
/// boundary, short enough that an abandoned request (the requesting supervisor
/// crashed before recycling) self-clears instead of stalling the loop forever.
const DEFAULT_RECYCLE_YIELD_TTL_SECS: u64 = 120;
const RECYCLE_YIELD_TTL_SECS_ENV: &str = "AGENT_DOC_RECYCLE_YIELD_TTL_SECS";

/// Persisted recycle-yield request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecycleYieldRequest {
    /// Why the supervisor asked the loop to yield (e.g. `stale_binary_drain`).
    pub reason: String,
    /// Unix seconds the request was written/refreshed.
    pub requested_secs: u64,
}

/// Canonical reason for a stale-binary self-recycle yield.
pub const RECYCLE_YIELD_STALE_BINARY: &str = "stale_binary_drain";

/// Reason for a fresh-binary state-flush self-recycle yield (`#wd40`): the
/// installed binary already matches, but an explicit `admin recycle` wants the
/// supervisor process restarted to flush stale in-memory state (e.g. a lagging
/// CRDT projection driving `#rt83` phantom-pin churn).
pub const RECYCLE_YIELD_STATE_FLUSH: &str = "state_flush_drain";

/// Resolve the request TTL, honoring the `AGENT_DOC_RECYCLE_YIELD_TTL_SECS`
/// override.
pub fn recycle_yield_ttl() -> Duration {
    let secs = std::env::var(RECYCLE_YIELD_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RECYCLE_YIELD_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the recycle-yield request path for a document. Mirrors
/// [`crate::drain_owner`]: hash the document path and land the sidecar in the
/// nearest ancestor `.agent-doc/` directory.
fn recycle_yield_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir
                .join(RECYCLE_YIELD_DIR)
                .join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(RECYCLE_YIELD_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

/// Write (or refresh) a recycle-yield request for `file` with the current
/// heartbeat. Idempotent: re-requesting just refreshes the timestamp so a
/// long-running stale supervisor keeps the request live until the loop yields.
pub fn request_recycle_yield(file: &str, reason: &str) -> Result<()> {
    let path = recycle_yield_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create recycle-yield dir {}", parent.display()))?;
    }
    let request = RecycleYieldRequest {
        reason: reason.to_string(),
        requested_secs: now_secs(),
    };
    let body =
        serde_json::to_string(&request).context("failed to serialize recycle-yield request")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write recycle-yield request {}", path.display()))?;
    Ok(())
}

/// Read the raw recycle-yield request for `file` (regardless of freshness).
pub fn read_recycle_yield(file: &str) -> Option<RecycleYieldRequest> {
    let path = recycle_yield_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Return the recycle-yield request iff a stale supervisor *currently* wants the
/// in-session loop to yield (the request exists and is fresh against `now`).
/// `None` means the loop should drain as usual.
pub fn fresh_recycle_yield(file: &str, now: u64) -> Option<RecycleYieldRequest> {
    let request = read_recycle_yield(file)?;
    agent_doc_lease::timestamp_is_fresh(request.requested_secs, now, recycle_yield_ttl())
        .then_some(request)
}

/// Convenience boolean: is a fresh recycle-yield request pending for `file`
/// right now? Best-effort and read-only.
pub fn recycle_yield_pending(file: &Path) -> bool {
    fresh_recycle_yield(&file.to_string_lossy(), now_secs()).is_some()
}

/// Best-effort clear of the recycle-yield request (the fresh post-recycle
/// supervisor drops it, and the loop may drop it after yielding).
pub fn clear_recycle_yield(file: &str) {
    let path = recycle_yield_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear recycle-yield request {}: {err}",
            path.display()
        );
    }
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

        request_recycle_yield(&file, RECYCLE_YIELD_STALE_BINARY).unwrap();
        let request = read_recycle_yield(&file).expect("request present after write");
        assert_eq!(request.reason, RECYCLE_YIELD_STALE_BINARY);

        // Fresh against a `now` near the request...
        assert!(fresh_recycle_yield(&file, request.requested_secs).is_some());
        // ...stale against a `now` far past the TTL.
        assert!(
            fresh_recycle_yield(&file, request.requested_secs + 10_000).is_none(),
            "an old request must hand the loop back to a normal drain"
        );
    }

    #[test]
    fn absent_request_reads_as_none() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md").to_string_lossy().to_string();
        assert!(read_recycle_yield(&file).is_none());
        assert!(fresh_recycle_yield(&file, now_secs()).is_none());
    }

    #[test]
    fn clear_removes_the_request_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        request_recycle_yield(&file, RECYCLE_YIELD_STALE_BINARY).unwrap();
        clear_recycle_yield(&file);
        assert!(read_recycle_yield(&file).is_none());
        // Idempotent: clearing an absent request must not panic or warn-fail.
        clear_recycle_yield(&file);
    }
}
