//! Pure supervisor process lifecycle decisions.
//!
//! This module owns restart/recycle/install policy. It does not spawn, kill,
//! exec, probe files, inspect panes, or mutate documents.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorRecycleAction {
    None,
    Detect,
    RecycleImmediate,
    RecycleDebounced,
    EscalateKillRelaunch,
    DeferUnsafeCheckpoint,
    DeferCycleOpen,
}

/// The strongest supervisor-recycle checkpoint observed on this watch tick.
///
/// This is deliberately narrower than a generic "idle" fact. An in-place
/// supervisor `execve` preserves the harness child, so an explicit operator
/// recycle may run at [`Self::SafeIntraTurn`] once the document cycle is closed.
/// Routine maintenance still requires [`Self::TurnBoundary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorRecycleCheckpoint {
    /// At least one supervisor IPC handler is still in flight.
    Unsafe,
    /// Supervisor IPC is drained, but the harness turn marker remains active.
    SafeIntraTurn,
    /// Supervisor IPC is drained and the harness reports a real turn boundary.
    TurnBoundary,
}

impl SupervisorRecycleCheckpoint {
    pub fn from_observation(turn_boundary: bool, supervisor_ipc_drained: bool) -> Self {
        if !supervisor_ipc_drained {
            Self::Unsafe
        } else if turn_boundary {
            Self::TurnBoundary
        } else {
            Self::SafeIntraTurn
        }
    }

    pub fn is_safe(self) -> bool {
        !matches!(self, Self::Unsafe)
    }
}

pub const MAX_REEXEC_ESCALATIONS: u32 = 2;

pub fn reexec_escalation_within_bound(attempts: u32, max: u32) -> bool {
    attempts < max
}

/// `#supselfheal` Phase 2 (`#supselfheal-wedgetrigger`) — classify the
/// `write_wedged` evidence input consumed by [`supervisor_recycle_action`].
///
/// The editor-IPC write path is wedged when a nominally-active listener refuses
/// at least `threshold` consecutive writes without proving delivery. Failures
/// against an inactive listener are missing-listener blocks, not active-listener
/// wedges.
pub fn write_wedged_from_ipc_failures(
    consecutive_failures: u64,
    listener_nominally_active: bool,
    threshold: u64,
) -> bool {
    listener_nominally_active && consecutive_failures >= threshold
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationContinuity {
    /// No durable response and no exact live-child transport handoff exists.
    None,
    /// The response is capture-backed and the fresh generation can replay the
    /// closeout state machine without re-running the harness turn.
    CapturedResponse,
    /// The exact harness child, PTY, session, and open cycle remain live while
    /// only the supervisor image changes. This is adopt-only continuity: child
    /// loss must never cause the turn to be re-dispatched.
    PreservedHarnessChild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationTransitionAdmission {
    Permit,
    DeferUnsafeCheckpoint,
    DeferOpenCycle,
}

/// The topology gate for replacing a supervisor generation.
///
/// An open cycle is stronger evidence than prompt-idle, binary-stale, or elapsed
/// watch-tick observations. It may cross a generation boundary only when that
/// exact cycle has durable response-replay continuity or the replacement can
/// adopt the exact live harness child without replay. There is deliberately no
/// timeout input.
pub fn generation_transition_admission(
    cycle_open: bool,
    continuity: GenerationContinuity,
    supervisor_ipc_drained: bool,
) -> GenerationTransitionAdmission {
    if cycle_open && matches!(continuity, GenerationContinuity::None) {
        return GenerationTransitionAdmission::DeferOpenCycle;
    }
    if !supervisor_ipc_drained {
        return GenerationTransitionAdmission::DeferUnsafeCheckpoint;
    }
    GenerationTransitionAdmission::Permit
}

/// Whether an already-durable captured response should drive a supervisor
/// generation refresh at this checkpoint.
///
/// `#gh181`: the durable capture is itself the replay checkpoint named by
/// [`generation_transition_admission`]. It must not depend on a second,
/// incidental producer (a fresh editor-replica request, a one-shot IPC wedge,
/// or a resume retry that happened to latch `needs_operator`) before a stale or
/// explicitly requested generation may cross the open cycle. Auto-refresh
/// still honors its opt-in, and every path still requires drained supervisor
/// IPC.
pub fn captured_cycle_generation_refresh_due(
    generation_stale: bool,
    auto_recycle: bool,
    explicit_admin: bool,
    supervisor_ipc_drained: bool,
    durable_response_captured: bool,
) -> bool {
    supervisor_ipc_drained
        && durable_response_captured
        && (explicit_admin || (generation_stale && auto_recycle))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootResumeAction {
    None,
    AdoptSurvivingChild,
    RedispatchInterruptedTurn,
    SpawnWithoutRedispatch,
}

pub fn boot_resume_action(
    is_recycle_boot: bool,
    cycle_open: bool,
    child_survived: bool,
    already_consumed: bool,
    redispatch_if_child_died: bool,
) -> BootResumeAction {
    if !is_recycle_boot || !cycle_open {
        return BootResumeAction::None;
    }
    if child_survived {
        return BootResumeAction::AdoptSurvivingChild;
    }
    if !redispatch_if_child_died {
        return BootResumeAction::SpawnWithoutRedispatch;
    }
    if already_consumed {
        return BootResumeAction::None;
    }
    BootResumeAction::RedispatchInterruptedTurn
}

/// How the supervisor delivers an operator **Clear Session Context** IPC request
/// (`#idleclearrestart`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorClearDelivery {
    /// Type the harness-native clear command (`/clear`, OpenCode `/new`) into
    /// the live child through the supervisor's guarded pane-input path. The
    /// child process, and every background agent/shell it owns, survives.
    TypeNativeClear,
    /// Replace the child through the supervised fresh restart. Reserved for the
    /// cases where there is no responsive composer to type into, or the
    /// harness's native clear command itself terminates the process.
    RestartFresh(OperatorClearRestartReason),
}

/// Why an operator Clear must replace the child instead of typing the
/// harness-native clear command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorClearRestartReason {
    /// The harness child is gone (crash, Stop Agent): there is no live
    /// composer, so a fresh child is the only way to reach a cleared session.
    ChildNotAlive,
    /// The actor is `waiting_input`: the supervisor's own restart-or-quit
    /// keepalive prompt (or a harness permission prompt) owns the input, not a
    /// dispatch-ready composer. Typing `/clear` there would answer the wrong
    /// prompt.
    SupervisorWaitingInput,
    /// The harness exits successfully after consuming its native clear (Codex,
    /// `#clear-ready-restart`); in a route-owned pane that clean exit races the
    /// completion watcher and can tear the supervisor down before a replacement
    /// child launches.
    HarnessClearExitsProcess,
}

impl OperatorClearRestartReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChildNotAlive => "child_not_alive",
            Self::SupervisorWaitingInput => "supervisor_waiting_input",
            Self::HarnessClearExitsProcess => "harness_clear_exits_process",
        }
    }
}

