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

/// Consecutive idle-prompt polls a cleared pane must show a fresh harness prompt
/// before a lingering manual clear cooldown auto-expires and the active go-mode
/// queue drain resumes (`#clearcontresume`). At `AUTO_TRIGGER_POLL_INTERVAL`
/// (500ms) this is ~2s of proven idle pane after the `/clear`, so a resumed drain
/// never injects into an in-flight clear.
pub const CLEAR_COOLDOWN_RESUME_IDLE_TICKS: u32 = 4;

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
pub enum IdleQueueDrainDecision {
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
    /// A live editor reports recent typing for this document; the supervisor
    /// must not read/submit a half-edited queue head.
    SkipEditorTyping,
    /// This exact head was already dispatched and has not advanced/drained yet —
    /// suppress re-firing so a stuck head cannot spin the watch into a hot loop.
    SkipAlreadyDispatched,
    /// A manual/editor route command is already submitting into the owned pane.
    /// Do not add a supervisor reset or drain command until that submit finishes.
    SkipRouteSubmitInFlight,
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
    SkipEditorTyping,
    SkipRouteSubmitInFlight,
    SkipAlreadyResetHead,
    SkipNoResetNeeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdleQueueContextClearInFlightDecision {
    Ignore,
    WaitForIdle,
    ResubmitPendingClear,
    WaitForPendingClear,
    AwaitSettle,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdleQueueContextClearInFlightFacts {
    pub marker_active: bool,
    pub prompt_visible: bool,
    pub turn_active: bool,
    pub route_submit_in_flight: bool,
    pub clear_already_pending: Option<bool>,
    pub already_resubmitted: bool,
    pub settled_idle_ticks: u32,
    pub settle_threshold: u32,
}

/// `#cleandrainsup`: whether the supervisor idle-watch must force a context
/// `/clear` before dispatching the active queue head because it is a
/// `[clean-session]` item. A `[clean-session]` tag is an explicit per-item request
/// for a fresh agent context, so it warrants a `/clear` independent of the global
/// `agent_doc_queue_context_reset` opt-in (which only governs accretion-driven
/// pre-emptive clears). It is still suppressed during a clear cooldown so it never
/// clears into an in-flight `/clear`. Pure so the policy is unit-testable without a
/// live pane.
pub(crate) fn clean_session_head_forces_context_reset(
    active_head_is_clean_session: bool,
    clear_cooldown_active: bool,
) -> bool {
    active_head_is_clean_session && !clear_cooldown_active
}

pub(crate) fn idle_queue_context_reset_decision(
    prompt_visible: bool,
    turn_active: bool,
    route_submit_in_flight: bool,
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
    if route_submit_in_flight {
        return IdleQueueContextResetDecision::SkipRouteSubmitInFlight;
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

pub(crate) fn idle_queue_context_reset_decision_with_editor_typing(
    prompt_visible: bool,
    turn_active: bool,
    route_submit_in_flight: bool,
    editor_typing_active: bool,
    active_head: Option<&str>,
    last_context_reset_head: Option<&str>,
    reset_required: bool,
) -> IdleQueueContextResetDecision {
    if active_head.is_some() && editor_typing_active {
        return IdleQueueContextResetDecision::SkipEditorTyping;
    }
    idle_queue_context_reset_decision(
        prompt_visible,
        turn_active,
        route_submit_in_flight,
        active_head,
        last_context_reset_head,
        reset_required,
    )
}

/// Pure idle-queue drain decision. Kept side-effect free so the busy→idle drain
/// state machine is deterministically testable without a live pane.
///
/// `prompt_visible` is the supervisor's idle signal (a dispatch-ready harness
/// prompt is on screen). `active_head` is the live `queue_active: true` ready
/// head (`queue_continuation::live_continuation_head`), and `last_dispatched`
/// is the head this watch last injected a trigger for.
pub fn idle_queue_drain_decision(
    clear_cooldown_active: bool,
    prompt_visible: bool,
    turn_active: bool,
    self_driving_loop_active: bool,
    route_submit_in_flight: bool,
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
        // A route command already owns this pane input window. Reset/drain would
        // otherwise concatenate after its `agent-doc <FILE>` reopen.
        Some(_) if route_submit_in_flight => IdleQueueDrainDecision::SkipRouteSubmitInFlight,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleQueueDrainDecisionFacts<'a> {
    pub clear_cooldown_active: bool,
    pub prompt_visible: bool,
    pub turn_active: bool,
    pub self_driving_loop_active: bool,
    pub route_submit_in_flight: bool,
    pub editor_typing_active: bool,
    pub active_head: Option<&'a str>,
    pub last_dispatched: Option<&'a str>,
}

pub fn idle_queue_drain_decision_with_editor_typing(
    facts: IdleQueueDrainDecisionFacts<'_>,
) -> IdleQueueDrainDecision {
    if !facts.clear_cooldown_active && facts.active_head.is_some() && facts.editor_typing_active {
        return IdleQueueDrainDecision::SkipEditorTyping;
    }
    idle_queue_drain_decision(
        facts.clear_cooldown_active,
        facts.prompt_visible,
        facts.turn_active,
        facts.self_driving_loop_active,
        facts.route_submit_in_flight,
        facts.active_head,
        facts.last_dispatched,
    )
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
pub fn clear_cooldown_resume_ready(
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

/// `#qflood2` — after the idle-queue watch sends a `/clear` (an opt-in context
/// reset, or a `/clear` queue head), the next drain trigger must NOT be injected
/// until the cleared pane has shown a fresh idle harness prompt for
/// `settle_threshold` consecutive polls. Otherwise the trigger lands in the
/// composer alongside the still-in-flight `/clear` and the harness sees one
/// concatenated line (the operator-observed `/clear /agent-doc <FILE>`). This is
/// the same debounce idea as `clear_cooldown_resume_ready`, but for the
/// supervisor's OWN clears (which never write the manual cooldown marker), so it
/// is tracked by idle-watch memory and mirrored to a short-lived marker that
/// survives supervisor recycle/restart. Pure so the settle gate is unit-testable
/// without a live pane.
pub fn drain_blocked_awaiting_clear_settle(
    awaiting_clear_settle: bool,
    prompt_visible: bool,
    turn_active: bool,
    settled_idle_ticks: u32,
    settle_threshold: u32,
) -> bool {
    if !awaiting_clear_settle {
        return false;
    }
    // A pane that is not at a fresh idle prompt has definitely not settled, so
    // keep blocking regardless of the tick count.
    if !prompt_visible || turn_active {
        return true;
    }
    settled_idle_ticks < settle_threshold
}

/// Decide how a restarted idle-queue watcher should honor a persisted
/// supervisor-owned context clear marker. A visible pending clear is recovered
/// by one bare submit key, then the watcher waits for the normal settle debounce.
pub(crate) fn idle_queue_context_clear_in_flight_decision(
    facts: IdleQueueContextClearInFlightFacts,
) -> IdleQueueContextClearInFlightDecision {
    if !facts.marker_active {
        return IdleQueueContextClearInFlightDecision::Ignore;
    }
    if !facts.prompt_visible || facts.turn_active || facts.route_submit_in_flight {
        return IdleQueueContextClearInFlightDecision::WaitForIdle;
    }
    if drain_dispatch_dedup_skip(facts.clear_already_pending) {
        return if facts.already_resubmitted {
            IdleQueueContextClearInFlightDecision::WaitForPendingClear
        } else {
            IdleQueueContextClearInFlightDecision::ResubmitPendingClear
        };
    }
    if facts.settled_idle_ticks >= facts.settle_threshold {
        IdleQueueContextClearInFlightDecision::Settled
    } else {
        IdleQueueContextClearInFlightDecision::AwaitSettle
    }
}

/// `#qflood2` — whether the idle-queue watch should skip injecting the next
/// drain payload (trigger or `/clear`) because that exact text is already
/// pending/visible in the owned pane's composer. `payload_already_pending` is
/// the live pane-capture dedup signal: `Some(true)` = the payload is already in
/// the composer (re-sending would stack a duplicate, e.g.
/// `/agent-doc <FILE>/agent-doc <FILE>`); `Some(false)` = proven absent;
/// `None` = the pane could not be captured. A failed capture must never suppress
/// a legitimate dispatch, so only a proven positive match dedups.
pub fn drain_dispatch_dedup_skip(payload_already_pending: Option<bool>) -> bool {
    matches!(payload_already_pending, Some(true))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BetweenTurnCommandKind {
    Clear,
    AgentDoc,
}

impl BetweenTurnCommandKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "/clear",
            Self::AgentDoc => "/agent-doc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BetweenTurnEnqueuePlan {
    pub kept: Vec<BetweenTurnCommandKind>,
    pub deduped: usize,
}

impl BetweenTurnEnqueuePlan {
    pub fn kept_labels(&self) -> String {
        self.kept
            .iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn command_occurrences(text: &str, command: &str) -> usize {
    let command = command.trim();
    if command.is_empty() {
        return 0;
    }
    text.match_indices(command).count()
}

/// `#qdedup`: compose supervisor/PCP between-turn enqueues as a set, not an
/// append-only string. Repeated requests may arrive while the current turn is
/// still active; when the pane reaches the idle boundary, only one clear command
/// and one agent-doc trigger should remain, in that order. Counting raw
/// occurrences also recognizes the historical malformed line
/// `/clear /agent-doc <FILE>/agent-doc <FILE>` so the duplicate can be measured
/// and suppressed instead of appended again.
pub fn between_turn_enqueue_plan<'a>(
    requested: impl IntoIterator<Item = &'a str>,
    clear_command: &str,
    trigger_command: &str,
) -> BetweenTurnEnqueuePlan {
    let mut clear_count = 0usize;
    let mut trigger_count = 0usize;
    for item in requested {
        clear_count += command_occurrences(item, clear_command);
        trigger_count += command_occurrences(item, trigger_command);
    }
    let mut kept = Vec::with_capacity(2);
    if clear_count > 0 {
        kept.push(BetweenTurnCommandKind::Clear);
    }
    if trigger_count > 0 {
        kept.push(BetweenTurnCommandKind::AgentDoc);
    }
    BetweenTurnEnqueuePlan {
        kept,
        deduped: clear_count
            .saturating_add(trigger_count)
            .saturating_sub(clear_count.min(1).saturating_add(trigger_count.min(1))),
    }
}

/// `#ctlrecycle` R3 — what the idle supervisor watch should do about a binary that
/// no longer matches the installed agent-doc. Pure so the policy is unit-testable.
#[derive(Debug, PartialEq, Eq)]
pub enum SupervisorRecycleAction {
    /// Not stale, or not at a turn boundary — do nothing.
    None,
    /// Stale + at a turn boundary but auto-recycle was explicitly opted OUT
    /// (falsey env/frontmatter/project knob): surface it so the operator restarts
    /// deliberately. With the `#supselfheal` default-on, this only fires when an
    /// operator turned self-recycle off.
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
    /// `#supselfheal` Phase 3 (`#supselfheal-reexecescalate`): a stale supervisor
    /// whose in-place `execve` re-exec already failed (a deleted-inode `ENOENT`
    /// from a fresh `make install`, or another syscall error) cannot converge onto
    /// the fresh binary by recycling again — re-trying the same doomed `execve`
    /// just re-logs `continue_current_binary` forever. Escalate to one bounded
    /// (`MAX_REEXEC_ESCALATIONS`) kill+relaunch of the harness child so the wedge
    /// is reclaimed deterministically instead of sitting indefinitely.
    EscalateKillRelaunch,
}

/// `#supselfheal` Phase 3 — maximum number of times a stale supervisor whose
/// in-place `execve` re-exec keeps failing may escalate to a kill+relaunch before
/// the idle watch gives up and continues on the current binary (surfacing the
/// one-time operator-restart hint). Bounded so a relaunch that itself never clears
/// the staleness cannot spin the watch into an unbounded kill loop.
pub const MAX_REEXEC_ESCALATIONS: u32 = 2;

/// `#supselfheal` Phase 3 — whether a re-exec-failure escalation may still fire,
/// given how many kill+relaunch escalations have already been attempted this
/// supervisor lifetime. Pure so the bound is unit-testable without the live
/// kill/relaunch machinery; the caller increments the counter on each escalation.
pub fn reexec_escalation_within_bound(attempts: u32, max: u32) -> bool {
    attempts < max
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
/// supervisor. Explicit intent **overrides the default-OFF opt-out** — a stale
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
/// bad, so — like `explicit_admin` — it **overrides the default-OFF opt-out** and
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
pub fn supervisor_recycle_action(
    stale: bool,
    auto_recycle: bool,
    turn_boundary: bool,
    head_pending: bool,
    explicit_admin: bool,
    write_wedged: bool,
    reexec_failed: bool,
) -> SupervisorRecycleAction {
    // Never drop a live turn — every recycle arm below respects the boundary.
    if !turn_boundary {
        return SupervisorRecycleAction::None;
    }
    if stale {
        if reexec_failed {
            // Phase 3: the in-place re-exec already failed; recycling onto it again is
            // pointless. Escalate to a bounded kill+relaunch instead of looping
            // `continue_current_binary`. This wins over every recycle arm below.
            return SupervisorRecycleAction::EscalateKillRelaunch;
        }
        if explicit_admin || write_wedged {
            // Operator/agent explicit `admin recycle`, or a wedged editor-IPC write —
            // recycle now, overriding the default-OFF opt-out. The next turn boundary
            // is the deliberate restart point; a wedge must never stay `Detect`.
            return SupervisorRecycleAction::RecycleImmediate;
        }
        if !auto_recycle {
            return SupervisorRecycleAction::Detect;
        }
        return if head_pending {
            SupervisorRecycleAction::RecycleImmediate
        } else {
            SupervisorRecycleAction::RecycleDebounced
        };
    }
    // `#wd40` state-flush: the binary is current, so there is nothing to *swap*, but
    // an explicit operator `admin recycle` is a deliberate request to restart the
    // supervisor *process* to flush stale in-memory state — e.g. a lagging CRDT
    // projection (`projection_lag`) that keeps re-pinning a queue head (`#rt83`
    // phantom-pin churn) even though the installed binary matches. Honor it so
    // `admin recycle` is never a silent no-op on a fresh binary (the prior behavior
    // that left the operator unable to flush the lagging projection). A bare wedge on
    // a fresh binary still stays a no-op — some other transient cause owns the
    // converge retry — so only the operator's explicit intent forces a fresh flush.
    if explicit_admin {
        return SupervisorRecycleAction::RecycleImmediate;
    }
    SupervisorRecycleAction::None
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
    would_recycle_at_boundary && drain_owner_active && !turn_boundary
}

/// `#agentreloadrestart` — what the idle/turn-boundary supervisor watch should do
/// about a detected harness change: the harness resolved from current frontmatter
/// (`agent:`) differs from the one the running supervisor launched with. Gated
/// exactly like the `#supselfheal` auto-recycle ([`supervisor_recycle_action`]):
/// only act at a quiet dispatch-ready boundary, never mid-turn, and only when the
/// operator opted in (knob ON, the default). Pure so the boundary policy is
/// unit-testable without a live pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentChangeRestartAction {
    /// No harness change, the knob is off, or nothing to do.
    None,
    /// Harness changed + knob on + the pane is at a quiet dispatch-ready prompt —
    /// restart the agent with the freshly-resolved harness now.
    Restart,
    /// Harness changed + knob on, but the pane is not at a quiet boundary yet
    /// (no prompt visible / mid-turn) — wait for the next boundary; never
    /// interrupt an in-flight turn.
    WaitForBoundary,
}

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
    if !harness_changed || !knob_on {
        return AgentChangeRestartAction::None;
    }
    if !prompt_visible || turn_active {
        return AgentChangeRestartAction::WaitForBoundary;
    }
    AgentChangeRestartAction::Restart
}

/// `#agentreloadrestart` Phase 1b — does a re-resolved harness binary force a FRESH
/// spawn of the new harness? True exactly when the binary the running supervisor
/// launched with differs from the one re-resolved from CURRENT frontmatter. A
/// harness change must spawn the new harness fresh (and must NOT adopt the old
/// child preserved across a supervisor reexec). Pure so the restart loop's
/// re-resolution branch is unit-testable, and so the INERTNESS invariant — same
/// binary ⇒ no fresh-spawn swap — is asserted directly.
pub fn harness_change_forces_fresh_spawn(running_binary: &str, resolved_binary: &str) -> bool {
    running_binary != resolved_binary
}

/// `#supautoinstall` — what the idle supervisor watch should do about agent-doc's OWN
/// source being newer than the installed binary (the dogfood "a finalize just committed
/// a source edit but nobody built+installed it" state). Pure so the policy is
/// unit-testable. This is the install rung that PRECEDES [`supervisor_recycle_action`]:
/// once the auto-install lands the fresh binary, `process_binary_is_stale` flips true and
/// the existing recycle path hot-reloads onto it.
#[derive(Debug, PartialEq, Eq)]
pub enum SupervisorInstallAction {
    /// Source not newer than the installed binary, not at a turn boundary, or not an
    /// agent-doc dogfood session document — do nothing.
    None,
    /// Source is newer + at a turn boundary but auto-install is opted OUT:
    /// surface it once so the operator runs the manual dogfood refresh deliberately.
    Detect,
    /// Source is newer + auto-install opt-in + turn boundary: build+install at the next
    /// idle-grace boundary. The caller always debounces this (a build is heavy, so unlike
    /// the recycle rung there is no head-pending "immediate" fast path) — a momentary idle
    /// gap mid-edit must never trip a multi-minute build.
    Install,
}

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
    if !source_newer || !turn_boundary {
        return SupervisorInstallAction::None;
    }
    if !auto_install {
        return SupervisorInstallAction::Detect;
    }
    SupervisorInstallAction::Install
}

/// `#supkill-bg` — what an explicit operator `restart-supervisor` (IPC `Restart`)
/// should do at the supervisor's next idle tick, framed as blue/green
/// drain-and-supersede. Pure so the policy is unit-testable independent of the live
/// pty/execve machinery.
#[derive(Debug, PartialEq, Eq)]
pub enum SupervisorRestartAction {
    /// No restart pending — do nothing.
    None,
    /// A restart is pending but a turn is still in flight: WAIT for the in-flight
    /// turn to finish before acting (`#supkill-bg` part 1 — the old supervisor
    /// drains its last turn instead of being torn down mid-turn). The kill/relaunch
    /// is suppressed until the boundary.
    AwaitDrain,
    /// At a turn boundary with a stale binary: hot-reload in place via `execve`,
    /// preserving the live harness child + pane (`#supkill-bg` part 2 — in-place
    /// re-exec drain is the default healthy restart for the stale-build/`#fcc0`
    /// case). No dropped turn.
    ReexecInPlace,
    /// At a turn boundary with a fresh binary: nothing to upgrade, so the normal
    /// kill-child → relaunch path serves the restart (same binary, same pane).
    RelaunchChild,
}

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
    if !restart_requested {
        return SupervisorRestartAction::None;
    }
    if !turn_boundary {
        return SupervisorRestartAction::AwaitDrain;
    }
    if reexec_intent {
        SupervisorRestartAction::ReexecInPlace
    } else {
        SupervisorRestartAction::RelaunchChild
    }
}

/// Post-child-exit dispatch for the `start` supervisor run loop. After the
/// harness child exits, the loop checks the IPC-requested flags in a fixed
/// priority order before falling back to the normal crash-policy / clean-exit
/// classification. This pure enum models that decision so the "Stop Agent"
/// keepalive contract can be unit-tested without a live PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostChildExitAction {
    /// Route-owned single-cycle completion: exit the supervisor.
    RouteOwnedComplete,
    /// `IpcMethod::Stop` / `stop_requested`: exit the whole supervisor.
    ExitSupervisor,
    /// `IpcMethod::StopAgent` / `stop_agent_requested`: keep the supervisor alive
    /// and land on the restart-or-quit keepalive prompt — never exit, never
    /// auto-restart, regardless of the harness `clean_exit_behavior`.
    StopAgentKeepalive,
    /// `restart_requested`: auto-restart (continue the loop) per the IPC restart.
    AutoRestart,
    /// Fall through to the normal crash-policy / clean-exit classification (which
    /// may itself prompt, auto-restart, or halt).
    NormalExitClassification,
}

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
    if route_owned_completion {
        return PostChildExitAction::RouteOwnedComplete;
    }
    if stop_requested {
        return PostChildExitAction::ExitSupervisor;
    }
    if stop_agent_requested {
        return PostChildExitAction::StopAgentKeepalive;
    }
    if restart_requested {
        return PostChildExitAction::AutoRestart;
    }
    PostChildExitAction::NormalExitClassification
}

/// `#suphandoff` — PCP-owned red/green supervisor handoff state.
///
/// This models the future two-process replacement path separately from the
/// already-live `execve` preserve-child hot reload. Until the project controller
/// commits the lease promotion, the old supervisor remains the only authoritative
/// owner of the child/pty/session socket; the new supervisor is a private-socket
/// standby and must not touch live child I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcpSupervisorHandoffState {
    /// No handoff is in progress.
    Idle,
    /// PCP accepted the request and should launch a replacement supervisor on a
    /// private socket. The old supervisor remains authoritative.
    LaunchingStandby,
    /// Standby process exists; PCP is probing freshness/capabilities/readiness.
    ProbingStandby,
    /// Standby is healthy but fenced. Wait for `prompt_visible && !turn_active`.
    AwaitTurnBoundary,
    /// Turn boundary reached; PCP is compare-and-swap promoting the lease.
    PromotingLease,
    /// Lease promotion committed; the new supervisor may adopt child ownership.
    TransferringOwnership,
    /// Child ownership is transferred; stop the old supervisor generation.
    StoppingOld,
    /// New supervisor owns the lease and child; handoff is complete.
    Complete,
    /// Pre-promotion failure or abort: terminate the standby and keep old active.
    RollingBack,
    /// Failure after lease promotion. Do not silently roll back; require repair.
    BlockedPostPromotion,
}

impl PcpSupervisorHandoffState {
    /// Whether the old supervisor is still the authoritative child/pty owner.
    /// This stays true through standby launch/probe/drain and only flips after
    /// PCP commits the lease promotion.
    pub fn old_supervisor_authoritative(self) -> bool {
        matches!(
            self,
            Self::Idle
                | Self::LaunchingStandby
                | Self::ProbingStandby
                | Self::AwaitTurnBoundary
                | Self::PromotingLease
                | Self::RollingBack
        )
    }

