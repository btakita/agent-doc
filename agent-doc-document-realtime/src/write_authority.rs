//! # Module: write_authority
//!
//! ## Spec (`#pcpc5cut` — 08b document write-authority, post-cutover end state)
//! Realizes the `specs/08b-single-process-control-plane.md` document-write
//! authority: **every** same-process visible-document `.md` disk write
//! serializes through the session actor's single in-process ordered write queue
//! (`#pcpc3`, adapted by orchestration's `write_queue`) instead of a bare
//! `flock`-only
//! `atomic_write`. This is the structural root-fix for the supervisor self-race
//! / exit-75 (`#ipc-crdt-response-drift` / R6): a route-owned supervisor write
//! and an agent finalize write could interleave between `flock` acquisitions
//! within one process, and `flock` only serializes *across* processes.
//!
//! ## History
//! This shipped through the 08b migration gate ladder
//! (`off → shadow → dual-write → authority → removed`), each rung behind the
//! `AGENT_DOC_WRITE_AUTHORITY` rollback flag. The cutover is now **complete**:
//! the flag, the gate enum, and the bare-`atomic_write` `off` bypass were
//! removed at the removal rung, so queue routing is unconditional for visible
//! documents. `.agent-doc/` sidecar/snapshot writes and writes already executing
//! on the session-actor owner thread still take the raw path (the latter
//! prevents a re-entrant mailbox deadlock).
//!
//! ## Agentic Contracts
//! - [`is_visible_document`] gates routing to the editor-visible `.md` only —
//!   `.agent-doc/` sidecar/snapshot writes are never rerouted, matching the same
//!   predicate `record_document_write_provenance` uses.
//! - [`within_owner_scope`] / [`owner_scope_guard`] are the re-entrancy guard.
//!   The orchestration write-queue adapter installs the guard while a job runs
//!   on the session-actor owner thread; `atomic_write` checks it and does the
//!   raw write when already inside the owner thread. Without this, a routed
//!   write whose job calls `atomic_write` again would re-enter the same
//!   document's blocking mailbox and deadlock (the owner thread cannot drain the
//!   nested envelope while it is busy running the outer job).
//!
//! ## Evals
//! - `visible_document_predicate_skips_agent_doc_sidecars`
//! - `owner_scope_guard_sets_and_restores_thread_local`
//! - (write-path routing + no-deadlock coverage lives in `write.rs` /
//!   `write_queue.rs` SimWorld tests)

use std::cell::Cell;
use std::path::Path;

/// Whether `path` is the editor-visible session document (not a `.agent-doc/`
/// sidecar/snapshot). Routing applies only to visible documents, matching
/// `write::record_document_write_provenance`.
pub fn is_visible_document(path: &Path) -> bool {
    !path.components().any(|c| c.as_os_str() == ".agent-doc")
}

thread_local! {
    /// True while the current thread is executing a session-actor owner job.
    /// Set by [`owner_scope_guard`]; read by `atomic_write` to avoid re-entering
    /// the blocking mailbox (which would deadlock the owner thread).
    static IN_OWNER_SCOPE: Cell<bool> = const { Cell::new(false) };
}

/// Whether the calling thread is already inside a session-actor owner job.
pub fn within_owner_scope() -> bool {
    IN_OWNER_SCOPE.with(|c| c.get())
}

/// RAII guard that marks the current thread as inside a session-actor owner job
/// for its lifetime, restoring the previous value on drop. Installed by
/// the orchestration write queue around the serialized job.
#[must_use = "the owner scope ends when the guard is dropped"]
pub struct OwnerScopeGuard {
    previous: bool,
}

impl Drop for OwnerScopeGuard {
    fn drop(&mut self) {
        IN_OWNER_SCOPE.with(|c| c.set(self.previous));
    }
}

/// Enter a session-actor owner scope, returning a guard that restores the prior
/// state on drop. Nested entry is supported (the guard saves and restores the
/// previous value).
pub fn owner_scope_guard() -> OwnerScopeGuard {
    let previous = IN_OWNER_SCOPE.with(|c| c.replace(true));
    OwnerScopeGuard { previous }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn visible_document_predicate_skips_agent_doc_sidecars() {
        assert!(is_visible_document(&PathBuf::from("/proj/plan.md")));
        assert!(!is_visible_document(&PathBuf::from(
            "/proj/.agent-doc/live-buffer/abc"
        )));
        assert!(!is_visible_document(&PathBuf::from(
            "/proj/.agent-doc/snapshots/x.md"
        )));
    }

    #[test]
    fn owner_scope_guard_sets_and_restores_thread_local() {
        assert!(!within_owner_scope());
        {
            let _g = owner_scope_guard();
            assert!(within_owner_scope());
            {
                // Nested entry stays true and restores to the prior (true) state.
                let _g2 = owner_scope_guard();
                assert!(within_owner_scope());
            }
            assert!(within_owner_scope());
        }
        assert!(!within_owner_scope());
    }
}
