//! Pure auto-trigger readiness deadline policy.

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct AutoTriggerMonitor {
    started_at: Instant,
    timeout: Duration,
    timed_out: bool,
}

impl AutoTriggerMonitor {
    pub fn new(started_at: Instant, timeout: Duration) -> Self {
        Self {
            started_at,
            timeout,
            timed_out: false,
        }
    }

    pub fn note_no_prompt(&mut self, now: Instant) -> bool {
        if self.timed_out || now.duration_since(self.started_at) < self.timeout {
            return false;
        }
        self.timed_out = true;
        true
    }

    pub fn stop_outcome(&self) -> AutoTriggerStopOutcome {
        if self.timed_out {
            AutoTriggerStopOutcome::Timeout
        } else {
            AutoTriggerStopOutcome::Cancelled
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AutoTriggerOutcome {
    NotNeeded = 0,
    Pending = 1,
    Sent = 2,
    Timeout = 3,
    SendFailed = 4,
    Cancelled = 5,
    SkippedClearCooldown = 6,
}

impl AutoTriggerOutcome {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Pending,
            2 => Self::Sent,
            3 => Self::Timeout,
            4 => Self::SendFailed,
            5 => Self::Cancelled,
            6 => Self::SkippedClearCooldown,
            _ => Self::NotNeeded,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotNeeded => "not_needed",
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Timeout => "timeout",
            Self::SendFailed => "send_failed",
            Self::Cancelled => "cancelled",
            Self::SkippedClearCooldown => "skipped_clear_cooldown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerStopOutcome {
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CapabilityProofGate {
    NotRequired = 0,
    Pending = 1,
    Proven = 2,
    Failed = 3,
}

impl CapabilityProofGate {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Pending,
            2 => Self::Proven,
            3 => Self::Failed,
            _ => Self::NotRequired,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerCooldownAction {
    Wait,
    Timeout,
}

pub fn auto_trigger_clear_cooldown_action(
    monitor: &mut AutoTriggerMonitor,
    now: Instant,
) -> AutoTriggerCooldownAction {
    if monitor.note_no_prompt(now) {
        AutoTriggerCooldownAction::Timeout
    } else {
        AutoTriggerCooldownAction::Wait
    }
}

/// Hard-deadline decision for the auto-trigger no-prompt wait (`#startupdeadline`).
///
/// The auto-trigger thread used to log a provisional timeout and then keep
/// watching the child forever. A harness that never becomes dispatch-ready
/// therefore left the session silently hanging. This makes the deadline hard:
/// once the monitor's timeout expires without a dispatch-ready prompt, the
/// thread fails closed instead of continuing to poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerNoPromptAction {
    Continue,
    FailClosed,
}

pub fn auto_trigger_no_prompt_action(
    monitor: &mut AutoTriggerMonitor,
    now: Instant,
) -> AutoTriggerNoPromptAction {
    if monitor.note_no_prompt(now) {
        AutoTriggerNoPromptAction::FailClosed
    } else {
        AutoTriggerNoPromptAction::Continue
    }
}

/// What to do after a supervisor auto-trigger inject typed its trigger into a
/// tmux pane (`#restartfreshtriggerstranded`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerSubmitFollowUp {
    /// The composer cleared (or the harness already started the turn): nothing to do.
    Accepted,
    /// The trigger is still sitting unsubmitted in the composer. Resend the bare
    /// harness submit key once.
    ResubmitSubmitKey,
    /// The trigger survived a resubmit, or the pane could not be captured. The
    /// caller must fail closed rather than report a delivered prompt.
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoTriggerSubmitFacts {
    /// Whether the pane capture succeeded. A failed capture is NOT evidence of
    /// delivery — it is an unverified submit.
    pub pane_captured: bool,
    /// Whether the injected trigger text is still visible in the composer.
    pub trigger_pending_in_composer: bool,
    /// Whether the caller has already resent the submit key once for this inject.
    pub already_resubmitted: bool,
}

/// Decide the follow-up for a supervisor auto-trigger pane inject
/// (`#restartfreshtriggerstranded`).
///
/// The live failure this exists for: after a harness-switch restart spawned a
/// fresh `claude` pane, the pane reconciled `ready` a second before the inject
/// fired, so the submit key raced a still-initializing composer. The trigger was
/// typed and then sat there unsubmitted — the operator-visible "the prompt was
/// sent but not submitted". `route`'s fresh-start path already resubmits a
/// stranded trigger (`#jbtsiftnosub2`), but a restart-fresh spawn never goes
/// through it, so the supervisor inject needs the same guarantee at its own
/// entry point.
///
/// A failed capture deliberately fails closed instead of optimistically
/// reporting `Sent`: silently claiming delivery is exactly what stranded the
/// operator's request.
pub const fn auto_trigger_submit_follow_up(
    facts: AutoTriggerSubmitFacts,
) -> AutoTriggerSubmitFollowUp {
    if !facts.pane_captured {
        return AutoTriggerSubmitFollowUp::FailClosed;
    }
    if !facts.trigger_pending_in_composer {
        return AutoTriggerSubmitFollowUp::Accepted;
    }
    if facts.already_resubmitted {
        AutoTriggerSubmitFollowUp::FailClosed
    } else {
        AutoTriggerSubmitFollowUp::ResubmitSubmitKey
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stranded_auto_trigger_resubmits_once_then_fails_closed() {
        // `#restartfreshtriggerstranded`: the live repro. The restart-fresh claude
        // pane reconciled `ready`, the inject typed the trigger, and the submit key
        // was swallowed by the still-initializing composer.
        let stranded = AutoTriggerSubmitFacts {
            pane_captured: true,
            trigger_pending_in_composer: true,
            already_resubmitted: false,
        };
        assert_eq!(
            auto_trigger_submit_follow_up(stranded),
            AutoTriggerSubmitFollowUp::ResubmitSubmitKey
        );

        // One resubmit only — a trigger that survives it is a real failure, and
        // retrying forever would hammer the composer with Enter keys.
        assert_eq!(
            auto_trigger_submit_follow_up(AutoTriggerSubmitFacts {
                already_resubmitted: true,
                ..stranded
            }),
            AutoTriggerSubmitFollowUp::FailClosed
        );
    }

    #[test]
    fn cleared_composer_is_accepted_and_failed_capture_is_never_optimistic() {
        assert_eq!(
            auto_trigger_submit_follow_up(AutoTriggerSubmitFacts {
                pane_captured: true,
                trigger_pending_in_composer: false,
                already_resubmitted: false,
            }),
            AutoTriggerSubmitFollowUp::Accepted
        );
        // A submitted turn stays accepted even on the post-resubmit re-check.
        assert_eq!(
            auto_trigger_submit_follow_up(AutoTriggerSubmitFacts {
                pane_captured: true,
                trigger_pending_in_composer: false,
                already_resubmitted: true,
            }),
            AutoTriggerSubmitFollowUp::Accepted
        );
        // Capture failure must not be read as delivery.
        for already_resubmitted in [false, true] {
            assert_eq!(
                auto_trigger_submit_follow_up(AutoTriggerSubmitFacts {
                    pane_captured: false,
                    trigger_pending_in_composer: false,
                    already_resubmitted,
                }),
                AutoTriggerSubmitFollowUp::FailClosed
            );
        }
    }

    #[test]
    fn auto_trigger_outcome_round_trips_stable_wire_values() {
        let cases = [
            (0, AutoTriggerOutcome::NotNeeded, "not_needed"),
            (1, AutoTriggerOutcome::Pending, "pending"),
            (2, AutoTriggerOutcome::Sent, "sent"),
            (3, AutoTriggerOutcome::Timeout, "timeout"),
            (4, AutoTriggerOutcome::SendFailed, "send_failed"),
            (5, AutoTriggerOutcome::Cancelled, "cancelled"),
            (
                6,
                AutoTriggerOutcome::SkippedClearCooldown,
                "skipped_clear_cooldown",
            ),
        ];

        for (wire_value, outcome, label) in cases {
            assert_eq!(AutoTriggerOutcome::from_u8(wire_value), outcome);
            assert_eq!(outcome as u8, wire_value);
            assert_eq!(outcome.as_str(), label);
        }
        assert_eq!(
            AutoTriggerOutcome::from_u8(7),
            AutoTriggerOutcome::NotNeeded
        );
    }

    #[test]
    fn capability_proof_gate_round_trips_stable_wire_values() {
        let cases = [
            (0, CapabilityProofGate::NotRequired),
            (1, CapabilityProofGate::Pending),
            (2, CapabilityProofGate::Proven),
            (3, CapabilityProofGate::Failed),
        ];

        for (wire_value, gate) in cases {
            assert_eq!(CapabilityProofGate::from_u8(wire_value), gate);
            assert_eq!(gate as u8, wire_value);
        }
        assert_eq!(
            CapabilityProofGate::from_u8(4),
            CapabilityProofGate::NotRequired
        );
    }

    #[test]
    fn auto_trigger_monitor_cancels_before_timeout() {
        let monitor = AutoTriggerMonitor::new(Instant::now(), Duration::from_secs(30));
        assert_eq!(monitor.stop_outcome(), AutoTriggerStopOutcome::Cancelled);
    }

    #[test]
    fn auto_trigger_monitor_preserves_timeout_after_deadline() {
        let start = Instant::now();
        let mut monitor = AutoTriggerMonitor::new(start, Duration::from_millis(5));
        assert!(monitor.note_no_prompt(start + Duration::from_millis(5)));
        assert!(!monitor.note_no_prompt(start + Duration::from_millis(10)));
        assert_eq!(monitor.stop_outcome(), AutoTriggerStopOutcome::Timeout);
    }

    #[test]
    fn auto_trigger_clear_cooldown_waits_until_timeout_instead_of_terminal_skip() {
        let start = Instant::now();
        let mut monitor = AutoTriggerMonitor::new(start, Duration::from_millis(5));

        assert_eq!(
            auto_trigger_clear_cooldown_action(&mut monitor, start + Duration::from_millis(4)),
            AutoTriggerCooldownAction::Wait
        );
        assert_eq!(
            auto_trigger_clear_cooldown_action(&mut monitor, start + Duration::from_millis(5)),
            AutoTriggerCooldownAction::Timeout
        );
        assert_eq!(
            auto_trigger_clear_cooldown_action(&mut monitor, start + Duration::from_millis(10)),
            AutoTriggerCooldownAction::Wait,
            "timeout is reported once; the caller exits after recording it"
        );
    }

    #[test]
    fn auto_trigger_no_prompt_continues_before_deadline_then_fails_closed() {
        let start = Instant::now();
        let mut monitor = AutoTriggerMonitor::new(start, Duration::from_millis(5));

        assert_eq!(
            auto_trigger_no_prompt_action(&mut monitor, start + Duration::from_millis(4)),
            AutoTriggerNoPromptAction::Continue
        );
        assert_eq!(
            auto_trigger_no_prompt_action(&mut monitor, start + Duration::from_millis(5)),
            AutoTriggerNoPromptAction::FailClosed
        );
        assert_eq!(
            auto_trigger_no_prompt_action(&mut monitor, start + Duration::from_millis(10)),
            AutoTriggerNoPromptAction::Continue,
            "fail-closed fires once; the caller returns after recording the startup-miss"
        );
        assert_eq!(monitor.stop_outcome(), AutoTriggerStopOutcome::Timeout);
    }
}
