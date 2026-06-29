//! Pure controller dispatch admission helpers.

use std::time::Duration;

pub const DISPATCH_COALESCED_IN_FLIGHT_MARKER: &str = "failed_stage=coalesced_in_flight";
pub const DISPATCH_STALE_GENERATION_REDIRECT_MARKER: &str = "stale_generation_redirect";
pub const DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER: &str = "supervisor_restart_redirect";
pub const STALE_QUEUE_PAUSE_INVARIANT_ID: &str = "stale_queue_pause";
pub const STALE_QUEUE_PAUSE_NEXT_ACTION: &str = "restart_supervisor_once_and_retry";
pub const DISPATCH_RECOVERY_OUTCOME_CONTRACT_VERSION: &str = "binary-outcome-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DispatchRecoveryOutcomeClass {
    Recoverable,
}

impl DispatchRecoveryOutcomeClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recoverable => "recoverable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DispatchRecoveryOutcome {
    pub contract_version: &'static str,
    pub class: DispatchRecoveryOutcomeClass,
    pub invariant_id: &'static str,
    pub proof_marker: &'static str,
    pub next_action: &'static str,
}

impl DispatchRecoveryOutcome {
    pub const fn stale_queue_pause() -> Self {
        Self {
            contract_version: DISPATCH_RECOVERY_OUTCOME_CONTRACT_VERSION,
            class: DispatchRecoveryOutcomeClass::Recoverable,
            invariant_id: STALE_QUEUE_PAUSE_INVARIANT_ID,
            proof_marker: DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER,
            next_action: STALE_QUEUE_PAUSE_NEXT_ACTION,
        }
    }

