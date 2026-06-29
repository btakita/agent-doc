//! Pure, side-effect-free supervisor decision policy used by the `start` idle-queue
//! watch and the `#ctlrecycle` R3 self-recycle. Extracted from `start.rs` so the
//! decision state machines (and their deterministic tests) live in one focused module
//! instead of being interleaved with the supervisor's I/O-bearing run loop. Nothing
//! here touches `SupervisorShared`, the filesystem, or a live pane — every function is
//! a total function over its inputs.
//!
//! The recycle + clear policy predicates (`clear_cooldown_resume_ready`,
//! `supervisor_recycle_action`, `idle_queue_drain_decision`) and the
//! `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` debounce are `pub` so the offline SimWorld
//! engine can drive the same production decision functions the live supervisor
//! idle-queue watch uses (`#clearcontresume` recycle + clear pipeline
//! simulation), rather than reimplementing the policy in the test harness.

#[cfg(test)]
pub(crate) use agent_doc_queue::queue::idle_queue_context_reset_decision;
pub use agent_doc_queue::queue::{
    BetweenTurnCommandKind, BetweenTurnEnqueuePlan, CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
    IdleQueueDrainDecision, IdleQueueDrainDecisionFacts, between_turn_enqueue_plan,
    clear_cooldown_resume_ready, drain_blocked_awaiting_clear_settle, drain_dispatch_dedup_skip,
    idle_queue_drain_decision, idle_queue_drain_decision_with_editor_typing,
};
pub(crate) use agent_doc_queue::queue::{
    IdleQueueContextClearInFlightDecision, IdleQueueContextClearInFlightFacts,
    IdleQueueContextResetDecision, clean_session_head_forces_context_reset,
    idle_queue_context_clear_in_flight_decision,
    idle_queue_context_reset_decision_with_editor_typing,
};

/// `#ctlrecycle` R3 — what the idle supervisor watch should do about a binary that
/// no longer matches the installed agent-doc. Pure so the policy is unit-testable.
pub use agent_doc_supervisor::lifecycle::SupervisorRecycleAction;

/// `#supselfheal` Phase 3 — maximum number of times a stale supervisor whose
/// in-place `execve` re-exec keeps failing may escalate to a kill+relaunch before
/// the idle watch gives up and continues on the current binary (surfacing the
/// one-time operator-restart hint). Bounded so a relaunch that itself never clears
/// the staleness cannot spin the watch into an unbounded kill loop.
pub const MAX_REEXEC_ESCALATIONS: u32 = agent_doc_supervisor::lifecycle::MAX_REEXEC_ESCALATIONS;

/// `#supselfheal` Phase 3 — whether a re-exec-failure escalation may still fire,
/// given how many kill+relaunch escalations have already been attempted this
/// supervisor lifetime. Pure so the bound is unit-testable without the live
/// kill/relaunch machinery; the caller increments the counter on each escalation.
pub fn reexec_escalation_within_bound(attempts: u32, max: u32) -> bool {
    agent_doc_supervisor::lifecycle::reexec_escalation_within_bound(attempts, max)
}

/// `#midturn-recycle-resume` Phase B — how many CONSECUTIVE idle-watch ticks a
/// recycle may be deferred for an open agent-doc cycle (`DeferCycleOpen`) before
/// the watch escalates and forces the recycle anyway.
///
/// Phase A made a mid-cycle `execve` impossible by deferring every recycle arm
/// while a cycle is open. That deferral is correct for the common case (the cycle
/// commits sub-second and the recycle fires at the next boundary), but a cycle that
/// NEVER closes — a wedged finalize, a stranded `preflight_started` whose harness
/// child died, a leaked in-flight IPC handler — would otherwise starve the recycle
/// forever, leaving the supervisor stuck on a stale binary or unable to honor an
/// operator restart.
///
/// The idle-watch poll interval is `AUTO_TRIGGER_POLL_INTERVAL` (500ms), so the
/// default `40` ticks is ~20 seconds of a continuously-open cycle. A healthy
/// finalize closes its cycle in well under a second (a couple of ticks), so a
/// legitimate cycle never reaches the bound; 20s is long enough that only a
/// genuinely-wedged cycle escalates. The escalation then forces the recycle (drops
/// the interlock for this tick) so a never-closing cycle cannot indefinitely block
/// a stale-binary self-recycle or an operator restart — the open cycle's
/// `#durablerecycle` checkpoint is still on disk, so the genuinely-interrupted turn
/// is re-dispatched on the fresh boot (see [`boot_resume_action`]).
pub const MAX_CYCLE_OPEN_DEFER_TICKS: u32 =
    agent_doc_supervisor::lifecycle::MAX_CYCLE_OPEN_DEFER_TICKS;

/// `#midturn-recycle-resume` Phase B — whether a consecutive run of cycle-open
/// recycle deferrals has reached the escalation bound. Pure so the threshold is
/// unit-testable without the live idle-watch loop; the caller tracks the streak
/// (incrementing on each `DeferCycleOpen` tick, resetting to 0 on any non-defer
/// tick) and forces the recycle once this returns true.
pub fn cycle_open_defer_escalates(consecutive_defers: u32, max: u32) -> bool {
    agent_doc_supervisor::lifecycle::cycle_open_defer_escalates(consecutive_defers, max)
}

/// `#midturn-recycle-resume` Phase B — what a fresh supervisor image (born from an
/// `execve` recycle) should do with the `#durablerecycle` turn checkpoint on boot.
///
/// Phase A guarantees a recycle cannot `execve` mid-cycle, so in the steady state a
/// fresh boot's checkpoint is either committed (nothing to resume) or open with a
/// SURVIVING adopted harness child that is still running the turn (do not
/// re-trigger — the child resumes itself). Phase B handles the residual case Phase A
/// cannot prevent: the harness child died across the recycle window (it crashed, was
/// killed, or the recycle escalated past `MAX_CYCLE_OPEN_DEFER_TICKS` and forced the
/// `execve` over a wedged cycle). Then the surviving-child resume never happens and
/// the interrupted turn must be actively re-dispatched from the checkpoint.
pub use agent_doc_supervisor::lifecycle::BootResumeAction;

