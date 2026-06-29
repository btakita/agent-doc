//! Pure controller dispatch admission helpers.

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