    /// Whether normal code may let the standby touch live child/pty state.
    /// A private-socket standby is intentionally no-touch before promotion; a
    /// post-promotion block is repair-only and should not continue normal I/O.
    pub fn standby_may_touch_child(self) -> bool {
        matches!(
            self,
            Self::TransferringOwnership | Self::StoppingOld | Self::Complete
        )
    }

    /// Whether PCP has committed the lease/generation promotion. Past this
    /// point rollback is no longer the safe automatic response.
    pub fn lease_promoted(self) -> bool {
        matches!(
            self,
            Self::TransferringOwnership
                | Self::StoppingOld
                | Self::Complete
                | Self::BlockedPostPromotion
        )
    }
}

/// Events consumed by the pure PCP supervisor handoff transition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcpSupervisorHandoffEvent {
    RequestAccepted,
    StandbyStarted,
    StandbyStartFailed,
    StandbyHealthy,
    StandbyFailed,
    TurnBoundaryReached,
    PromotionCommitted,
    PromotionFailed,
    OwnershipTransferred,
    OldSupervisorStopped,
    RollbackComplete,
    AbortRequested,
}

/// Side effect the PCP runtime must perform after a handoff transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcpSupervisorHandoffAction {
    None,
    LaunchStandby,
    ProbeStandby,
    WaitTurnBoundary,
    PromoteLease,
    TransferChildOwnership,
    StopOldSupervisor,
    CompleteHandoff,
    RollbackStandby,
    EscalateRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcpSupervisorHandoffTransition {
    pub state: PcpSupervisorHandoffState,
    pub action: PcpSupervisorHandoffAction,
}

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
    use PcpSupervisorHandoffAction::*;
    use PcpSupervisorHandoffEvent::*;
    use PcpSupervisorHandoffState::*;

    let (state, action) = match (state, event) {
        (Idle, RequestAccepted) => (LaunchingStandby, LaunchStandby),

        (LaunchingStandby, StandbyStarted) => (ProbingStandby, ProbeStandby),
        (LaunchingStandby, StandbyStartFailed) => (RollingBack, RollbackStandby),

        (ProbingStandby, StandbyHealthy) => (AwaitTurnBoundary, WaitTurnBoundary),
        (ProbingStandby, StandbyFailed) => (RollingBack, RollbackStandby),

        (AwaitTurnBoundary, TurnBoundaryReached) => (PromotingLease, PromoteLease),
        (AwaitTurnBoundary, StandbyFailed) => (RollingBack, RollbackStandby),

        (PromotingLease, PromotionCommitted) => (TransferringOwnership, TransferChildOwnership),
        (PromotingLease, PromotionFailed | StandbyFailed) => (RollingBack, RollbackStandby),

        (TransferringOwnership, OwnershipTransferred) => (StoppingOld, StopOldSupervisor),
        (TransferringOwnership, StandbyFailed) => (BlockedPostPromotion, EscalateRepair),

        (StoppingOld, OldSupervisorStopped) => (Complete, CompleteHandoff),
        (StoppingOld, StandbyFailed) => (BlockedPostPromotion, EscalateRepair),

        (RollingBack, RollbackComplete) => (Idle, None),

        (Complete, _) => (Complete, None),
        (BlockedPostPromotion, _) => (BlockedPostPromotion, EscalateRepair),

        (Idle, AbortRequested) => (Idle, None),
        (
            LaunchingStandby | ProbingStandby | AwaitTurnBoundary | PromotingLease,
            AbortRequested,
        ) => (RollingBack, RollbackStandby),
        (TransferringOwnership | StoppingOld, AbortRequested) => {
            (BlockedPostPromotion, EscalateRepair)
        }

        (state, _) => (state, None),
    };

    PcpSupervisorHandoffTransition { state, action }
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
    fn agent_change_restart_decision_policy() {
        use AgentChangeRestartAction as A;
        // Unchanged harness → always None (complete no-op), regardless of boundary.
        assert_eq!(
            agent_change_restart_decision(false, true, true, false),
            A::None
        );
        // Knob off → None even when the harness changed at a quiet boundary.
        assert_eq!(
            agent_change_restart_decision(true, false, true, false),
            A::None
        );
        // Changed + knob on + quiet dispatch-ready boundary → Restart.
        assert_eq!(
            agent_change_restart_decision(true, true, true, false),
            A::Restart
        );
        // Changed + knob on but mid-turn → wait (never interrupt).
        assert_eq!(
            agent_change_restart_decision(true, true, true, true),
            A::WaitForBoundary
        );
        // Changed + knob on but no prompt visible yet → wait.
        assert_eq!(
            agent_change_restart_decision(true, true, false, false),
            A::WaitForBoundary
        );
    }

    #[test]
    fn harness_change_forces_fresh_spawn_predicate() {
        // INERTNESS: same binary ⇒ no fresh-spawn swap (the common same-harness
        // restart path is byte-identical — run.rs leaves harness/base_args/env and
        // `pending_adopt` untouched).
        assert!(!harness_change_forces_fresh_spawn("claude", "claude"));
        assert!(!harness_change_forces_fresh_spawn("codex", "codex"));
        assert!(!harness_change_forces_fresh_spawn("opencode", "opencode"));
        // A real harness change ⇒ force a fresh spawn of the new harness.
        assert!(harness_change_forces_fresh_spawn("claude", "opencode"));
        assert!(harness_change_forces_fresh_spawn("claude", "codex"));
        assert!(harness_change_forces_fresh_spawn("codex", "claude"));
    }

    #[test]
    fn restart_action_is_the_phase1b_trigger_value() {
        // `#agentreloadrestart` Phase 1b wiring contract, modelable WITHOUT a live
        // PTY: the idle-watch sets `restart_requested` exactly when the pure policy
        // returns `Restart` (changed + knob on + quiet dispatch-ready boundary).
        // Every other policy outcome must NOT trigger a restart.
        use AgentChangeRestartAction as A;
        let should_request_restart = |d: A| matches!(d, A::Restart);
        // The boundary case that triggers.
        assert!(should_request_restart(agent_change_restart_decision(
            true, true, true, false
        )));
        // Mid-turn / no prompt / unchanged / knob-off must NOT trigger.
        assert!(!should_request_restart(agent_change_restart_decision(
            true, true, true, true
        )));
        assert!(!should_request_restart(agent_change_restart_decision(
            true, true, false, false
        )));
        assert!(!should_request_restart(agent_change_restart_decision(
            false, true, true, false
        )));
        assert!(!should_request_restart(agent_change_restart_decision(
            true, false, true, false
        )));
    }

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
            supervisor_recycle_action(false, true, true, true, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(false, true, true, false, false, false, false),
            None
        );
        // Stale but mid-turn (not at a boundary) → never act, even with the flag on.
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, false, false, false),
            None
        );
        assert_eq!(
            supervisor_recycle_action(true, true, false, false, false, false, false),
            None
        );
        // Stale at a turn boundary, auto-recycle OFF → surface only, regardless of
        // whether a queue head is pending.
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, false, false),
            Detect
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, true, false, false, false),
            Detect
        );
        // Stale + boundary + opt-in ON, a queue head waiting → recycle NOW so the next
        // queue item runs on the fresh binary (debounce bypassed).
        assert_eq!(
            supervisor_recycle_action(true, true, true, true, false, false, false),
            RecycleImmediate
        );
        // Stale + boundary + opt-in ON, no head waiting → debounced recycle.
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, false, false, false),
            RecycleDebounced
        );
    }

    #[test]
    fn supervisor_recycle_action_explicit_admin_overrides_opt_out() {
        use SupervisorRecycleAction::*;
        // `#supselfheal` Phase 1: an explicit `admin recycle` overrides the
        // default-OFF opt-out and recycles a stale supervisor immediately at the
        // next turn boundary — the gentle, non-disruptive supervisor-recycle path.
        // Auto-recycle OFF + explicit admin → RecycleImmediate (overrides Detect),
        // regardless of whether a head is pending.
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, false, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, true, true, false, false),
            RecycleImmediate
        );
        // Auto-recycle ON + explicit admin → still immediate (no debounce wait).
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, true, false, false),
            RecycleImmediate
        );
        // Explicit admin NEVER drops a live turn: mid-turn (no boundary) stays None.
        assert_eq!(
            supervisor_recycle_action(true, false, false, false, true, false, false),
            None
        );
        // `#wd40` state-flush: explicit admin on a FRESH binary now recycles the
        // process to flush stale in-memory state (e.g. a lagging CRDT projection),
        // rather than no-opping and leaving the operator unable to clear it.
        assert_eq!(
            supervisor_recycle_action(false, false, true, false, true, false, false),
            RecycleImmediate
        );
        // ...but still never mid-turn (no boundary) on a fresh binary either.
        assert_eq!(
            supervisor_recycle_action(false, false, false, false, true, false, false),
            None
        );
        // A fresh binary WITHOUT explicit admin stays None (no spurious flush).
        assert_eq!(
            supervisor_recycle_action(false, true, true, true, false, false, false),
            None
        );
    }

    #[test]
    fn supervisor_recycle_action_write_wedge_overrides_opt_out() {
        use SupervisorRecycleAction::*;
        // `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`): a wedged editor-IPC
        // write against a nominally-active listener is the clearest proof the live
        // binary is bad. Like explicit admin, it overrides the default-OFF opt-out
        // and recycles immediately at the boundary — a wedge must never stay Detect.
        // Auto-recycle OFF + write_wedged → RecycleImmediate (overrides Detect).
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, true, false),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, true, false, true, false),
            RecycleImmediate
        );
        // Auto-recycle ON + write_wedged → immediate (no debounce wait, even with no
        // head pending — the wedge does not wait for an idle boundary that may never
        // come).
        assert_eq!(
            supervisor_recycle_action(true, true, true, false, false, true, false),
            RecycleImmediate
        );
        // A wedge NEVER drops a live turn: mid-turn (no boundary) stays None.
        assert_eq!(
            supervisor_recycle_action(true, false, false, false, false, true, false),
            None
        );
        // A wedge on a FRESH binary → nothing to recycle (the wedge is not a stale
        // binary; some other transient cause owns the converge retry).
        assert_eq!(
            supervisor_recycle_action(false, false, true, false, false, true, false),
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
            supervisor_recycle_action(true, true, true, true, false, false, true),
            EscalateKillRelaunch
        );
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, false, false, true),
            EscalateKillRelaunch
        );
        // Escalation wins over the wedge/admin RecycleImmediate arms (re-trying the
        // failed execve would otherwise be chosen).
        assert_eq!(
            supervisor_recycle_action(true, false, true, false, true, true, true),
            EscalateKillRelaunch
        );
        // Escalation still respects turn_boundary — a failed re-exec mid-turn must
        // not drop the live turn.
        assert_eq!(
            supervisor_recycle_action(true, true, false, true, false, false, true),
            None
        );
        // A failed re-exec on a FRESH binary → nothing to escalate.
        assert_eq!(
            supervisor_recycle_action(false, true, true, true, false, false, true),
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
                /* reexec_failed */ false,
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
            supervisor_recycle_action(false, true, true, false, false, false, false),
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

    #[test]
    fn post_child_exit_action_priority_and_stop_agent_keepalive() {
        use PostChildExitAction::*;
        // Args: (route_owned_completion, stop_requested, stop_agent_requested, restart_requested)

        // No flags → fall through to the normal crash-policy/clean-exit classification.
        assert_eq!(
            post_child_exit_action(false, false, false, false),
            NormalExitClassification
        );

        // Route-owned completion wins over everything (it's the first check).
        assert_eq!(
            post_child_exit_action(true, true, true, true),
            RouteOwnedComplete
        );

        // `stop_requested` (Kill Supervisor) exits the supervisor and beats
        // stop-agent + restart.
        assert_eq!(
            post_child_exit_action(false, true, true, true),
            ExitSupervisor
        );

        // KEYSTONE: `stop_agent_requested` ⇒ keepalive, and it beats
        // `restart_requested` so "Stop Agent" NEVER auto-restarts. It is also
        // independent of any harness clean-exit behavior because this path runs
        // BEFORE the normal classification.
        assert_eq!(
            post_child_exit_action(false, false, true, true),
            StopAgentKeepalive
        );
        assert_eq!(
            post_child_exit_action(false, false, true, false),
            StopAgentKeepalive
        );

        // `restart_requested` alone auto-restarts.
        assert_eq!(
            post_child_exit_action(false, false, false, true),
            AutoRestart
        );

        // Stop Agent must not exit the supervisor and must not auto-restart:
        // for every combination where stop_agent is set (and neither route-owned
        // completion nor a hard stop is requested), the action is the keepalive.
        for restart in [false, true] {
            let action = post_child_exit_action(false, false, true, restart);
            assert_eq!(action, StopAgentKeepalive);
            assert_ne!(action, ExitSupervisor);
            assert_ne!(action, AutoRestart);
        }
    }

    #[test]
    fn pcp_supervisor_handoff_happy_path_promotes_only_at_boundary() {
        use PcpSupervisorHandoffAction::*;
        use PcpSupervisorHandoffEvent::*;
        use PcpSupervisorHandoffState::*;

        let mut state = Idle;
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());
        assert!(!state.lease_promoted());

        let transition = pcp_supervisor_handoff_transition(state, RequestAccepted);
        assert_eq!(transition.action, LaunchStandby);
        state = transition.state;
        assert_eq!(state, LaunchingStandby);
        assert!(state.old_supervisor_authoritative());

        let transition = pcp_supervisor_handoff_transition(state, StandbyStarted);
        assert_eq!(transition.action, ProbeStandby);
        state = transition.state;
        assert_eq!(state, ProbingStandby);
        assert!(state.old_supervisor_authoritative());

        let transition = pcp_supervisor_handoff_transition(state, StandbyHealthy);
        assert_eq!(transition.action, WaitTurnBoundary);
        state = transition.state;
        assert_eq!(state, AwaitTurnBoundary);
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());

        let transition = pcp_supervisor_handoff_transition(state, TurnBoundaryReached);
        assert_eq!(transition.action, PromoteLease);
        state = transition.state;
        assert_eq!(state, PromotingLease);
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());
        assert!(!state.lease_promoted());

        let transition = pcp_supervisor_handoff_transition(state, PromotionCommitted);
        assert_eq!(transition.action, TransferChildOwnership);
        state = transition.state;
        assert_eq!(state, TransferringOwnership);
        assert!(!state.old_supervisor_authoritative());
        assert!(state.standby_may_touch_child());
        assert!(state.lease_promoted());

        let transition = pcp_supervisor_handoff_transition(state, OwnershipTransferred);
        assert_eq!(transition.action, StopOldSupervisor);
        state = transition.state;
        assert_eq!(state, StoppingOld);

        let transition = pcp_supervisor_handoff_transition(state, OldSupervisorStopped);
        assert_eq!(transition.action, CompleteHandoff);
        assert_eq!(transition.state, Complete);
        assert!(transition.state.standby_may_touch_child());
        assert!(transition.state.lease_promoted());
    }

    #[test]
    fn pcp_supervisor_handoff_pre_promotion_failure_rolls_back_to_old_owner() {
        use PcpSupervisorHandoffAction::*;
        use PcpSupervisorHandoffEvent::*;
        use PcpSupervisorHandoffState::*;

        let mut state = pcp_supervisor_handoff_transition(Idle, RequestAccepted).state;
        state = pcp_supervisor_handoff_transition(state, StandbyStarted).state;
        state = pcp_supervisor_handoff_transition(state, StandbyHealthy).state;
        assert_eq!(state, AwaitTurnBoundary);

        let transition = pcp_supervisor_handoff_transition(state, StandbyFailed);
        assert_eq!(transition.action, RollbackStandby);
        state = transition.state;
        assert_eq!(state, RollingBack);
        assert!(state.old_supervisor_authoritative());
        assert!(!state.standby_may_touch_child());
        assert!(!state.lease_promoted());

        let transition = pcp_supervisor_handoff_transition(state, RollbackComplete);
        assert_eq!(transition.action, None);
        assert_eq!(transition.state, Idle);
        assert!(transition.state.old_supervisor_authoritative());
    }

    #[test]
    fn pcp_supervisor_handoff_post_promotion_failure_requires_repair() {
        use PcpSupervisorHandoffAction::*;
        use PcpSupervisorHandoffEvent::*;
        use PcpSupervisorHandoffState::*;

        let mut state = Idle;
        for event in [
            RequestAccepted,
            StandbyStarted,
            StandbyHealthy,
            TurnBoundaryReached,
            PromotionCommitted,
        ] {
            state = pcp_supervisor_handoff_transition(state, event).state;
        }
        assert_eq!(state, TransferringOwnership);
        assert!(state.lease_promoted());

        let transition = pcp_supervisor_handoff_transition(state, StandbyFailed);
        assert_eq!(transition.action, EscalateRepair);
        assert_eq!(transition.state, BlockedPostPromotion);
        assert!(!transition.state.old_supervisor_authoritative());
        assert!(!transition.state.standby_may_touch_child());
        assert!(transition.state.lease_promoted());
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
