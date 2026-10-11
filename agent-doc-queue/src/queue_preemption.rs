//! Operator-command preemption of an active `agent:queue auto` loop
//! (`#autoloop-command-preemption`).
//!
//! When a self-driving `queue: start` + `agent:queue auto go` loop keeps the
//! pane busy ~continuously, an operator command (JB `Clear Session Context` /
//! `agent-doc session clear`) can never find a quiet window and is hard-blocked
//! (`session_clear_live_busy_guard_blocked` / "Wait for the turn to finish").
//! This module owns the pure policy and the durable queue-state transforms that
//! let such a command PREEMPT the loop: pause it, run in the inter-iteration
//! idle gap, then resume — instead of being unrunnable on an auto-looping doc.
//!
//! Phase 1 (this module): the pure decision + durable pause/resume content
//! transforms — non-destructive and fully unit-tested. Phase 2 (gated) wires
//! this into the `session clear` / `interrupt-clear` command path and changes
//! the JB "blocked" message to "deferred until the turn finishes". Phase 3
//! (gated) is the operator live-verify on a busy auto-looping Codex pane.
//! See `tasks/agent-doc/plan-autoloop-command-preemption.md`.

use agent_doc_frontmatter::frontmatter::{self, QueueControl};
use anyhow::Result;

/// What an operator command should do with an active auto-queue loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorQueuePreemption {
    /// The queue is not actively looping (or is already stopped): run the
    /// command normally — there is no loop to preempt.
    RunImmediately,
    /// A NON-interrupting operator command arrived while the queue is
    /// auto-looping. Pause the loop with a durable `queue: stop`, let the
    /// in-flight turn finish (do NOT kill it), run the command in the idle gap,
    /// then resume by restoring `resume_to`.
    PauseRunResume { resume_to: QueueControl },
}

/// Decide how an operator command interacts with the queue loop.
///
/// - A command that explicitly interrupts the live turn ("Interrupt and Clear")
///   stays the destructive path and runs immediately — it is never deferred,
///   because the operator already chose to discard in-flight work.
/// - A non-interrupting command on an active auto-queue defers: pause → run →
///   resume, so the operator gets a quiet window without losing queue items or
///   silently killing in-flight work.
/// - Anything on an inactive queue runs immediately — there is no loop to pause.
pub fn plan_operator_queue_preemption(
    queue_active: bool,
    command_interrupts: bool,
) -> OperatorQueuePreemption {
    if queue_active && !command_interrupts {
        OperatorQueuePreemption::PauseRunResume {
            resume_to: QueueControl::Start,
        }
    } else {
        OperatorQueuePreemption::RunImmediately
    }
}

/// What a non-interrupting operator `clear` should do when it meets an
/// `alive-busy` pane (`#autoloop-command-preemption` Phase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyClearOutcome {
    /// No active auto-queue loop to preempt: keep the existing fail-closed hard
    /// block (`session_clear_live_busy_guard_blocked`). A bare busy pane with no
    /// self-driving loop still leaves the operator a quiet window once the turn
    /// ends, so blocking is correct — there is nothing to preempt.
    HardBlock,
    /// An auto-queue loop is active, so the pane is busy ~continuously and the
    /// operator never gets a quiet window. Pause the loop (durable clear
    /// cooldown) and defer the clear to the next inter-iteration idle gap
    /// instead of hard-blocking, so the command becomes runnable without
    /// silently killing the in-flight turn.
    PauseAndDefer,
    /// A prior non-interrupting clear is already queued for the supervisor's
    /// next idle gap. Do not refresh the marker, rewrite cooldown state, or send
    /// another clear into the live pane.
    AlreadyDeferred,
    /// (`#clearqueuedliveturn`) No auto-queue loop, but the busy pane is running
    /// a harness turn the operator-clear guard proved live, and the document's
    /// supervisor owns that pane and can observe the turn end. Queue one clear
    /// for delivery at that idle boundary instead of refusing: the operator
    /// asked for a clear, the turn will end, and the supervisor is the actor
    /// that sees it end. Nothing is typed into the running turn.
    DeferBehindLiveTurn,
    /// (`#clearqueuedliveturn`) A clear already queued behind a live turn is
    /// pending and the operator pressed Clear again while a turn is live.
    /// Re-arm the ONE queued clear (a new clear epoch) so it belongs to the turn
    /// that is live now, or about to start, instead of absorbing the press into
    /// the older deferral. The older deferral is fenced to an earlier turn, so
    /// the newer prompt the operator started before this press would supersede
    /// it and drop both presses (Clear → Run Agent Doc → Clear ended with zero
    /// clears, haiven-dev `tasks/sdk.md`, 2026-10-11). Still one clear; nothing
    /// is typed into the running turn.
    RearmBehindLiveTurn,
}

