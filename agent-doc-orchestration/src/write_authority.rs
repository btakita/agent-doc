//! # Module: write_authority
//!
//! ## Spec (`#pcpc5cut` — 08b document write-authority cutover gate ladder)
//! Drives the `specs/08b-single-process-control-plane.md` migration for the
//! **session `.md` document disk write**: moving every same-process writer onto
//! the session actor's single in-process ordered write queue (`#pcpc3`,
//! [`crate::write_queue`]) instead of the bare `flock`-only `atomic_write`. This
//! is the structural root-fix for the supervisor self-race / exit-75
//! (`#ipc-crdt-response-drift` / R6): a route-owned supervisor write and an
//! agent finalize write can interleave between `flock` acquisitions within one
//! process, and `flock` only serializes *across* processes.
//!
//! Per 08b §"Document write and watch authority" and §"Migration gates", the
//! cutover follows a gated ladder, each rung with a rollback flag that returns
//! the previous write authority:
//!
//! - **`off`** (default) — exactly today's behavior: bare `atomic_write`, no
//!   observation, no routing. Shipped users are unchanged until an operator opts
//!   in. This is the rollback target for every later rung.
//! - **`shadow`** (`#pcpc5a`) — the real write still goes through bare
//!   `atomic_write` (unchanged authority); after a successful editor-visible
//!   document write the binary *reports* what the serialized-queue path would
//!   route (canonical doc id + content hash + would-serialize decision) to
//!   `ops.log`, so an operator can confirm the routing target before flipping
//!   the authority. Observe-only: zero behavior change to the write itself.
//! - **`dual-write`** (`#pcpc5b`) — the real editor-visible document write is
//!   routed through the session actor's ordered write queue. The cross-process
//!   advisory `flock` stays as a backstop. This is the rung that removes the
//!   in-process interleave at the root.
//! - **`authority`** (`#pcpc5c`) — the ordered write queue is the sole
//!   in-process serializer for document writes (same routing as `dual-write`;
//!   the distinction is that `flock` is now only a foreign-process backstop, its
//!   documented end-state role).
//! - **`removed`** (`#pcpc5d`/`#pcpc5e`) — reserved for deleting the
//!   out-of-process supervisor direct-disk writers and demoting the editor
//!   plugin WatchService to read-only buffer reporting. Those rungs additionally
//!   require live IntelliJ/Codex editor-boundary proof and are tracked under
//!   review `#xkpf`; at this write-routing layer `removed` behaves like
//!   `authority`.
//!
//! ## Agentic Contracts
//! - [`current_gate`] reads `AGENT_DOC_WRITE_AUTHORITY` on every call (cheap,
//!   no caching) so an operator can advance or roll back a gate without
//!   restarting a long-lived process. An unrecognized value logs one warning and
//!   falls back to `Off` (fail-safe to current behavior, never fail-closed on a
//!   document write).
//! - [`is_visible_document`] gates routing/observation to the editor-visible
//!   `.md` only — `.agent-doc/` sidecar/snapshot writes are never rerouted,
//!   matching the same predicate `record_document_write_provenance` uses.
//! - [`within_owner_scope`] / [`owner_scope_guard`] are the re-entrancy guard.
//!   [`crate::write_queue::run_serialized`] installs the guard while a job runs
//!   on the session-actor owner thread; `atomic_write` checks it and does the
//!   raw write when already inside the owner thread. Without this, a routed
//!   write whose job calls `atomic_write` again would re-enter the same
//!   document's blocking mailbox and deadlock (the owner thread cannot drain the
//!   nested envelope while it is busy running the outer job).
//!
//! ## Evals
//! - `gate_parses_known_values_and_defaults_off`
//! - `gate_unknown_value_falls_back_off`
//! - `visible_document_predicate_skips_agent_doc_sidecars`
//! - `owner_scope_guard_sets_and_restores_thread_local`
//! - (write-path routing + no-deadlock coverage lives in `write.rs` /
//!   `write_queue.rs` SimWorld tests)

use std::cell::Cell;
use std::path::Path;

/// Environment variable that selects the active write-authority migration gate.
pub const ENV_VAR: &str = "AGENT_DOC_WRITE_AUTHORITY";