/// True when the harness's native clear command resets the conversation in
/// place, keeping the harness process (and its background agents/shells)
/// alive. Codex is the one harness whose `/clear` ends the process
/// (`#clear-ready-restart`).
pub fn native_clear_keeps_harness_process(harness_binary: &str) -> bool {
    agent_doc_harness::HarnessConfig::from_agent_name(harness_binary).binary != "codex"
}

/// Decide how an operator Clear reaches the harness (`#idleclearrestart`).
///
/// A plain Clear at an idle, dispatch-ready prompt on a responsive child TYPES
/// the harness-native clear command, exactly like the deferred
/// (`#clearqueuedliveturn`) delivery: restarting the child would kill every
/// Claude background agent and shell it owns with no report (2026-10-11 live
/// test: a background agent stopped at 12/40 ticks). A restart is kept only
/// where typing cannot work — no live child, the supervisor's keepalive /
/// permission prompt owns the input, or the harness's clear exits the
/// process. A non-idle actor (busy, starting) keeps the pre-existing typed
/// delivery; the client-side `operator_clear` guard has already refused
/// protected input and queued a clear behind a live turn.
pub fn operator_clear_delivery(
    harness_binary: &str,
    waiting_input: bool,
    ready: bool,
    child_alive: bool,
) -> OperatorClearDelivery {
    if !(waiting_input || ready) {
        return OperatorClearDelivery::TypeNativeClear;
    }
    if !child_alive {
        return OperatorClearDelivery::RestartFresh(OperatorClearRestartReason::ChildNotAlive);
    }
    if waiting_input {
        return OperatorClearDelivery::RestartFresh(
            OperatorClearRestartReason::SupervisorWaitingInput,
        );
    }
    if !native_clear_keeps_harness_process(harness_binary) {
        return OperatorClearDelivery::RestartFresh(
            OperatorClearRestartReason::HarnessClearExitsProcess,
        );
    }
    OperatorClearDelivery::TypeNativeClear
}

#[cfg(test)]
mod operator_clear_delivery_tests {
    use super::*;

    /// `#idleclearrestart`: the 2026-10-11 shape. Claude at an idle prompt with
    /// background agents running must receive a typed `/clear`, never a
    /// restart that kills the agents.
    #[test]
    fn idle_ready_claude_types_native_clear() {
        assert_eq!(
            operator_clear_delivery("claude", false, true, true),
            OperatorClearDelivery::TypeNativeClear
        );
    }

    #[test]
    fn idle_ready_opencode_and_grok_type_native_clear() {
        for harness in ["opencode", "grok"] {
            assert_eq!(
                operator_clear_delivery(harness, false, true, true),
                OperatorClearDelivery::TypeNativeClear,
                "{harness}"
            );
        }
    }

    /// `#clear-ready-restart`: Codex exits after consuming `/clear`, so its
    /// idle clear keeps the supervised fresh restart.
    #[test]
    fn idle_ready_codex_keeps_fresh_restart() {
        assert_eq!(
            operator_clear_delivery("codex", false, true, true),
            OperatorClearDelivery::RestartFresh(
                OperatorClearRestartReason::HarnessClearExitsProcess
            )
        );
    }

    #[test]
    fn waiting_input_keeps_fresh_restart_for_every_harness() {
        for harness in ["claude", "codex", "opencode"] {
            assert_eq!(
                operator_clear_delivery(harness, true, false, true),
                OperatorClearDelivery::RestartFresh(
                    OperatorClearRestartReason::SupervisorWaitingInput
                ),
                "{harness}"
            );
        }
    }

    #[test]
    fn idle_prompt_without_a_live_child_restarts() {
        for (waiting_input, ready) in [(true, false), (false, true)] {
            assert_eq!(
                operator_clear_delivery("claude", waiting_input, ready, false),
                OperatorClearDelivery::RestartFresh(OperatorClearRestartReason::ChildNotAlive)
            );
        }
    }

    /// A non-idle actor keeps the pre-existing typed delivery for every
    /// harness; the client guard already queued or refused unsafe presses.
    #[test]
    fn non_idle_actor_types_native_clear() {
        for harness in ["claude", "codex", "opencode"] {
            for child_alive in [true, false] {
                assert_eq!(
                    operator_clear_delivery(harness, false, false, child_alive),
                    OperatorClearDelivery::TypeNativeClear,
                    "{harness} child_alive={child_alive}"
                );
            }
        }
    }

    #[test]
    fn restart_reasons_have_stable_log_tokens() {
        assert_eq!(
            OperatorClearRestartReason::ChildNotAlive.as_str(),
            "child_not_alive"
        );
        assert_eq!(
            OperatorClearRestartReason::SupervisorWaitingInput.as_str(),
            "supervisor_waiting_input"
        );
        assert_eq!(
            OperatorClearRestartReason::HarnessClearExitsProcess.as_str(),
            "harness_clear_exits_process"
        );
    }
}

/// Whether an operator `session_restart` may replace the harness child now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorRestartAdmission {
    /// No live child is at risk — spawn the replacement.
    Spawn,
    /// A document cycle is open and the current child still owns it. Spawning
    /// now would leave two children rendering through one pane's PTY proxy.
    DeferCycleOpen,
}

