//! `#poolidleheartbeat` — the project controller's subagent pool heartbeat tick.
//!
//! A running worker's claim needs a refresh at half its window, and the
//! coordinator is the only party with evidence the worker is alive. Every
//! surface that told it so needed the coordinator to be doing something: a
//! trigger command, preflight, `plan`, or a tool call's PostToolUse. A
//! coordinator that ended its turn to wait for background workers does none of
//! those, which is the state the runbook tells it to wait in. On 2026-11-05 such
//! a coordinator let 12 live workers' claims expire twice.
//!
//! The controller is the one process with a clock running across that wait, so
//! it ticks: it derives the due set, and when the coordinator's pane is idle it
//! delivers the trigger through the same background route the orphan drain uses
//! (existing pane only, deferring behind a live turn). The woken coordinator's
//! preflight carries the exact refresh commands.
//!
//! The tick delivers and never refreshes. A controller-side refresh would have
//! to assert a liveness it cannot observe, and would erase the stale heartbeat
//! that marks a stopped worker frozen.
//!
//! This module is the pure decision. The caller supplies observations and
//! executes the outcome.

/// How often the tick looks for due heartbeats. A heartbeat falls due an hour
/// before its claim expires under the default TTL, so a minute of latency is
/// nothing, and the controller's ten-second watchdog cadence would only add
/// ledger reads.
pub const POOL_HEARTBEAT_TICK_INTERVAL_SECS: u64 = 60;

/// Minimum spacing of delivery attempts for one document. A route that deferred
/// behind a live turn is retried here, not on every tick.
pub const POOL_HEARTBEAT_MIN_ATTEMPT_INTERVAL_SECS: u64 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolHeartbeatSkip {
    /// This exact due set already reached the coordinator.
    AlreadyDelivered,
    /// An operator pause holds every unattended injection; expiry stays safe
    /// because it never frees a slot or offers the row again.
    QueuePaused,
    /// A self-driving `/loop` reaches its own planning boundary.
    LoopOwnsDrain,
    /// The actor is in a turn or waiting on the operator; that turn's boundary
    /// or tool calls surface the due set, and a live turn is never typed into.
    PaneBusy,
    RecentlyAttempted,
}

impl PoolHeartbeatSkip {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyDelivered => "already_delivered",
            Self::QueuePaused => "queue_paused",
            Self::LoopOwnsDrain => "loop_owns_drain",
            Self::PaneBusy => "pane_busy",
            Self::RecentlyAttempted => "recently_attempted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolHeartbeatDecision {
    Skip(PoolHeartbeatSkip),
    Dispatch,
}

/// Everything the tick decides from, for one document with a due wake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolHeartbeatObservation {
    pub delivered: bool,
    pub queue_paused: bool,
    pub loop_owns_drain: bool,
    pub pane_busy: bool,
    pub secs_since_last_attempt: Option<u64>,
}

pub fn pool_heartbeat_decision(
    observation: PoolHeartbeatObservation,
    min_attempt_interval_secs: u64,
) -> PoolHeartbeatDecision {
    use PoolHeartbeatSkip::*;
    let skip = if observation.delivered {
        Some(AlreadyDelivered)
    } else if observation.queue_paused {
        Some(QueuePaused)
    } else if observation.loop_owns_drain {
        Some(LoopOwnsDrain)
    } else if observation.pane_busy {
        Some(PaneBusy)
    } else if observation
        .secs_since_last_attempt
        .is_some_and(|secs| secs < min_attempt_interval_secs)
    {
        Some(RecentlyAttempted)
    } else {
        None
    };
    skip.map_or(PoolHeartbeatDecision::Dispatch, PoolHeartbeatDecision::Skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: u64 = POOL_HEARTBEAT_MIN_ATTEMPT_INTERVAL_SECS;

    fn idle_coordinator() -> PoolHeartbeatObservation {
        PoolHeartbeatObservation {
            delivered: false,
            queue_paused: false,
            loop_owns_drain: false,
            pane_busy: false,
            secs_since_last_attempt: None,
        }
    }

    #[test]
    fn an_idle_coordinator_with_a_due_set_is_woken() {
        assert_eq!(
            pool_heartbeat_decision(idle_coordinator(), INTERVAL),
            PoolHeartbeatDecision::Dispatch
        );
        let retry = PoolHeartbeatObservation {
            secs_since_last_attempt: Some(INTERVAL),
            ..idle_coordinator()
        };
        assert_eq!(
            pool_heartbeat_decision(retry, INTERVAL),
            PoolHeartbeatDecision::Dispatch,
            "a deferred route is retried once the interval has passed"
        );
    }

    /// Every combination: any held fact holds the wake, whatever the others say.
    #[test]
    fn a_wake_dispatches_only_with_every_gate_clear() {
        for bits in 0..32_u32 {
            let bit = |n: u32| bits & (1 << n) != 0;
            let observation = PoolHeartbeatObservation {
                delivered: bit(0),
                queue_paused: bit(1),
                loop_owns_drain: bit(2),
                pane_busy: bit(3),
                secs_since_last_attempt: bit(4).then_some(INTERVAL - 1),
            };
            assert_eq!(
                pool_heartbeat_decision(observation, INTERVAL) == PoolHeartbeatDecision::Dispatch,
                bits == 0,
                "{observation:?}"
            );
        }
    }

    #[test]
    fn each_hold_names_its_reason() {
        for (observation, skip) in [
            (
                PoolHeartbeatObservation {
                    delivered: true,
                    ..idle_coordinator()
                },
                PoolHeartbeatSkip::AlreadyDelivered,
            ),
            (
                PoolHeartbeatObservation {
                    queue_paused: true,
                    ..idle_coordinator()
                },
                PoolHeartbeatSkip::QueuePaused,
            ),
            (
                PoolHeartbeatObservation {
                    loop_owns_drain: true,
                    ..idle_coordinator()
                },
                PoolHeartbeatSkip::LoopOwnsDrain,
            ),
            (
                PoolHeartbeatObservation {
                    pane_busy: true,
                    ..idle_coordinator()
                },
                PoolHeartbeatSkip::PaneBusy,
            ),
            (
                PoolHeartbeatObservation {
                    secs_since_last_attempt: Some(0),
                    ..idle_coordinator()
                },
                PoolHeartbeatSkip::RecentlyAttempted,
            ),
        ] {
            assert_eq!(
                pool_heartbeat_decision(observation, INTERVAL),
                PoolHeartbeatDecision::Skip(skip)
            );
        }
    }
}
