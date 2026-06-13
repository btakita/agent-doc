//! Pure, side-effect-free supervisor decision policy used by the `start` idle-queue
//! watch and the `#ctlrecycle` R3 self-recycle. Extracted from `start.rs` so the
//! decision state machines (and their deterministic tests) live in one focused module
//! instead of being interleaved with the supervisor's I/O-bearing run loop. Nothing
//! here touches `SupervisorShared`, the filesystem, or a live pane — every function is
//! a total function over its inputs.

/// Decision for the supervisor idle-queue watch
/// (`#jb-run-agent-doc-busy-queue-dispatch-deadlock`).
///
/// When a busy-pane `Run Agent Doc` route enqueues a prompt into `agent:queue
/// auto` and returns `Ok`, the drain is harness-delegated. A Claude session not
/// actively running `/loop` has no guaranteed idle-drain trigger, so the queued
/// head can sit forever (the operator-perceived "deadlock"). The supervisor
/// already watches the owned pane for an idle harness prompt for the one-shot
/// restart auto-trigger; this watch reuses that idle signal to drain a live
/// active-queue head on the busy→idle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdleQueueDrainDecision {
    /// Inject the harness-specific drain payload so the next cycle drains the
    /// active queue head.
    Dispatch,
    /// A recent explicit session clear suppresses passive queue dispatch until
    /// the cooldown expires or an explicit route invocation clears the marker.
    SkipClearCooldown,
    /// The pane is still busy on an active turn — never inject (no-inject-into-active-turn).
    SkipNotIdle,
    /// The harness Stop/idle hook has not ended the owning pane's turn yet.
    SkipTurnActive,
    /// No active `queue_active: true` head remains to drain.
    SkipNoActiveHead,
    /// This exact head was already dispatched and has not advanced/drained yet —
    /// suppress re-firing so a stuck head cannot spin the watch into a hot loop.
    SkipAlreadyDispatched,
    /// A self-driving harness loop (Claude Code `/loop`) holds a fresh
    /// drain-owner lease and owns this drain. The supervisor defers so the two
    /// owners do not both inject `agent-doc <FILE>` into the live input queue
    /// (#kp5z / #qflood).
    SkipSelfDrivingLoopOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdleQueueContextResetDecision {
    Reset,
    SkipNoActiveHead,
    SkipNotIdle,
    SkipTurnActive,
    SkipAlreadyResetHead,
    SkipNoResetNeeded,
}

pub(crate) fn idle_queue_context_reset_decision(
    prompt_visible: bool,
    turn_active: bool,
    active_head: Option<&str>,
    last_context_reset_head: Option<&str>,
    reset_required: bool,
) -> IdleQueueContextResetDecision {
    let Some(head) = active_head else {
        return IdleQueueContextResetDecision::SkipNoActiveHead;
    };
    if !prompt_visible {
        return IdleQueueContextResetDecision::SkipNotIdle;
    }
    if turn_active {
        return IdleQueueContextResetDecision::SkipTurnActive;
    }
    if last_context_reset_head == Some(head) {
        return IdleQueueContextResetDecision::SkipAlreadyResetHead;
    }
    if reset_required {
        IdleQueueContextResetDecision::Reset
    } else {
        IdleQueueContextResetDecision::SkipNoResetNeeded
    }
}

