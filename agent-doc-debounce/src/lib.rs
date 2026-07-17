//! Bounded settle budgets for Lazily current-state transitions.
//!
//! This crate intentionally owns no document state. In particular, it does
//! not create typing, live-buffer, status, or write-provenance sidecars and it
//! does not maintain a parallel editor-buffer model. Lazily owns live current
//! state; the agent-doc state ledger owns durable transition facts.

/// Maximum time a command may observe a pending Lazily current transition
/// before returning control to the durable recovery state machine.
pub fn authority_settle_max_wait(settle_ms: u64) -> std::time::Duration {
    std::time::Duration::from_secs(if settle_ms > 3000 {
        (settle_ms / 1000) + 1
    } else {
        3
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_settle_budget_has_a_small_floor_and_scales() {
        assert_eq!(authority_settle_max_wait(0).as_secs(), 3);
        assert_eq!(authority_settle_max_wait(3_000).as_secs(), 3);
        assert_eq!(authority_settle_max_wait(5_500).as_secs(), 6);
    }
}