/// Decide what a non-interrupting `clear` does against a busy pane: a busy pane
/// driven by an active auto-queue loop defers (pause → idle gap → run); a pane
/// running a proven-live harness turn whose end the supervisor can observe
/// queues the clear behind that turn (`#clearqueuedliveturn`); any other busy
/// pane keeps the hard block, because no actor would ever deliver a queued
/// clear there. The destructive "Interrupt and Clear" path never reaches here —
/// it interrupts immediately.
pub fn plan_busy_clear(
    queue_active: bool,
    deferred_clear_pending: bool,
    pending_is_live_turn_deferral: bool,
    live_turn_deliverable: bool,
) -> BusyClearOutcome {
    if deferred_clear_pending {
        // An auto-loop deferral owns delivery at the loop's idle gap; only a
        // live-turn deferral is re-armed against the turn live now.
        return if pending_is_live_turn_deferral && !queue_active && live_turn_deliverable {
            BusyClearOutcome::RearmBehindLiveTurn
        } else {
            BusyClearOutcome::AlreadyDeferred
        };
    }
    if queue_active {
        BusyClearOutcome::PauseAndDefer
    } else if live_turn_deliverable {
        BusyClearOutcome::DeferBehindLiveTurn
    } else {
        BusyClearOutcome::HardBlock
    }
}

/// (`#clearqueuedliveturn`) Supersession of a deferred operator clear by a
/// newer prompt.
///
/// A deferred clear belongs to the turn that was running when the operator
/// asked for it. If a NEWER prompt starts on the pane before the clear is
/// delivered (the operator typed or routed another prompt, or the harness ran
/// a message the operator queued in its composer), the operator has chosen to
/// continue the conversation, and delivering the clear after that turn would
/// wipe context the new prompt depends on. The clear is then retired, never
/// delivered.
///
/// The turn identity is the harness-authored turn-active lease's start second
/// (`UserPromptSubmit` writes it once per prompt; nothing refreshes it mid-turn).
/// The baseline is the FIRST lease observed for this deferral, not the lease at
/// deferral time: a clear queued on `dispatch_turn_running` evidence can be
/// recorded before the dispatched turn's hook writes its lease, and that lease
/// is the deferred turn itself, not a newer prompt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeferredClearTurnBaseline {
    clear_epoch: Option<u64>,
    turn_started_at: Option<u64>,
}

impl DeferredClearTurnBaseline {
    /// Observe the pane's current turn lease (`None` when no turn is live) for
    /// the deferred clear `clear_epoch`. Returns `true` when a newer prompt has
    /// superseded the clear. A different `clear_epoch` restarts the baseline.
    pub fn observe(&mut self, clear_epoch: u64, current_turn_started_at: Option<u64>) -> bool {
        if self.clear_epoch != Some(clear_epoch) {
            *self = Self {
                clear_epoch: Some(clear_epoch),
                turn_started_at: current_turn_started_at,
            };
            return false;
        }
        match (self.turn_started_at, current_turn_started_at) {
            (Some(baseline), Some(current)) => current > baseline,
            (None, Some(current)) => {
                self.turn_started_at = Some(current);
                false
            }
            (_, None) => false,
        }
    }

    /// Adopt `turn_started_at` as the deferred clear's own turn: the newer
    /// prompt was one the operator started BEFORE pressing Clear (a dispatch
    /// accepted at or before the press), so the clear belongs after it rather
    /// than being superseded by it (`#clearqueuedliveturn`, Clear → Run Agent
    /// Doc → Clear).
    pub fn adopt(&mut self, turn_started_at: u64) {
        self.turn_started_at = Some(turn_started_at);
    }

