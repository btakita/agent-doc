//! Pure response-capture lifecycle policy.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    Captured,
    WriteApplied,
    Replayed,
    Committed,
    Discarded,
}

pub const fn capture_state_rank(state: CaptureState) -> u8 {
    match state {
        CaptureState::Captured => 0,
        CaptureState::WriteApplied => 1,
        CaptureState::Replayed => 2,
        CaptureState::Committed => 3,
        CaptureState::Discarded => 4,
    }
}

pub const fn capture_state_can_advance(from: CaptureState, to: CaptureState) -> bool {
    capture_state_rank(to) >= capture_state_rank(from)
}

pub const fn capture_state_is_repairable(state: CaptureState) -> bool {
    matches!(
        state,
        CaptureState::Captured | CaptureState::WriteApplied | CaptureState::Replayed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_state_rank_orders_lifecycle_forward() {
        assert!(
            capture_state_rank(CaptureState::Captured)
                < capture_state_rank(CaptureState::WriteApplied)
        );
        assert!(
            capture_state_rank(CaptureState::WriteApplied)
                < capture_state_rank(CaptureState::Replayed)
        );
        assert!(
            capture_state_rank(CaptureState::Replayed)
                < capture_state_rank(CaptureState::Committed)
        );
        assert!(
            capture_state_rank(CaptureState::Committed)
                < capture_state_rank(CaptureState::Discarded)
        );
    }

    #[test]
    fn capture_state_can_advance_rejects_backward_transitions() {
        assert!(capture_state_can_advance(
            CaptureState::Captured,
            CaptureState::Committed
        ));
        assert!(!capture_state_can_advance(
            CaptureState::Committed,
            CaptureState::Replayed
        ));
    }

    #[test]
    fn capture_state_is_repairable_accepts_unfinished_states() {
        assert!(capture_state_is_repairable(CaptureState::Captured));
        assert!(capture_state_is_repairable(CaptureState::WriteApplied));
        assert!(capture_state_is_repairable(CaptureState::Replayed));
    }

    #[test]
    fn capture_state_is_repairable_rejects_terminal_states() {
        assert!(!capture_state_is_repairable(CaptureState::Committed));
        assert!(!capture_state_is_repairable(CaptureState::Discarded));
    }
}