/// `#midturn-recycle-resume` Phase B — pure boot-resume policy. Decides what a fresh
/// supervisor image should do with the durable turn checkpoint, given:
/// - `is_recycle_boot`: this image was launched by a supervisor self-`execve`
///   (`ReexecState::from_env().is_some()`).
/// - `cycle_open`: the loaded `#durablerecycle` checkpoint is still open
///   (`CycleState::is_open()` — not Committed/Abandoned).
/// - `child_survived`: the harness child PID handed across the `execve` still names
///   a live (non-zombie) process the fresh image can adopt.
/// - `already_consumed`: a prior boot already re-dispatched this checkpoint
///   (`recycle_resume_consumed` latch), so this boot must not re-dispatch again.
///
/// Idempotency is layered: a committed cycle is never open (so `None`); a surviving
/// child adopts without re-trigger (so the turn is not double-run); and an
/// already-consumed checkpoint never re-dispatches a second time even if the child
/// is still absent. Only an open + child-died + not-yet-consumed checkpoint
/// re-dispatches.
pub fn boot_resume_action(
    is_recycle_boot: bool,
    cycle_open: bool,
    child_survived: bool,
    already_consumed: bool,
) -> BootResumeAction {
    agent_doc_supervisor::lifecycle::boot_resume_action(
        is_recycle_boot,
        cycle_open,
        child_survived,
        already_consumed,
    )
}

/// `#ctlrecycle` R3 / `#suprecyclequeue` / `#supselfheal` — pure recycle policy for the
/// `start` supervisor. On unix the recycle is a blue/green `execve`-preserve-child
/// hot-reload that keeps the live agent child + pane, so it is safe enough to default ON
/// (the caller passes `auto_recycle` resolved from `resolve_supervisor_auto_recycle`,
/// default-on). When an operator explicitly opts OUT (falsey env/frontmatter/project
/// knob), a stale supervisor at a turn boundary only surfaces (`Detect`).
///
/// `turn_boundary` is `prompt_visible && !turn_active` — the dispatch-ready point after
/// a turn (or queue item) completes. `head_pending` is whether an `agent:queue` head is
/// still waiting to drain: when one is, the *next* queue item is the deliberate restart
/// point, so we recycle promptly (`RecycleImmediate`); when none is, we keep the
/// idle-grace debounce (`RecycleDebounced`, applied by the caller via
/// `recycle_debounce_decision`).
///
/// `#supselfheal` Phase 1 (`#supselfheal-adminrecycle`): `explicit_admin` is set
/// when an operator/agent explicitly requested `agent-doc admin recycle` for this
/// supervisor. Explicit intent **overrides an explicit auto-recycle opt-out** — a stale
/// supervisor recycles immediately at the next turn boundary even when
/// `auto_recycle` is false, so `admin recycle` (the gentle fix the closeout path
/// itself recommends) can actually clear a stale-binary supervisor wedge. It still
/// respects `turn_boundary` (never drops a live turn). On a fresh binary it no
/// longer no-ops: an explicit `admin recycle` recycles the *process* to flush stale
/// in-memory supervisor state (a lagging CRDT projection driving `#rt83` phantom-pin
/// churn even when the installed binary already matches — `#wd40`).
///
/// `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): `write_wedged` is the typed
/// fact derived in the write/converge closeout path from repeated
/// `send_failed`/`no_ack`/`retry_without_disk_write` against a nominally-active
/// editor IPC listener. A wedge is the clearest possible proof the live binary is
/// bad, so — like `explicit_admin` — it **overrides an explicit auto-recycle opt-out** and
/// recycles immediately at the next turn boundary instead of sitting in `Detect`.
/// The wedge never has to wait for an opt-in or an idle-grace debounce; the only
/// gate it still respects is `turn_boundary` (never drop a live turn).
///
/// `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): `reexec_failed` is set
/// once a prior in-place `execve` re-exec returned an error (deleted-inode `ENOENT`
/// from a fresh `make install`, or another syscall failure). Re-trying the same
/// doomed `execve` only re-logs `continue_current_binary` forever, so a stale
/// supervisor with a failed re-exec escalates to a bounded kill+relaunch
/// (`EscalateKillRelaunch`) regardless of the opt-in/admin/wedge inputs — that is
/// the deterministic recovery the wedge plan requires. It still respects
/// `turn_boundary` and is a no-op when not stale.
///
/// `#midturn-recycle-resume`: `cycle_open` is the agent-doc-cycle interlock the
/// coarse harness `turn_boundary` does not provide. `turn_boundary` keys off the
/// whole-turn `turn_active` marker (`UserPromptSubmit`→`Stop`), but the agent-doc
/// `preflight → finalize` cycle runs as sub-steps INSIDE one harness turn and owns
/// an in-flight IPC ack connection. If the marker is stale/expired or any window
/// reports a boundary while a cycle is still open, an `execve` recycle would sever
/// that connection mid-cycle and produce `live_prompt_drift_after_preflight`
/// against the pre-recycle preflight baseline (the root of the
/// visible-repair-required wedge). When `cycle_open` is true, every
/// recycle arm below is deferred (`DeferCycleOpen`) so the `execve` only fires at a
/// TRUE quiescent boundary once the cycle commits and IPC drains. This wins over
/// `explicit_admin` / `write_wedged` / `reexec_failed` too — none of those may
/// `execve` mid-finalize; they wait the sub-second until the cycle closes.
// Eight independent boundary facts gate this pure recycle policy (staleness,
// opt-in, harness turn boundary, queue head, admin/wedge/reexec overrides, and the
// `#midturn-recycle-resume` agent-doc-cycle interlock). Each is a distinct decision
// input, not a struct's worth of related state, so a parameter object would only
// obscure the truth table the unit tests assert against.
#[allow(clippy::too_many_arguments)]
pub fn supervisor_recycle_action(
    stale: bool,
    auto_recycle: bool,
    turn_boundary: bool,
    head_pending: bool,
    explicit_admin: bool,
    write_wedged: bool,
    reexec_failed: bool,
    cycle_open: bool,
) -> SupervisorRecycleAction {
    agent_doc_supervisor::lifecycle::supervisor_recycle_action(
        stale,
        auto_recycle,
        turn_boundary,
        head_pending,
        explicit_admin,
        write_wedged,
        reexec_failed,
        cycle_open,
    )
}

