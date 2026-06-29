//! Supervisor recycle-in-flight marker (`#jbdisprecycle`).
//!
//! Distinct from [`crate::recycle_yield`], which asks a self-driving `/loop` to
//! yield ONE boundary so a *stale-binary* supervisor can reach its idle
//! `execve` recycle. This marker answers a different question for a different
//! reader: **"is the project's supervisor physically mid-recycle right now?"**,
//! read by the short-lived `route` dispatch process before it types a trigger
//! into a pane.
//!
//! ## Why this exists
//!
//! `lib-install` auto-recycle drives the supervisor through an `execve`
//! hot-reload burst (`supervisor_auto_install` → `supervisor_perform_reexec`).
//! During that window the project controller is tearing down / restarting its
//! session actors (`start_session failed` for a sibling doc is the smoking
//! gun). A JB `Run Agent Doc` dispatch that lands here reads a *transiently*
//! "ready" pane, types the trigger, but the in-flight route/submit is
//! interrupted by the `execve` so the Enter never lands — the operator sees the
//! trigger typed-without-submit (`#rdypoll`/`#jbclrdispdup` no-submit class).
//!
//! ## Scope: project root, not document
//!
//! A recycle is a property of the **supervisor/project controller**, not one
//! document. The breaking recycle in the live repro was triggered for
//! `agent-doc-bugs2.md` / `tsift.md` yet broke an `sampleportal.md`
//! dispatch — all three share one project controller. So this marker is keyed
//! on the project root (nearest ancestor `.agent-doc/` directory) with a FIXED
//! filename, shared across every document the controller hosts. The dispatch
//! resolves the project root for its target document and checks the same
//! marker.
//!
//! ## TTL
//!
//! Short (default 15s): long enough to span an `execve` recycle + fresh
//! supervisor startup, short enough that a crashed-before-clear marker
//! self-expires instead of wedging dispatch forever. The fresh post-recycle
//! supervisor clears the marker when its watch loop initializes; the TTL is the
//! backstop.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding the recycle-in-flight
/// marker. Mirrors the sibling `.agent-doc/recycle-yield` sidecar layout.
const RECYCLE_INFLIGHT_DIR: &str = ".agent-doc/recycle-inflight";

/// Fixed marker filename — project-scoped, shared across all documents the
/// supervisor hosts (unlike the per-document hash used by `recycle_yield`).
const RECYCLE_INFLIGHT_FILE: &str = "supervisor.json";

/// Default marker freshness window. Spans an `execve` recycle + fresh
/// supervisor startup; self-expires if the recycler dies before clearing.
const DEFAULT_RECYCLE_INFLIGHT_TTL_SECS: u64 = 15;
const RECYCLE_INFLIGHT_TTL_SECS_ENV: &str = "AGENT_DOC_RECYCLE_INFLIGHT_TTL_SECS";

/// Canonical reason for a `lib-install` auto-recycle (rebuild + `execve`).
pub const RECYCLE_INFLIGHT_AUTO_INSTALL: &str = "auto_install_reexec";
/// Canonical reason for an operator-requested supervisor restart reexec.
pub const RECYCLE_INFLIGHT_RESTART: &str = "restart_reexec";

/// Persisted recycle-in-flight marker body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecycleInflightMarker {
    /// Why the supervisor is recycling (e.g. `auto_install_reexec`).
    pub reason: String,
    /// Unix seconds the marker was written/refreshed.
    pub marked_secs: u64,
}

/// Resolve the marker TTL, honoring the `AGENT_DOC_RECYCLE_INFLIGHT_TTL_SECS`
/// override.
pub fn recycle_inflight_ttl() -> Duration {
    let secs = std::env::var(RECYCLE_INFLIGHT_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RECYCLE_INFLIGHT_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the project-scoped marker path for a document. Walks up from the
/// document to the nearest ancestor `.agent-doc/` directory (mirrors
/// [`crate::recycle_yield`]) but lands a FIXED filename so the marker is shared
/// across every document under that project root.
fn recycle_inflight_path(file: &str) -> PathBuf {
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(RECYCLE_INFLIGHT_DIR).join(RECYCLE_INFLIGHT_FILE);
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(RECYCLE_INFLIGHT_DIR)
                .join(RECYCLE_INFLIGHT_FILE);
        }
    }
}