    /// The start second of the turn this deferral currently belongs to.
    pub fn turn_started_at(&self) -> Option<u64> {
        self.turn_started_at
    }

    /// Forget the baseline (the deferred clear was delivered or retired).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// (`#clearqueuedliveturn`) Is the pane at an idle boundary where a deferred
/// clear may be delivered? A dispatch-ready prompt with no turn-active lease is
/// the auto-loop gap. A clear queued behind a live turn additionally waits out
/// a `running` dispatch receipt: that turn may have been proven live by the
/// receipt alone, and a composer redrawn between tool calls is not an idle
/// boundary (GH #232). The receipt settles on the turn's `Ready` edge.
pub fn deferred_clear_delivery_ready(
    prompt_visible: bool,
    turn_active: bool,
    live_turn_deferral: bool,
    dispatch_turn_running: bool,
) -> bool {
    prompt_visible && !turn_active && !(live_turn_deferral && dispatch_turn_running)
}

/// What the supervisor idle-queue watch should do about a deferred operator
/// clear on a given tick (`#autoloop-command-preemption` Phase 2b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredClearStep {
    /// No deferred clear is pending — the watch behaves exactly as before
    /// (drain decision unchanged). This is the common path and MUST be a
    /// complete no-op so the existing auto-loop cannot regress.
    None,
    /// A deferred clear is pending but the pane is still mid-turn — wait for the
    /// inter-iteration idle gap; do not interrupt in-flight work.
    WaitForIdle,
    /// A deferred clear is pending and the pane has reached a dispatch-ready
    /// prompt — deliver the clear now, then clear both markers to resume the
    /// loop.
    Deliver,
}

/// Decide the deferred-clear step for one watch tick. Deliver only when a clear
/// is actually pending AND the pane shows a dispatch-ready prompt; otherwise
/// wait (pending-but-busy) or no-op (nothing pending). Keeping this pure makes
/// the live-thread wiring trivially testable and guarantees the
/// nothing-pending path is a no-op.
pub fn plan_deferred_clear_step(
    deferred_clear_pending: bool,
    prompt_ready: bool,
) -> DeferredClearStep {
    match (deferred_clear_pending, prompt_ready) {
        (false, _) => DeferredClearStep::None,
        (true, false) => DeferredClearStep::WaitForIdle,
        (true, true) => DeferredClearStep::Deliver,
    }
}

/// Durably hold an active auto-queue with `queue: pause` while an operator
/// command runs. Returns the rewritten content plus the control to restore on
/// resume.
///
/// Idempotent: holding an already-paused queue reports the prior control
/// (`Pause`) so a caller can tell that no resume is owed. A queue with no
/// control is in its default `go` mode, so its prior is `Start`.
pub fn pause_queue_for_operator_command(content: &str) -> Result<(String, QueueControl)> {
    let (fm, _) = frontmatter::parse(content)?;
    let prior = if fm.queue.as_deref().and_then(QueueControl::parse) == Some(QueueControl::Pause) {
        QueueControl::Pause
    } else {
        QueueControl::Start
    };
    let paused = frontmatter::merge_queue_hold(content)?;
    Ok((paused, prior))
}