/// The 08b document write-authority migration gate (see module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteAuthorityGate {
    /// Bare `atomic_write`, no observation, no routing. Default + rollback target.
    Off,
    /// Observe-only: real write unchanged, report the would-route decision.
    Shadow,
    /// Route the real document write through the ordered write queue.
    DualWrite,
    /// Ordered write queue is the sole in-process serializer.
    Authority,
    /// Reserved for supervisor/plugin-writer removal (live-gated); routes like
    /// `Authority` at this layer.
    Removed,
}

impl WriteAuthorityGate {
    /// Parse a gate token. Accepts the canonical spellings plus a few
    /// punctuation/case variants. Returns `None` for unknown values so the
    /// caller can warn and fail safe.
    pub fn parse(s: &str) -> Option<WriteAuthorityGate> {
        match s.trim().to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "" | "off" => Some(WriteAuthorityGate::Off),
            "shadow" => Some(WriteAuthorityGate::Shadow),
            "dual-write" | "dualwrite" => Some(WriteAuthorityGate::DualWrite),
            "authority" => Some(WriteAuthorityGate::Authority),
            "removed" | "removal" => Some(WriteAuthorityGate::Removed),
            _ => None,
        }
    }

    /// Stable lowercase token for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            WriteAuthorityGate::Off => "off",
            WriteAuthorityGate::Shadow => "shadow",
            WriteAuthorityGate::DualWrite => "dual-write",
            WriteAuthorityGate::Authority => "authority",
            WriteAuthorityGate::Removed => "removed",
        }
    }

    /// Whether this gate routes the real document write through the session
    /// actor's ordered write queue (`dual-write` and beyond).
    pub fn routes_through_queue(self) -> bool {
        matches!(
            self,
            WriteAuthorityGate::DualWrite | WriteAuthorityGate::Authority | WriteAuthorityGate::Removed
        )
    }
}

/// Resolve the active gate from the environment on each call. An unrecognized
/// value warns once to stderr and falls back to `Off` (never fail-closed on a
/// document write).
pub fn current_gate() -> WriteAuthorityGate {
    match std::env::var(ENV_VAR) {
        Ok(raw) => WriteAuthorityGate::parse(&raw).unwrap_or_else(|| {
            eprintln!(
                "[write-authority] WARNING: unrecognized {ENV_VAR}={raw:?}; falling back to off"
            );
            WriteAuthorityGate::Off
        }),
        Err(_) => WriteAuthorityGate::Off,
    }
}

/// Whether `path` is the editor-visible session document (not a `.agent-doc/`
/// sidecar/snapshot). Routing and observation apply only to visible documents,
/// matching `write::record_document_write_provenance`.
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
/// [`crate::write_queue::run_serialized`] around the serialized job.
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
    fn gate_parses_known_values_and_defaults_off() {
        assert_eq!(WriteAuthorityGate::parse(""), Some(WriteAuthorityGate::Off));
        assert_eq!(
            WriteAuthorityGate::parse("off"),
            Some(WriteAuthorityGate::Off)
        );
        assert_eq!(
            WriteAuthorityGate::parse("SHADOW"),
            Some(WriteAuthorityGate::Shadow)
        );
        assert_eq!(
            WriteAuthorityGate::parse("dual_write"),
            Some(WriteAuthorityGate::DualWrite)
        );
        assert_eq!(
            WriteAuthorityGate::parse(" Authority "),
            Some(WriteAuthorityGate::Authority)
        );
        assert_eq!(
            WriteAuthorityGate::parse("removal"),
            Some(WriteAuthorityGate::Removed)
        );
        assert!(WriteAuthorityGate::Off.as_str() == "off");
        assert!(!WriteAuthorityGate::Shadow.routes_through_queue());
        assert!(WriteAuthorityGate::DualWrite.routes_through_queue());
        assert!(WriteAuthorityGate::Authority.routes_through_queue());
        assert!(WriteAuthorityGate::Removed.routes_through_queue());
    }

    #[test]
    fn gate_unknown_value_falls_back_off() {
        assert_eq!(WriteAuthorityGate::parse("banana"), None);
    }

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