/// Gate an operator restart on the same open-cycle condition the recycle path
/// uses (`#haivendupsession`).
///
/// The recycle path has always deferred here — `supervisor_recycle_action`
/// returns `DeferCycleOpen`, logged as `supervisor_recycle_deferred_cycle_open`.
/// The restart path had no equivalent gate, and the asymmetry is visible in
/// `src/haiven-dev/.agent-doc/logs/ops.log` on 2026-08-03: recycle deferred on
/// `agent_doc_cycle_open` at 07:27:54 and again at 07:28:32, while
/// `restart_continue_spawn` fired between them at 07:28:29. The controller had
/// already logged `ipc_accepted_deferred reason=live_supervisor_owns_drain` —
/// but that deferral only means the *controller* declined to escalate; the IPC
/// request had been accepted by the live supervisor, which spawned anyway.
///
/// With `--route-owned-reap-policy keep-alive` the previous child is not
/// reaped, so both stayed attached to pane `%95` and clearing the visible
/// session revealed the other underneath.
///
/// `child_alive` is required as well as `cycle_open`: once the child is gone
/// there is nothing to double up, so a restart after a crash must still spawn.
pub fn supervisor_restart_admission(
    cycle_open: bool,
    child_alive: bool,
) -> SupervisorRestartAdmission {
    if cycle_open && child_alive {
        return SupervisorRestartAdmission::DeferCycleOpen;
    }
    SupervisorRestartAdmission::Spawn
}

/// Gate the child replacement an operator **Clear Session Context** requests
/// (`#clearcapturedcycle`).
///
/// A context clear changes the harness conversation, not the document. The
/// open-cycle gate ([`supervisor_restart_admission`]) exists so a restart never
/// replaces a child that still OWNS a turn. Once the actor is back at a
/// dispatch-ready prompt and the open cycle's response is durably captured,
/// the child owns nothing: the remaining closeout is the supervisor's keyed
/// captured-finalize resume (`#midturn-recycle-resume`, `#gh181`), which runs
/// without the harness and keeps the retained write durable either way.
///
/// Holding the clear behind that closeout made it wait on DOCUMENT convergence.
/// Observed 2026-10-16 on `src/haiven-dev/tasks/sdk.md` (Codex): the retained
/// write was blocked by `editor_holds_unmerged_visible_text` (06:01:53Z), the
/// operator pressed Clear Session Context at 06:03:31Z and 06:03:41Z, both were
/// latched behind `agent_doc_cycle_open`, the client reported "Cleared", and no
/// `restart_fresh_spawn` ever ran — the latch was later overwritten by an
/// install recycle and lost. The cycle itself committed at 06:05:50Z without
/// any harness involvement.
///
/// Every other shape keeps the shared gate: a live turn (`busy`), a harness
/// permission prompt (`waiting_input`), and an uncaptured preflight still own
/// the cycle.
pub fn operator_clear_restart_admission(
    cycle_open: bool,
    child_alive: bool,
    actor_ready: bool,
    cycle_response_captured: bool,
) -> SupervisorRestartAdmission {
    if cycle_open && child_alive && actor_ready && cycle_response_captured {
        return SupervisorRestartAdmission::Spawn;
    }
    supervisor_restart_admission(cycle_open, child_alive)
}

#[cfg(test)]
mod operator_clear_restart_admission_tests {
    use super::*;

    /// `#clearcapturedcycle`: the exact 2026-10-16 sdk.md shape — Codex at its
    /// ready prompt, the cycle open only because its captured response's
    /// closeout waits on the editor. The clear replaces the child now.
    #[test]
    fn ready_actor_over_a_captured_cycle_clears_now() {
        assert_eq!(
            operator_clear_restart_admission(true, true, true, true),
            SupervisorRestartAdmission::Spawn
        );
    }

    /// A captured response with the harness still inside its turn (busy, or a
    /// permission prompt reported as not-ready) still owns the cycle.
    #[test]
    fn captured_cycle_without_a_ready_prompt_keeps_the_open_cycle_gate() {
        assert_eq!(
            operator_clear_restart_admission(true, true, false, true),
            SupervisorRestartAdmission::DeferCycleOpen
        );
    }

    /// An uncaptured open cycle (a preflight the harness has not answered)
    /// is still the harness's work; the clear waits as before.
    #[test]
    fn uncaptured_open_cycle_keeps_the_open_cycle_gate() {
        assert_eq!(
            operator_clear_restart_admission(true, true, true, false),
            SupervisorRestartAdmission::DeferCycleOpen
        );
    }