/// Pure idle-queue drain decision. Kept side-effect free so the busy→idle drain
/// state machine is deterministically testable without a live pane.
///
/// `prompt_visible` is the supervisor's idle signal (a dispatch-ready harness
/// prompt is on screen). `active_head` is the live `queue_active: true` ready
/// head (`queue_continuation::live_continuation_head`), and `last_dispatched`
/// is the head this watch last injected a trigger for.
pub(crate) fn idle_queue_drain_decision(
    clear_cooldown_active: bool,
    prompt_visible: bool,
    turn_active: bool,
    self_driving_loop_active: bool,
    active_head: Option<&str>,
    last_dispatched: Option<&str>,
) -> IdleQueueDrainDecision {
    if clear_cooldown_active {
        return IdleQueueDrainDecision::SkipClearCooldown;
    }
    match active_head {
        // No active head: nothing to drain. The caller clears `last_dispatched`
        // so a future re-enqueue of the same prompt text fires again.
        None => IdleQueueDrainDecision::SkipNoActiveHead,
        // Never inject while the pane is mid-turn — that is the
        // no-inject-into-active-turn invariant the route busy path enforces.
        Some(_) if !prompt_visible => IdleQueueDrainDecision::SkipNotIdle,
        // The renderer can briefly show an idle-looking prompt before the
        // harness Stop/idle hook has completed the whole turn.
        Some(_) if turn_active => IdleQueueDrainDecision::SkipTurnActive,
        // A self-driving harness loop owns this drain (fresh drain-owner lease):
        // defer so the supervisor and `/loop` do not both inject the next
        // `agent-doc <FILE>` trigger and flood the live input queue.
        Some(_) if self_driving_loop_active => IdleQueueDrainDecision::SkipSelfDrivingLoopOwner,
        // Dedup against the head we already fired for. A head that is still
        // present after we dispatched (dispatch failed, or the cycle has not
        // consumed it yet) must not be re-fired every idle tick.
        Some(head) if last_dispatched == Some(head) => {
            IdleQueueDrainDecision::SkipAlreadyDispatched
        }
        Some(_) => IdleQueueDrainDecision::Dispatch,
    }
}

/// Whether a lingering *manual* clear cooldown should be auto-expired so an
/// active go-mode queue drain resumes (`#clearcontresume`).
///
/// The clear cooldown (`queue_continuation::write_clear_cooldown`) is written
/// after an operator `session clear` / JB `Clear Exchange` / a delivered
/// deferred clear. Historically it suppressed passive queue dispatch
/// **indefinitely** — `idle_queue_drain_decision` returns `SkipClearCooldown`
/// until an explicit `route::run` ("Run Agent Doc") or a deferred-clear
/// delivery drops the marker. That stalls an active go-mode drain: after a
/// recycle + clear the operator had to manually restart the queue.
///
/// The cooldown's only real job is to avoid dispatching a trigger *into* an
/// in-flight `/clear` before the pane settles. Once the cleared pane has shown
/// a fresh idle harness prompt for `resume_threshold` consecutive polls (the
/// same debounce idea as `STALE_BUSY_RECONCILE_TICKS`), AND there is an active
/// `queue_active: true` head to drain, AND no operator-deferred clear is still
/// pending delivery (that path owns its own resume), the marker has served its
/// purpose. The recycle + clear is then a continuation *step*, not a stop.
///
/// Pure so the policy is unit-testable without a live pane. The caller drops
/// the cooldown marker and lets the next tick dispatch the head normally when
/// this returns `true`. When there is no active head (a plain operator clear
/// with no queue), this returns `false` so the cooldown stays
/// authoritative-until-operator-route, preserving the non-go behavior.
pub(crate) fn clear_cooldown_resume_ready(
    clear_cooldown_active: bool,
    has_active_head: bool,
    prompt_visible: bool,
    turn_active: bool,
    deferred_operator_clear_pending: bool,
    settled_idle_ticks: u32,
    resume_threshold: u32,
) -> bool {
    clear_cooldown_active
        && has_active_head
        && prompt_visible
        && !turn_active
        && !deferred_operator_clear_pending
        && settled_idle_ticks >= resume_threshold
}