/// Mark (or refresh) the recycle-in-flight marker for `file`'s project with the
/// current heartbeat. Idempotent: re-marking just refreshes the timestamp so a
/// multi-step recycle (install → reexec) keeps the marker live across the whole
/// window.
pub fn mark_recycle_inflight(file: &str, reason: &str) -> Result<()> {
    let path = recycle_inflight_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create recycle-inflight dir {}", parent.display())
        })?;
    }
    let marker = RecycleInflightMarker {
        reason: reason.to_string(),
        marked_secs: now_secs(),
    };
    let body =
        serde_json::to_string(&marker).context("failed to serialize recycle-inflight marker")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write recycle-inflight marker {}", path.display()))?;
    Ok(())
}

/// Read the raw recycle-in-flight marker for `file`'s project (regardless of
/// freshness).
pub fn read_recycle_inflight(file: &str) -> Option<RecycleInflightMarker> {
    let path = recycle_inflight_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Return the marker iff the project's supervisor is *currently* mid-recycle
/// (the marker exists and is fresh against `now`). `None` means dispatch may
/// proceed with the normal ready probe.
pub fn fresh_recycle_inflight(file: &str, now: u64) -> Option<RecycleInflightMarker> {
    let marker = read_recycle_inflight(file)?;
    agent_doc_lease::timestamp_is_fresh(marker.marked_secs, now, recycle_inflight_ttl())
        .then_some(marker)
}

/// Convenience boolean: is the project's supervisor mid-recycle right now?
/// Best-effort and read-only — the dispatch path's pre-inject guard.
pub fn recycle_inflight_pending(file: &str) -> bool {
    fresh_recycle_inflight(file, now_secs()).is_some()
}

/// R2/R3 (`#jbdisprecycle`): bound on how long a caller waits for an in-flight
/// supervisor recycle to settle before giving up. Slightly under the marker TTL
/// so a genuinely-stuck recycle surfaces as retryable rather than hanging the
/// caller indefinitely.
pub const RECYCLE_SETTLE_WAIT: Duration = Duration::from_secs(10);
/// Poll cadence while waiting for a recycle to settle.
pub const RECYCLE_SETTLE_POLL: Duration = Duration::from_millis(250);

/// Block up to `timeout` (polling every `poll`) for the project's supervisor
/// recycle to settle — the in-flight marker clearing (fresh supervisor watch
/// loop start) or expiring (TTL backstop). Returns `true` if the recycle settled
/// within the window (including the no-recycle-in-flight fast path), `false` if
/// it never settled. Shared by R2 (`start_session` retry) and R3 (submit-once
/// resubmit) so both wait the same bounded window before acting.
pub fn wait_for_recycle_settle(file: &str, timeout: Duration, poll: Duration) -> bool {
    if !recycle_inflight_pending(file) {
        return true;
    }
    let started = std::time::Instant::now();
    while recycle_inflight_pending(file) {
        if started.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(poll);
    }
    true
}

/// R2 (`#jbdisprecycle`): a `start_session` that fails *while the project
/// supervisor is mid-`execve` recycle* raced the hot-reload (the controller is
/// tearing down / restarting its session actors) — it is a transient race, not a
/// terminal error. Retry (after the recycle settles) while attempts remain and
/// the supervisor was actually recycling; otherwise surface the error as before.
/// Pure so the retry policy is unit-testable without a live controller.
pub fn start_session_retryable_during_recycle(
    recycle_pending: bool,
    attempts_used: usize,
    max_attempts: usize,
) -> bool {
    recycle_pending && attempts_used < max_attempts
}

/// R3 (`#jbdisprecycle`): once the routed trigger has already been typed into the
/// composer (`dispatch_inject attempt>=1`) but the submit has not been accepted,
/// a recycle in flight means the submit keystroke was dropped across the
/// `execve` boundary. The recovery is to wait for the recycle to settle and then
/// land the submit exactly ONCE — never another full trigger re-type, which would
/// log `dispatch_inject attempt=2` (the `#rdypoll` restack symptom). Returns true
/// when the caller must wait-for-settle before its single resubmit.
pub fn recycle_interrupted_resubmit_should_wait(
    trigger_already_injected: bool,
    recycle_pending: bool,
) -> bool {
    trigger_already_injected && recycle_pending
}

/// Best-effort clear of the recycle-in-flight marker. The fresh post-recycle
/// supervisor drops it when its watch loop initializes; the TTL is the backstop.
pub fn clear_recycle_inflight(file: &str) {
    let path = recycle_inflight_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear recycle-inflight marker {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_then_read_roundtrips_a_fresh_marker() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        mark_recycle_inflight(&file, RECYCLE_INFLIGHT_AUTO_INSTALL).unwrap();
        let marker = read_recycle_inflight(&file).expect("marker present after write");
        assert_eq!(marker.reason, RECYCLE_INFLIGHT_AUTO_INSTALL);

        // Fresh against a `now` near the mark...
        assert!(fresh_recycle_inflight(&file, marker.marked_secs).is_some());
        // ...stale against a `now` far past the TTL.
        assert!(
            fresh_recycle_inflight(&file, marker.marked_secs + 10_000).is_none(),
            "an old marker must let dispatch proceed with the normal ready probe"
        );
    }

    #[test]
    fn marker_is_project_scoped_shared_across_documents() {
        // Two sibling documents under one project root resolve to the SAME
        // marker path — a recycle marked for one doc gates a dispatch for the
        // other (the cross-document #jbdisprecycle repro: agent-doc-bugs2.md
        // recycle broke an sampleportal.md dispatch).
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("nested").join("b.md");
        std::fs::create_dir_all(doc_b.parent().unwrap()).unwrap();
        let doc_a = doc_a.to_string_lossy().to_string();
        let doc_b = doc_b.to_string_lossy().to_string();

        mark_recycle_inflight(&doc_a, RECYCLE_INFLIGHT_AUTO_INSTALL).unwrap();
        assert!(
            recycle_inflight_pending(&doc_b),
            "a recycle marked via doc A must be visible to a dispatch for sibling doc B"
        );
    }

    #[test]
    fn wait_for_settle_returns_immediately_when_no_recycle_pending() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        // No marker written → fast-path true, no sleeping.
        assert!(wait_for_recycle_settle(
            &file,
            Duration::from_secs(10),
            Duration::from_millis(250)
        ));
    }

    #[test]
    fn wait_for_settle_gives_up_when_recycle_never_clears() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        mark_recycle_inflight(&file, RECYCLE_INFLIGHT_AUTO_INSTALL).unwrap();
        // A fresh marker stays pending; a sub-poll timeout must give up (false)
        // rather than block forever, so the caller can fail closed / retry.
        let settled =
            wait_for_recycle_settle(&file, Duration::from_millis(0), Duration::from_millis(1));
        assert!(
            !settled,
            "an unsettling recycle must return false at the timeout, not hang"
        );
    }

    #[test]
    fn start_session_retry_only_while_recycling_and_budget_remains() {
        // Retry only when the supervisor is actually recycling AND attempts remain.
        assert!(start_session_retryable_during_recycle(true, 0, 2));
        assert!(start_session_retryable_during_recycle(true, 1, 2));
        // Budget exhausted → surface the error (terminal).
        assert!(!start_session_retryable_during_recycle(true, 2, 2));
        // Not recycling → a real start_session failure is terminal, never retried.
        assert!(!start_session_retryable_during_recycle(false, 0, 2));
    }

    #[test]
    fn recycle_interrupted_resubmit_waits_only_when_injected_and_recycling() {
        // Trigger typed + recycle in flight → wait for settle, then submit once.
        assert!(recycle_interrupted_resubmit_should_wait(true, true));
        // Recycle in flight but nothing injected yet → R1 pre-inject gate owns it.
        assert!(!recycle_interrupted_resubmit_should_wait(false, true));
        // Injected but no recycle → normal budgeted resubmit, no extra wait.
        assert!(!recycle_interrupted_resubmit_should_wait(true, false));
    }

    #[test]
    fn clear_removes_the_marker_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        mark_recycle_inflight(&file, RECYCLE_INFLIGHT_AUTO_INSTALL).unwrap();
        clear_recycle_inflight(&file);
        assert!(read_recycle_inflight(&file).is_none());
        // Idempotent: clearing an absent marker must not panic or warn-fail.
        clear_recycle_inflight(&file);
    }
}