    /// Outside the crossed shape the clear gate IS the shared restart gate.
    #[test]
    fn every_other_shape_agrees_with_the_shared_restart_gate() {
        for cycle_open in [false, true] {
            for child_alive in [false, true] {
                for actor_ready in [false, true] {
                    for captured in [false, true] {
                        if cycle_open && child_alive && actor_ready && captured {
                            continue;
                        }
                        assert_eq!(
                            operator_clear_restart_admission(
                                cycle_open,
                                child_alive,
                                actor_ready,
                                captured
                            ),
                            supervisor_restart_admission(cycle_open, child_alive),
                            "cycle_open={cycle_open} child_alive={child_alive} \
                             actor_ready={actor_ready} captured={captured}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod restart_admission_tests {
    use super::*;

    /// `#haivendupsession`: the exact 2026-08-03 shape. A cycle was open (the
    /// recycle path deferred on it at 07:27:54 and 07:28:32) and the child was
    /// still live, yet `restart_continue_spawn` fired at 07:28:29 between them,
    /// leaving two children on pane %95 under a keep-alive reap policy.
    #[test]
    fn open_cycle_with_live_child_defers_the_restart() {
        assert_eq!(
            supervisor_restart_admission(true, true),
            SupervisorRestartAdmission::DeferCycleOpen
        );
    }

    /// A restart after the child is gone has nothing to double up. Deferring
    /// here would strand the operator with no harness at all, which is why the
    /// gate needs both facts and not just `cycle_open`.
    #[test]
    fn dead_child_still_spawns_even_mid_cycle() {
        assert_eq!(
            supervisor_restart_admission(true, false),
            SupervisorRestartAdmission::Spawn
        );
    }

    /// No open cycle is the ordinary operator restart and must stay immediate.
    #[test]
    fn closed_cycle_spawns_regardless_of_child_state() {
        for child_alive in [true, false] {
            assert_eq!(
                supervisor_restart_admission(false, child_alive),
                SupervisorRestartAdmission::Spawn,
                "a closed cycle must never defer (child_alive={child_alive})"
            );
        }
    }

    /// The restart gate must agree with the recycle gate on the same facts —
    /// the whole defect was the two paths disagreeing.
    #[test]
    fn restart_gate_agrees_with_recycle_gate_on_an_open_cycle() {
        // The operator-request shape: an explicit request, away from a turn
        // boundary, with a cycle open. `explicit_admin` is the recycle-path
        // analogue of an operator `session_restart`.
        let recycle = supervisor_recycle_action(
            false, // stale
            true,  // auto_recycle
            SupervisorRecycleCheckpoint::SafeIntraTurn,
            false, // head_pending
            true,  // explicit_admin
            false, // write_wedged
            false, // capture_backed_refresh
            false, // reexec_failed
            true,  // cycle_open
        );
        assert!(
            matches!(recycle, SupervisorRecycleAction::DeferCycleOpen),
            "recycle must defer on an open cycle, got {recycle:?}"
        );
        assert_eq!(
            supervisor_restart_admission(true, true),
            SupervisorRestartAdmission::DeferCycleOpen,
            "restart must defer on the same facts"
        );
    }
}

/// Whether a native cdylib `reload_library` intent may be published to one live
/// editor process now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeReloadAdmission {
    /// No attached document is mid-cycle — retire the old generation.
    Publish,
    /// An attached document holds an open cycle. Publishing now retires the
    /// native generation that owns that document's Lazily replica, and the
    /// replacement generation does not carry it forward.
    DeferCycleOpen,
}

/// Gate a native cdylib reload on the same open-cycle condition the recycle and
/// restart paths use (`#installstrandsreplica`).
///
/// `#deploy-just-do-it` mandates a `make install` after every fix, and install
/// fans a typed `reload_library` intent out to every hot-reload-capable editor
/// process. The JetBrains handoff quiesces each replica against the outgoing
/// generation, publishes the replacement, and re-registers. When the document
/// that *ordered* the install is itself attached and mid-cycle, that
/// re-registration does not converge: the binary then observes
/// `editor_attached_model_missing`, `missing_replica` recovery exhausts, and the
/// disk descent is correctly refused — so the session that made the fix cannot
/// close itself out, with no in-binary recovery. Observed 2026-09-11 23:36 on
/// `tasks/agent-doc/agent-doc-bugs.md`, one minute after that session's second
/// install; the IDE had BOTH `libagent_doc-1789182099748.so` and
/// `libagent_doc-1789184163251.so` mapped, and the replica lived in the one the
/// reload replaced. `admin reload-lib` did not clear it; only an operator
/// re-attach in the editor did.
///
/// The old generation keeps working, so deferring costs the editor nothing but
/// running the previous build until the cycle closes. Stranding it costs the
/// operator the whole closeout. Defer.
pub fn native_reload_admission(attached_cycle_open: bool) -> NativeReloadAdmission {
    if attached_cycle_open {
        return NativeReloadAdmission::DeferCycleOpen;
    }
    NativeReloadAdmission::Publish
}

#[cfg(test)]
mod native_reload_admission_tests {
    use super::*;

    /// `#installstrandsreplica`: the exact 2026-09-11 shape — the document that
    /// ordered the install was attached and mid-cycle when the fan-out ran.
    #[test]
    fn open_cycle_on_an_attached_document_defers_the_reload() {
        assert_eq!(
            native_reload_admission(true),
            NativeReloadAdmission::DeferCycleOpen
        );
    }

    /// Deferring forever would freeze every editor on the build it happened to
    /// load first, so a closed cycle must publish immediately.
    #[test]
    fn closed_cycle_publishes_the_new_generation() {
        assert_eq!(
            native_reload_admission(false),
            NativeReloadAdmission::Publish
        );
    }

    /// All three generation-retiring gates read the same fact. The recycle and
    /// restart pair already had to be reconciled once (`#haivendupsession`); the
    /// reload fan-out is the third path that retires live state mid-cycle.
    #[test]
    fn reload_gate_agrees_with_recycle_and_restart_gates_on_an_open_cycle() {
        let recycle = supervisor_recycle_action(
            false, // stale
            true,  // auto_recycle
            SupervisorRecycleCheckpoint::SafeIntraTurn,
            false, // head_pending
            true,  // explicit_admin
            false, // write_wedged
            false, // capture_backed_refresh
            false, // reexec_failed
            true,  // cycle_open
        );
        assert!(
            matches!(recycle, SupervisorRecycleAction::DeferCycleOpen),
            "recycle must defer on an open cycle, got {recycle:?}"
        );
        assert_eq!(
            supervisor_restart_admission(true, true),
            SupervisorRestartAdmission::DeferCycleOpen,
            "restart must defer on the same facts"
        );
        assert_eq!(
            native_reload_admission(true),
            NativeReloadAdmission::DeferCycleOpen,
            "the native reload fan-out must defer on the same facts"
        );
    }
}

/// A `start_session` failure during supervisor recycle is a transient controller
/// teardown race while retry budget remains.
pub fn start_session_retryable_during_recycle(
    recycle_pending: bool,
    attempts_used: usize,
    max_attempts: usize,
) -> bool {
    recycle_pending && attempts_used < max_attempts
}

/// After the trigger has already been injected, an in-flight recycle means the
/// submit may have been dropped across `execve`; wait before the single resubmit.
pub fn recycle_interrupted_resubmit_should_wait(
    trigger_already_injected: bool,
    recycle_pending: bool,
) -> bool {
    trigger_already_injected && recycle_pending
}

/// Whether a supervisor that wants to recycle must do so now because its own
/// captured-response resume is stuck (`#stalesupresumedeadlock`).
///
/// The captured-finalize resume runs inside the supervisor process, so a stale
/// generation judges the retained response with stale code. On 2026-09-30
/// (06:17-06:35 UTC, `tasks/agent-doc/agent-doc-bugs.md`) a route-owned
/// supervisor built on 2026-09-28 latched `captured_finalize_resume_needs_operator`
/// every ~6s ("heading already committed", which the installed build handles)
/// while `supervisor_recycle_deferred_cycle_open` held its recycle: the open
/// cycle needed the new code, and the new code needed the recycle. It was
/// broken by hand with an empty-stdin `write --commit`.
///
/// A latched verdict from a generation that is already due for replacement is
/// not an operator problem yet — the replacement has not looked. The response is
/// durably captured (that is what the resume is replaying), so the recycle is
/// capture-backed and crosses the open cycle like a write wedge. The fresh
/// generation starts without the latch and without the recycle request (the
/// marker is cleared before `execve`), so this fires at most once per episode;
/// a verdict the fresh build also latches stays with the operator.
pub fn stale_generation_capture_resume_needs_recycle(
    recycle_wanted: bool,
    capture_resume_latched_needs_operator: bool,
) -> bool {
    recycle_wanted && capture_resume_latched_needs_operator
}

#[allow(clippy::too_many_arguments)]
pub fn supervisor_recycle_action(
    stale: bool,
    auto_recycle: bool,
    checkpoint: SupervisorRecycleCheckpoint,
    head_pending: bool,
    explicit_admin: bool,
    write_wedged: bool,
    capture_backed_refresh: bool,
    reexec_failed: bool,
    cycle_open: bool,
) -> SupervisorRecycleAction {
    let continuity = if write_wedged || capture_backed_refresh {
        GenerationContinuity::CapturedResponse
    } else {
        GenerationContinuity::None
    };
    supervisor_recycle_action_with_continuity(
        stale,
        auto_recycle,
        checkpoint,
        head_pending,
        explicit_admin,
        write_wedged,
        capture_backed_refresh,
        reexec_failed,
        cycle_open,
        continuity,
    )
}

/// Full generation-transition policy, including the exact live-child handoff
/// used for a stale-supervisor refresh during an open harness turn.
#[allow(clippy::too_many_arguments)]
pub fn supervisor_recycle_action_with_continuity(
    stale: bool,
    auto_recycle: bool,
    checkpoint: SupervisorRecycleCheckpoint,
    head_pending: bool,
    explicit_admin: bool,
    write_wedged: bool,
    capture_backed_refresh: bool,
    reexec_failed: bool,
    cycle_open: bool,
    continuity: GenerationContinuity,
) -> SupervisorRecycleAction {
    match generation_transition_admission(cycle_open, continuity, checkpoint.is_safe()) {
        GenerationTransitionAdmission::DeferOpenCycle => {
            return SupervisorRecycleAction::DeferCycleOpen;
        }
        GenerationTransitionAdmission::DeferUnsafeCheckpoint => {
            return if explicit_admin
                || write_wedged
                || capture_backed_refresh
                || (cycle_open && matches!(continuity, GenerationContinuity::PreservedHarnessChild))
            {
                SupervisorRecycleAction::DeferUnsafeCheckpoint
            } else {
                SupervisorRecycleAction::None
            };
        }
        GenerationTransitionAdmission::Permit => {}
    }

    // A live-child handoff is the non-destructive mid-turn path: execve only
    // replaces this supervisor image and the fresh image adopts the exact PTY
    // and child without injecting or replaying the document turn. A failed
    // exec remains on the old image. It must never fall through to the
    // kill+relaunch escalation while that uncaptured cycle is open.
    if cycle_open && matches!(continuity, GenerationContinuity::PreservedHarnessChild) {
        if reexec_failed {
            return SupervisorRecycleAction::DeferCycleOpen;
        }
        if explicit_admin || (stale && auto_recycle) {
            return SupervisorRecycleAction::RecycleImmediate;
        }
        return if stale {
            SupervisorRecycleAction::Detect
        } else {
            SupervisorRecycleAction::None
        };
    }

    // The transition owner consumes the live IPC-drain observation directly.
    // Durable recycle intents remain pending until a safe checkpoint rather
    // than relying on each effect caller to reproduce this guard.
    // `#midturn-wedge-recycle`: a proven editor-IPC write wedge means the active
    // turn/cycle can never reach its own boundary — closeout is blocked on a
    // convergence receipt that will not arrive, so `turn_boundary` never becomes
    // true and the `cycle_open` defer below would wait forever. That is the exact
    // deadlock that previously forced the operator to run `admin recycle` by hand
    // mid-turn. A wedge therefore escalates immediately, bypassing the boundary and
    // cycle-open gates. The idle-watch caller pre-gates `write_wedged` on the first
    // available SAFE intra-turn checkpoint (no IPC connection in flight), so the
    // `execve` cannot sever an active patch apply, and latches a `recycle_attempted`
    // flag on the dewedge marker before the execve so this is once-per-episode and
    // cannot recycle-loop. The in-flight response is capture-backed and is recovered
    // via redispatch/replay on the fresh supervisor.
    // `capture_backed_refresh` has the same liveness shape as a proven write
    // wedge: the open closeout cycle cannot reach its boundary until this
    // generation is replaced. It is raised for a typed stale editor-delivery
    // request (the replica worker must be refreshed), for a stale/requested
    // generation whose open cycle already owns a typed durable response
    // capture (`#gh181`), and for a stale generation whose own
    // captured-response resume latched `needs_operator`
    // (`#stalesupresumedeadlock`, see
    // [`stale_generation_capture_resume_needs_recycle`]). The caller only raises
    // it at a capture-backed safe checkpoint, so deferring on `cycle_open` would
    // be a circular wait while an immediate in-place recycle preserves the
    // response.
    if write_wedged || capture_backed_refresh {
        return if reexec_failed {
            SupervisorRecycleAction::EscalateKillRelaunch
        } else {
            SupervisorRecycleAction::RecycleImmediate
        };
    }

    if !matches!(checkpoint, SupervisorRecycleCheckpoint::TurnBoundary) {
        // `#supboundarylivelock`: an EXPLICIT operator recycle must not be
        // silently dropped or wait on its own harness turn forever.
        //
        // `turn_boundary` is `prompt_visible && !turn_active`. During closeout,
        // the cycle can commit while the harness turn marker correctly remains
        // active until the final response returns. Waiting for that marker from
        // inside closeout creates a circular wait: `admin recycle` stays pending
        // even though supervisor IPC is drained and there is no open cycle.
        //
        // The in-place `execve` preserves the harness child. Once the document
        // cycle is closed, a safe intra-turn checkpoint therefore completes the
        // explicit recycle without interrupting the turn. An open cycle remains
        // authoritative and continues to defer.
        if explicit_admin {
            return if cycle_open {
                SupervisorRecycleAction::DeferCycleOpen
            } else {
                SupervisorRecycleAction::RecycleImmediate
            };
        }
        return SupervisorRecycleAction::None;
    }

    if stale {
        if reexec_failed {
            return SupervisorRecycleAction::EscalateKillRelaunch;
        }
        if explicit_admin {
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

    if explicit_admin {
        return SupervisorRecycleAction::RecycleImmediate;
    }
    SupervisorRecycleAction::None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorInstallAction {
    None,
    Detect,
    Install,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorRestartAction {
    None,
    AwaitDrain,
    ReexecInPlace,
    RelaunchChild,
}

pub fn supervisor_restart_action(
    restart_requested: bool,
    reexec_intent: bool,
    turn_boundary: bool,
    stale_reexec_safe_checkpoint: bool,
    cycle_open: bool,
) -> SupervisorRestartAction {
    if !restart_requested {
        return SupervisorRestartAction::None;
    }
    if matches!(
        generation_transition_admission(
            cycle_open,
            GenerationContinuity::None,
            stale_reexec_safe_checkpoint,
        ),
        GenerationTransitionAdmission::DeferOpenCycle
    ) {
        return SupervisorRestartAction::AwaitDrain;
    }
    // Re-exec replaces only the stale supervisor host; execve normally preserves
    // the harness child, pane, and durable cycle checkpoint. It must still wait
    // for BOTH authorities to report a safe boundary: supervisor IPC is drained
    // and the harness-owned turn lease is idle. Child adoption is recovery, not
    // permission to interrupt a live turn; if adoption fails there is no generic
    // harness-turn checkpoint from which to continue. The turn lease has its own
    // TTL, so a genuinely stale marker cannot block replacement forever.
    if reexec_intent {
        return if turn_boundary && stale_reexec_safe_checkpoint {
            SupervisorRestartAction::ReexecInPlace
        } else {
            SupervisorRestartAction::AwaitDrain
        };
    }
    // A fresh-binary relaunch replaces the harness child, so it still needs the
    // real turn boundary.
    if !turn_boundary {
        return SupervisorRestartAction::AwaitDrain;
    }
    SupervisorRestartAction::RelaunchChild
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#supboundarylivelock`: an explicit `admin recycle` against a busy,
    /// non-stale supervisor used to wait for the harness turn marker even after
    /// the document cycle committed. The in-place exec preserves the harness
    /// child, so a cycle-closed, IPC-drained checkpoint must complete it.
    #[test]
    fn explicit_admin_recycle_uses_cycle_closed_safe_intra_turn_checkpoint() {
        use SupervisorRecycleAction::*;
        use SupervisorRecycleCheckpoint::*;

        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                SafeIntraTurn,
                false,
                true,
                false,
                false,
                false,
                false,
            ),
            RecycleImmediate,
            "a drained, cycle-closed checkpoint must honor the explicit recycle"
        );

        assert_eq!(
            supervisor_recycle_action(
                false, false, Unsafe, false, true, false, false, false, false,
            ),
            DeferUnsafeCheckpoint,
            "an in-flight IPC handler keeps the durable request pending"
        );

        // Without an explicit request there is nothing to preserve.
        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                SafeIntraTurn,
                false,
                false,
                false,
                false,
                false,
                false,
            ),
            None,
            "routine maintenance still waits for a real turn boundary"
        );

        // At a real boundary the explicit request is consumed as before.
        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                TurnBoundary,
                false,
                true,
                false,
                false,
                false,
                false,
            ),
            RecycleImmediate,
            "reaching a boundary must still honor the explicit request immediately"
        );
    }

    #[test]
    fn recycle_action_policy() {
        use SupervisorRecycleAction::*;
        use SupervisorRecycleCheckpoint::*;

        assert_eq!(
            supervisor_recycle_action(
                false,
                true,
                TurnBoundary,
                true,
                false,
                false,
                false,
                false,
                false,
            ),
            None
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                SafeIntraTurn,
                true,
                false,
                false,
                false,
                false,
                false,
            ),
            None
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                false,
                TurnBoundary,
                false,
                false,
                false,
                false,
                false,
                false,
            ),
            Detect
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                TurnBoundary,
                true,
                false,
                false,
                false,
                false,
                false,
            ),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                TurnBoundary,
                false,
                false,
                false,
                false,
                false,
                false,
            ),
            RecycleDebounced
        );
    }

    #[test]
    fn durable_captured_cycle_drives_stale_or_requested_generation_refresh() {
        assert!(captured_cycle_generation_refresh_due(
            true, true, false, true, true,
        ));
        assert!(captured_cycle_generation_refresh_due(
            false, false, true, true, true,
        ));
        assert!(
            !captured_cycle_generation_refresh_due(true, false, false, true, true),
            "automatic generation refresh must still honor the opt-in"
        );
        assert!(
            !captured_cycle_generation_refresh_due(true, true, false, false, true),
            "an in-flight supervisor IPC handler is never a safe generation boundary"
        );
        assert!(
            !captured_cycle_generation_refresh_due(true, true, false, true, false),
            "an uncaptured preflight has no replay checkpoint"
        );
    }

    #[test]
    fn recycle_defers_all_recycle_arms_while_cycle_open() {
        use SupervisorRecycleAction::*;
        use SupervisorRecycleCheckpoint::*;

        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                TurnBoundary,
                true,
                false,
                false,
                false,
                false,
                true,
            ),
            DeferCycleOpen
        );
        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                TurnBoundary,
                false,
                true,
                false,
                false,
                false,
                true,
            ),
            DeferCycleOpen
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                false,
                TurnBoundary,
                false,
                true,
                false,
                false,
                false,
                true,
            ),
            DeferCycleOpen
        );
        // `#midturn-wedge-recycle`: a proven write wedge no longer defers on
        // cycle_open — the wedged cycle is precisely the one that never closes, so
        // deferring would deadlock. Covered by `wedge_recycles_immediately_mid_turn`.
        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                TurnBoundary,
                true,
                false,
                false,
                false,
                true,
                true,
            ),
            DeferCycleOpen
        );

        assert_eq!(
            supervisor_recycle_action(
                true,
                false,
                TurnBoundary,
                false,
                true,
                false,
                false,
                false,
                false,
            ),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                false,
                TurnBoundary,
                false,
                false,
                true,
                false,
                false,
                false,
            ),
            RecycleImmediate
        );
        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                TurnBoundary,
                true,
                false,
                false,
                false,
                true,
                false,
            ),
            EscalateKillRelaunch
        );
    }

    /// `#stalesupresumedeadlock`: the 2026-09-30 06:17-06:35 shape. A recycle
    /// was due, the cycle was open with a durable capture, and the resume in the
    /// stale generation latched `needs_operator` again and again.
    #[test]
    fn stale_generation_latched_capture_resume_recycles_across_the_open_cycle() {
        use SupervisorRecycleAction::*;
        use SupervisorRecycleCheckpoint::*;

        assert!(stale_generation_capture_resume_needs_recycle(true, true));
        // Without the latch the resume is still making its own progress, and
        // without a due recycle there is no newer generation to hand it to.
        assert!(!stale_generation_capture_resume_needs_recycle(true, false));
        assert!(!stale_generation_capture_resume_needs_recycle(false, true));

        // Args: (stale, auto, checkpoint, head_pending, admin, write_wedged,
        // capture_backed_refresh, reexec_failed, cycle_open)
        let deadlocked = supervisor_recycle_action(
            true,
            true,
            SafeIntraTurn,
            false,
            true,
            false,
            false,
            false,
            true,
        );
        assert_eq!(deadlocked, DeferCycleOpen, "the pre-fix circular wait");

        let refreshed = supervisor_recycle_action(
            true,
            true,
            SafeIntraTurn,
            false,
            true,
            false,
            stale_generation_capture_resume_needs_recycle(true, true),
            false,
            true,
        );
        assert_eq!(refreshed, RecycleImmediate);

        // Still never severs in-flight supervisor IPC.
        let inflight = supervisor_recycle_action(
            true,
            true,
            Unsafe,
            false,
            true,
            false,
            stale_generation_capture_resume_needs_recycle(true, true),
            false,
            true,
        );
        assert_eq!(inflight, DeferUnsafeCheckpoint);
    }

    #[test]
    fn wedge_recycles_immediately_mid_turn() {
        use SupervisorRecycleAction::*;
        use SupervisorRecycleCheckpoint::*;

        // Args: (stale, auto, checkpoint, head_pending, admin, write_wedged,
        // capture_backed_refresh, reexec_failed, cycle_open)
        // The whole point of `#midturn-wedge-recycle`: a proven wedge recycles even
        // when NOT at a turn boundary — the wedged turn can never reach one.
        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                SafeIntraTurn,
                false,
                false,
                true,
                false,
                false,
                false,
            ),
            RecycleImmediate
        );
        // ...and even mid-cycle: the open cycle is the one that is wedged.
        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                SafeIntraTurn,
                false,
                false,
                true,
                false,
                false,
                true,
            ),
            RecycleImmediate
        );
        // Fires on a fresh (non-stale) binary — this session's exact case, where the
        // supervisor was `fresh` yet the editor-IPC write never converged.
        assert_eq!(
            supervisor_recycle_action(
                false,
                true,
                SafeIntraTurn,
                false,
                false,
                true,
                false,
                false,
                true,
            ),
            RecycleImmediate
        );
        // A wedge whose in-place execve already failed escalates to kill+relaunch
        // instead of re-execing a binary that cannot converge.
        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                SafeIntraTurn,
                false,
                false,
                true,
                false,
                true,
                true,
            ),
            EscalateKillRelaunch
        );
        // A durable explicit admin request is different from routine
        // maintenance: once IPC is drained and the cycle is closed, its in-place
        // exec preserves the harness child and must not wait on the turn marker.
        assert_eq!(
            supervisor_recycle_action(
                true,
                true,
                SafeIntraTurn,
                true,
                true,
                false,
                false,
                false,
                false,
            ),
            RecycleImmediate
        );
    }

    /// Live `ad.md` / pane %192 shape (2026-10-13): an uncaptured open cycle is
    /// running in the exact child this supervisor can carry across execve. The
    /// stale image refreshes at the first drained I/O checkpoint, while every
    /// missing/unsafe/failed handoff stays non-destructive.
    #[test]
    fn live_child_continuity_recycles_open_turn_without_replay_or_kill() {
        use GenerationContinuity::*;
        use SupervisorRecycleAction::*;
        use SupervisorRecycleCheckpoint::*;

        let decide = |checkpoint, continuity, reexec_failed| {
            supervisor_recycle_action_with_continuity(
                true,
                true,
                checkpoint,
                false,
                false,
                false,
                false,
                reexec_failed,
                true,
                continuity,
            )
        };

        assert_eq!(
            decide(SafeIntraTurn, PreservedHarnessChild, false),
            RecycleImmediate
        );
        assert_eq!(
            decide(Unsafe, PreservedHarnessChild, false),
            DeferUnsafeCheckpoint
        );
        assert_eq!(
            decide(SafeIntraTurn, GenerationContinuity::None, false),
            DeferCycleOpen
        );
        assert_eq!(
            decide(SafeIntraTurn, PreservedHarnessChild, true),
            DeferCycleOpen,
            "failed execve must not escalate to child kill/relaunch mid-turn"
        );
    }

    #[test]
    fn stale_editor_delivery_recycles_capture_backed_open_cycle() {
        use SupervisorRecycleAction::*;
        use SupervisorRecycleCheckpoint::*;

        assert_eq!(
            supervisor_recycle_action(
                /* stale binary */ false,
                /* auto */ false,
                /* checkpoint */ SafeIntraTurn,
                /* head pending */ false,
                /* admin */ false,
                /* write wedged */ false,
                /* editor delivery stale */ true,
                /* reexec failed */ false,
                /* cycle open */ true,
            ),
            RecycleImmediate,
            "the stale replica refresh must not wait for the cycle it unblocks",
        );
        assert_eq!(
            supervisor_recycle_action(
                false,
                false,
                SafeIntraTurn,
                false,
                false,
                false,
                true,
                true,
                true,
            ),
            EscalateKillRelaunch,
        );

        assert_eq!(
            supervisor_recycle_action(false, false, Unsafe, false, true, false, true, false, true,),
            DeferUnsafeCheckpoint,
            "an in-flight handler remains authoritative even for a stale delivery worker",
        );
    }

    #[test]
    fn write_wedged_classifier_trips_only_against_active_listener_at_threshold() {
        let threshold = 3;

        assert!(write_wedged_from_ipc_failures(threshold, true, threshold));
        assert!(write_wedged_from_ipc_failures(
            threshold + 1,
            true,
            threshold
        ));
        assert!(!write_wedged_from_ipc_failures(
            threshold - 1,
            true,
            threshold
        ));
        assert!(!write_wedged_from_ipc_failures(
            threshold + 5,
            false,
            threshold
        ));
        assert!(!write_wedged_from_ipc_failures(0, true, threshold));
    }

    #[test]
    fn generation_transition_gate_has_no_timeout_escape() {
        use GenerationContinuity::*;
        use GenerationTransitionAdmission::*;

        for ipc_drained in [false, true] {
            assert_eq!(
                generation_transition_admission(true, None, ipc_drained),
                DeferOpenCycle,
                "an uncaptured open cycle dominates every IPC observation"
            );
        }
        assert_eq!(
            generation_transition_admission(true, CapturedResponse, false),
            DeferUnsafeCheckpoint
        );
        assert_eq!(
            generation_transition_admission(true, CapturedResponse, true),
            Permit
        );
        assert_eq!(generation_transition_admission(false, None, true), Permit);
        assert_eq!(
            generation_transition_admission(true, PreservedHarnessChild, true),
            Permit,
            "the exact live child/PTY handoff carries the open cycle without replay"
        );
    }

    #[test]
    fn boot_resume_redispatches_only_when_cycle_open_and_child_died() {
        use BootResumeAction::*;

        assert_eq!(boot_resume_action(false, true, false, false, true), None);
        assert_eq!(boot_resume_action(true, false, false, false, true), None);
        assert_eq!(
            boot_resume_action(true, true, true, false, true),
            AdoptSurvivingChild
        );
        assert_eq!(
            boot_resume_action(true, true, false, false, true),
            RedispatchInterruptedTurn
        );
        assert_eq!(boot_resume_action(true, true, false, true, true), None);
        assert_eq!(
            boot_resume_action(true, true, false, false, false),
            SpawnWithoutRedispatch,
            "an adopt-only mid-turn handoff never replays an in-flight turn"
        );
    }

    #[test]
    fn start_session_retry_only_while_recycling_and_budget_remains() {
        assert!(start_session_retryable_during_recycle(true, 0, 2));
        assert!(start_session_retryable_during_recycle(true, 1, 2));
        assert!(!start_session_retryable_during_recycle(true, 2, 2));
        assert!(!start_session_retryable_during_recycle(false, 0, 2));
    }

    #[test]
    fn recycle_interrupted_resubmit_waits_only_when_injected_and_recycling() {
        assert!(recycle_interrupted_resubmit_should_wait(true, true));
        assert!(!recycle_interrupted_resubmit_should_wait(false, true));
        assert!(!recycle_interrupted_resubmit_should_wait(true, false));
    }

    #[test]
    fn reexec_escalation_is_bounded() {
        assert!(reexec_escalation_within_bound(0, MAX_REEXEC_ESCALATIONS));
        assert!(reexec_escalation_within_bound(
            MAX_REEXEC_ESCALATIONS - 1,
            MAX_REEXEC_ESCALATIONS
        ));
        assert!(!reexec_escalation_within_bound(
            MAX_REEXEC_ESCALATIONS,
            MAX_REEXEC_ESCALATIONS
        ));
    }

    #[test]
    fn install_action_policy() {
        use SupervisorInstallAction::*;

        assert_eq!(supervisor_install_action(false, true, true), None);
        assert_eq!(supervisor_install_action(true, true, false), None);
        assert_eq!(supervisor_install_action(true, false, true), Detect);
        assert_eq!(supervisor_install_action(true, true, true), Install);
    }

    #[test]
    fn restart_action_drain_and_supersede_policy() {
        use SupervisorRestartAction::*;

        assert_eq!(
            supervisor_restart_action(false, true, true, true, false),
            None
        );
        assert_eq!(
            supervisor_restart_action(true, true, false, false, false),
            AwaitDrain
        );
        assert_eq!(
            supervisor_restart_action(true, true, false, true, false),
            AwaitDrain
        );
        assert_eq!(
            supervisor_restart_action(true, true, true, false, false),
            AwaitDrain
        );
        assert_eq!(
            supervisor_restart_action(true, true, true, true, false),
            ReexecInPlace
        );
        assert_eq!(
            supervisor_restart_action(true, false, false, true, false),
            AwaitDrain
        );
        assert_eq!(
            supervisor_restart_action(true, false, true, false, false),
            RelaunchChild
        );
        assert_eq!(
            supervisor_restart_action(true, true, true, true, true),
            AwaitDrain,
            "an uncaptured open cycle blocks stale in-place replacement"
        );
        assert_eq!(
            supervisor_restart_action(true, false, true, true, true),
            AwaitDrain,
            "an uncaptured open cycle blocks fresh child relaunch"
        );
    }
}
