//! `#orphandrain` — controller-side drain for documents with no supervisor.
//!
//! The idle-queue watch that advances a `queue: go` document lives in the
//! **supervisor** (`agent-doc start --route-owned`). That is fine while a
//! supervisor exists — and a silent dead end when one cannot be started.
//!
//! A document whose owner pane is a live agent TUI is exactly that case. The
//! replacement path refuses to cold-start a supervisor by typing a shell command
//! into a live `claude`/`codex` pane (correct — that would corrupt the TUI), and
//! then gives up. The document ends up with no supervisor, therefore no idle
//! watch, therefore no auto-drain: an active queue with drainable heads sits
//! still until a human triggers `Run Agent Doc`. Measured on a live workspace,
//! one such document logged ZERO idle-watch ticks while six siblings logged
//! 2203-3686 each.
//!
//! The insight is that a supervisor is not actually required to advance the
//! queue. A supervisor owns a harness *child*; draining only needs the trigger
//! delivered into an idle owner pane — which is precisely what an editor route
//! dispatch already does. So the project controller, which is already running
//! and already sweeps documents on a watchdog tick, can perform that dispatch
//! for supervisor-less documents.
//!
//! This module is the pure decision. It performs no IO so every gate is
//! testable: the caller supplies observations and executes the outcome.

/// Effective queue-control observation for controller orphan recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanDrainQueueControl {
    Runnable,
    Paused,
    /// A control-plane read failure fails open so a transient SQLite problem
    /// cannot strand otherwise-drainable work.
    ReadFailedFailOpen,
}

impl OrphanDrainQueueControl {
    pub fn allows_unattended_drain(self) -> bool {
        self != Self::Paused
    }

    pub fn diagnostic_event(self, has_drainable_head: bool) -> Option<OrphanDrainEvent> {
        match self {
            Self::Paused if has_drainable_head => Some(OrphanDrainEvent::QueuePausedSuppressed),
            Self::ReadFailedFailOpen => Some(OrphanDrainEvent::QueueControlReadFailed),
            Self::Runnable | Self::Paused => None,
        }
    }
}

/// Stable ops-log vocabulary for the entire orphan-drain subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanDrainEvent {
    AuthorityReadFailed,
    DocumentHashFailed,
    DrainOwnerReadFailed,
    BackoffReadFailed,
    QueueControlReadFailed,
    QueuePausedSuppressed,
    BackoffClaimFailed,
    Dispatch,
    DispatchSkipped,
    DispatchFailed,
    WorkerSettled,
    WorkerFailed,
    WorkerPanicked,
}

impl OrphanDrainEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthorityReadFailed => "controller_orphan_drain_authority_failed",
            Self::DocumentHashFailed => "controller_orphan_drain_hash_failed",
            Self::DrainOwnerReadFailed => "controller_orphan_drain_owner_read_failed",
            Self::BackoffReadFailed => "controller_orphan_drain_backoff_read_failed",
            Self::QueueControlReadFailed => "controller_orphan_drain_queue_control_read_failed",
            Self::QueuePausedSuppressed => "controller_orphan_drain_suppressed",
            Self::BackoffClaimFailed => "controller_orphan_drain_backoff_claim_failed",
            Self::Dispatch => "controller_orphan_drain_dispatch",
            Self::DispatchSkipped => "controller_orphan_drain_dispatch_skipped",
            Self::DispatchFailed => "controller_orphan_drain_dispatch_failed",
            Self::WorkerSettled => "controller_orphan_drain_worker_settled",
            Self::WorkerFailed => "controller_orphan_drain_worker_failed",
            Self::WorkerPanicked => "controller_orphan_drain_worker_panicked",
        }
    }
}

