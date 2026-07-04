//! Reason labels for the PCP-backed supervisor recycle projection.
//!
//! The old `.agent-doc/recycle-inflight` marker store has been retired. Runtime
//! callers publish [`agent_doc_state_backbone::StateFact::SupervisorRecycleStarted`]
//! and wait on the project controller's lazily-backed recycle projection instead.

/// Canonical reason for a `lib-install` auto-recycle.
pub const RECYCLE_INFLIGHT_AUTO_INSTALL: &str = "auto_install_reexec";

/// Canonical reason for an operator-requested supervisor restart reexec.
pub const RECYCLE_INFLIGHT_RESTART: &str = "restart_reexec";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_are_stable_for_backbone_events() {
        assert_eq!(RECYCLE_INFLIGHT_AUTO_INSTALL, "auto_install_reexec");
        assert_eq!(RECYCLE_INFLIGHT_RESTART, "restart_reexec");
    }
}