/// `#ctlrecycle` R3 — what the idle supervisor watch should do about a binary that
/// no longer matches the installed agent-doc. Pure so the policy is unit-testable.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SupervisorRecycleAction {
    /// Not stale, or not at a turn boundary — do nothing.
    None,
    /// Stale + at a turn boundary but auto-recycle is off: surface it so the
    /// operator restarts deliberately.
    Detect,
    /// Stale + auto-recycle + a queue head is waiting to drain: recycle NOW so the
    /// next queue item runs on the fresh binary. The inter-queue-item boundary is the
    /// deliberate restart point (`#suprecyclequeue`), so the brief-idle debounce that
    /// guards against transient lulls is bypassed.
    RecycleImmediate,
    /// Stale + auto-recycle + no queue head waiting (end of drain, or between
    /// unrelated turns): recycle once the idle-grace debounce elapses so a momentary
    /// idle gap never thrashes the live agent child.
    RecycleDebounced,
}

/// `#ctlrecycle` R3 / `#suprecyclequeue` — pure recycle policy for the `start`
/// supervisor. Recycling ends the operator's live agent child, so any recycle requires
/// the opt-in flag; without it a stale supervisor at a turn boundary only surfaces
/// (`Detect`).
///
/// `turn_boundary` is `prompt_visible && !turn_active` — the dispatch-ready point after
/// a turn (or queue item) completes. `head_pending` is whether an `agent:queue` head is
/// still waiting to drain: when one is, the *next* queue item is the deliberate restart
/// point, so we recycle promptly (`RecycleImmediate`); when none is, we keep the
/// idle-grace debounce (`RecycleDebounced`, applied by the caller via
/// `recycle_debounce_decision`).
pub(crate) fn supervisor_recycle_action(
    stale: bool,
    auto_recycle: bool,
    turn_boundary: bool,
    head_pending: bool,
) -> SupervisorRecycleAction {
    if !stale || !turn_boundary {
        return SupervisorRecycleAction::None;
    }
    if !auto_recycle {
        return SupervisorRecycleAction::Detect;
    }
    if head_pending {
        SupervisorRecycleAction::RecycleImmediate
    } else {
        SupervisorRecycleAction::RecycleDebounced
    }
}

pub(crate) const REEXEC_CHILD_PID_ENV: &str = "AGENT_DOC_REEXEC_CHILD_PID";
pub(crate) const REEXEC_MASTER_FD_ENV: &str = "AGENT_DOC_REEXEC_MASTER_FD";

/// `#ctlrecycle` R3 — state handed from a stale supervisor to its freshly-`execve`'d
/// self so the new image re-adopts the live harness child instead of spawning a new
/// one. Marshaled through the environment (preserved across `execve`); the document
/// argv is preserved by re-exec'ing with the same args, so only the child PID and the
/// inherited pty master fd need carrying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReexecState {
    pub(crate) child_pid: u32,
    pub(crate) master_fd: i32,
}

impl ReexecState {
    /// Pure parse of the two handoff values; `None` if either is missing/invalid or
    /// would not name a live child + inherited fd.
    fn parse(pid: &str, fd: &str) -> Option<Self> {
        let child_pid = pid.trim().parse::<u32>().ok()?;
        let master_fd = fd.trim().parse::<i32>().ok()?;
        if child_pid == 0 || master_fd < 0 {
            return None;
        }
        Some(Self {
            child_pid,
            master_fd,
        })
    }

    /// Read the re-entry state from the environment, if this process was launched by
    /// a supervisor self-`execve`. `None` for a normal start.
    pub(crate) fn from_env() -> Option<Self> {
        Self::parse(
            &std::env::var(REEXEC_CHILD_PID_ENV).ok()?,
            &std::env::var(REEXEC_MASTER_FD_ENV).ok()?,
        )
    }