/// Why a supervisor-less document was not dispatched this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanDrainSkip {
    /// The queue is not active — nothing is owed.
    QueueInactive,
    /// No head the supervisor-scope drain would take (operator-gated or noise).
    NoDrainableHead,
    /// A supervisor exists; its own idle watch owns this document.
    SupervisorAlive,
    /// An in-session `/loop` holds the drain lease — dispatching would double-drive.
    LoopOwnsDrain,
    /// The owner pane is busy; a live turn must never be interrupted.
    PaneBusy,
    /// Dispatched too recently — back off rather than spam the pane.
    RecentlyDispatched,
}

/// What the controller watchdog should do for one supervisor-less document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanDrainDecision {
    Skip(OrphanDrainSkip),
    /// Deliver the document trigger into the idle owner pane.
    Dispatch,
}

/// Observations for one document at one watchdog tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrphanDrainObservation {
    pub queue_active: bool,
    /// A head the SUPERVISOR-scope drain would take. Operator-gated heads are
    /// already excluded by that scope, so this never dispatches for work only a
    /// human can do.
    pub has_drainable_head: bool,
    pub supervisor_alive: bool,
    /// A fresh `claude_loop` drain-owner lease exists (an in-session loop is
    /// already draining). Short-TTL and self-expiring, so a stopped loop releases
    /// this on its own and the controller takes over.
    pub loop_owns_drain: bool,
    pub pane_busy: bool,
    /// Seconds since this document was last dispatched by THIS path.
    pub secs_since_last_dispatch: Option<u64>,
}

/// Minimum spacing between controller-issued drain dispatches for one document.
///
/// A dispatched turn takes time to start and mark the pane busy, so without
/// spacing a fast tick could dispatch repeatedly into the same pane before the
/// first trigger registers.
pub const DEFAULT_MIN_DISPATCH_INTERVAL_SECS: u64 = 90;

