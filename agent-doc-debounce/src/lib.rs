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

/// `#crdtpushdrain`: how often a settle wait re-requests an urgent CRDT delivery
/// drain while the current transition stays `delivery_pending`. Bounded well under
/// the no-progress budget so a drain that applies nothing gets several attempts
/// instead of one, while staying far coarser than the 100ms observe poll.
pub const URGENT_DRAIN_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

/// `#routeprogresswait`: how much longer a settle wait may run when the Lazily
/// frontier is *actively advancing*, as a multiple of the no-progress budget.
///
/// The no-progress budget is the right deadline for a **wedged** transition, but
/// applying it to a healthy one defers a frontier that is converging normally.
/// Observed text changing between polls is positive evidence of progress, so the
/// no-progress timer resets on every change and only genuine stalls hit the
/// deadline. The ceiling bounds the pathological case where the document is
/// edited continuously and never quiesces.
pub const PROGRESS_WAIT_CEILING_MULTIPLIER: u32 = 6;

/// Poll cadence for observing a Lazily current transition.
pub const SETTLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// Timing budgets for one settle wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleBudget {
    /// Deadline applied to a transition that is making no progress.
    pub no_progress: std::time::Duration,
    /// Absolute ceiling for a transition that advances but never converges.
    pub progress_ceiling: std::time::Duration,
    /// Cadence for re-requesting an urgent delivery drain.
    pub urgent_drain_interval: std::time::Duration,
}

impl SettleBudget {
    /// Derive the standard budgets from a no-progress deadline.
    pub fn from_no_progress(no_progress: std::time::Duration) -> Self {
        Self {
            no_progress,
            progress_ceiling: no_progress.saturating_mul(PROGRESS_WAIT_CEILING_MULTIPLIER),
            urgent_drain_interval: URGENT_DRAIN_RETRY_INTERVAL,
        }
    }
}

/// Elapsed timers observed by the caller for one settle poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleTimers {
    /// Time since the observed frontier last advanced.
    pub stalled_for: std::time::Duration,
    /// Time since the settle wait started.
    pub total_elapsed: std::time::Duration,
    /// Time since the last urgent drain request, or `None` if none was sent yet.
    pub since_last_urgent_drain: Option<std::time::Duration>,
}

/// Why a settle wait gave up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleDeferReason {
    /// The frontier stopped advancing and stayed pending past the no-progress budget.
    NoProgress,
    /// The frontier kept advancing but never converged before the absolute ceiling.
    ProgressCeiling,
}

/// What the caller should do after one settle observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleAction {
    /// The transition settled; proceed.
    Ready,
    /// Keep waiting, optionally re-requesting an urgent delivery drain first.
    Wait { request_urgent_drain: bool },
    /// Give up and fail closed.
    Defer { reason: SettleDeferReason },
}

/// Shared settle-wait decision for Lazily current-transition polls.
///
/// Both the route startup wait and the preflight pre-mutation wait drive this
/// function so an actively-converging frontier is never deferred on wall-clock
/// alone (`#routeprogresswait`) and a pending delivery keeps getting pulled
/// (`#crdtpushdrain`) instead of polling a frontier nobody drains.
pub fn settle_step(
    ready: bool,
    delivery_pending: bool,
    timers: SettleTimers,
    budget: SettleBudget,
) -> SettleAction {
    if ready {
        return SettleAction::Ready;
    }
    if timers.stalled_for >= budget.no_progress {
        return SettleAction::Defer {
            reason: SettleDeferReason::NoProgress,
        };
    }
    if timers.total_elapsed >= budget.progress_ceiling {
        return SettleAction::Defer {
            reason: SettleDeferReason::ProgressCeiling,
        };
    }
    let request_urgent_drain = delivery_pending
        && timers
            .since_last_urgent_drain
            .is_none_or(|since| since >= budget.urgent_drain_interval);
    SettleAction::Wait {
        request_urgent_drain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn timers(stalled_ms: u64, total_ms: u64, drain_ms: Option<u64>) -> SettleTimers {
        SettleTimers {
            stalled_for: Duration::from_millis(stalled_ms),
            total_elapsed: Duration::from_millis(total_ms),
            since_last_urgent_drain: drain_ms.map(Duration::from_millis),
        }
    }

    fn budget() -> SettleBudget {
        SettleBudget::from_no_progress(Duration::from_millis(300))
    }

    #[test]
    fn settled_transition_is_ready() {
        assert_eq!(
            settle_step(true, false, timers(9_999, 9_999, None), budget()),
            SettleAction::Ready
        );
    }

    #[test]
    fn pending_delivery_requests_the_first_urgent_drain_immediately() {
        assert_eq!(
            settle_step(false, true, timers(0, 0, None), budget()),
            SettleAction::Wait {
                request_urgent_drain: true
            }
        );
    }

    #[test]
    fn urgent_drain_retries_on_a_bounded_cadence_not_once() {
        assert_eq!(
            settle_step(false, true, timers(10, 10, Some(100)), budget()),
            SettleAction::Wait {
                request_urgent_drain: false
            }
        );
        assert_eq!(
            settle_step(false, true, timers(10, 10, Some(750)), budget()),
            SettleAction::Wait {
                request_urgent_drain: true
            }
        );
    }

    #[test]
    fn only_delivery_pending_states_request_a_drain() {
        assert_eq!(
            settle_step(false, false, timers(10, 10, None), budget()),
            SettleAction::Wait {
                request_urgent_drain: false
            }
        );
    }

    /// `#routeprogresswait`: an advancing frontier resets `stalled_for`, so total
    /// elapsed far past the no-progress budget must still keep waiting.
    #[test]
    fn an_advancing_frontier_is_not_deferred_on_wall_clock_alone() {
        assert_eq!(
            settle_step(false, true, timers(50, 1_200, Some(0)), budget()),
            SettleAction::Wait {
                request_urgent_drain: false
            }
        );
    }

    #[test]
    fn a_stalled_frontier_still_fails_closed() {
        assert_eq!(
            settle_step(false, true, timers(300, 300, Some(0)), budget()),
            SettleAction::Defer {
                reason: SettleDeferReason::NoProgress
            }
        );
    }

    #[test]
    fn a_frontier_that_never_quiesces_hits_the_absolute_ceiling() {
        assert_eq!(
            settle_step(false, true, timers(10, 1_800, Some(0)), budget()),
            SettleAction::Defer {
                reason: SettleDeferReason::ProgressCeiling
            }
        );
    }

    #[test]
    fn authority_settle_budget_has_a_small_floor_and_scales() {
        assert_eq!(authority_settle_max_wait(0).as_secs(), 3);
        assert_eq!(authority_settle_max_wait(3_000).as_secs(), 3);
        assert_eq!(authority_settle_max_wait(5_500).as_secs(), 6);
    }
}
