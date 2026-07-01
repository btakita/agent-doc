//! Pure supervisor heartbeat-loss policy.

/// Compare a previously-observed heartbeat against the current value to decide
/// whether the supervisor task looks wedged.
///
/// `min_advance` is the number of beats the controller expected to see in the
/// elapsed interval.
pub fn heartbeat_lost(previous: u64, current: u64, min_advance: u64) -> bool {
    current.saturating_sub(previous) < min_advance
}

#[cfg(test)]
mod tests {
    use super::heartbeat_lost;

    #[test]
    fn heartbeat_lost_respects_minimum_advance_threshold() {
        assert!(!heartbeat_lost(10, 10, 0));
        assert!(heartbeat_lost(10, 10, 1));
        assert!(heartbeat_lost(10, 11, 2));
        assert!(!heartbeat_lost(10, 11, 1));
        assert!(!heartbeat_lost(10, 15, 5));
        assert!(!heartbeat_lost(10, 16, 5));
    }

    #[test]
    fn heartbeat_lost_saturates_when_current_regresses() {
        assert!(heartbeat_lost(10, 9, 1));
        assert!(heartbeat_lost(u64::MAX, 0, 1));
        assert!(!heartbeat_lost(u64::MAX, 0, 0));
    }
}