/// Decide whether the controller should drain a supervisor-less document.
///
/// Fail-quiet by construction: every gate is a skip, never an error. The order
/// matters — cheap ownership gates precede liveness so the common healthy case
/// (a supervisor exists) exits first and the reason is the most specific one.
pub fn orphan_drain_decision(
    observation: OrphanDrainObservation,
    min_dispatch_interval_secs: u64,
) -> OrphanDrainDecision {
    use OrphanDrainSkip::*;
    if observation.supervisor_alive {
        return OrphanDrainDecision::Skip(SupervisorAlive);
    }
    if !observation.queue_active {
        return OrphanDrainDecision::Skip(QueueInactive);
    }
    if !observation.has_drainable_head {
        return OrphanDrainDecision::Skip(NoDrainableHead);
    }
    if observation.loop_owns_drain {
        return OrphanDrainDecision::Skip(LoopOwnsDrain);
    }
    if observation.pane_busy {
        return OrphanDrainDecision::Skip(PaneBusy);
    }
    if observation
        .secs_since_last_dispatch
        .is_some_and(|secs| secs < min_dispatch_interval_secs)
    {
        return OrphanDrainDecision::Skip(RecentlyDispatched);
    }
    OrphanDrainDecision::Dispatch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drainable_orphan() -> OrphanDrainObservation {
        OrphanDrainObservation {
            queue_active: true,
            has_drainable_head: true,
            supervisor_alive: false,
            loop_owns_drain: false,
            pane_busy: false,
            secs_since_last_dispatch: None,
        }
    }

    /// The gap this closes: an active queue with drainable heads, no supervisor
    /// (because the owner pane is a live agent TUI that must not be typed into),
    /// and an idle pane. Before this, nothing advanced it but a human.
    #[test]
    fn a_supervisorless_document_with_drainable_work_is_dispatched() {
        assert_eq!(
            orphan_drain_decision(drainable_orphan(), DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Dispatch
        );
    }

    /// A supervisor's own idle watch owns its documents. This path exists only
    /// for the documents that watch can never reach.
    #[test]
    fn a_supervised_document_is_left_to_its_supervisor() {
        let observation = OrphanDrainObservation {
            supervisor_alive: true,
            ..drainable_orphan()
        };
        assert_eq!(
            orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Skip(OrphanDrainSkip::SupervisorAlive)
        );
    }

    /// `#kp5z`/`#qflood`: an in-session loop holding the lease is the single
    /// drain owner. Dispatching alongside it is exactly the duplicate-trigger
    /// flood the lease exists to prevent.
    #[test]
    fn an_in_session_loop_keeps_sole_ownership_of_the_drain() {
        let observation = OrphanDrainObservation {
            loop_owns_drain: true,
            ..drainable_orphan()
        };
        assert_eq!(
            orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Skip(OrphanDrainSkip::LoopOwnsDrain)
        );
    }

    /// Never interrupt a live turn — the whole point of an idle-gated drain.
    #[test]
    fn a_busy_pane_is_never_interrupted() {
        let observation = OrphanDrainObservation {
            pane_busy: true,
            ..drainable_orphan()
        };
        assert_eq!(
            orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Skip(OrphanDrainSkip::PaneBusy)
        );
    }

    /// Operator-gated / noise heads are excluded upstream by the supervisor drain
    /// scope, so "queue active" alone must not dispatch.
    #[test]
    fn an_active_queue_with_no_drainable_head_is_not_dispatched() {
        let observation = OrphanDrainObservation {
            has_drainable_head: false,
            ..drainable_orphan()
        };
        assert_eq!(
            orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Skip(OrphanDrainSkip::NoDrainableHead)
        );
    }

    #[test]
    fn an_inactive_queue_is_not_dispatched() {
        let observation = OrphanDrainObservation {
            queue_active: false,
            ..drainable_orphan()
        };
        assert_eq!(
            orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Skip(OrphanDrainSkip::QueueInactive)
        );
    }

    #[test]
    fn queue_control_observations_are_typed_and_read_failure_is_fail_open() {
        assert!(!OrphanDrainQueueControl::Paused.allows_unattended_drain());
        assert!(
            OrphanDrainQueueControl::ReadFailedFailOpen.allows_unattended_drain(),
            "a transient control-plane read failure must not strand queue progress"
        );
        assert_eq!(
            OrphanDrainQueueControl::ReadFailedFailOpen
                .diagnostic_event(true)
                .map(OrphanDrainEvent::as_str),
            Some("controller_orphan_drain_queue_control_read_failed"),
        );
        assert_eq!(
            OrphanDrainQueueControl::Paused
                .diagnostic_event(true)
                .map(OrphanDrainEvent::as_str),
            Some("controller_orphan_drain_suppressed"),
        );
    }

    /// A dispatched turn needs time to start and mark the pane busy; without
    /// spacing a fast tick would re-dispatch before the first trigger lands.
    #[test]
    fn dispatch_backs_off_until_the_interval_elapses() {
        let recent = OrphanDrainObservation {
            secs_since_last_dispatch: Some(10),
            ..drainable_orphan()
        };
        assert_eq!(
            orphan_drain_decision(recent, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Skip(OrphanDrainSkip::RecentlyDispatched)
        );

        let elapsed = OrphanDrainObservation {
            secs_since_last_dispatch: Some(DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            ..drainable_orphan()
        };
        assert_eq!(
            orphan_drain_decision(elapsed, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Dispatch
        );
    }

    /// A live supervisor wins over every other skip reason, so the reported
    /// reason stays the most specific one rather than an incidental gate.
    #[test]
    fn supervisor_liveness_is_reported_ahead_of_other_gates() {
        let observation = OrphanDrainObservation {
            queue_active: false,
            has_drainable_head: false,
            supervisor_alive: true,
            loop_owns_drain: true,
            pane_busy: true,
            secs_since_last_dispatch: Some(0),
        };
        assert_eq!(
            orphan_drain_decision(observation, DEFAULT_MIN_DISPATCH_INTERVAL_SECS),
            OrphanDrainDecision::Skip(OrphanDrainSkip::SupervisorAlive)
        );
    }
}
