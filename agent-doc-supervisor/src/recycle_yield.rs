//! Reason labels for recycle-yield requests carried by the PCP recycle graph.
//!
//! The old `.agent-doc/recycle-yield` sidecar has been retired. Idle-watch now
//! publishes a lazily-backed supervisor recycle projection, and
//! queue/preflight/session-check query that projection to yield one boundary.

/// Canonical reason for a stale-binary self-recycle yield.
pub const RECYCLE_YIELD_STALE_BINARY: &str = "stale_binary_drain";

/// Reason for a fresh-binary state-flush self-recycle yield.
pub const RECYCLE_YIELD_STATE_FLUSH: &str = "state_flush_drain";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_are_stable_for_backbone_events() {
        assert_eq!(RECYCLE_YIELD_STALE_BINARY, "stale_binary_drain");
        assert_eq!(RECYCLE_YIELD_STATE_FLUSH, "state_flush_drain");
    }
}