/// Resume after the command completes. Restoring `Pause` is a deliberate
/// no-resume (the queue was already held before the command); restoring
/// `Start` lifts the hold back to the default `go` (`#queuestopremove`).
pub fn resume_queue_after_operator_command(
    content: &str,
    resume_to: QueueControl,
) -> Result<String> {
    if resume_to.is_active() {
        frontmatter::lift_queue_hold(content)
    } else {
        Ok(content.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_interrupting_command_on_active_queue_defers() {
        assert_eq!(
            plan_operator_queue_preemption(true, false),
            OperatorQueuePreemption::PauseRunResume {
                resume_to: QueueControl::Start
            },
            "a plain Clear on an auto-looping doc must pause → run → resume"
        );
    }

    #[test]
    fn interrupting_command_runs_immediately_even_when_active() {
        // "Interrupt and Clear" is the explicit destructive path — never deferred.
        assert_eq!(
            plan_operator_queue_preemption(true, true),
            OperatorQueuePreemption::RunImmediately
        );
    }

    #[test]
    fn command_on_inactive_queue_runs_immediately() {
        assert_eq!(
            plan_operator_queue_preemption(false, false),
            OperatorQueuePreemption::RunImmediately,
            "no active loop means nothing to preempt"
        );
        assert_eq!(
            plan_operator_queue_preemption(false, true),
            OperatorQueuePreemption::RunImmediately
        );
    }

    #[test]
    fn busy_clear_on_active_loop_pauses_and_defers() {
        assert_eq!(
            plan_busy_clear(true, false, false, false),
            BusyClearOutcome::PauseAndDefer,
            "a busy pane driven by an active auto-loop must defer, not hard-block"
        );
    }

    #[test]
    fn busy_clear_without_active_loop_hard_blocks() {
        assert_eq!(
            plan_busy_clear(false, false, false, false),
            BusyClearOutcome::HardBlock,
            "a busy pane with no loop keeps the existing fail-closed block"
        );
    }

    #[test]
    fn busy_clear_already_deferred_does_not_refresh_or_send() {
        assert_eq!(
            plan_busy_clear(true, true, false, false),
            BusyClearOutcome::AlreadyDeferred,
            "a repeated non-interrupting clear must not stack another clear while the first is queued"
        );
        assert_eq!(
            plan_busy_clear(false, true, false, true),
            BusyClearOutcome::AlreadyDeferred,
            "the pending deferred-clear marker owns delivery even if queue state changed"
        );
    }

    /// `#clearqueuedliveturn`: Clear → Run Agent Doc → Clear. The second press
    /// re-arms the one queued clear against the turn live now; absorbing it
    /// into the older deferral let the newer prompt supersede both presses.
    #[test]
    fn repeat_clear_rearms_a_live_turn_deferral_instead_of_absorbing_it() {
        assert_eq!(
            plan_busy_clear(false, true, true, true),
            BusyClearOutcome::RearmBehindLiveTurn
        );
        assert_eq!(
            plan_busy_clear(true, true, true, true),
            BusyClearOutcome::AlreadyDeferred,
            "an active auto-loop owns its deferral"
        );
        assert_eq!(
            plan_busy_clear(false, true, true, false),
            BusyClearOutcome::AlreadyDeferred,
            "without a supervisor to deliver a re-armed clear, keep the pending one"
        );
        assert_eq!(
            plan_busy_clear(false, true, false, true),
            BusyClearOutcome::AlreadyDeferred,
            "an auto-loop deferral is never re-armed"
        );
    }

    #[test]
    fn busy_clear_behind_live_turn_queues_when_the_supervisor_can_deliver() {
        assert_eq!(
            plan_busy_clear(false, false, false, true),
            BusyClearOutcome::DeferBehindLiveTurn,
            "a clear during a proven-live turn is queued for the turn's idle boundary, not refused"
        );
        assert_eq!(
            plan_busy_clear(true, false, false, true),
            BusyClearOutcome::PauseAndDefer,
            "an active auto-loop keeps its pause-and-defer path"
        );
        assert_eq!(
            plan_busy_clear(false, false, false, false),
            BusyClearOutcome::HardBlock,
            "with no supervisor to deliver it, a queued clear would be a lie: refuse"
        );
    }

    /// A newer prompt the operator started before the press is adopted as the
    /// clear's own turn; a prompt after it still supersedes.
    #[test]
    fn an_adopted_newer_turn_becomes_the_deferred_clears_turn() {
        let mut baseline = DeferredClearTurnBaseline::default();
        assert!(!baseline.observe(7, Some(100)));
        assert!(baseline.observe(7, Some(160)), "newer than the baseline");
        baseline.adopt(160);
        assert!(!baseline.observe(7, Some(160)));
        assert!(!baseline.observe(7, None));
        assert!(
            baseline.observe(7, Some(200)),
            "a prompt after the adopted turn supersedes"
        );
    }

    #[test]
    fn deferred_clear_is_superseded_only_by_a_newer_prompt() {
        let mut baseline = DeferredClearTurnBaseline::default();
        // Deferred while turn 100 runs.
        assert!(!baseline.observe(1, Some(100)));
        assert!(
            !baseline.observe(1, Some(100)),
            "the same turn never supersedes"
        );
        // The turn ends: idle is not a newer prompt.
        assert!(!baseline.observe(1, None));
        // A newer prompt started before the clear was delivered.
        assert!(baseline.observe(1, Some(160)));
    }

    #[test]
    fn deferred_clear_adopts_the_dispatched_turns_late_lease_as_its_baseline() {
        // Queued on `dispatch_turn_running` evidence before the hook wrote the
        // lease: the first lease seen IS the deferred turn, not a newer prompt.
        let mut baseline = DeferredClearTurnBaseline::default();
        assert!(!baseline.observe(4, None));
        assert!(!baseline.observe(4, Some(205)));
        assert!(!baseline.observe(4, Some(205)));
        assert!(baseline.observe(4, Some(260)));
    }

    #[test]
    fn deferred_clear_baseline_restarts_for_a_new_clear_epoch() {
        let mut baseline = DeferredClearTurnBaseline::default();
        assert!(!baseline.observe(1, Some(100)));
        assert!(
            !baseline.observe(2, Some(300)),
            "a fresh deferral baselines on its own live turn"
        );
        assert!(baseline.observe(2, Some(301)));
        baseline.reset();
        assert_eq!(baseline, DeferredClearTurnBaseline::default());
    }

    #[test]
    fn live_turn_deferral_waits_out_a_running_dispatch_receipt() {
        // Auto-loop gap: prompt + no lease is enough (unchanged behaviour).
        assert!(deferred_clear_delivery_ready(true, false, false, true));
        // Live-turn deferral: a running receipt means the turn is still live
        // even when the composer is drawn and no lease exists yet.
        assert!(!deferred_clear_delivery_ready(true, false, true, true));
        assert!(deferred_clear_delivery_ready(true, false, true, false));
        assert!(!deferred_clear_delivery_ready(true, true, true, false));
        assert!(!deferred_clear_delivery_ready(false, false, true, false));
    }

    /// One supervisor-watch tick over the pane facts it observes, composed the
    /// way `idle_watch` composes them: supersession first, then the delivery
    /// gate, then the step.
    #[derive(Clone, Copy)]
    struct PaneTick {
        prompt_visible: bool,
        lease: Option<u64>,
        dispatch_running: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum TickOutcome {
        Wait,
        Deliver,
        Superseded,
    }

    fn run_live_turn_deferral(ticks: &[PaneTick]) -> Vec<TickOutcome> {
        let mut baseline = DeferredClearTurnBaseline::default();
        let mut out = Vec::new();
        for tick in ticks {
            if baseline.observe(7, tick.lease) {
                out.push(TickOutcome::Superseded);
                break;
            }
            let ready = deferred_clear_delivery_ready(
                tick.prompt_visible,
                tick.lease.is_some(),
                true,
                tick.dispatch_running,
            );
            match plan_deferred_clear_step(true, ready) {
                DeferredClearStep::Deliver => {
                    out.push(TickOutcome::Deliver);
                    break;
                }
                DeferredClearStep::WaitForIdle => out.push(TickOutcome::Wait),
                DeferredClearStep::None => unreachable!("a deferral is pending"),
            }
        }
        out
    }

    /// `#clearqueuedliveturn`: the pane %149 incident. The operator pressed
    /// Clear Session Context while a dispatched turn ran (proven by its
    /// `running` receipt before the hook wrote a lease). The composer redraws
    /// between tool calls; the clear must wait the whole turn out and land
    /// exactly once, at the idle boundary.
    #[test]
    fn queued_clear_waits_out_the_live_turn_and_delivers_once_at_idle() {
        let ticks = [
            // Dispatched, hook not yet written: composer drawn, receipt running.
            PaneTick {
                prompt_visible: true,
                lease: None,
                dispatch_running: true,
            },
            // Hook wrote the lease for the SAME turn.
            PaneTick {
                prompt_visible: false,
                lease: Some(500),
                dispatch_running: true,
            },
            // Composer redrawn between tool calls mid-turn.
            PaneTick {
                prompt_visible: true,
                lease: Some(500),
                dispatch_running: true,
            },
            // Stop: lease cleared, receipt not yet settled by the Ready edge.
            PaneTick {
                prompt_visible: true,
                lease: None,
                dispatch_running: true,
            },
            // Ready edge settled the receipt: idle boundary.
            PaneTick {
                prompt_visible: true,
                lease: None,
                dispatch_running: false,
            },
            PaneTick {
                prompt_visible: true,
                lease: None,
                dispatch_running: false,
            },
        ];
        assert_eq!(
            run_live_turn_deferral(&ticks),
            vec![
                TickOutcome::Wait,
                TickOutcome::Wait,
                TickOutcome::Wait,
                TickOutcome::Wait,
                TickOutcome::Deliver,
            ]
        );
    }

    /// `#clearqueuedliveturn`: the operator started a newer prompt before the
    /// queued clear was delivered (it ran straight from the composer queue, so
    /// the watch never saw an idle tick). The clear must never fire.
    #[test]
    fn queued_clear_is_retired_when_a_newer_prompt_starts_first() {
        let ticks = [
            PaneTick {
                prompt_visible: false,
                lease: Some(500),
                dispatch_running: false,
            },
            PaneTick {
                prompt_visible: false,
                lease: Some(500),
                dispatch_running: false,
            },
            PaneTick {
                prompt_visible: false,
                lease: Some(530),
                dispatch_running: false,
            },
            PaneTick {
                prompt_visible: true,
                lease: None,
                dispatch_running: false,
            },
        ];
        assert_eq!(
            run_live_turn_deferral(&ticks),
            vec![
                TickOutcome::Wait,
                TickOutcome::Wait,
                TickOutcome::Superseded
            ]
        );
    }

    #[test]
    fn deferred_clear_step_no_op_when_nothing_pending() {
        // The common path: no deferred clear → complete no-op so the existing
        // auto-loop drain cannot regress, regardless of prompt readiness.
        assert_eq!(
            plan_deferred_clear_step(false, true),
            DeferredClearStep::None
        );
        assert_eq!(
            plan_deferred_clear_step(false, false),
            DeferredClearStep::None
        );
    }

    #[test]
    fn deferred_clear_step_waits_while_pane_busy() {
        assert_eq!(
            plan_deferred_clear_step(true, false),
            DeferredClearStep::WaitForIdle,
            "a pending clear must not interrupt the in-flight turn"
        );
    }

    #[test]
    fn deferred_clear_step_delivers_on_idle_prompt() {
        assert_eq!(
            plan_deferred_clear_step(true, true),
            DeferredClearStep::Deliver,
            "deliver the deferred clear once the pane reaches a dispatch-ready prompt"
        );
    }

    #[test]
    fn pause_active_queue_records_prior_start_and_holds() {
        let content = "---\nqueue: start\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let (paused, prior) = pause_queue_for_operator_command(content).unwrap();
        assert_eq!(prior, QueueControl::Start, "active queue's prior is Start");
        let (fm, _) = frontmatter::parse(&paused).unwrap();
        assert_eq!(
            fm.queue_active,
            Some(false),
            "pause must durably deactivate the queue:\n{paused}"
        );
    }

    #[test]
    fn pause_held_queue_reports_pause_prior() {
        let content =
            "---\nqueue: pause\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let (_, prior) = pause_queue_for_operator_command(content).unwrap();
        assert_eq!(
            prior,
            QueueControl::Pause,
            "an already-held queue owes no resume"
        );
    }

    #[test]
    fn pause_then_resume_round_trips_to_active() {
        let content = "---\nqueue: start\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let (paused, prior) = pause_queue_for_operator_command(content).unwrap();
        assert_eq!(
            frontmatter::parse(&paused).unwrap().0.queue_active,
            Some(false)
        );
        assert!(paused.contains("queue: pause"), "{paused}");
        let resumed = resume_queue_after_operator_command(&paused, prior).unwrap();
        let (fm, _) = frontmatter::parse(&resumed).unwrap();
        assert!(
            fm.queue.is_none() && fm.queue_active.is_none(),
            "resume must lift the hold back to the default go:\n{resumed}"
        );
    }

    #[test]
    fn resume_to_pause_keeps_queue_paused() {
        let content =
            "---\nqueue: pause\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let resumed = resume_queue_after_operator_command(content, QueueControl::Pause).unwrap();
        assert_eq!(
            frontmatter::parse(&resumed).unwrap().0.queue_active,
            Some(false),
            "restoring Pause must not silently re-activate a queue that was already paused"
        );
    }
}
