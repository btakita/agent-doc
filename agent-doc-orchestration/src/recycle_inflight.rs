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

use agent_doc_supervisor::recycle_inflight::{
    RECYCLE_INFLIGHT_DIR, RECYCLE_INFLIGHT_FILE, RecycleInflightMarker, recycle_inflight_marker,
    recycle_inflight_marker_is_fresh,
};
use anyhow::{Context, Result};

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
    let marker = recycle_inflight_marker(reason, now_secs());
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
    recycle_inflight_marker_is_fresh(&marker, now).then_some(marker)
}

/// Convenience boolean: is the project's supervisor mid-recycle right now?
/// Best-effort and read-only — the dispatch path's pre-inject guard.
pub fn recycle_inflight_pending(file: &str) -> bool {
    fresh_recycle_inflight(file, now_secs()).is_some()
}

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

        mark_recycle_inflight(
            &file,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
        let marker = read_recycle_inflight(&file).expect("marker present after write");
        assert_eq!(
            marker.reason,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL
        );

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

        mark_recycle_inflight(
            &doc_a,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
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
        mark_recycle_inflight(
            &file,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
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
    fn clear_removes_the_marker_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        mark_recycle_inflight(
            &file,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
        clear_recycle_inflight(&file);
        assert!(read_recycle_inflight(&file).is_none());
        // Idempotent: clearing an absent marker must not panic or warn-fail.
        clear_recycle_inflight(&file);
    }
}