/// `#wd40` / `#staleloop-recycle-restart` — whether the idle-watch should ask a
/// self-driving in-session `/loop` to yield one inter-item boundary so a stale
/// supervisor can reach its recycle boundary and `execve`-hot-reload onto the
/// freshly-installed binary.
///
/// The gap this closes: [`supervisor_recycle_action`] only fires at a
/// `turn_boundary` (`prompt_visible && !turn_active`). A continuously
/// self-draining Claude Code `/loop` holds a fresh drain-owner lease AND keeps
/// the harness `turn_active` back-to-back, so the supervisor never reaches that
/// boundary and a freshly-installed binary never hot-reloads — the root of this
/// session's `content_ours` finalize drift + `#rt83` phantom-pin flood. The
/// operator had to manually `make install` + `admin recycle` + end-turn to force
/// the boundary; this automates it.
///
/// Request a yield only when all of:
/// - `would_recycle_at_boundary`: a recycle (or the Phase-3 kill+relaunch
///   escalation) WOULD fire if the boundary were reached — i.e.
///   [`supervisor_recycle_action`] with `turn_boundary = true` is not `None` /
///   `Detect`. A bare `Detect` (auto-recycle opted out, no admin/wedge) does not
///   hot-reload, so yielding the loop for it would only stall the drain.
/// - `drain_owner_active`: a self-driving loop currently owns the drain (a fresh
///   drain-owner lease). Without an attended loop to yield there is nothing to
///   signal — a non-`/loop` harness reaches the boundary on its own.
/// - `!turn_boundary`: the boundary is NOT already reachable. When it is, the
///   recycle fires directly and no yield is needed.
///
/// Pure so the policy is unit-testable without the live supervisor / loop.
pub fn stale_drain_recycle_yield_requested(
    would_recycle_at_boundary: bool,
    drain_owner_active: bool,
    turn_boundary: bool,
) -> bool {
    agent_doc_queue::queue::stale_drain_recycle_yield_requested(
        would_recycle_at_boundary,
        drain_owner_active,
        turn_boundary,
    )
}

/// `#agentreloadrestart` — what the idle/turn-boundary supervisor watch should do
/// about a detected harness change: the harness resolved from current frontmatter
/// (`agent:`) differs from the one the running supervisor launched with. Gated
/// exactly like the `#supselfheal` auto-recycle ([`supervisor_recycle_action`]):
/// only act at a quiet dispatch-ready boundary, never mid-turn, and only when the
/// operator opted in (knob ON, the default). Pure so the boundary policy is
/// unit-testable without a live pane.
pub use agent_doc_supervisor::agent_change::AgentChangeRestartAction;

/// Decide the agent-change-restart action for one watch tick. Restart only when
/// the harness actually changed, the knob is on, AND the pane is idle at a
/// dispatch-ready prompt; otherwise wait (changed-but-busy) or no-op (unchanged /
/// opted out). Keeping this pure makes the boundary gate trivially testable and
/// guarantees an unchanged harness is a complete no-op.
pub fn agent_change_restart_decision(
    harness_changed: bool,
    knob_on: bool,
    prompt_visible: bool,
    turn_active: bool,
) -> AgentChangeRestartAction {
    agent_doc_supervisor::agent_change::agent_change_restart_decision(
        harness_changed,
        knob_on,
        prompt_visible,
        turn_active,
    )
}

/// `#agentreloadrestart` Phase 1b — does a re-resolved harness binary force a FRESH
/// spawn of the new harness? True exactly when the binary the running supervisor
/// launched with differs from the one re-resolved from CURRENT frontmatter. A
/// harness change must spawn the new harness fresh (and must NOT adopt the old
/// child preserved across a supervisor reexec). Pure so the restart loop's
/// re-resolution branch is unit-testable, and so the INERTNESS invariant — same
/// binary ⇒ no fresh-spawn swap — is asserted directly.
pub fn harness_change_forces_fresh_spawn(running_binary: &str, resolved_binary: &str) -> bool {
    agent_doc_supervisor::agent_change::harness_change_forces_fresh_spawn(
        running_binary,
        resolved_binary,
    )
}

/// `#supautoinstall` — what the idle supervisor watch should do about agent-doc's OWN
/// source being newer than the installed binary (the dogfood "a finalize just committed
/// a source edit but nobody built+installed it" state). Pure so the policy is
/// unit-testable. This is the install rung that PRECEDES [`supervisor_recycle_action`]:
/// once the auto-install lands the fresh binary, `process_binary_is_stale` flips true and
/// the existing recycle path hot-reloads onto it.
pub use agent_doc_supervisor::lifecycle::SupervisorInstallAction;

/// `#supautoinstall` — pure auto-install policy for the dogfooding `start` supervisor.
/// Mirrors [`supervisor_recycle_action`]: only acts at a `turn_boundary`
/// (`prompt_visible && !turn_active`); when `auto_install` is opted OUT it surfaces the
/// source-ahead state once (`Detect`) so the operator runs the manual refresh, and when
/// opted in it requests a (caller-debounced) build+install (`Install`). The default is
/// ON only after the caller proves the document is an agent-doc dogfood session.
pub fn supervisor_install_action(
    source_newer: bool,
    auto_install: bool,
    turn_boundary: bool,
) -> SupervisorInstallAction {
    agent_doc_supervisor::lifecycle::supervisor_install_action(
        source_newer,
        auto_install,
        turn_boundary,
    )
}

/// `#supkill-bg` — what an explicit operator `restart-supervisor` (IPC `Restart`)
/// should do at the supervisor's next idle tick, framed as blue/green
/// drain-and-supersede. Pure so the policy is unit-testable independent of the live
/// pty/execve machinery.
pub use agent_doc_supervisor::lifecycle::SupervisorRestartAction;

/// `#supkill-bg` — pure drain-and-supersede restart policy for the `start`
/// supervisor. An explicit `restart-supervisor` is a deliberate operator request, so
/// (unlike the opt-in [`supervisor_recycle_action`]) it always acts — but it still
/// **drains** the in-flight turn first (`AwaitDrain` until `turn_boundary`) so a live
/// turn is never dropped. At the boundary it prefers in-place `execve` re-exec when
/// the binary is stale ([`ReexecInPlace`], the zero-downtime binary upgrade) and
/// otherwise falls back to a normal child relaunch ([`RelaunchChild`]).
///
/// `reexec_intent` is set true when the supervisor binary is stale at request time
/// (the IPC handler stamps it from the idle-watch's live staleness probe). A wedged
/// supervisor that never reaches a `turn_boundary` is reclaimed by the external PCP
/// force-kill backstop ([`crate::supervisor_selfkill`], `#supkill-b`), not here.
pub fn supervisor_restart_action(
    restart_requested: bool,
    reexec_intent: bool,
    turn_boundary: bool,
) -> SupervisorRestartAction {
    agent_doc_supervisor::lifecycle::supervisor_restart_action(
        restart_requested,
        reexec_intent,
        turn_boundary,
    )
}

/// Post-child-exit dispatch for the `start` supervisor run loop. After the
/// harness child exits, the loop checks the IPC-requested flags in a fixed
/// priority order before falling back to the normal crash-policy / clean-exit
/// classification. This pure enum models that decision so the "Stop Agent"
/// keepalive contract can be unit-tested without a live PTY.
pub use agent_doc_supervisor::run_loop::PostChildExitAction;

