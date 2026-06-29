//! Binary-detected queue-stall policy (`#qstallguard` Layer B).
//!
//! This is the pure classifier for a clean closeout that required continuation
//! but did not continue on the next turn boundary. Orchestration owns the
//! one-shot sidecar IO; this module owns the turn lifecycle decision.

use serde::{Deserialize, Serialize};

/// The canonical `ops.log` / preflight-warning marker emitted on a detected stall.
pub const QUEUE_STALL_DETECTED: &str =
    "queue_stall_detected reason=no_valid_stop_with_continuation_required";

/// Persisted continuation-pending marker body. Written at a clean closeout that
/// required continuation; reconciled and cleared at the next preflight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationPending {
    /// The cycle id that committed with continuation still required.
    pub cycle_id: String,
    /// Unix seconds the marker was written.
    pub recorded_secs: u64,
}

/// Facts the stall classifier reasons over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StallFacts {
    /// A continuation-pending marker from the prior clean closeout is present.
    pub continuation_pending_marker: bool,
    /// This preflight still computes `queue_continuation_required=true`.
    pub continuation_required_now: bool,
    /// Loop-scope drainable head count this preflight.
    pub drainable_head_count: usize,
    /// A real user prompt edited the in-scope exchange tail this turn.
    pub user_prompt_preempts: bool,
    /// The operator stopped the queue via the sanctioned mechanism.
    pub queue_stopped: bool,
    /// The loop is actively continuing.
    pub loop_is_continuing: bool,
}

/// Outcome of reconciling the continuation-pending marker against the current
/// preflight facts. Every non-`NoMarker` outcome means the caller clears the
/// one-shot marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StallVerdict {
    /// No prior continuation marker.
    NoMarker,
    /// The loop continued.
    LoopContinued,
    /// A legitimate stop: user preemption, queue stopped, or nothing drainable.
    LegitimateStop,
    /// The drain stalled and should emit the carried diagnostic.
    Stalled(String),
}

/// Pure stall classifier (`#qstallguard` Layer B).
///
/// The valid-stop set mirrors the exhaustive loop skip-list: a real user prompt,
/// an operator-set `queue: stop`, or a drained queue. A degraded/stale
/// supervisor, high accretion, or a `semantic_completion_match` warning are not
/// valid stop reasons and intentionally do not suppress the diagnostic.
pub fn classify_stall(facts: &StallFacts) -> StallVerdict {
    if !facts.continuation_pending_marker {
        return StallVerdict::NoMarker;
    }
    if facts.loop_is_continuing {
        return StallVerdict::LoopContinued;
    }
    if facts.user_prompt_preempts
        || facts.queue_stopped
        || !facts.continuation_required_now
        || facts.drainable_head_count == 0
    {
        return StallVerdict::LegitimateStop;
    }
    StallVerdict::Stalled(format!(
        "{QUEUE_STALL_DETECTED} (prior cycle committed with {} drainable head(s) but the \
         loop neither continued nor recorded a valid stop reason - a real user prompt, \
         `queue: stop`, or a drained queue. A degraded/stale supervisor, high accretion, \
         or a semantic_completion_match warning are NOT valid stop reasons.)",
        facts.drainable_head_count
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> StallFacts {
        StallFacts {
            continuation_pending_marker: true,
            continuation_required_now: true,
            drainable_head_count: 1,
            user_prompt_preempts: false,
            queue_stopped: false,
            loop_is_continuing: false,
        }
    }

    #[test]
    fn stall_fires_when_drainable_work_remained_and_loop_did_not_continue() {
        match classify_stall(&base()) {
            StallVerdict::Stalled(msg) => {
                assert!(msg.contains("queue_stall_detected"));
                assert!(msg.contains("no_valid_stop_with_continuation_required"));
            }
            other => panic!("expected Stalled, got {other:?}"),
        }
    }

    #[test]
    fn no_marker_is_inert() {
        let facts = StallFacts {
            continuation_pending_marker: false,
            ..base()
        };
        assert_eq!(classify_stall(&facts), StallVerdict::NoMarker);
    }

    #[test]
    fn loop_continuation_clears_without_diagnostic() {
        let facts = StallFacts {
            loop_is_continuing: true,
            ..base()
        };
        assert_eq!(classify_stall(&facts), StallVerdict::LoopContinued);
    }

    #[test]
    fn each_valid_stop_reason_suppresses_the_diagnostic() {
        assert_eq!(
            classify_stall(&StallFacts {
                user_prompt_preempts: true,
                ..base()
            }),
            StallVerdict::LegitimateStop
        );
        assert_eq!(
            classify_stall(&StallFacts {
                queue_stopped: true,
                ..base()
            }),
            StallVerdict::LegitimateStop
        );
        assert_eq!(
            classify_stall(&StallFacts {
                drainable_head_count: 0,
                continuation_required_now: false,
                ..base()
            }),
            StallVerdict::LegitimateStop
        );
    }

    #[test]
    fn degraded_supervisor_is_not_a_valid_stop_reason() {
        let facts = base();
        assert!(matches!(classify_stall(&facts), StallVerdict::Stalled(_)));
    }
}
