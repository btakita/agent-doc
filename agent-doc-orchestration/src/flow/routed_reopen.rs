use super::types::{DispatchProof, FlowEvent, FlowName, FlowOutcome, FlowStage, RouteDecision};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectPaneSubmitStatus {
    Accepted,
    TimedOut,
}

pub fn direct_pane_submit_acceptance_timeout() -> Duration {
    Duration::from_secs(5)
}

pub fn direct_pane_submit_acceptance_budget() -> Duration {
    // tmux/control-mode delivery can spend the whole acceptance window plus a
    // final capture poll before pane input disappears. Keep the budget above
    // that window so "over_budget" means slower than the path can observe.
    Duration::from_secs(6)
}

pub fn routed_dispatch_start_timeout(test_mode: bool) -> Duration {
    routed_dispatch_start_timeout_for_binary(None, test_mode)
}

pub fn routed_dispatch_start_timeout_for_binary(binary: Option<&str>, test_mode: bool) -> Duration {
    if test_mode {
        if matches!(binary, Some("opencode")) {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(1)
        }
    } else if matches!(binary, Some("opencode")) {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(10)
    }
}

pub fn fresh_route_start_ack_timeout(test_mode: bool) -> Duration {
    if test_mode {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(30)
    }
}

pub fn routed_cycle_ack_timeout(live_child_for_file: bool, test_mode: bool) -> Duration {
    if test_mode {
        if live_child_for_file {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(1)
        }
    } else if live_child_for_file {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(15)
    }
}

pub fn existing_pane_ready_timeout(test_mode: bool) -> Duration {
    if test_mode {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(15)
    }
}

pub fn dispatch_only_starting_pane_ready_timeout_for_binary(
    binary: Option<&str>,
    test_mode: bool,
) -> Duration {
    if test_mode {
        Duration::from_millis(250)
    } else if matches!(binary, Some("opencode")) {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(2)
    }
}

