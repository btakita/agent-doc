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

use crate::frontmatter::{self, QueueControl};
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

/// Durably pause an active auto-queue by writing canonical inactive queue state.
/// Returns the rewritten content plus the control to restore on resume.
///
/// Idempotent: pausing an already-stopped queue still rewrites to the inactive
/// state and reports the prior control (`Stop`) so a caller can tell that no
/// resume is owed.
pub fn pause_queue_for_operator_command(content: &str) -> Result<(String, QueueControl)> {
    let was_active = frontmatter::parse(content)?.0.queue_active.unwrap_or(false);
    let prior = if was_active {
        QueueControl::Start
    } else {
        QueueControl::Stop
    };
    let paused = frontmatter::merge_queue_state(content, false)?;
    Ok((paused, prior))
}

/// Resume by restoring the recorded queue control after the command completes.
/// Restoring `Stop` is a deliberate no-resume (the queue was already paused
/// before the command, so it stays paused).
pub fn resume_queue_after_operator_command(
    content: &str,
    resume_to: QueueControl,
) -> Result<String> {
    frontmatter::merge_queue_state(content, resume_to.is_active())
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
    fn pause_active_queue_records_prior_start_and_stops() {
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
    fn pause_inactive_queue_reports_stop_prior() {
        let content = "---\nqueue: stop\nqueue_active: false\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let (_, prior) = pause_queue_for_operator_command(content).unwrap();
        assert_eq!(
            prior,
            QueueControl::Stop,
            "an already-stopped queue owes no resume"
        );
    }

    #[test]
    fn pause_then_resume_round_trips_to_active() {
        let content = "---\nqueue: start\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let (paused, prior) = pause_queue_for_operator_command(content).unwrap();
        assert_eq!(frontmatter::parse(&paused).unwrap().0.queue_active, Some(false));
        let resumed = resume_queue_after_operator_command(&paused, prior).unwrap();
        assert_eq!(
            frontmatter::parse(&resumed).unwrap().0.queue_active,
            Some(true),
            "resume must restore the loop the operator command paused:\n{resumed}"
        );
    }

    #[test]
    fn resume_to_stop_keeps_queue_paused() {
        let content = "---\nqueue: stop\nqueue_active: false\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let resumed = resume_queue_after_operator_command(content, QueueControl::Stop).unwrap();
        assert_eq!(
            frontmatter::parse(&resumed).unwrap().0.queue_active,
            Some(false),
            "restoring Stop must not silently re-activate a queue that was already paused"
        );
    }
}
