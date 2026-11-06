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
    /// The composer was read and the inject typed nothing (`#autotriggerdraftguard`).
    DraftRefused = 7,
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
            7 => Self::DraftRefused,
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
            Self::DraftRefused => "draft_refused",
        }
    }
}

/// What `auto_trigger_inject_command` may do after it has read the composer
/// (`#autotriggerdraftguard`).
///
/// The restart trigger and the Codex capacity `continue` retry both write
/// through that function, and neither caller runs the route composer guard.
/// A busy composer with no draft is still safe to type into: the capacity
/// retry exists to submit `continue` while Codex is showing its error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoTriggerDraftGuard {
    /// The read composer is empty enough to receive this command.
    Type,
    /// This exact command is already the idle composer's draft. Press Enter;
    /// do not type it again.
    SubmitStranded,
    /// Type nothing. `reason` is a stable log token.
    Refuse(&'static str),
}

/// Composer facts for [`auto_trigger_draft_guard`]. The caller has already
/// classified one ANSI or supervisor-screen read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoTriggerDraftFacts {
    /// False only when the destination could not be read. An empty screen is
    /// a completed read.
    pub captured: bool,
    pub agent_addressed: bool,
    /// History recall or an agent-doc notice sitting in the composer. Not ours
    /// to submit, and not safe to type onto.
    pub composer_residue: bool,
    pub protected_input: bool,
    /// An operator draft that is not this exact command.
    pub foreign_draft: bool,
    /// The current draft is exactly this command.
    pub command_drafted: bool,
    pub pane_busy: bool,
}

impl AutoTriggerDraftFacts {
    pub const fn unreadable() -> Self {
        Self {
            captured: false,
            agent_addressed: false,
            composer_residue: false,
            protected_input: false,
            foreign_draft: false,
            command_drafted: false,
            pane_busy: false,
        }
    }
}

/// Decide the inject from one composer read (`#autotriggerdraftguard`).
///
/// Order matches the route guard: an unread composer refuses, a different
/// draft refuses on an idle or a busy pane alike, and this command is
/// submitted only when it is the entire idle draft. Protected input is
/// checked after that so a stranded copy of this command is not reclassified
/// as a Codex "drafted prompt input" refusal.
pub const fn auto_trigger_draft_guard(facts: AutoTriggerDraftFacts) -> AutoTriggerDraftGuard {
    if !facts.captured {
        return AutoTriggerDraftGuard::Refuse("composer_unreadable");
    }
    if facts.agent_addressed {
        return AutoTriggerDraftGuard::Refuse("agent_addressed");
    }
    if facts.composer_residue {
        return AutoTriggerDraftGuard::Refuse("composer_residue");
    }
    if facts.foreign_draft {
        return AutoTriggerDraftGuard::Refuse("foreign_draft");
    }
    if facts.command_drafted && facts.pane_busy {
        return AutoTriggerDraftGuard::Refuse("command_behind_running_turn");
    }
    if facts.command_drafted {
        return AutoTriggerDraftGuard::SubmitStranded;
    }
    if facts.protected_input {
        return AutoTriggerDraftGuard::Refuse("protected_input");
    }
    AutoTriggerDraftGuard::Type
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
            (7, AutoTriggerOutcome::DraftRefused, "draft_refused"),
        ];

        for (wire_value, outcome, label) in cases {
            assert_eq!(AutoTriggerOutcome::from_u8(wire_value), outcome);
            assert_eq!(outcome as u8, wire_value);
            assert_eq!(outcome.as_str(), label);
        }
        assert_eq!(
            AutoTriggerOutcome::from_u8(8),
            AutoTriggerOutcome::NotNeeded
        );
    }

    fn facts(mut apply: impl FnMut(&mut AutoTriggerDraftFacts)) -> AutoTriggerDraftFacts {
        let mut facts = AutoTriggerDraftFacts {
            captured: true,
            ..AutoTriggerDraftFacts::unreadable()
        };
        apply(&mut facts);
        facts
    }

    #[test]
    fn auto_trigger_draft_guard_types_only_a_read_empty_composer() {
        assert_eq!(
            auto_trigger_draft_guard(AutoTriggerDraftFacts::unreadable()),
            AutoTriggerDraftGuard::Refuse("composer_unreadable")
        );
        assert_eq!(
            auto_trigger_draft_guard(facts(|_| {})),
            AutoTriggerDraftGuard::Type,
            "a busy-free empty read is safe to type into"
        );
        assert_eq!(
            auto_trigger_draft_guard(facts(|facts| facts.pane_busy = true)),
            AutoTriggerDraftGuard::Type,
            "the capacity retry types continue into a busy composer that holds no draft"
        );
    }

    #[test]
    fn auto_trigger_draft_guard_refuses_a_foreign_draft_idle_or_busy() {
        for pane_busy in [false, true] {
            assert_eq!(
                auto_trigger_draft_guard(facts(|facts| {
                    facts.foreign_draft = true;
                    facts.pane_busy = pane_busy;
                })),
                AutoTriggerDraftGuard::Refuse("foreign_draft")
            );
        }
        assert_eq!(
            auto_trigger_draft_guard(facts(|facts| facts.agent_addressed = true)),
            AutoTriggerDraftGuard::Refuse("agent_addressed")
        );
        assert_eq!(
            auto_trigger_draft_guard(facts(|facts| facts.composer_residue = true)),
            AutoTriggerDraftGuard::Refuse("composer_residue")
        );
        assert_eq!(
            auto_trigger_draft_guard(facts(|facts| facts.protected_input = true)),
            AutoTriggerDraftGuard::Refuse("protected_input")
        );
    }

    #[test]
    fn auto_trigger_draft_guard_submits_an_exact_idle_draft_and_refuses_it_while_busy() {
        assert_eq!(
            auto_trigger_draft_guard(facts(|facts| facts.command_drafted = true)),
            AutoTriggerDraftGuard::SubmitStranded
        );
        assert_eq!(
            auto_trigger_draft_guard(facts(|facts| {
                facts.command_drafted = true;
                facts.pane_busy = true;
                facts.protected_input = true;
            })),
            AutoTriggerDraftGuard::Refuse("command_behind_running_turn"),
            "a stranded command behind a running turn is not reclassified as protected input, and it is not submitted"
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