pub fn dispatch_only_starting_pane_recovery_timeout_for_binary(
    binary: Option<&str>,
    test_mode: bool,
) -> Duration {
    if test_mode {
        return Duration::from_millis(400);
    }
    match binary {
        Some("opencode") => Duration::from_secs(15),
        Some("claude") => Duration::from_secs(10),
        Some("codex") => Duration::from_secs(8),
        _ => Duration::from_secs(5),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryBudget {
    pub timeout: Duration,
    pub poll_interval: Duration,
}

impl RetryBudget {
    pub const fn new(timeout: Duration, poll_interval: Duration) -> Self {
        Self {
            timeout,
            poll_interval,
        }
    }
}

pub fn authoritative_actor_ready_retry_budget(
    binary: Option<&str>,
    test_mode: bool,
) -> RetryBudget {
    RetryBudget::new(
        dispatch_only_starting_pane_recovery_timeout_for_binary(binary, test_mode),
        Duration::from_millis(100),
    )
}

pub fn dispatch_only_starting_pane_ready_retry_budget(
    binary: Option<&str>,
    test_mode: bool,
) -> RetryBudget {
    RetryBudget::new(
        dispatch_only_starting_pane_ready_timeout_for_binary(binary, test_mode),
        Duration::from_millis(100),
    )
}

pub fn dispatch_only_starting_pane_recovery_retry_budget(
    binary: Option<&str>,
    test_mode: bool,
) -> RetryBudget {
    RetryBudget::new(
        dispatch_only_starting_pane_recovery_timeout_for_binary(binary, test_mode),
        Duration::from_millis(100),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorDispatchState {
    Ready,
    Starting,
    Busy,
    WaitingInput,
    Blocked,
    Closed,
    Missing,
}

impl ActorDispatchState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Starting => "starting",
            Self::Busy => "busy",
            Self::WaitingInput => "waiting_input",
            Self::Blocked => "blocked",
            Self::Closed => "closed",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorRuntimeHealth {
    Healthy,
    Restartable,
    Halted { restart_count: u32 },
    Unreachable,
    NoSocket,
}

impl ActorRuntimeHealth {
    pub fn label(self) -> String {
        match self {
            Self::Healthy => "healthy".to_string(),
            Self::Restartable => "restartable".to_string(),
            Self::Halted { restart_count } => {
                format!("halted(restart_count={restart_count})")
            }
            Self::Unreachable => "unreachable".to_string(),
            Self::NoSocket => "no_socket".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReopenMode {
    Managed,
    DispatchOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOnlyReopenDelivery {
    SupervisorIpcOnce,
    DirectPaneSubmit,
}

impl DispatchOnlyReopenDelivery {
    pub const fn submit_mode_for_harness(self, harness_binary: &str) -> &'static str {
        match self {
            DispatchOnlyReopenDelivery::SupervisorIpcOnce => "supervisor_normalized_submit",
            DispatchOnlyReopenDelivery::DirectPaneSubmit => {
                crate::sessions::tmux_submit_mode_for_harness(harness_binary)
            }
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            DispatchOnlyReopenDelivery::SupervisorIpcOnce => "supervisor_ipc_once",
            DispatchOnlyReopenDelivery::DirectPaneSubmit => "direct_pane_submit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedDispatchStartProof {
    CommandAcceptedOnly,
    HookPromptMatched,
    HookStateAdvanced,
    PaneStateChanged,
}

impl RoutedDispatchStartProof {
    pub const fn dispatch_stage_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "accepted",
            Self::HookPromptMatched => "consumed",
            Self::HookStateAdvanced => "submitted",
            Self::PaneStateChanged => "pane_state_changed",
        }
    }

    pub const fn proof_scope_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "accepted_only",
            Self::HookPromptMatched | Self::HookStateAdvanced | Self::PaneStateChanged => {
                "dispatch_start"
            }
        }
    }

    pub const fn proof_scope_description(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => {
                "accepted-only; no harness dispatch-start proof was available"
            }
            Self::HookPromptMatched => "dispatch-start proof matched the routed prompt",
            Self::HookStateAdvanced => "dispatch-start proof observed newer harness prompt state",
            Self::PaneStateChanged => "dispatch-start proof observed pane state leave idle chrome",
        }
    }

    pub const fn startup_miss_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "acceptance",
            Self::HookPromptMatched => "consumption",
            Self::HookStateAdvanced => "submission",
            Self::PaneStateChanged => "pane-state-change",
        }
    }

    pub const fn typed_proof(self) -> DispatchProof {
        match self {
            Self::CommandAcceptedOnly => DispatchProof::AcceptedOnly,
            Self::HookPromptMatched => DispatchProof::Consumed,
            Self::HookStateAdvanced | Self::PaneStateChanged => DispatchProof::DispatchStarted,
        }
    }
}

pub fn direct_pane_submit_outcome(
    status: DirectPaneSubmitStatus,
    dispatch_start_proof: Option<RoutedDispatchStartProof>,
) -> &'static str {
    match (status, dispatch_start_proof) {
        (DirectPaneSubmitStatus::Accepted, _) => "accepted",
        (DirectPaneSubmitStatus::TimedOut, Some(_)) => "acceptance_unobserved_dispatch_proven",
        (DirectPaneSubmitStatus::TimedOut, None) => "acceptance_unobserved",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchStartProofDecision {
    Accepted,
    FailClosedAcceptedOnly,
}

pub fn decide_dispatch_start_proof(
    proof: RoutedDispatchStartProof,
    dispatch_start_proof_required: bool,
) -> DispatchStartProofDecision {
    if proof == RoutedDispatchStartProof::CommandAcceptedOnly && dispatch_start_proof_required {
        DispatchStartProofDecision::FailClosedAcceptedOnly
    } else {
        DispatchStartProofDecision::Accepted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchStartProofFacts {
    pub proof: RoutedDispatchStartProof,
    pub dispatch_start_proof_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchStartProofClassification {
    pub decision: DispatchStartProofDecision,
    pub typed_proof: DispatchProof,
}

pub fn classify_dispatch_start_proof(
    facts: DispatchStartProofFacts,
) -> DispatchStartProofClassification {
    DispatchStartProofClassification {
        decision: decide_dispatch_start_proof(facts.proof, facts.dispatch_start_proof_required),
        typed_proof: facts.proof.typed_proof(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchOnlyProofPolicyFacts<'a> {
    pub harness_binary: &'a str,
    pub codex_dispatch_start_tracking_enabled: bool,
}

pub fn dispatch_only_dispatch_start_proof_required(
    facts: DispatchOnlyProofPolicyFacts<'_>,
) -> bool {
    match facts.harness_binary {
        "codex" => facts.codex_dispatch_start_tracking_enabled,
        "opencode" => false,
        _ => false,
    }
}

pub fn should_print_dispatch_only_unproven_progress(
    facts: DispatchOnlyProofPolicyFacts<'_>,
) -> bool {
    facts.harness_binary != "codex" || !facts.codex_dispatch_start_tracking_enabled
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchOnlyProofOutcomeFacts<'a> {
    pub file_display: &'a str,
    pub pane: &'a str,
    pub harness_binary: &'a str,
    pub delivery: DispatchOnlyReopenDelivery,
    pub dispatch_start: RoutedDispatchStartProof,
    pub timeout_secs: u64,
}

pub fn dispatch_only_sent_log_message(facts: DispatchOnlyProofOutcomeFacts<'_>) -> String {
    format!(
        "route_dispatch_only_sent file={} pane={} harness={} delivery={} submit_mode={} proof={} proof_scope={}",
        facts.file_display,
        facts.pane,
        facts.harness_binary,
        facts.delivery.label(),
        facts.delivery.submit_mode_for_harness(facts.harness_binary),
        facts.dispatch_start.dispatch_stage_label(),
        facts.dispatch_start.proof_scope_label()
    )
}

pub fn dispatch_only_sent_console_message(facts: DispatchOnlyProofOutcomeFacts<'_>) -> String {
    format!(
        "[route] dispatch-only {} reopen for {} was sent to pane {} via {} ({}) with {} proof ({})",
        facts.harness_binary,
        facts.file_display,
        facts.pane,
        facts.delivery.label(),
        facts.delivery.submit_mode_for_harness(facts.harness_binary),
        facts.dispatch_start.dispatch_stage_label(),
        facts.dispatch_start.proof_scope_description()
    )
}

pub fn accepted_only_dispatch_start_log_message(
    facts: DispatchOnlyProofOutcomeFacts<'_>,
) -> String {
    format!(
        "route_dispatch_only_submit_unproven file={} pane={} harness={} delivery={} submit_mode={} proof=accepted proof_scope=accepted_only timeout_secs={}",
        facts.file_display,
        facts.pane,
        facts.harness_binary,
        facts.delivery.label(),
        facts.delivery.submit_mode_for_harness(facts.harness_binary),
        facts.timeout_secs
    )
}

pub fn accepted_only_dispatch_start_refusal_message(
    facts: DispatchOnlyProofOutcomeFacts<'_>,
) -> String {
    format!(
        "dispatch-only {} reopen for {} was accepted in pane {} via {} ({}), but only pane-input acceptance proof was available after waiting {}s; treating this as not dispatched because no dispatch-start proof was recorded. Restore an idle {} prompt or restart the session and reroute again",
        facts.harness_binary,
        facts.file_display,
        facts.pane,
        facts.delivery.label(),
        facts.delivery.submit_mode_for_harness(facts.harness_binary),
        facts.timeout_secs,
        facts.harness_binary
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutedReopenFacts {
    pub actor_state: ActorDispatchState,
    pub prompt_ready: bool,
    pub has_prompt_bearing_work: bool,
    pub mode: ReopenMode,
    pub degraded_authority: bool,
    pub dispatch_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedReopenOutcome {
    pub decision: RouteDecision,
    pub reason: &'static str,
}

pub fn decide_authoritative_reopen(facts: RoutedReopenFacts) -> RoutedReopenOutcome {
    if facts.degraded_authority || !facts.dispatch_eligible {
        return RoutedReopenOutcome {
            decision: RouteDecision::FailClosed,
            reason: "degraded_authority",
        };
    }

    match facts.actor_state {
        ActorDispatchState::Ready if facts.prompt_ready => RoutedReopenOutcome {
            decision: RouteDecision::ReuseReady,
            reason: "ready_prompt",
        },
        ActorDispatchState::Ready => RoutedReopenOutcome {
            decision: RouteDecision::WaitForReady,
            reason: "ready_without_prompt_proof",
        },
        ActorDispatchState::Starting => RoutedReopenOutcome {
            decision: RouteDecision::WaitForReady,
            reason: "starting_requires_prompt_ready_barrier",
        },
        ActorDispatchState::Busy if facts.mode == ReopenMode::Managed => RoutedReopenOutcome {
            decision: RouteDecision::ReuseReady,
            reason: "managed_busy_actor_can_queue_once",
        },
        ActorDispatchState::Busy => RoutedReopenOutcome {
            decision: RouteDecision::FailClosed,
            reason: "dispatch_only_busy_actor_not_ready",
        },
        ActorDispatchState::WaitingInput
        | ActorDispatchState::Blocked
        | ActorDispatchState::Closed => RoutedReopenOutcome {
            decision: RouteDecision::FailClosed,
            reason: "actor_terminal_or_protected_state",
        },
        ActorDispatchState::Missing if facts.has_prompt_bearing_work => RoutedReopenOutcome {
            decision: RouteDecision::StartNew,
            reason: "missing_actor_with_prompt_work",
        },
        ActorDispatchState::Missing => RoutedReopenOutcome {
            decision: RouteDecision::StartNew,
            reason: "missing_actor",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoritativeActorDispatchAction {
    FocusOnly,
    DispatchOnlyBusyQueue,
    RecoverDispatchOnlyWaitingInput,
    ManagedSupervisorQueue,
    FailClosed,
    DispatchOnlyDirectPane,
    ManagedSupervisorIpc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativeActorDispatchActionFacts {
    pub mode: ReopenMode,
    pub actor_state: ActorDispatchState,
    pub has_prompt_bearing_work: bool,
    pub reopen_decision: RouteDecision,
}

pub fn classify_authoritative_actor_dispatch_action(
    facts: AuthoritativeActorDispatchActionFacts,
) -> AuthoritativeActorDispatchAction {
    if actor_dispatch_blocker_reason(facts.actor_state).is_some() {
        if !facts.has_prompt_bearing_work {
            return AuthoritativeActorDispatchAction::FocusOnly;
        }
        if facts.mode == ReopenMode::DispatchOnly
            && actor_can_queue_optimistically(facts.actor_state)
            && facts.reopen_decision == RouteDecision::FailClosed
        {
            return AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue;
        }
        if facts.mode == ReopenMode::DispatchOnly
            && actor_waiting_input_recoverable(facts.actor_state)
        {
            return AuthoritativeActorDispatchAction::RecoverDispatchOnlyWaitingInput;
        }
        if facts.reopen_decision == RouteDecision::ReuseReady
            && actor_can_queue_optimistically(facts.actor_state)
        {
            return AuthoritativeActorDispatchAction::ManagedSupervisorQueue;
        }
        return AuthoritativeActorDispatchAction::FailClosed;
    }

    match facts.mode {
        ReopenMode::DispatchOnly => AuthoritativeActorDispatchAction::DispatchOnlyDirectPane,
        ReopenMode::Managed => AuthoritativeActorDispatchAction::ManagedSupervisorIpc,
    }
}

/// A plain dispatch-only reopen (IDE `Run Agent Doc`, no prompt-bearing work)
/// against a *busy* authoritative actor classifies as [`AuthoritativeActorDispatchAction::FocusOnly`]:
/// it focuses the pane but never injects the reopen trigger. Returning success
/// there reports a routed run to the IDE caller even though nothing was
/// submitted, so the operator sees no feedback (`#jb-run-agent-doc-command-route-miss`).
/// In that exact shape the route must fail closed with the busy-not-ready signal
/// instead, so the IDE surfaces a "session still running" notification. Managed
/// reopens and non-busy blocker states (`WaitingInput` / `Blocked` / `Closed`,
/// which have their own recovery/terminal handling) keep the focus-only success.
pub fn dispatch_only_focus_only_should_fail_closed(
    mode: ReopenMode,
    actor_state: ActorDispatchState,
) -> bool {
    mode == ReopenMode::DispatchOnly && actor_state == ActorDispatchState::Busy
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptReadyBarrierFacts {
    pub actor_state: ActorDispatchState,
    pub prompt_ready: bool,
    pub dispatch_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptReadyBarrierDecision {
    Ready,
    Terminal,
    Continue,
}

pub fn classify_prompt_ready_barrier(facts: PromptReadyBarrierFacts) -> PromptReadyBarrierDecision {
    if facts.actor_state == ActorDispatchState::Ready
        && facts.prompt_ready
        && facts.dispatch_eligible
    {
        return PromptReadyBarrierDecision::Ready;
    }
    // `#run-agent-doc-busy-ready-deadlock` (#snrun): a `Busy` projection whose
    // live pane proves a current-generation dispatch-ready prompt is a stale busy
    // projection, not a real active turn. The post-wait repair
    // (`busy_projection_repaired_by_ready_prompt`) already promotes this exact
    // state to `Ready` and dispatches — but only AFTER the full `--wait-for-ready`
    // timeout (e.g. JB `Run Agent Doc`'s 60s), which the operator perceives as a
    // deadlock. Recognize the repair in the wait barrier so the loop returns
    // `Ready` on the first poll that proves the prompt instead of spinning to the
    // deadline. `prompt_ready` for a `Busy` actor is only ever true when the live
    // capture matched a harness dispatch-ready prompt, so this never returns
    // `Ready` for a genuinely mid-turn pane.
    if busy_projection_repaired_by_ready_prompt(facts.actor_state, facts.prompt_ready)
        && facts.dispatch_eligible
    {
        return PromptReadyBarrierDecision::Ready;
    }
    if actor_start_wait_terminal_state(facts.actor_state) {
        return PromptReadyBarrierDecision::Terminal;
    }
    PromptReadyBarrierDecision::Continue
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoritativeActorReadyFacts {
    pub pane_id: String,
    pub generation: u64,
    pub actor_state: ActorDispatchState,
    pub supervisor_health: String,
    pub runtime_state: String,
    pub prompt_ready: bool,
    pub last_transition_reason: String,
    pub last_transition_caller: String,
}

impl AuthoritativeActorReadyFacts {
    pub fn log_fields(&self) -> String {
        format!(
            "pane={} generation={} actor_state={} supervisor_health={} runtime_state={} prompt_ready={} last_transition_reason={} last_transition_caller={}",
            self.pane_id,
            self.generation,
            self.actor_state.as_str(),
            self.supervisor_health,
            self.runtime_state,
            self.prompt_ready,
            self.last_transition_reason,
            self.last_transition_caller
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativePromptReadyBarrierFacts<'a> {
    pub ready_facts: &'a AuthoritativeActorReadyFacts,
    pub dispatch_eligible: bool,
}

pub fn classify_authoritative_prompt_ready_barrier(
    facts: AuthoritativePromptReadyBarrierFacts<'_>,
) -> PromptReadyBarrierDecision {
    classify_prompt_ready_barrier(PromptReadyBarrierFacts {
        actor_state: facts.ready_facts.actor_state,
        prompt_ready: facts.ready_facts.prompt_ready,
        dispatch_eligible: facts.dispatch_eligible,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartingActorLogFacts<'a> {
    pub file_display: &'a str,
    pub harness_binary: &'a str,
    pub timeout: Duration,
    pub elapsed: Duration,
    pub ready_facts: &'a AuthoritativeActorReadyFacts,
}

pub fn starting_actor_not_ready_log_line(facts: StartingActorLogFacts<'_>) -> String {
    format!(
        "route_authoritative_actor_starting_not_ready file={} harness={} timeout_ms={} elapsed_ms={} {}",
        facts.file_display,
        facts.harness_binary,
        facts.timeout.as_millis(),
        facts.elapsed.as_millis(),
        facts.ready_facts.log_fields()
    )
}

pub fn starting_actor_ready_log_line(
    file_display: &str,
    harness_binary: &str,
    elapsed: Duration,
    facts: &AuthoritativeActorReadyFacts,
) -> String {
    format!(
        "route_starting_actor_ready file={} harness={} elapsed_ms={} {}",
        file_display,
        harness_binary,
        elapsed.as_millis(),
        facts.log_fields()
    )
}

pub fn starting_actor_terminal_log_line(
    file_display: &str,
    harness_binary: &str,
    elapsed: Duration,
    facts: &AuthoritativeActorReadyFacts,
) -> String {
    format!(
        "route_authoritative_actor_starting_terminal file={} harness={} elapsed_ms={} {}",
        file_display,
        harness_binary,
        elapsed.as_millis(),
        facts.log_fields()
    )
}

pub fn starting_actor_timeout_coalesced_log_line(
    file_display: &str,
    harness_binary: &str,
    elapsed: Duration,
    facts: &AuthoritativeActorReadyFacts,
) -> String {
    format!(
        "route_starting_actor_timeout_coalesced file={} harness={} elapsed_ms={} {}",
        file_display,
        harness_binary,
        elapsed.as_millis(),
        facts.log_fields()
    )
}

pub const fn actor_start_wait_terminal_state(state: ActorDispatchState) -> bool {
    matches!(
        state,
        ActorDispatchState::Closed | ActorDispatchState::Blocked
    )
}

pub const fn actor_dispatch_blocker_reason(state: ActorDispatchState) -> Option<&'static str> {
    match state {
        ActorDispatchState::Ready => None,
        ActorDispatchState::Starting => Some("the authoritative actor is still starting"),
        ActorDispatchState::Busy => Some("the authoritative actor is busy"),
        ActorDispatchState::WaitingInput => {
            Some("the authoritative actor is waiting for user input")
        }
        ActorDispatchState::Closed => Some("the authoritative actor is closed"),
        ActorDispatchState::Blocked => Some("the authoritative actor is blocked"),
        ActorDispatchState::Missing => Some("the authoritative actor is missing"),
    }
}

pub const fn actor_can_queue_optimistically(state: ActorDispatchState) -> bool {
    matches!(state, ActorDispatchState::Busy)
}

/// Direct pane evidence repairs a stale busy projection (#snrun).
///
/// When the authoritative actor is projected `Busy` but the live pane proves a
/// dispatch-ready prompt in the current generation (`prompt_ready`), it is not
/// actually mid-turn — the busy/lease projection is stale. Callers promote it to
/// `Ready` so a dispatch-only route dispatches to the proven-ready pane instead
/// of queuing the prompt into `agent:queue auto`.
///
/// A `Busy` projection WITHOUT a proven ready prompt is left untouched and fails
/// closed (queues), per the direct-evidence rule (idle direct evidence repairs a
/// stale busy projection; busy direct evidence stays fail-closed). `prompt_ready`
/// for a `Busy` actor is only ever true when the live pane capture matched a
/// harness dispatch-ready prompt, so this never dispatches into a running turn.
pub const fn busy_projection_repaired_by_ready_prompt(
    actor_state: ActorDispatchState,
    prompt_ready: bool,
) -> bool {
    matches!(actor_state, ActorDispatchState::Busy) && prompt_ready
}

pub const fn actor_waiting_input_recoverable(state: ActorDispatchState) -> bool {
    matches!(state, ActorDispatchState::WaitingInput)
}

pub fn actor_recovery_hint(state: ActorDispatchState, file_display: &str) -> String {
    match state {
        ActorDispatchState::Starting => format!(
            "Wait for the pane to show a dispatch-ready prompt (`prompt_ready=true`), then rerun `agent-doc {file_display}`. If the pane stays stuck, restart the owner with `agent-doc start {file_display}`."
        ),
        ActorDispatchState::Busy => {
            "Wait for the active turn to finish before rerouting this document.".to_string()
        }
        ActorDispatchState::WaitingInput => format!(
            "Answer the supervisor prompt in the pane, or restart the owner with `agent-doc start {file_display}`."
        ),
        ActorDispatchState::Closed => {
            format!("Start a new owner with `agent-doc start {file_display}` before rerouting.")
        }
        ActorDispatchState::Blocked => format!(
            "Inspect the pane diagnostics, then restart the owner with `agent-doc start {file_display}`."
        ),
        ActorDispatchState::Ready | ActorDispatchState::Missing => String::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyPaneAutoFixOutcome {
    RetryRoute,
    RetryRouteAfterSupervisorRestart,
    RetryRouteAfterFreshRestart,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusyPaneAutoFixFacts {
    pub test_hook_changed: bool,
    pub fix_made_changes: bool,
    pub supervisor_healthy: bool,
    pub restarted_supervisor: bool,
}

pub fn busy_existing_pane_auto_fix_outcome(facts: BusyPaneAutoFixFacts) -> BusyPaneAutoFixOutcome {
    if facts.restarted_supervisor {
        return BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart;
    }
    if facts.test_hook_changed || facts.fix_made_changes {
        return BusyPaneAutoFixOutcome::RetryRoute;
    }
    if facts.supervisor_healthy {
        BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart
    } else {
        BusyPaneAutoFixOutcome::FailClosed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativeRuntimeFacts {
    pub health: ActorRuntimeHealth,
    pub actor_state_present: bool,
}

pub fn authoritative_actor_dispatch_guard_reason(
    facts: AuthoritativeRuntimeFacts,
) -> Option<String> {
    if facts.health != ActorRuntimeHealth::Healthy {
        return Some(format!("supervisor health is {}", facts.health.label()));
    }
    if !facts.actor_state_present {
        return Some("supervisor actor_state is missing".to_string());
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DegradedAuthoritativeActorFacts<'a> {
    pub actor_pane: &'a str,
    pub transition_caller: &'a str,
    pub transition_reason: &'a str,
    pub registered_pane: Option<&'a str>,
    pub live_owner_pane: Option<&'a str>,
}

pub fn can_use_degraded_authoritative_actor(facts: DegradedAuthoritativeActorFacts<'_>) -> bool {
    if facts.transition_caller == "register" && facts.transition_reason == "register" {
        return false;
    }
    facts.registered_pane == Some(facts.actor_pane)
        || facts.live_owner_pane == Some(facts.actor_pane)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DegradedAuthoritativeActorDirectSubmit<'a> {
    pub file_display: &'a str,
    pub pane_id: &'a str,
    pub harness_binary: &'a str,
    pub generation: u64,
    pub record_state: &'a str,
    pub supervisor_health: &'a str,
    pub runtime_actor_state: &'a str,
    pub reason: &'a str,
}

pub fn degraded_authoritative_actor_direct_submit_log_message(
    facts: DegradedAuthoritativeActorDirectSubmit<'_>,
) -> String {
    format!(
        "route_dispatch_only_authoritative_degraded_direct_pane file={} pane={} harness={} generation={} record_state={} supervisor_health={} runtime_actor_state={} reason={}",
        facts.file_display,
        facts.pane_id,
        facts.harness_binary,
        facts.generation,
        facts.record_state,
        facts.supervisor_health,
        facts.runtime_actor_state,
        facts.reason
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedReopenGuardReason {
    AcceptedOnlyDispatchStartProof,
    StartingActorNotReady,
    StartingActorNotReadyUnpersisted,
    DispatchOnlyBusyActorNotReady,
    /// The reopen was refused because the live pane is stuck in an interactive
    /// shell substate (e.g. `reverse-i-search` / history search) that is not a
    /// dispatch-ready composer (`#snrun`). Distinct from the generic
    /// `DispatchOnlyBusyActorNotReady` busy actor so the failure names the
    /// terminal substate that blocked dispatch.
    BlockedInInteractiveSubstate,
}

impl RoutedReopenGuardReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AcceptedOnlyDispatchStartProof => "accepted_only_dispatch_start_proof",
            Self::StartingActorNotReady => "starting_actor_not_ready",
            Self::StartingActorNotReadyUnpersisted => "starting_actor_not_ready_unpersisted",
            Self::DispatchOnlyBusyActorNotReady => "dispatch_only_busy_actor_not_ready",
            Self::BlockedInInteractiveSubstate => "blocked_in_interactive_substate",
        }
    }
}

/// Does a live dispatch-blocker reason denote an interactive shell substate
/// (reverse/history search) rather than a normal busy harness turn?
///
/// Mirrors the `"interactive shell ..."` reasons produced by
/// [`crate::harness::HarnessConfig::dispatch_blocker_reason`] (e.g.
/// `interactive shell reverse-i-search`, `interactive shell history search`).
/// Pure — used to pick the dedicated [`RoutedReopenGuardReason`] for the
/// dispatch-only fail-closed path.
pub fn is_interactive_shell_substate_reason(reason: &str) -> bool {
    reason.trim_start().starts_with("interactive shell")
}

/// Select the fail-closed guard reason for a refused dispatch-only reopen,
/// preferring the dedicated interactive-substate reason when the live blocker
/// is an interactive shell substate.
pub fn dispatch_only_blocked_guard_reason(blocker_reason: &str) -> RoutedReopenGuardReason {
    if is_interactive_shell_substate_reason(blocker_reason) {
        RoutedReopenGuardReason::BlockedInInteractiveSubstate
    } else {
        RoutedReopenGuardReason::DispatchOnlyBusyActorNotReady
    }
}

pub fn prompt_ready_barrier_failed_event(reason: RoutedReopenGuardReason) -> FlowEvent {
    FlowEvent::new(
        FlowName::RoutedReopen,
        FlowStage::PromptReadyBarrier,
        FlowOutcome::FailedClosed,
    )
    .with_reason(reason.as_str())
}

pub fn dispatch_proof_failed_event(reason: RoutedReopenGuardReason) -> FlowEvent {
    FlowEvent::new(
        FlowName::RoutedReopen,
        FlowStage::DispatchProof,
        FlowOutcome::FailedClosed,
    )
    .with_reason(reason.as_str())
}

pub fn log_prompt_ready_barrier_failed(file: &Path, reason: RoutedReopenGuardReason) {
    super::proof::log_flow_event(file, prompt_ready_barrier_failed_event(reason));
}

pub fn log_dispatch_proof_failed(file: &Path, reason: RoutedReopenGuardReason) {
    super::proof::log_flow_event(file, dispatch_proof_failed_event(reason));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_substate_gets_dedicated_guard_reason() {
        // #snrun: a refused dispatch-only reopen on an interactive shell substate
        // (reverse/history search) must carry the dedicated guard reason, distinct
        // from the generic busy-actor reason.
        for reason in [
            "interactive shell reverse-i-search",
            "interactive shell history search",
            "  interactive shell reverse-i-search",
        ] {
            assert!(is_interactive_shell_substate_reason(reason), "{reason}");
            assert_eq!(
                dispatch_only_blocked_guard_reason(reason),
                RoutedReopenGuardReason::BlockedInInteractiveSubstate,
            );
            assert_eq!(
                dispatch_only_blocked_guard_reason(reason).as_str(),
                "blocked_in_interactive_substate",
            );
        }
        for reason in [
            "active codex turn",
            "queued draft in composer",
            "active claude turn",
        ] {
            assert!(!is_interactive_shell_substate_reason(reason), "{reason}");
            assert_eq!(
                dispatch_only_blocked_guard_reason(reason),
                RoutedReopenGuardReason::DispatchOnlyBusyActorNotReady,
            );
        }
    }

    #[test]
    fn starting_actor_waits_for_prompt_ready_barrier() {
        let outcome = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Starting,
            prompt_ready: false,
            has_prompt_bearing_work: true,
            mode: ReopenMode::DispatchOnly,
            degraded_authority: false,
            dispatch_eligible: true,
        });

        assert_eq!(outcome.decision, RouteDecision::WaitForReady);
        assert_eq!(outcome.reason, "starting_requires_prompt_ready_barrier");
    }

    #[test]
    fn dispatch_only_busy_actor_fails_closed() {
        let outcome = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Busy,
            prompt_ready: false,
            has_prompt_bearing_work: true,
            mode: ReopenMode::DispatchOnly,
            degraded_authority: false,
            dispatch_eligible: true,
        });

        assert_eq!(outcome.decision, RouteDecision::FailClosed);
    }

    #[test]
    fn managed_busy_actor_can_queue_once() {
        let outcome = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Busy,
            prompt_ready: false,
            has_prompt_bearing_work: true,
            mode: ReopenMode::Managed,
            degraded_authority: false,
            dispatch_eligible: true,
        });

        assert_eq!(outcome.decision, RouteDecision::ReuseReady);
    }

    #[test]
    fn busy_projection_repaired_only_with_proven_ready_prompt() {
        // #snrun: a Busy projection + proven live ready prompt is a stale busy
        // projection → repair to Ready and dispatch. Busy without a proven ready
        // prompt is genuinely busy → leave it (fail closed / queue).
        assert!(
            busy_projection_repaired_by_ready_prompt(ActorDispatchState::Busy, true),
            "busy + proven ready prompt is a stale projection to repair"
        );
        assert!(
            !busy_projection_repaired_by_ready_prompt(ActorDispatchState::Busy, false),
            "busy without a proven ready prompt must not be repaired (stays fail-closed)"
        );
        // Non-busy states are never repaired through this path.
        for state in [
            ActorDispatchState::Ready,
            ActorDispatchState::Starting,
            ActorDispatchState::WaitingInput,
            ActorDispatchState::Blocked,
            ActorDispatchState::Closed,
            ActorDispatchState::Missing,
        ] {
            assert!(
                !busy_projection_repaired_by_ready_prompt(state, true),
                "only Busy is repaired by this rule; {state:?} must not be"
            );
        }
    }

    #[test]
    fn repaired_busy_actor_dispatches_directly_in_dispatch_only_mode() {
        // After the route promotes a stale-busy actor to Ready (the repair), the
        // pure decision + classification flow dispatches to the proven-ready pane
        // instead of queuing — the end state the #snrun fix produces.
        let outcome = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Ready,
            prompt_ready: true,
            has_prompt_bearing_work: true,
            mode: ReopenMode::DispatchOnly,
            degraded_authority: false,
            dispatch_eligible: true,
        });
        assert_eq!(outcome.decision, RouteDecision::ReuseReady);
        assert_eq!(
            classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
                mode: ReopenMode::DispatchOnly,
                actor_state: ActorDispatchState::Ready,
                has_prompt_bearing_work: true,
                reopen_decision: outcome.decision,
            }),
            AuthoritativeActorDispatchAction::DispatchOnlyDirectPane
        );
    }

    #[test]
    fn authoritative_actor_dispatch_action_classifies_delivery_boundary() {
        assert_eq!(
            classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
                mode: ReopenMode::DispatchOnly,
                actor_state: ActorDispatchState::Ready,
                has_prompt_bearing_work: true,
                reopen_decision: RouteDecision::ReuseReady,
            }),
            AuthoritativeActorDispatchAction::DispatchOnlyDirectPane
        );
        assert_eq!(
            classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
                mode: ReopenMode::Managed,
                actor_state: ActorDispatchState::Ready,
                has_prompt_bearing_work: true,
                reopen_decision: RouteDecision::ReuseReady,
            }),
            AuthoritativeActorDispatchAction::ManagedSupervisorIpc
        );
        assert_eq!(
            classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
                mode: ReopenMode::Managed,
                actor_state: ActorDispatchState::Busy,
                has_prompt_bearing_work: true,
                reopen_decision: RouteDecision::ReuseReady,
            }),
            AuthoritativeActorDispatchAction::ManagedSupervisorQueue
        );
        assert_eq!(
            classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
                mode: ReopenMode::DispatchOnly,
                actor_state: ActorDispatchState::Busy,
                has_prompt_bearing_work: true,
                reopen_decision: RouteDecision::FailClosed,
            }),
            AuthoritativeActorDispatchAction::DispatchOnlyBusyQueue
        );
        assert_eq!(
            classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
                mode: ReopenMode::DispatchOnly,
                actor_state: ActorDispatchState::WaitingInput,
                has_prompt_bearing_work: true,
                reopen_decision: RouteDecision::FailClosed,
            }),
            AuthoritativeActorDispatchAction::RecoverDispatchOnlyWaitingInput
        );
        assert_eq!(
            classify_authoritative_actor_dispatch_action(AuthoritativeActorDispatchActionFacts {
                mode: ReopenMode::Managed,
                actor_state: ActorDispatchState::Blocked,
                has_prompt_bearing_work: false,
                reopen_decision: RouteDecision::FailClosed,
            }),
            AuthoritativeActorDispatchAction::FocusOnly
        );
    }

    #[test]
    fn dispatch_only_focus_only_fails_closed_only_for_busy_actor() {
        // `#jb-run-agent-doc-command-route-miss`: a plain dispatch-only reopen on a
        // BUSY actor classifies as FocusOnly but must fail closed (not silently
        // succeed) so the IDE surfaces a "still running" notification.
        assert!(dispatch_only_focus_only_should_fail_closed(
            ReopenMode::DispatchOnly,
            ActorDispatchState::Busy
        ));
        // Managed reopens keep the focus-only success.
        assert!(!dispatch_only_focus_only_should_fail_closed(
            ReopenMode::Managed,
            ActorDispatchState::Busy
        ));
        // Non-busy blocker states keep their own recovery/terminal handling.
        for state in [
            ActorDispatchState::WaitingInput,
            ActorDispatchState::Blocked,
            ActorDispatchState::Closed,
            ActorDispatchState::Starting,
            ActorDispatchState::Ready,
        ] {
            assert!(!dispatch_only_focus_only_should_fail_closed(
                ReopenMode::DispatchOnly,
                state
            ));
        }
    }

    #[test]
    fn prompt_ready_barrier_requires_state_prompt_and_eligibility() {
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Starting,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Continue
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Ready,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Continue
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Ready,
                prompt_ready: true,
                dispatch_eligible: false,
            }),
            PromptReadyBarrierDecision::Continue
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Ready,
                prompt_ready: true,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Ready
        );
    }

    #[test]
    fn prompt_ready_barrier_repairs_busy_projection_with_proven_ready_prompt() {
        // `#run-agent-doc-busy-ready-deadlock`: a Busy projection with a proven
        // current-generation ready prompt must resolve the wait barrier to Ready
        // immediately (the busy-projection repair) instead of spinning to the
        // --wait-for-ready deadline.
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Busy,
                prompt_ready: true,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Ready
        );
        // Busy without a proven ready prompt stays fail-closed (keep waiting).
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Busy,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Continue
        );
        // A proven ready prompt without dispatch eligibility must not dispatch.
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Busy,
                prompt_ready: true,
                dispatch_eligible: false,
            }),
            PromptReadyBarrierDecision::Continue
        );
    }

    #[test]
    fn prompt_ready_barrier_surfaces_terminal_states() {
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Closed,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Blocked,
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
    }

    #[test]
    fn authoritative_ready_facts_own_log_shape_and_barrier_input() {
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%42".to_string(),
            generation: 7,
            actor_state: ActorDispatchState::Ready,
            supervisor_health: "healthy".to_string(),
            runtime_state: "ready".to_string(),
            prompt_ready: true,
            last_transition_reason: "dispatch_bind".to_string(),
            last_transition_caller: "route".to_string(),
        };

        assert_eq!(
            classify_authoritative_prompt_ready_barrier(AuthoritativePromptReadyBarrierFacts {
                ready_facts: &facts,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Ready
        );
        assert!(facts.log_fields().contains("generation=7"));
        assert!(
            starting_actor_ready_log_line(
                "/tmp/doc.md",
                "codex",
                Duration::from_millis(12),
                &facts
            )
            .contains("route_starting_actor_ready")
        );
        assert!(
            starting_actor_not_ready_log_line(StartingActorLogFacts {
                file_display: "/tmp/doc.md",
                harness_binary: "codex",
                timeout: Duration::from_secs(8),
                elapsed: Duration::from_secs(8),
                ready_facts: &facts,
            })
            .contains("timeout_ms=8000")
        );
    }

    #[test]
    fn degraded_authority_requires_current_pane_binding() {
        let facts = DegradedAuthoritativeActorFacts {
            actor_pane: "%42",
            transition_caller: "route",
            transition_reason: "dispatch_bind",
            registered_pane: Some("%42"),
            live_owner_pane: None,
        };
        assert!(can_use_degraded_authoritative_actor(facts));

        assert!(can_use_degraded_authoritative_actor(
            DegradedAuthoritativeActorFacts {
                registered_pane: None,
                live_owner_pane: Some("%42"),
                ..facts
            }
        ));
        assert!(!can_use_degraded_authoritative_actor(
            DegradedAuthoritativeActorFacts {
                registered_pane: Some("%99"),
                live_owner_pane: Some("%99"),
                ..facts
            }
        ));
        assert!(!can_use_degraded_authoritative_actor(
            DegradedAuthoritativeActorFacts {
                transition_caller: "register",
                transition_reason: "register",
                registered_pane: Some("%42"),
                live_owner_pane: Some("%42"),
                ..facts
            }
        ));
    }

    #[test]
    fn degraded_direct_submit_log_names_supervisor_reason() {
        let message = degraded_authoritative_actor_direct_submit_log_message(
            DegradedAuthoritativeActorDirectSubmit {
                file_display: "/tmp/doc.md",
                pane_id: "%42",
                harness_binary: "codex",
                generation: 2,
                record_state: "ready",
                supervisor_health: "no_socket",
                runtime_actor_state: "missing",
                reason: "supervisor health is no_socket",
            },
        );

        assert!(message.contains("route_dispatch_only_authoritative_degraded_direct_pane"));
        assert!(message.contains("supervisor_health=no_socket"));
        assert!(message.contains("runtime_actor_state=missing"));
        assert!(message.contains("reason=supervisor health is no_socket"));
    }

    #[test]
    fn runtime_guard_requires_healthy_supervisor_with_actor_state() {
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: ActorRuntimeHealth::Healthy,
                actor_state_present: true,
            })
            .is_none()
        );
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: ActorRuntimeHealth::NoSocket,
                actor_state_present: true,
            })
            .unwrap()
            .contains("no_socket")
        );
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: ActorRuntimeHealth::Healthy,
                actor_state_present: false,
            })
            .unwrap()
            .contains("missing")
        );
    }

    #[test]
    fn accepted_only_dispatch_start_proof_fails_only_when_required() {
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::CommandAcceptedOnly, true),
            DispatchStartProofDecision::FailClosedAcceptedOnly
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::CommandAcceptedOnly, false),
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::HookPromptMatched, true),
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::PaneStateChanged, true),
            DispatchStartProofDecision::Accepted
        );
        let classification = classify_dispatch_start_proof(DispatchStartProofFacts {
            proof: RoutedDispatchStartProof::HookStateAdvanced,
            dispatch_start_proof_required: true,
        });
        assert_eq!(
            classification.decision,
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(classification.typed_proof, DispatchProof::DispatchStarted);
    }

    #[test]
    fn direct_submit_outcome_separates_acceptance_from_dispatch_proof() {
        assert_eq!(
            direct_pane_submit_outcome(DirectPaneSubmitStatus::Accepted, None),
            "accepted"
        );
        assert_eq!(
            direct_pane_submit_outcome(DirectPaneSubmitStatus::TimedOut, None),
            "acceptance_unobserved"
        );
        assert_eq!(
            direct_pane_submit_outcome(
                DirectPaneSubmitStatus::TimedOut,
                Some(RoutedDispatchStartProof::PaneStateChanged),
            ),
            "acceptance_unobserved_dispatch_proven"
        );
    }

    #[test]
    fn routed_dispatch_start_timeout_uses_opencode_redraw_budget() {
        assert_eq!(
            routed_dispatch_start_timeout_for_binary(Some("opencode"), false),
            Duration::from_secs(15)
        );
        assert_eq!(
            routed_dispatch_start_timeout_for_binary(Some("codex"), false),
            Duration::from_secs(10)
        );
        assert_eq!(
            routed_dispatch_start_timeout_for_binary(Some("opencode"), true),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn dispatch_only_proof_policy_requires_hook_visible_codex_and_opencode() {
        assert!(dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "codex",
                codex_dispatch_start_tracking_enabled: true,
            }
        ));
        assert!(!dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "codex",
                codex_dispatch_start_tracking_enabled: false,
            }
        ));
        assert!(!dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "opencode",
                codex_dispatch_start_tracking_enabled: false,
            }
        ));
        assert!(!dispatch_only_dispatch_start_proof_required(
            DispatchOnlyProofPolicyFacts {
                harness_binary: "claude",
                codex_dispatch_start_tracking_enabled: false,
            }
        ));
    }

    #[test]
    fn retry_budgets_are_centralized_by_harness_and_test_mode() {
        assert_eq!(
            authoritative_actor_ready_retry_budget(Some("codex"), true),
            RetryBudget::new(Duration::from_millis(400), Duration::from_millis(100))
        );
        assert_eq!(
            dispatch_only_starting_pane_ready_retry_budget(Some("codex"), true),
            RetryBudget::new(Duration::from_millis(250), Duration::from_millis(100))
        );
        assert_eq!(
            dispatch_only_starting_pane_recovery_retry_budget(Some("opencode"), false).timeout,
            Duration::from_secs(15)
        );
    }

    #[test]
    fn dispatch_only_sent_messages_preserve_proof_scope() {
        let facts = DispatchOnlyProofOutcomeFacts {
            file_display: "/tmp/doc.md",
            pane: "%7",
            harness_binary: "codex",
            delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
            dispatch_start: RoutedDispatchStartProof::CommandAcceptedOnly,
            timeout_secs: 10,
        };

        let log = dispatch_only_sent_log_message(facts);
        assert!(log.contains("submit_mode=tmux_literal_text_enter_key"));
        assert!(log.contains("proof=accepted"));
        assert!(log.contains("proof_scope=accepted_only"));

        let refusal = accepted_only_dispatch_start_refusal_message(facts);
        assert!(refusal.contains("tmux_literal_text_enter_key"));
        assert!(refusal.contains("only pane-input acceptance proof was available"));
        assert!(refusal.contains("treating this as not dispatched"));
    }

    #[test]
    fn dispatch_only_direct_submit_mode_is_harness_specific() {
        assert_eq!(
            DispatchOnlyReopenDelivery::DirectPaneSubmit.submit_mode_for_harness("codex"),
            "tmux_literal_text_enter_key"
        );
        assert_eq!(
            DispatchOnlyReopenDelivery::DirectPaneSubmit.submit_mode_for_harness("opencode"),
            "tmux_literal_text_enter_key"
        );
        assert_eq!(
            DispatchOnlyReopenDelivery::DirectPaneSubmit.submit_mode_for_harness("claude"),
            "tmux_literal_text_enter_key"
        );
        assert_eq!(
            DispatchOnlyReopenDelivery::SupervisorIpcOnce.submit_mode_for_harness("codex"),
            "supervisor_normalized_submit"
        );
    }

    #[test]
    fn busy_pane_auto_fix_decision_prefers_explicit_retry_evidence() {
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: false,
                fix_made_changes: false,
                supervisor_healthy: true,
                restarted_supervisor: false,
            }),
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart
        );
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: true,
                fix_made_changes: false,
                supervisor_healthy: false,
                restarted_supervisor: false,
            }),
            BusyPaneAutoFixOutcome::RetryRoute
        );
        assert_eq!(
            busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: false,
                fix_made_changes: false,
                supervisor_healthy: false,
                restarted_supervisor: true,
            }),
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart
        );
    }

    #[test]
    fn route_failure_events_are_owned_by_routed_reopen_flow() {
        let prompt_event =
            prompt_ready_barrier_failed_event(RoutedReopenGuardReason::StartingActorNotReady);
        assert_eq!(prompt_event.flow, FlowName::RoutedReopen);
        assert_eq!(prompt_event.stage, FlowStage::PromptReadyBarrier);
        assert_eq!(prompt_event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(
            prompt_event.reason.as_deref(),
            Some("starting_actor_not_ready")
        );

        let proof_event =
            dispatch_proof_failed_event(RoutedReopenGuardReason::AcceptedOnlyDispatchStartProof);
        assert_eq!(proof_event.flow, FlowName::RoutedReopen);
        assert_eq!(proof_event.stage, FlowStage::DispatchProof);
        assert_eq!(proof_event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(
            proof_event.reason.as_deref(),
            Some("accepted_only_dispatch_start_proof")
        );
    }
}