    /// `(key, value)` env pairs to set before `execve` so the new image re-adopts.
    pub(crate) fn to_env(self) -> [(String, String); 2] {
        [
            (REEXEC_CHILD_PID_ENV.to_string(), self.child_pid.to_string()),
            (REEXEC_MASTER_FD_ENV.to_string(), self.master_fd.to_string()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_recycle_action_policy() {
        use SupervisorRecycleAction::*;
        // `#ctlrecycle` R3 / `#suprecyclequeue` policy truth table.
        // (stale, auto_recycle, turn_boundary, head_pending)
        // Fresh binary → never act.
        assert_eq!(supervisor_recycle_action(false, true, true, true), None);
        assert_eq!(supervisor_recycle_action(false, true, true, false), None);
        // Stale but mid-turn (not at a boundary) → never act, even with the flag on.
        assert_eq!(supervisor_recycle_action(true, true, false, true), None);
        assert_eq!(supervisor_recycle_action(true, true, false, false), None);
        // Stale at a turn boundary, auto-recycle OFF → surface only, regardless of
        // whether a queue head is pending.
        assert_eq!(supervisor_recycle_action(true, false, true, false), Detect);
        assert_eq!(supervisor_recycle_action(true, false, true, true), Detect);
        // Stale + boundary + opt-in ON, a queue head waiting → recycle NOW so the next
        // queue item runs on the fresh binary (debounce bypassed).
        assert_eq!(
            supervisor_recycle_action(true, true, true, true),
            RecycleImmediate
        );
        // Stale + boundary + opt-in ON, no head waiting → debounced recycle.
        assert_eq!(
            supervisor_recycle_action(true, true, true, false),
            RecycleDebounced
        );
    }

    #[test]
    fn reexec_state_parses_valid_handoff() {
        assert_eq!(
            ReexecState::parse("4242", "7"),
            Some(ReexecState {
                child_pid: 4242,
                master_fd: 7,
            })
        );
        // Whitespace tolerated.
        assert_eq!(
            ReexecState::parse(" 9 ", " 12 "),
            Some(ReexecState {
                child_pid: 9,
                master_fd: 12,
            })
        );
    }

    #[test]
    fn reexec_state_rejects_invalid_handoff() {
        // pid 0 is not a live child; fd < 0 is not an inherited fd.
        assert_eq!(ReexecState::parse("0", "7"), None);
        assert_eq!(ReexecState::parse("4242", "-1"), None);
        // Non-numeric never re-adopts.
        assert_eq!(ReexecState::parse("x", "7"), None);
        assert_eq!(ReexecState::parse("4242", "fd"), None);
    }

    #[test]
    fn reexec_state_env_round_trips() {
        let state = ReexecState {
            child_pid: 4242,
            master_fd: 7,
        };
        let env = state.to_env();
        assert_eq!(env[0].0, REEXEC_CHILD_PID_ENV);
        assert_eq!(env[0].1, "4242");
        assert_eq!(env[1].0, REEXEC_MASTER_FD_ENV);
        assert_eq!(env[1].1, "7");
        // Marshaled values parse back to the same state.
        assert_eq!(ReexecState::parse(&env[0].1, &env[1].1), Some(state));
    }

    // #jb-run-agent-doc-busy-queue-dispatch-deadlock: the supervisor idle-queue
    // watch must drain a live active-queue head on the busy→idle transition,
    // never inject mid-turn, and never hot-loop on a stuck head.
    #[test]
    fn idle_queue_drain_dispatches_when_idle_with_fresh_active_head() {
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::Dispatch
        );
    }

    #[test]
    fn idle_queue_drain_skips_when_pane_busy_even_with_active_head() {
        // No-inject-into-active-turn: a busy pane (no dispatch-ready prompt)
        // never receives an injected trigger, mirroring the route busy path.
        assert_eq!(
            idle_queue_drain_decision(false, false, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipNotIdle
        );
    }

    #[test]
    fn idle_queue_drain_waits_for_turn_status_idle_even_with_visible_prompt() {
        assert_eq!(
            idle_queue_drain_decision(false, true, true, false, Some("/clear"), None),
            IdleQueueDrainDecision::SkipTurnActive
        );
    }

    #[test]
    fn idle_queue_drain_skips_during_clear_cooldown() {
        assert_eq!(
            idle_queue_drain_decision(true, true, true, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipClearCooldown
        );
    }

    // #clearcontresume: a lingering manual clear cooldown must not suppress an
    // active go-mode queue drain forever. Once the cleared pane settles to a
    // fresh idle prompt and a head is waiting, the cooldown auto-expires so the
    // recycle + clear is a continuation step, not a stall.
    #[test]
    fn clear_cooldown_resumes_active_go_drain_after_settle() {
        // cooldown + active head + idle + settled (>= threshold) + no deferred
        // operator clear → resume.
        assert!(clear_cooldown_resume_ready(
            true,  // clear_cooldown_active
            true,  // has_active_head
            true,  // prompt_visible
            false, // turn_active
            false, // deferred_operator_clear_pending
            4,     // settled_idle_ticks
            4,     // resume_threshold
        ));
    }

    #[test]
    fn clear_cooldown_holds_until_pane_settles() {
        // Not yet settled for the full debounce window → keep skipping so we
        // never dispatch into an in-flight `/clear`.
        assert!(!clear_cooldown_resume_ready(
            true, true, true, false, false, 3, 4,
        ));
        // Pane not idle yet (mid-turn) → never resume.
        assert!(!clear_cooldown_resume_ready(
            true, true, true, true, false, 9, 4,
        ));
        // No visible prompt → never resume.
        assert!(!clear_cooldown_resume_ready(
            true, true, false, false, false, 9, 4,
        ));
    }

    #[test]
    fn clear_cooldown_stays_authoritative_without_active_head() {
        // A plain operator clear with no active queue head keeps the cooldown
        // authoritative-until-operator-route (non-go behavior preserved).
        assert!(!clear_cooldown_resume_ready(
            true, false, true, false, false, 9, 4,
        ));
        // No cooldown at all → nothing to resume.
        assert!(!clear_cooldown_resume_ready(
            false, true, true, false, false, 9, 4,
        ));
    }

    #[test]
    fn clear_cooldown_defers_to_pending_deferred_operator_clear() {
        // The operator explicitly deferred a clear (paused the loop); that path
        // owns delivery + resume, so the auto-resume must stand down.
        assert!(!clear_cooldown_resume_ready(
            true, true, true, false, true, 9, 4,
        ));
    }

    // #clearcontresume: model the idle-watch tick loop's cooldown accounting
    // (the counter increment/reset in `spawn_idle_queue_watch_thread`) over a
    // sequence of polls, asserting resume fires exactly once, only after the
    // debounce window, and only while a head is waiting on an idle pane. This is
    // the deterministic regression for the integrated behavior without live tmux.
    #[test]
    fn clear_cooldown_resume_tick_sequence_debounces_then_resumes() {
        const THRESHOLD: u32 = 4;
        // (clear_cooldown_active, has_head, prompt_visible, turn_active, deferred)
        let ticks = [
            (true, true, false, false, false), // pane still settling (no prompt)
            (true, true, true, true, false),   // prompt back but turn still active
            (true, true, true, false, false),  // idle tick 1
            (true, true, true, false, false),  // idle tick 2
            (true, true, true, false, false),  // idle tick 3
            (true, true, true, false, false),  // idle tick 4 → resume here
            (true, true, true, false, false),  // (would also resume, but cooldown gone)
        ];
        let mut idle_ticks: u32 = 0;
        let mut resumed_at: Option<usize> = None;
        for (i, (cooldown, head, visible, turn, deferred)) in ticks.iter().enumerate() {
            if *cooldown && *head && *visible && !*turn {
                idle_ticks = idle_ticks.saturating_add(1);
            } else {
                idle_ticks = 0;
            }
            if clear_cooldown_resume_ready(
                *cooldown, *head, *visible, *turn, *deferred, idle_ticks, THRESHOLD,
            ) {
                resumed_at = Some(i);
                break; // production drops the marker + `continue`s; loop ends here
            }
        }
        // Indices 0,1 reset the counter; 2..=5 accumulate; the 4th idle tick is
        // index 5, so resume fires there — not before.
        assert_eq!(resumed_at, Some(5));
    }

    #[test]
    fn idle_queue_context_reset_dispatches_clear_once_per_head() {
        assert_eq!(
            idle_queue_context_reset_decision(true, false, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::Reset
        );
        assert_eq!(
            idle_queue_context_reset_decision(true, false, Some("do [#a]"), Some("do [#a]"), true),
            IdleQueueContextResetDecision::SkipAlreadyResetHead
        );
        assert_eq!(
            idle_queue_context_reset_decision(true, false, Some("do [#b]"), Some("do [#a]"), true),
            IdleQueueContextResetDecision::Reset
        );
    }

    #[test]
    fn idle_queue_context_reset_waits_for_idle_and_active_head() {
        assert_eq!(
            idle_queue_context_reset_decision(false, false, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::SkipNotIdle
        );
        assert_eq!(
            idle_queue_context_reset_decision(true, false, None, None, true),
            IdleQueueContextResetDecision::SkipNoActiveHead
        );
        assert_eq!(
            idle_queue_context_reset_decision(true, false, Some("do [#a]"), None, false),
            IdleQueueContextResetDecision::SkipNoResetNeeded
        );
    }

    #[test]
    fn idle_queue_context_reset_waits_for_turn_status_idle() {
        assert_eq!(
            idle_queue_context_reset_decision(true, true, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::SkipTurnActive
        );
    }

    #[test]
    fn idle_queue_drain_skips_when_no_active_head() {
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, None, None),
            IdleQueueDrainDecision::SkipNoActiveHead
        );
        assert_eq!(
            idle_queue_drain_decision(false, false, true, false, None, Some("do [#a]")),
            IdleQueueDrainDecision::SkipNoActiveHead
        );
    }

    #[test]
    fn idle_queue_drain_dedups_already_dispatched_head() {
        // The head we already fired for is still present (cycle has not consumed
        // it yet, or the dispatch failed to drain) — suppress re-firing so a
        // stuck head cannot spin the watch every idle tick.
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, Some("do [#a]"), Some("do [#a]")),
            IdleQueueDrainDecision::SkipAlreadyDispatched
        );
    }

    #[test]
    fn idle_queue_drain_fires_again_when_head_advances() {
        // A different head than the last dispatched one re-fires — the queue
        // advanced to a new prompt that still needs an idle drain.
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, Some("do [#b]"), Some("do [#a]")),
            IdleQueueDrainDecision::Dispatch
        );
    }

    #[test]
    fn idle_queue_drain_defers_to_self_driving_loop_owner() {
        // A fresh drain-owner lease (Claude Code `/loop`) owns the drain: the
        // supervisor defers even on an idle pane with a fresh, un-dispatched head
        // so the two owners do not flood the live input queue (#kp5z / #qflood).
        assert_eq!(
            idle_queue_drain_decision(false, true, false, true, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipSelfDrivingLoopOwner
        );
        // The loop-owner gate also wins over the already-dispatched dedup arm.
        assert_eq!(
            idle_queue_drain_decision(false, true, false, true, Some("do [#a]"), Some("do [#a]")),
            IdleQueueDrainDecision::SkipSelfDrivingLoopOwner
        );
        // No/stale lease (`self_driving_loop_active=false`) ⇒ supervisor drains.
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::Dispatch
        );
        // A busy pane still short-circuits before the loop-owner check.
        assert_eq!(
            idle_queue_drain_decision(false, false, false, true, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipNotIdle
        );
        // No active head still wins (nothing to defer about).
        assert_eq!(
            idle_queue_drain_decision(false, true, false, true, None, None),
            IdleQueueDrainDecision::SkipNoActiveHead
        );
    }
}