/// Pure model of the `start` supervisor run loop's post-child-exit flag dispatch.
///
/// The priority order mirrors `start/run.rs`: route-owned completion first, then
/// `stop_requested` (exit), then `stop_agent_requested` (keepalive — kill child,
/// keep supervisor, no auto-restart), then `restart_requested` (auto-restart),
/// then normal classification. Crucially, `StopAgentKeepalive` is returned
/// regardless of `restart_requested` or any harness clean-exit behavior, proving
/// "Stop Agent" never auto-restarts and never exits the supervisor.
pub fn post_child_exit_action(
    route_owned_completion: bool,
    stop_requested: bool,
    stop_agent_requested: bool,
    restart_requested: bool,
) -> PostChildExitAction {
    agent_doc_supervisor::run_loop::post_child_exit_action(
        route_owned_completion,
        stop_requested,
        stop_agent_requested,
        restart_requested,
    )
}

/// `#suphandoff` — compatibility aliases for the controller-owned red/green
/// supervisor handoff state. New code should use
/// `agent_doc_supervisor::handoff::ControllerSupervisorHandoff*`.
pub use agent_doc_supervisor::handoff::{
    ControllerSupervisorHandoffAction as PcpSupervisorHandoffAction,
    ControllerSupervisorHandoffEvent as PcpSupervisorHandoffEvent,
    ControllerSupervisorHandoffState as PcpSupervisorHandoffState,
    ControllerSupervisorHandoffTransition as PcpSupervisorHandoffTransition,
};

/// `#suphandoff` — formal PCP state machine for a future true red/green
/// supervisor replacement.
///
/// The live implementation should use this only as the controller-side policy
/// for a separate standby process. The current production restart path still uses
/// [`supervisor_restart_action`] plus `execve` because that preserves the child
/// without cross-process fd transfer.
pub fn pcp_supervisor_handoff_transition(
    state: PcpSupervisorHandoffState,
    event: PcpSupervisorHandoffEvent,
) -> PcpSupervisorHandoffTransition {
    agent_doc_supervisor::handoff::controller_supervisor_handoff_transition(state, event)
}