    pub fn log_fields(&self) -> String {
        format!(
            "binary_outcome={} invariant={} proof_marker={} next_action={}",
            self.class.as_str(),
            self.invariant_id,
            self.proof_marker,
            self.next_action
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StaleQueuePauseRecovery {
    pub stale_pid: u32,
    pub outcome: DispatchRecoveryOutcome,
}

impl StaleQueuePauseRecovery {
    pub fn new(stale_pid: u32) -> Self {
        Self {
            stale_pid,
            outcome: DispatchRecoveryOutcome::stale_queue_pause(),
        }
    }
}

pub fn dispatch_should_coalesce_in_flight(
    in_flight_same_cycle: bool,
    operator_driven: bool,
) -> bool {
    in_flight_same_cycle && !operator_driven
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchActorState {
    Ready,
    Busy,
    Other,
}

pub const STARTING_ACTOR_TIMEOUT_REASON: &str = "starting_actor_timeout";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartingTimeoutActorFacts<'a> {
    pub actor_blocked: bool,
    pub last_transition_reason: &'a str,
    pub prompt_ready: bool,
}

pub fn actor_blocked_by_starting_timeout(facts: StartingTimeoutActorFacts<'_>) -> bool {
    facts.actor_blocked && facts.last_transition_reason == STARTING_ACTOR_TIMEOUT_REASON
}

pub fn starting_timeout_blocked_actor_can_recover(facts: StartingTimeoutActorFacts<'_>) -> bool {
    actor_blocked_by_starting_timeout(facts) && facts.prompt_ready
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchRuntimeHealth {
    Healthy,
    Restartable,
    Halted { restart_count: u32 },
    Unreachable,
    NoSocket,
}

impl DispatchRuntimeHealth {
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
pub struct AuthoritativeRuntimeFacts {
    pub health: DispatchRuntimeHealth,
    pub actor_state_present: bool,
}

pub fn authoritative_actor_dispatch_guard_reason(
    facts: AuthoritativeRuntimeFacts,
) -> Option<String> {
    if facts.health != DispatchRuntimeHealth::Healthy {
        return Some(format!("supervisor health is {}", facts.health.label()));
    }
    if !facts.actor_state_present {
        return Some("supervisor actor_state is missing".to_string());
    }
    None
}

pub fn dispatch_only_busy_should_wait_for_ready(
    dispatch_only: bool,
    actor_state: DispatchActorState,
    has_queue_fallback: bool,
    pane_active_turn_busy: bool,
) -> bool {
    dispatch_only
        && actor_state == DispatchActorState::Busy
        && !has_queue_fallback
        && !pane_active_turn_busy
}

pub fn dispatch_only_should_probe_active_turn_cue(
    dispatch_only: bool,
    actor_state: DispatchActorState,
    prompt_context_present: bool,
    has_existing_inactive_queue_fallback: bool,
) -> bool {
    if !dispatch_only {
        return false;
    }
    match actor_state {
        DispatchActorState::Ready => true,
        DispatchActorState::Busy => {
            !prompt_context_present && !has_existing_inactive_queue_fallback
        }
        DispatchActorState::Other => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutedDispatchStartProof {
    CommandAcceptedOnly,
    DispatchStartUnproven,
    HookPromptMatched,
    HookStateAdvanced,
    PaneStateChanged,
}

impl RoutedDispatchStartProof {
    pub const fn dispatch_stage_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "accepted",
            Self::DispatchStartUnproven => "accepted_without_dispatch_start_proof",
            Self::HookPromptMatched => "consumed",
            Self::HookStateAdvanced => "submitted",
            Self::PaneStateChanged => "pane_state_changed",
        }
    }

    pub const fn proof_scope_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly | Self::DispatchStartUnproven => "accepted_only",
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
            Self::DispatchStartUnproven => "accepted-only; harness dispatch-start proof timed out",
            Self::HookPromptMatched => "dispatch-start proof matched the routed prompt",
            Self::HookStateAdvanced => "dispatch-start proof observed newer harness prompt state",
            Self::PaneStateChanged => "dispatch-start proof observed pane state leave idle chrome",
        }
    }

    pub const fn startup_miss_label(self) -> &'static str {
        match self {
            Self::CommandAcceptedOnly => "acceptance",
            Self::DispatchStartUnproven => "accepted-without-dispatch-proof",
            Self::HookPromptMatched => "consumption",
            Self::HookStateAdvanced => "submission",
            Self::PaneStateChanged => "pane-state-change",
        }
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
    if proof == RoutedDispatchStartProof::DispatchStartUnproven
        || proof == RoutedDispatchStartProof::CommandAcceptedOnly && dispatch_start_proof_required
    {
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
}

pub fn classify_dispatch_start_proof(
    facts: DispatchStartProofFacts,
) -> DispatchStartProofClassification {
    DispatchStartProofClassification {
        decision: decide_dispatch_start_proof(facts.proof, facts.dispatch_start_proof_required),
    }
}

pub fn dispatch_only_dispatch_start_proof_required(_harness_binary: &str) -> bool {
    false
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
pub enum DirectPaneSubmitStatus {
    Accepted,
    TimedOut,
}

pub fn direct_pane_submit_acceptance_timeout() -> Duration {
    Duration::from_secs(1)
}

pub fn direct_pane_submit_acceptance_budget() -> Duration {
    // tmux/control-mode delivery can spend the whole acceptance window plus a
    // final capture poll before pane input disappears. Keep the budget above
    // that window so "over_budget" means slower than the path can observe.
    Duration::from_millis(1500)
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

/// Decision for the route-dispatch drain retry loop after a mid-drain `repair`
/// plus `session_check` failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchDrainRetryDecision {
    /// A concurrent finalize in another process closed the cycle, so the drain is
    /// satisfied and routed dispatch may proceed.
    ConcurrentlyClosed,
    /// The cycle advanced concurrently and attempts remain, so back off and retry.
    Retry,
    /// No concurrent progress was observed, or attempts are exhausted, so fail closed.
    GiveUp,
}

/// Classify whether a mid-drain failure is a transient concurrent-finalize race.
///
/// `original_*` is the cycle observed when the drain started; `reloaded` is the
/// cycle re-read after the failed check as `(cycle_id, phase, is_open)`, or
/// `None` when no open cycle remains on disk.
pub fn dispatch_drain_retry_decision(
    original_cycle_id: &str,
    original_phase: agent_doc_turn::CyclePhase,
    reloaded: Option<(&str, agent_doc_turn::CyclePhase, bool)>,
    attempt: u32,
    max_attempts: u32,
) -> DispatchDrainRetryDecision {
    match reloaded {
        None => DispatchDrainRetryDecision::ConcurrentlyClosed,
        Some((_, _, false)) => DispatchDrainRetryDecision::ConcurrentlyClosed,
        Some((cycle_id, phase, true)) => {
            let progressed = cycle_id != original_cycle_id || phase != original_phase;
            if progressed && attempt + 1 < max_attempts {
                DispatchDrainRetryDecision::Retry
            } else {
                DispatchDrainRetryDecision::GiveUp
            }
        }
    }
}

pub fn dispatch_error_is_coalesced(message: &str) -> bool {
    message.contains(DISPATCH_COALESCED_IN_FLIGHT_MARKER)
}

pub fn dispatch_command_kind_is_operator_reopen(command_kind: &str) -> bool {
    matches!(command_kind, "managed_reopen" | "dispatch_only_reopen")
}

pub fn dispatch_error_stale_generation_redirect_target(message: &str) -> Option<u64> {
    if !message.contains(DISPATCH_STALE_GENERATION_REDIRECT_MARKER) {
        return None;
    }
    message.split("retry_generation=").nth(1).and_then(|rest| {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse::<u64>().ok()
    })
}

pub fn pause_reason_is_stale_supervisor_churn_stop(reason: &str) -> bool {
    let r = reason.to_ascii_lowercase();
    if r.contains("supervisor_binary_stale")
        || r.contains("stale supervisor")
        || r.contains("stale host supervisor")
        || r.contains("stale route-owned supervisor")
    {
        return true;
    }
    let is_churn_stop = r.contains("churn-stop") || r.contains("churn_stop");
    is_churn_stop && r.contains("needs operator recycle")
}

pub fn stale_supervisor_pid_from_pause_reason(reason: &str) -> Option<u32> {
    let lower = reason.to_ascii_lowercase();
    let rest = lower.split("pid").nth(1)?;
    let digits: String = rest
        .trim_start_matches([' ', '=', ':', '#'])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

pub fn stale_queue_pause_pid_from_dispatch_error(message: &str) -> Option<u32> {
    if message.contains(DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER) {
        let pid = message
            .split("stale_pid=")
            .nth(1)
            .map(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .and_then(|digits| digits.parse::<u32>().ok())
            .unwrap_or(0);
        return Some(pid);
    }
    if message.contains("failed_stage=queue_paused")
        && pause_reason_is_stale_supervisor_churn_stop(message)
    {
        return Some(stale_supervisor_pid_from_pause_reason(message).unwrap_or(0));
    }
    None
}

pub fn stale_queue_pause_recovery_from_dispatch_error(
    message: &str,
) -> Option<StaleQueuePauseRecovery> {
    stale_queue_pause_pid_from_dispatch_error(message).map(StaleQueuePauseRecovery::new)
}

pub fn spent_preset_id_from_pause_reason(reason: &str) -> Option<String> {
    let marker = " preset head is spent";
    let lower = reason.to_ascii_lowercase();
    if let Some(idx) = lower.find(marker) {
        let candidate = lower[..idx]
            .rsplit(|ch: char| ch.is_whitespace() || matches!(ch, ':' | ';' | ',' | '(' | '['))
            .next()?
            .trim()
            .trim_start_matches('#');
        if valid_preset_pause_id(candidate) {
            return Some(candidate.to_string());
        }
    }
    preset_token_unserviceable_id_from_pause_reason(&lower)
}

fn preset_token_unserviceable_id_from_pause_reason(lower_reason: &str) -> Option<String> {
    if !lower_reason.contains("preset-token") || !lower_reason.contains("un-drainable") {
        return None;
    }
    let (_, rest) = lower_reason.split_once("(#")?;
    let candidate: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    if valid_preset_pause_id(&candidate) {
        Some(candidate)
    } else {
        None
    }
}

fn valid_preset_pause_id(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_dispatch_coalesces_only_when_same_cycle_in_flight() {
        assert!(dispatch_should_coalesce_in_flight(true, false));
        assert!(!dispatch_should_coalesce_in_flight(true, true));
        assert!(!dispatch_should_coalesce_in_flight(false, false));
        assert!(!dispatch_should_coalesce_in_flight(false, true));
    }

    #[test]
    fn dispatch_only_busy_wait_skips_when_queue_fallback_exists() {
        assert!(!dispatch_only_busy_should_wait_for_ready(
            true,
            DispatchActorState::Busy,
            true,
            false
        ));
        assert!(dispatch_only_busy_should_wait_for_ready(
            true,
            DispatchActorState::Busy,
            false,
            false
        ));
        assert!(!dispatch_only_busy_should_wait_for_ready(
            false,
            DispatchActorState::Busy,
            false,
            false
        ));
        assert!(!dispatch_only_busy_should_wait_for_ready(
            true,
            DispatchActorState::Ready,
            false,
            false
        ));
    }

    #[test]
    fn dispatch_only_busy_wait_skips_on_proven_active_turn() {
        assert!(!dispatch_only_busy_should_wait_for_ready(
            true,
            DispatchActorState::Busy,
            false,
            true
        ));
    }

    #[test]
    fn dispatch_only_active_turn_probe_covers_ready_and_busy_no_fallback() {
        assert!(dispatch_only_should_probe_active_turn_cue(
            true,
            DispatchActorState::Ready,
            false,
            false
        ));
        assert!(dispatch_only_should_probe_active_turn_cue(
            true,
            DispatchActorState::Ready,
            true,
            true
        ));
        assert!(dispatch_only_should_probe_active_turn_cue(
            true,
            DispatchActorState::Busy,
            false,
            false
        ));
        assert!(!dispatch_only_should_probe_active_turn_cue(
            true,
            DispatchActorState::Busy,
            true,
            false
        ));
        assert!(!dispatch_only_should_probe_active_turn_cue(
            true,
            DispatchActorState::Busy,
            false,
            true
        ));
        assert!(!dispatch_only_should_probe_active_turn_cue(
            false,
            DispatchActorState::Ready,
            false,
            false
        ));
        assert!(!dispatch_only_should_probe_active_turn_cue(
            true,
            DispatchActorState::Other,
            false,
            false
        ));
    }

    #[test]
    fn authoritative_runtime_guard_requires_healthy_supervisor_with_actor_state() {
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: DispatchRuntimeHealth::Healthy,
                actor_state_present: true,
            })
            .is_none()
        );
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: DispatchRuntimeHealth::NoSocket,
                actor_state_present: true,
            })
            .unwrap()
            .contains("no_socket")
        );
        assert!(
            authoritative_actor_dispatch_guard_reason(AuthoritativeRuntimeFacts {
                health: DispatchRuntimeHealth::Healthy,
                actor_state_present: false,
            })
            .unwrap()
            .contains("missing")
        );
    }

    #[test]
    fn dispatch_start_proof_fails_only_when_required_or_unproven() {
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::CommandAcceptedOnly, true),
            DispatchStartProofDecision::FailClosedAcceptedOnly
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::CommandAcceptedOnly, false),
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::DispatchStartUnproven, false),
            DispatchStartProofDecision::FailClosedAcceptedOnly
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::HookPromptMatched, true),
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(
            decide_dispatch_start_proof(RoutedDispatchStartProof::PaneStateChanged, true),
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(
            classify_dispatch_start_proof(DispatchStartProofFacts {
                proof: RoutedDispatchStartProof::HookStateAdvanced,
                dispatch_start_proof_required: true,
            })
            .decision,
            DispatchStartProofDecision::Accepted
        );
        assert_eq!(
            RoutedDispatchStartProof::HookStateAdvanced.dispatch_stage_label(),
            "submitted"
        );
        assert_eq!(
            RoutedDispatchStartProof::HookStateAdvanced.proof_scope_label(),
            "dispatch_start"
        );
        assert_eq!(
            RoutedDispatchStartProof::HookStateAdvanced.startup_miss_label(),
            "submission"
        );
    }

    #[test]
    fn dispatch_only_start_proof_policy_accepts_enter_delivery_for_all_harnesses() {
        assert!(!dispatch_only_dispatch_start_proof_required("codex"));
        assert!(!dispatch_only_dispatch_start_proof_required("opencode"));
        assert!(!dispatch_only_dispatch_start_proof_required("claude"));
    }

    #[test]
    fn starting_timeout_recovery_requires_blocked_timeout_and_prompt_ready() {
        let blocked_timeout_ready = StartingTimeoutActorFacts {
            actor_blocked: true,
            last_transition_reason: STARTING_ACTOR_TIMEOUT_REASON,
            prompt_ready: true,
        };
        assert!(actor_blocked_by_starting_timeout(blocked_timeout_ready));
        assert!(starting_timeout_blocked_actor_can_recover(
            blocked_timeout_ready
        ));

        assert!(!starting_timeout_blocked_actor_can_recover(
            StartingTimeoutActorFacts {
                prompt_ready: false,
                ..blocked_timeout_ready
            }
        ));
        assert!(!actor_blocked_by_starting_timeout(
            StartingTimeoutActorFacts {
                actor_blocked: false,
                ..blocked_timeout_ready
            }
        ));
        assert!(!actor_blocked_by_starting_timeout(
            StartingTimeoutActorFacts {
                last_transition_reason: "ordinary_block",
                ..blocked_timeout_ready
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
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("opencode"), false),
            Duration::from_secs(15)
        );
        assert_eq!(
            dispatch_only_starting_pane_recovery_retry_budget(Some("opencode"), false).timeout,
            Duration::from_secs(15)
        );
        assert_eq!(
            dispatch_only_starting_pane_recovery_timeout_for_binary(Some("claude"), false),
            Duration::from_secs(10)
        );
        assert_eq!(
            dispatch_only_starting_pane_recovery_timeout_for_binary(Some("codex"), false),
            Duration::from_secs(8)
        );
    }

    #[test]
    fn direct_pane_submit_budget_allows_acceptance_poll_slack() {
        assert_eq!(
            direct_pane_submit_acceptance_timeout(),
            Duration::from_secs(1)
        );
        assert_eq!(
            direct_pane_submit_acceptance_budget(),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn direct_pane_submit_outcome_separates_acceptance_from_dispatch_proof() {
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
                Some(RoutedDispatchStartProof::HookStateAdvanced),
            ),
            "acceptance_unobserved_dispatch_proven"
        );
    }

    #[test]
    fn drain_retry_concurrent_close_when_cycle_gone() {
        assert_eq!(
            dispatch_drain_retry_decision(
                "cyc-1",
                agent_doc_turn::CyclePhase::PreflightStarted,
                None,
                0,
                3
            ),
            DispatchDrainRetryDecision::ConcurrentlyClosed
        );
    }

    #[test]
    fn drain_retry_concurrent_close_when_cycle_no_longer_open() {
        assert_eq!(
            dispatch_drain_retry_decision(
                "cyc-1",
                agent_doc_turn::CyclePhase::PreflightStarted,
                Some(("cyc-1", agent_doc_turn::CyclePhase::Committed, false)),
                0,
                3
            ),
            DispatchDrainRetryDecision::ConcurrentlyClosed
        );
    }

    #[test]
    fn drain_retry_retries_when_phase_advanced_and_attempts_remain() {
        assert_eq!(
            dispatch_drain_retry_decision(
                "cyc-1",
                agent_doc_turn::CyclePhase::PreflightStarted,
                Some(("cyc-1", agent_doc_turn::CyclePhase::WriteApplied, true)),
                0,
                3
            ),
            DispatchDrainRetryDecision::Retry
        );
    }

    #[test]
    fn drain_retry_retries_when_cycle_id_changed_and_attempts_remain() {
        assert_eq!(
            dispatch_drain_retry_decision(
                "cyc-1",
                agent_doc_turn::CyclePhase::PreflightStarted,
                Some(("cyc-2", agent_doc_turn::CyclePhase::PreflightStarted, true)),
                1,
                3
            ),
            DispatchDrainRetryDecision::Retry
        );
    }

    #[test]
    fn drain_retry_gives_up_when_no_progress_observed() {
        assert_eq!(
            dispatch_drain_retry_decision(
                "cyc-1",
                agent_doc_turn::CyclePhase::PreflightStarted,
                Some(("cyc-1", agent_doc_turn::CyclePhase::PreflightStarted, true)),
                0,
                3
            ),
            DispatchDrainRetryDecision::GiveUp
        );
    }

    #[test]
    fn drain_retry_gives_up_when_attempts_exhausted_despite_progress() {
        assert_eq!(
            dispatch_drain_retry_decision(
                "cyc-1",
                agent_doc_turn::CyclePhase::PreflightStarted,
                Some(("cyc-1", agent_doc_turn::CyclePhase::WriteApplied, true)),
                2,
                3
            ),
            DispatchDrainRetryDecision::GiveUp
        );
    }

    #[test]
    fn coalesced_error_marker_survives_wrapping() {
        let wrapped = format!(
            "project controller command `dispatch` failed: dispatch blocked: {}",
            DISPATCH_COALESCED_IN_FLIGHT_MARKER
        );

        assert!(dispatch_error_is_coalesced(&wrapped));
        assert!(!dispatch_error_is_coalesced(
            "dispatch blocked for x: failed_stage=queue_paused"
        ));
    }

    #[test]
    fn operator_reopen_command_kind_is_explicit() {
        assert!(dispatch_command_kind_is_operator_reopen("managed_reopen"));
        assert!(dispatch_command_kind_is_operator_reopen(
            "dispatch_only_reopen"
        ));
        assert!(!dispatch_command_kind_is_operator_reopen(
            "idle_queue_continuation"
        ));
        assert!(!dispatch_command_kind_is_operator_reopen("loop"));
    }

    #[test]
    fn stale_generation_redirect_extracts_retry_generation() {
        let wrapped = format!(
            "project controller command `dispatch` failed: {} retry_generation=42",
            DISPATCH_STALE_GENERATION_REDIRECT_MARKER
        );

        assert_eq!(
            dispatch_error_stale_generation_redirect_target(&wrapped),
            Some(42)
        );
        assert_eq!(
            dispatch_error_stale_generation_redirect_target("stale generation retry_generation=42"),
            None
        );
        assert_eq!(
            dispatch_error_stale_generation_redirect_target(&format!(
                "{} retry_generation=x",
                DISPATCH_STALE_GENERATION_REDIRECT_MARKER
            )),
            None
        );
    }

    #[test]
    fn stale_supervisor_churn_stop_classification_extracts_pid() {
        let reason =
            "churn-stop: head re-injected by stale supervisor pid1368698; needs operator recycle";
        assert!(pause_reason_is_stale_supervisor_churn_stop(reason));
        assert_eq!(
            stale_supervisor_pid_from_pause_reason(reason),
            Some(1368698)
        );

        let marked =
            "dispatch blocked: supervisor_restart_redirect stale_pid=42 failed_stage=queue_paused";
        assert_eq!(stale_queue_pause_pid_from_dispatch_error(marked), Some(42));
        let recovery = stale_queue_pause_recovery_from_dispatch_error(marked).unwrap();
        assert_eq!(recovery.stale_pid, 42);
        assert_eq!(
            recovery.outcome.class,
            DispatchRecoveryOutcomeClass::Recoverable
        );
        assert_eq!(
            recovery.outcome.invariant_id,
            STALE_QUEUE_PAUSE_INVARIANT_ID
        );
        assert_eq!(
            recovery.outcome.proof_marker,
            DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER
        );
        assert_eq!(recovery.outcome.next_action, STALE_QUEUE_PAUSE_NEXT_ACTION);
        assert_eq!(
            recovery.outcome.log_fields(),
            "binary_outcome=recoverable invariant=stale_queue_pause proof_marker=supervisor_restart_redirect next_action=restart_supervisor_once_and_retry"
        );

        let legacy =
            "dispatch blocked: failed_stage=queue_paused reason=stale host supervisor pid 9";
        assert_eq!(stale_queue_pause_pid_from_dispatch_error(legacy), Some(9));

        assert_eq!(
            stale_queue_pause_pid_from_dispatch_error("failed_stage=queue_paused reason=operator"),
            None
        );
    }

    #[test]
    fn spent_preset_pause_ids_are_extracted_from_supported_shapes() {
        assert_eq!(
            spent_preset_id_from_pause_reason("#abc-123 preset head is spent"),
            Some("abc-123".to_string())
        );
        assert_eq!(
            spent_preset_id_from_pause_reason("preset-token item is un-drainable (#review_queue)"),
            Some("review_queue".to_string())
        );
        assert_eq!(spent_preset_id_from_pause_reason("no preset here"), None);
    }
}