/// `#ctlrecycle` R3 — state handed from a stale supervisor to its freshly-`execve`'d
/// self so the new image re-adopts the live harness child instead of spawning a new
/// one. Marshaled through the environment (preserved across `execve`); the document
/// argv is preserved by re-exec'ing with the same args, so only the child PID and the
/// inherited pty master fd need carrying.
pub(crate) use agent_doc_supervisor_process::{
    REEXEC_CHILD_PID_ENV, REEXEC_MASTER_FD_ENV, ReexecState,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_session_head_forces_context_reset_policy() {
        // #cleandrainsup: a clean-session head forces a /clear, except during a
        // clear cooldown (never clear into an in-flight clear). A non-clean-session
        // head never forces a clear here (the opt-in/accretion path owns that).
        assert!(clean_session_head_forces_context_reset(true, false));
        assert!(!clean_session_head_forces_context_reset(true, true));
        assert!(!clean_session_head_forces_context_reset(false, false));
        assert!(!clean_session_head_forces_context_reset(false, true));
    }

    #[test]
    fn supervisor_recycle_action_policy() {
        use SupervisorRecycleAction::*;
        // `#ctlrecycle` R3 / `#suprecyclequeue` policy truth table.
        // (stale, auto_recycle, turn_boundary, head_pending, explicit_admin,
        //  write_wedged, reexec_failed)
        // Fresh binary → never act.
        assert_eq!(
            supervisor_recycle_action(false, true, true, true, false, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(false, true, true, false, false, false, false, false),
            None
        );
        // Stale but mid-turn (not at a boundary) → never act, even with the flag on.
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, false, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(true, true, false, false, false, false, false, false),
            None
        );
        // Stale at a turn boundary, auto-recycle OFF → surface only, regardless of
        // whether a queue head is pending.
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, false, false, false),
            Detect
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, true, false, false, false, false),
            Detect
        );
        // Stale + boundary + opt-in ON, a queue head waiting → recycle NOW so the next
        // queue item runs on the fresh binary (debounce bypassed).
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, false),
            RecycleImmediate
        );
        // Stale + boundary + opt-in ON, no head waiting → debounced recycle.
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, false, false, false, false),
            RecycleDebounced
        );
    }

    #[test]
    fn supervisor_recycle_action_defers_while_agent_doc_cycle_open() {
        use SupervisorRecycleAction::*;
        // `#midturn-recycle-resume`: even when the harness reports a turn boundary,
        // a HEALTHY open agent-doc cycle (preflight taken, finalize not committed, or
        // an IPC ack connection in flight) must DEFER the `execve` recycle. Firing it
        // mid-cycle severs the in-flight IPC listener and drives the next finalize
        // into `live_prompt_drift_after_preflight` against the pre-recycle preflight
        // baseline (the root of the `content_ours` wedge).
        // (args: stale, auto_recycle, turn_boundary, head_pending, explicit_admin,
        //  write_wedged, reexec_failed, cycle_open)

        // Stale + auto + head pending, no admin/wedge/reexec proof the binary is bad —
        // would be RecycleImmediate; a healthy cycle still defers (closes sub-second).
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, true),
            DeferCycleOpen
        );
        // Stale + auto + no head — would be RecycleDebounced; healthy cycle defers.
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, false, false, false, true),
            DeferCycleOpen
        );
        // `#wd40` explicit-admin flush on a FRESH binary — the binary CAN commit, so
        // the open cycle still defers (no process flush mid-finalize). Only a STALE
        // binary makes the cycle un-committable, so a fresh-binary admin flush is not
        // a deadlock.
        assert_eq!(
            supervisor_recycle_action(false, false, true, false, true, false, false, true),
            DeferCycleOpen
        );

        // `#recycledeadlock`: when the binary is STALE *and* a fact proves it cannot
        // commit the open cycle (explicit admin recycle, a write wedge, or a failed
        // re-exec), the defer would be an INFINITE wait — the finalize keeps failing
        // the IPC proof on the same bad binary, so the cycle never closes. Break the
        // deadlock: fall through to the stale recycle arms (the caller force-abandons
        // the wedged cycle + the fresh supervisor re-dispatches its head).
        // Explicit admin recycle on a stale binary, cycle open → recycle now.
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, false, false, true),
            RecycleImmediate
        );
        // Write-wedge on a stale binary, cycle open → recycle now.
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, true, false, true),
            RecycleImmediate
        );
        // Reexec-failure on a stale binary, cycle open → escalate kill+relaunch (a
        // doomed re-exec cannot converge; the open cycle is reclaimed deterministically).
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, true, true),
            EscalateKillRelaunch
        );

        // CONVERSE: with the SAME inputs but cycle_open=false, the normal matrix is
        // restored — once the cycle commits and IPC drains the deferred recycle fires.
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, true, false),
            EscalateKillRelaunch
        );

        // A still-running turn (no boundary) plus an open cycle stays None — the
        // harness-boundary gate runs first and already defers; cycle_open does not
        // change that.
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, false, false, false, true),
            None
        );
    }

    #[test]
    fn cycle_open_defer_escalates_after_threshold_only() {
        // `#midturn-recycle-resume` Phase B: the streak escalates only once it REACHES
        // the bound, so a healthy cycle (a couple of ticks) never forces a recycle.
        assert!(!cycle_open_defer_escalates(0, MAX_CYCLE_OPEN_DEFER_TICKS));
        assert!(!cycle_open_defer_escalates(1, MAX_CYCLE_OPEN_DEFER_TICKS));
        assert!(!cycle_open_defer_escalates(
            MAX_CYCLE_OPEN_DEFER_TICKS - 1,
            MAX_CYCLE_OPEN_DEFER_TICKS
        ));
        // At and past the bound: escalate (force the recycle over a never-closing cycle).
        assert!(cycle_open_defer_escalates(
            MAX_CYCLE_OPEN_DEFER_TICKS,
            MAX_CYCLE_OPEN_DEFER_TICKS
        ));
        assert!(cycle_open_defer_escalates(
            MAX_CYCLE_OPEN_DEFER_TICKS + 5,
            MAX_CYCLE_OPEN_DEFER_TICKS
        ));
        // Small explicit bound for clarity.
        assert!(!cycle_open_defer_escalates(2, 3));
        assert!(cycle_open_defer_escalates(3, 3));
    }

    #[test]
    fn boot_resume_action_redispatches_only_when_cycle_open_and_child_died() {
        use BootResumeAction::*;
        // `#midturn-recycle-resume` Phase B truth table.
        // (args: is_recycle_boot, cycle_open, child_survived, already_consumed)

        // Not a recycle boot → never resume, regardless of cycle/child state.
        assert_eq!(boot_resume_action(false, true, false, false), None);
        assert_eq!(boot_resume_action(false, true, true, false), None);

        // Recycle boot, cycle COMMITTED/closed (cycle_open=false) → nothing to resume.
        assert_eq!(boot_resume_action(true, false, false, false), None);
        assert_eq!(boot_resume_action(true, false, true, false), None);

        // Recycle boot, cycle OPEN, child SURVIVED → adopt without re-trigger
        // (idempotency: the surviving child is still running the turn).
        assert_eq!(
            boot_resume_action(true, true, true, false),
            AdoptSurvivingChild
        );
        // The surviving-child adopt wins even if a prior consume latched — a live
        // child must never be double-run by re-dispatch.
        assert_eq!(
            boot_resume_action(true, true, true, true),
            AdoptSurvivingChild
        );

        // Recycle boot, cycle OPEN, child DIED, NOT yet consumed → re-dispatch the
        // genuinely-interrupted turn. The ONLY arm that re-dispatches.
        assert_eq!(
            boot_resume_action(true, true, false, false),
            RedispatchInterruptedTurn
        );

        // Recycle boot, cycle OPEN, child DIED, but ALREADY consumed by a prior boot
        // → do not re-dispatch again (idempotency against a second boot).
        assert_eq!(boot_resume_action(true, true, false, true), None);
    }

    #[test]
    fn supervisor_recycle_action_clearing_cycle_open_restores_recycle_for_escalation() {
        use SupervisorRecycleAction::*;
        // `#midturn-recycle-resume` Phase B: when the idle-watch escalation fires it
        // recomputes the recycle action with `effective_cycle_open=false`, so the
        // recycle that the open cycle had been deferring fires. Prove the converse the
        // escalation relies on: the SAME stale+auto+head inputs that DeferCycleOpen
        // with cycle_open=true return RecycleImmediate with cycle_open=false.
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, true),
            DeferCycleOpen
        );
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false, false),
            RecycleImmediate
        );
    }

    #[test]
    fn supervisor_recycle_action_explicit_admin_overrides_opt_out() {
        use SupervisorRecycleAction::*;
        // `#supselfheal` Phase 1: an explicit `admin recycle` overrides the
        // explicit auto-recycle opt-out and recycles a stale supervisor immediately at the
        // next turn boundary — the gentle, non-disruptive supervisor-recycle path.
        // Auto-recycle OFF + explicit admin → RecycleImmediate (overrides Detect),
        // regardless of whether a head is pending.
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, false, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, true, true, false, false, false),
            RecycleImmediate
        );
        // Auto-recycle ON + explicit admin → still immediate (no debounce wait).
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, true, false, false, false),
            RecycleImmediate
        );
        // Explicit admin NEVER drops a live turn: mid-turn (no boundary) stays None.
        assert_eq!(
            supervisor_recycle_action(true, false, false, false, true, false, false, false),
            None
        );
        // `#wd40` state-flush: explicit admin on a FRESH binary now recycles the
        // process to flush stale in-memory state (e.g. a lagging CRDT projection),
        // rather than no-opping and leaving the operator unable to clear it.
        assert_eq!(
            supervisor_recycle_action(false, false, true, false, true, false, false, false),
            RecycleImmediate
        );
        // ...but still never mid-turn (no boundary) on a fresh binary either.
        assert_eq!(
            supervisor_recycle_action(false, false, false, false, true, false, false, false),
            None
        );
        // A fresh binary WITHOUT explicit admin stays None (no spurious flush).
        assert_eq!(
            supervisor_recycle_action(false, true, true, true, false, false, false, false),
            None
        );
    }

    #[test]
    fn supervisor_recycle_action_write_wedge_overrides_opt_out() {
        use SupervisorRecycleAction::*;
        // `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): a wedged editor-IPC
        // write against a nominally-active listener is the clearest proof the live
        // binary is bad. Like explicit admin, it overrides an explicit auto-recycle opt-out
        // and recycles immediately at the boundary — a wedge must never stay Detect.
        // Auto-recycle OFF + write_wedged → RecycleImmediate (overrides Detect).
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, true, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, true, false, true, false, false),
            RecycleImmediate
        );
        // Auto-recycle ON + write_wedged → immediate (no debounce wait, even with no
        // head pending — the wedge does not wait for an idle boundary that may never
        // come).
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, false, true, false, false),
            RecycleImmediate
        );
        // A wedge NEVER drops a live turn: mid-turn (no boundary) stays None.
        assert_eq!(
            supervisor_recycle_action(true, false, false, false, false, true, false, false),
            None
        );
        // A wedge on a FRESH binary → nothing to recycle (the wedge is not a stale
        // binary; some other transient cause owns the converge retry).
        assert_eq!(
            supervisor_recycle_action(false, false, true, false, false, true, false, false),
            None
        );
    }

    #[test]
    fn supervisor_recycle_action_reexec_failure_escalates_to_kill_relaunch() {
        use SupervisorRecycleAction::*;
        // `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): once an in-place
        // re-exec has failed, recycling onto the same doomed `execve` is pointless —
        // escalate to a bounded kill+relaunch regardless of the opt-in/admin/wedge
        // inputs.
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, true, false),
            EscalateKillRelaunch
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, false, true, false),
            EscalateKillRelaunch
        );
        // Escalation wins over the wedge/admin RecycleImmediate arms (re-trying the
        // failed execve would otherwise be chosen).
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, true, true, false),
            EscalateKillRelaunch
        );
        // Escalation still respects turn_boundary — a failed re-exec mid-turn must
        // not drop the live turn.
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, false, false, true, false),
            None
        );
        // A failed re-exec on a FRESH binary → nothing to escalate.
        assert_eq!(
            supervisor_recycle_action(false, true, true, true, false, false, true, false),
            None
        );
    }

    #[test]
    fn reexec_escalation_bound_caps_retries() {
        // `#supselfheal` Phase 3: the bounded escalation may fire only while fewer
        // than MAX_REEXEC_ESCALATIONS kill+relaunches have already been attempted.
        assert!(reexec_escalation_within_bound(0, MAX_REEXEC_ESCALATIONS));
        assert!(reexec_escalation_within_bound(
            MAX_REEXEC_ESCALATIONS - 1,
            MAX_REEXEC_ESCALATIONS
        ));
        assert!(!reexec_escalation_within_bound(
            MAX_REEXEC_ESCALATIONS,
            MAX_REEXEC_ESCALATIONS
        ));
        assert!(!reexec_escalation_within_bound(
            MAX_REEXEC_ESCALATIONS + 1,
            MAX_REEXEC_ESCALATIONS
        ));
    }

    #[test]
    fn stale_drain_recycle_yield_policy() {
        // `#wd40` truth table. (would_recycle_at_boundary, drain_owner_active, turn_boundary)
        // Not stale / no recycle would fire → never request a yield (nothing to
        // gain; would only stall the drain).
        assert!(!stale_drain_recycle_yield_requested(false, true, false));
        // No self-driving loop owns the drain → the supervisor reaches the boundary
        // on its own; nothing to signal.
        assert!(!stale_drain_recycle_yield_requested(true, false, false));
        // The boundary is already reachable → the recycle fires directly, no yield.
        assert!(!stale_drain_recycle_yield_requested(true, true, true));
        // Stale + a loop owns the drain + boundary unreachable (turn_active) → this
        // is the exact wedge `#wd40` fixes: request the yield.
        assert!(stale_drain_recycle_yield_requested(true, true, false));
    }

    #[test]
    fn wd40_fresh_binary_explicit_admin_flush_yields_during_active_drain() {
        // `#wd40` state-flush end-to-end: the installed binary already matches
        // (`stale = false`, the live `supervisor:fresh` + `projection_lag=true`
        // condition), an operator ran `admin recycle` to flush the lagging CRDT
        // projection, and a self-driving loop holds the drain `turn_active`.
        // 1) A recycle WOULD fire at a boundary now (explicit admin forces it even on
        //    a fresh binary — no longer a silent no-op).
        let would_recycle_at_boundary = !matches!(
            supervisor_recycle_action(
                /* stale */ false, /* auto_recycle */ true,
                /* turn_boundary */ true, /* head_pending */ false,
                /* explicit_admin */ true, /* write_wedged */ false,
                /* reexec_failed */ false, /* cycle_open */ false,
            ),
            SupervisorRecycleAction::None | SupervisorRecycleAction::Detect
        );
        assert!(
            would_recycle_at_boundary,
            "explicit admin must force a recycle at the boundary even on a fresh binary"
        );
        // 2) So the idle-watch requests the loop yield one boundary, letting the
        //    process restart fire and flush the stale projection.
        assert!(stale_drain_recycle_yield_requested(
            would_recycle_at_boundary,
            /* drain_owner_active */ true,
            /* turn_boundary */ false,
        ));
        // Without the explicit admin, a fresh binary mid-drain never yields (no
        // spurious flush / churn).
        let no_admin_recycle = !matches!(
            supervisor_recycle_action(false, true, true, false, false, false, false, false),
            SupervisorRecycleAction::None | SupervisorRecycleAction::Detect
        );
        assert!(!no_admin_recycle);
        assert!(!stale_drain_recycle_yield_requested(
            no_admin_recycle,
            true,
            false
        ));
    }

    #[test]
    fn supervisor_install_action_policy() {
        use SupervisorInstallAction::*;
        // `#supautoinstall` policy truth table. (source_newer, auto_install, turn_boundary)
        // Source not newer than the installed binary → never act.
        assert_eq!(supervisor_install_action(false, true, true), None);
        assert_eq!(supervisor_install_action(false, false, true), None);
        // Source newer but mid-turn (not at a boundary) → never act, even opted in.
        assert_eq!(supervisor_install_action(true, true, false), None);
        // Source newer at a turn boundary, auto-install opted out → surface only.
        assert_eq!(supervisor_install_action(true, false, true), Detect);
        // Source newer + boundary + opt-in ON → request the (caller-debounced) install.
        assert_eq!(supervisor_install_action(true, true, true), Install);
    }

    #[test]
    fn supervisor_restart_action_drain_and_supersede_policy() {
        use SupervisorRestartAction::*;
        // `#supkill-bg` drain-and-supersede truth table.
        // (restart_requested, reexec_intent, turn_boundary)
        // No request → never act, regardless of staleness or boundary.
        assert_eq!(supervisor_restart_action(false, true, true), None);
        assert_eq!(supervisor_restart_action(false, false, false), None);
        // Part 1 — a pending restart mid-turn DRAINS: wait for the boundary instead
        // of tearing down the live turn, whether or not the binary is stale.
        assert_eq!(supervisor_restart_action(true, true, false), AwaitDrain);
        assert_eq!(supervisor_restart_action(true, false, false), AwaitDrain);
        // Part 2 — at the boundary, a stale binary hot-reloads in place via execve
        // (the default healthy restart; the live child + pane are preserved).
        assert_eq!(supervisor_restart_action(true, true, true), ReexecInPlace);
        // At the boundary with a fresh binary, the normal child relaunch serves it.
        assert_eq!(supervisor_restart_action(true, false, true), RelaunchChild);
    }

    // #jb-run-agent-doc-busy-queue-dispatch-deadlock: the supervisor idle-queue
    // watch must drain a live active-queue head on the busy→idle transition,
    // never inject mid-turn, and never hot-loop on a stuck head.
    #[test]
    fn idle_queue_drain_dispatches_when_idle_with_fresh_active_head() {
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::Dispatch
        );
    }

    #[test]
    fn idle_queue_drain_skips_when_pane_busy_even_with_active_head() {
        // No-inject-into-active-turn: a busy pane (no dispatch-ready prompt)
        // never receives an injected trigger, mirroring the route busy path.
        assert_eq!(
            idle_queue_drain_decision(false, false, false, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipNotIdle
        );
    }

    #[test]
    fn idle_queue_drain_waits_for_turn_status_idle_even_with_visible_prompt() {
        assert_eq!(
            idle_queue_drain_decision(false, true, true, false, false, Some("/clear"), None),
            IdleQueueDrainDecision::SkipTurnActive
        );
    }

    #[test]
    fn idle_queue_drain_skips_during_clear_cooldown() {
        assert_eq!(
            idle_queue_drain_decision(true, true, true, false, false, Some("do [#a]"), None),
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
    fn drain_settle_gate_blocks_until_pane_idles_after_clear() {
        const THRESHOLD: u32 = 4;
        // Not awaiting a clear → never blocked regardless of ticks.
        assert!(!drain_blocked_awaiting_clear_settle(
            false, true, false, 0, THRESHOLD
        ));
        // Awaiting + pane not at a fresh idle prompt → blocked regardless of count.
        assert!(drain_blocked_awaiting_clear_settle(
            true, false, false, 99, THRESHOLD
        ));
        // Awaiting + turn still active → blocked.
        assert!(drain_blocked_awaiting_clear_settle(
            true, true, true, 99, THRESHOLD
        ));
        // Awaiting + idle but not enough consecutive ticks → still blocked.
        assert!(drain_blocked_awaiting_clear_settle(
            true, true, false, 3, THRESHOLD
        ));
        // Awaiting + idle + threshold reached → no longer blocked.
        assert!(!drain_blocked_awaiting_clear_settle(
            true, true, false, 4, THRESHOLD
        ));
    }

    #[test]
    fn context_clear_marker_resubmits_visible_pending_clear_once() {
        use IdleQueueContextClearInFlightDecision::*;
        const THRESHOLD: u32 = 4;

        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                marker_active: true,
                prompt_visible: true,
                turn_active: false,
                route_submit_in_flight: false,
                clear_already_pending: Some(true),
                already_resubmitted: false,
                settled_idle_ticks: 0,
                settle_threshold: THRESHOLD,
            }),
            ResubmitPendingClear
        );
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                marker_active: true,
                prompt_visible: true,
                turn_active: false,
                route_submit_in_flight: false,
                clear_already_pending: Some(true),
                already_resubmitted: true,
                settled_idle_ticks: 0,
                settle_threshold: THRESHOLD,
            }),
            WaitForPendingClear
        );
    }

    #[test]
    fn context_clear_marker_blocks_until_settled_idle_prompt() {
        use IdleQueueContextClearInFlightDecision::*;
        const THRESHOLD: u32 = 4;

        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                marker_active: false,
                prompt_visible: true,
                turn_active: false,
                route_submit_in_flight: false,
                clear_already_pending: Some(false),
                already_resubmitted: false,
                settled_idle_ticks: 99,
                settle_threshold: THRESHOLD,
            }),
            Ignore
        );
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                marker_active: true,
                prompt_visible: false,
                turn_active: false,
                route_submit_in_flight: false,
                clear_already_pending: Some(false),
                already_resubmitted: false,
                settled_idle_ticks: 99,
                settle_threshold: THRESHOLD,
            }),
            WaitForIdle
        );
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                marker_active: true,
                prompt_visible: true,
                turn_active: false,
                route_submit_in_flight: true,
                clear_already_pending: Some(false),
                already_resubmitted: false,
                settled_idle_ticks: 99,
                settle_threshold: THRESHOLD,
            }),
            WaitForIdle
        );
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                marker_active: true,
                prompt_visible: true,
                turn_active: false,
                route_submit_in_flight: false,
                clear_already_pending: Some(false),
                already_resubmitted: false,
                settled_idle_ticks: THRESHOLD - 1,
                settle_threshold: THRESHOLD,
            }),
            AwaitSettle
        );
        assert_eq!(
            idle_queue_context_clear_in_flight_decision(IdleQueueContextClearInFlightFacts {
                marker_active: true,
                prompt_visible: true,
                turn_active: false,
                route_submit_in_flight: false,
                clear_already_pending: Some(false),
                already_resubmitted: false,
                settled_idle_ticks: THRESHOLD,
                settle_threshold: THRESHOLD,
            }),
            Settled
        );
    }

    #[test]
    fn drain_settle_gate_tick_sequence_releases_after_consecutive_idle() {
        const THRESHOLD: u32 = 4;
        // (prompt_visible, turn_active) after the watch sent its own `/clear`.
        let ticks = [
            (false, false), // pane still processing the clear (no prompt)
            (true, true),   // prompt back but turn still active
            (true, false),  // idle tick 1
            (true, false),  // idle tick 2
            (true, false),  // idle tick 3
            (true, false),  // idle tick 4 → released here, dispatch allowed
        ];
        let mut awaiting = true;
        let mut idle_ticks: u32 = 0;
        let mut released_at: Option<usize> = None;
        for (i, (visible, turn)) in ticks.iter().enumerate() {
            if awaiting && *visible && !*turn {
                idle_ticks = idle_ticks.saturating_add(1);
            } else {
                idle_ticks = 0;
            }
            if awaiting && idle_ticks >= THRESHOLD {
                awaiting = false;
                released_at = Some(i);
            }
            // While still awaiting, the drain must stay blocked.
            if awaiting {
                assert!(drain_blocked_awaiting_clear_settle(
                    true, *visible, *turn, idle_ticks, THRESHOLD
                ));
            }
        }
        // Indices 0,1 reset the counter; 2..=5 accumulate; the 4th idle tick is
        // index 5, so the gate releases there — never injecting into the clear.
        assert_eq!(released_at, Some(5));
    }

    #[test]
    fn drain_dedup_skips_only_on_proven_pending_payload() {
        // Proven pending → skip the re-send (no stacked duplicate trigger).
        assert!(drain_dispatch_dedup_skip(Some(true)));
        // Proven absent → dispatch normally.
        assert!(!drain_dispatch_dedup_skip(Some(false)));
        // Unreadable pane capture → never suppress a legitimate dispatch.
        assert!(!drain_dispatch_dedup_skip(None));
    }

    #[test]
    fn between_turn_enqueue_plan_keeps_one_clear_and_one_trigger() {
        let plan = between_turn_enqueue_plan(
            [
                "/clear",
                "agent-doc tasks/agent-doc/agent-doc-bugs2.md",
                "/clear",
                "agent-doc tasks/agent-doc/agent-doc-bugs2.md",
            ],
            "/clear",
            "agent-doc tasks/agent-doc/agent-doc-bugs2.md",
        );

        assert_eq!(
            plan.kept,
            vec![
                BetweenTurnCommandKind::Clear,
                BetweenTurnCommandKind::AgentDoc
            ]
        );
        assert_eq!(plan.kept_labels(), "/clear,/agent-doc");
        assert_eq!(plan.deduped, 2);
    }

    #[test]
    fn between_turn_enqueue_plan_counts_concatenated_trigger_duplicate() {
        let plan = between_turn_enqueue_plan(
            [
                "/clear /agent-doc tasks/agent-doc/agent-doc-bugs2.md/agent-doc tasks/agent-doc/agent-doc-bugs2.md",
            ],
            "/clear",
            "/agent-doc tasks/agent-doc/agent-doc-bugs2.md",
        );

        assert_eq!(
            plan.kept,
            vec![
                BetweenTurnCommandKind::Clear,
                BetweenTurnCommandKind::AgentDoc
            ]
        );
        assert_eq!(plan.deduped, 1);
    }

    #[test]
    fn idle_queue_context_reset_dispatches_clear_once_per_head() {
        assert_eq!(
            idle_queue_context_reset_decision(true, false, false, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::Reset
        );
        assert_eq!(
            idle_queue_context_reset_decision(
                true,
                false,
                false,
                Some("do [#a]"),
                Some("do [#a]"),
                true
            ),
            IdleQueueContextResetDecision::SkipAlreadyResetHead
        );
        assert_eq!(
            idle_queue_context_reset_decision(
                true,
                false,
                false,
                Some("do [#b]"),
                Some("do [#a]"),
                true
            ),
            IdleQueueContextResetDecision::Reset
        );
    }

    #[test]
    fn idle_queue_context_reset_waits_for_idle_and_active_head() {
        assert_eq!(
            idle_queue_context_reset_decision(false, false, false, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::SkipNotIdle
        );
        assert_eq!(
            idle_queue_context_reset_decision(true, false, false, None, None, true),
            IdleQueueContextResetDecision::SkipNoActiveHead
        );
        assert_eq!(
            idle_queue_context_reset_decision(true, false, false, Some("do [#a]"), None, false),
            IdleQueueContextResetDecision::SkipNoResetNeeded
        );
    }

    #[test]
    fn idle_queue_context_reset_waits_for_turn_status_idle() {
        assert_eq!(
            idle_queue_context_reset_decision(true, true, false, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::SkipTurnActive
        );
    }

    #[test]
    fn idle_queue_context_reset_waits_for_route_submit_to_finish() {
        assert_eq!(
            idle_queue_context_reset_decision(true, false, true, Some("do [#a]"), None, true),
            IdleQueueContextResetDecision::SkipRouteSubmitInFlight
        );
    }

    #[test]
    fn idle_queue_context_reset_waits_for_editor_typing() {
        assert_eq!(
            idle_queue_context_reset_decision_with_editor_typing(
                true,
                false,
                false,
                true,
                Some("operator is still typing"),
                None,
                true,
            ),
            IdleQueueContextResetDecision::SkipEditorTyping
        );
    }

    #[test]
    fn idle_queue_drain_skips_when_no_active_head() {
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, false, None, None),
            IdleQueueDrainDecision::SkipNoActiveHead
        );
        assert_eq!(
            idle_queue_drain_decision(false, false, true, false, false, None, Some("do [#a]")),
            IdleQueueDrainDecision::SkipNoActiveHead
        );
    }

    #[test]
    fn idle_queue_drain_dedups_already_dispatched_head() {
        // The head we already fired for is still present (cycle has not consumed
        // it yet, or the dispatch failed to drain) — suppress re-firing so a
        // stuck head cannot spin the watch every idle tick.
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                false,
                false,
                Some("do [#a]"),
                Some("do [#a]")
            ),
            IdleQueueDrainDecision::SkipAlreadyDispatched
        );
    }

    #[test]
    fn idle_queue_drain_fires_again_when_head_advances() {
        // A different head than the last dispatched one re-fires — the queue
        // advanced to a new prompt that still needs an idle drain.
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                false,
                false,
                Some("do [#b]"),
                Some("do [#a]")
            ),
            IdleQueueDrainDecision::Dispatch
        );
    }

    #[test]
    fn idle_queue_drain_waits_for_route_submit_to_finish() {
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, true, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipRouteSubmitInFlight
        );
    }

    #[test]
    fn idle_queue_drain_waits_for_editor_typing() {
        assert_eq!(
            idle_queue_drain_decision_with_editor_typing(IdleQueueDrainDecisionFacts {
                clear_cooldown_active: false,
                prompt_visible: true,
                turn_active: false,
                self_driving_loop_active: false,
                route_submit_in_flight: false,
                editor_typing_active: true,
                active_head: Some("operator is still typing"),
                last_dispatched: None,
            }),
            IdleQueueDrainDecision::SkipEditorTyping
        );
    }

    #[test]
    fn idle_queue_drain_defers_to_self_driving_loop_owner() {
        // A fresh drain-owner lease (Claude Code `/loop`) owns the drain: the
        // supervisor defers even on an idle pane with a fresh, un-dispatched head
        // so the two owners do not flood the live input queue (#kp5z / #qflood).
        assert_eq!(
            idle_queue_drain_decision(false, true, false, true, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipSelfDrivingLoopOwner
        );
        // The loop-owner gate also wins over the already-dispatched dedup arm.
        assert_eq!(
            idle_queue_drain_decision(
                false,
                true,
                false,
                true,
                false,
                Some("do [#a]"),
                Some("do [#a]")
            ),
            IdleQueueDrainDecision::SkipSelfDrivingLoopOwner
        );
        // No/stale lease (`self_driving_loop_active=false`) ⇒ supervisor drains.
        assert_eq!(
            idle_queue_drain_decision(false, true, false, false, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::Dispatch
        );
        // A busy pane still short-circuits before the loop-owner check.
        assert_eq!(
            idle_queue_drain_decision(false, false, false, true, false, Some("do [#a]"), None),
            IdleQueueDrainDecision::SkipNotIdle
        );
        // No active head still wins (nothing to defer about).
        assert_eq!(
            idle_queue_drain_decision(false, true, false, true, false, None, None),
            IdleQueueDrainDecision::SkipNoActiveHead
        );
    }
}
