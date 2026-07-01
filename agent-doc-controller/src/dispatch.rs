//! Pure controller dispatch admission helpers.

use sha2::{Digest, Sha256};
use std::time::Duration;

pub const DISPATCH_COALESCED_IN_FLIGHT_MARKER: &str = "failed_stage=coalesced_in_flight";
pub const DISPATCH_STALE_GENERATION_REDIRECT_MARKER: &str = "stale_generation_redirect";
pub const DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER: &str = "supervisor_restart_redirect";
pub const STALE_QUEUE_PAUSE_INVARIANT_ID: &str = "stale_queue_pause";
pub const STALE_QUEUE_PAUSE_NEXT_ACTION: &str = "restart_supervisor_once_and_retry";
pub const DISPATCH_RECOVERY_OUTCOME_CONTRACT_VERSION: &str = "binary-outcome-v1";
const DISPATCH_BLOCKED_USER_FACING_OUTCOME_CONTRACT_VERSION: &str = "ui-outcome-v1";

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

pub const fn queue_pause_predates_boot(updated_at: u64, boot_timestamp: Option<u64>) -> bool {
    match boot_timestamp {
        Some(boot_timestamp) => updated_at < boot_timestamp,
        None => false,
    }
}

/// Seconds to report in dispatch-only busy / not-ready refusal messages.
/// Prefer the caller's explicit ready-wait override when one was used; otherwise
/// report the harness recovery-timeout default.
pub fn dispatch_only_busy_refusal_wait_secs(
    wait_for_ready_override: Option<Duration>,
    default: Duration,
) -> u64 {
    wait_for_ready_override.unwrap_or(default).as_secs()
}

pub fn dispatch_diagnostic_field<'a>(payload: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}=");
    payload
        .split_whitespace()
        .find_map(|token| token.strip_prefix(&prefix))
        .map(|value| value.trim_matches(|ch| matches!(ch, ',' | ';')))
        .filter(|value| !value.is_empty())
}

pub fn append_dispatch_proof_payload(diagnostic_payload: &str, proof_fields: &str) -> String {
    match (diagnostic_payload.is_empty(), proof_fields.is_empty()) {
        (_, true) => diagnostic_payload.to_string(),
        (true, false) => proof_fields.to_string(),
        (false, false) => format!("{diagnostic_payload} {proof_fields}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DispatchBlockedUserFacingOutcome {
    outcome: &'static str,
    class: &'static str,
    next_action: &'static str,
    unblocker: Option<&'static str>,
}

impl DispatchBlockedUserFacingOutcome {
    const fn log_fields(self) -> DispatchBlockedUserFacingOutcomeFields {
        DispatchBlockedUserFacingOutcomeFields(self)
    }
}

struct DispatchBlockedUserFacingOutcomeFields(DispatchBlockedUserFacingOutcome);

impl std::fmt::Display for DispatchBlockedUserFacingOutcomeFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outcome = self.0;
        write!(
            f,
            "ui_outcome_contract={} ui_outcome={} ui_outcome_class={} next_action={}",
            DISPATCH_BLOCKED_USER_FACING_OUTCOME_CONTRACT_VERSION,
            outcome.outcome,
            outcome.class,
            outcome.next_action
        )?;
        if let Some(unblocker) = outcome.unblocker {
            write!(f, " unblocker={unblocker}")?;
        }
        Ok(())
    }
}

pub fn dispatch_blocked_user_facing_outcome_fields(stage: &str, reason: &str) -> String {
    let lower = reason.to_ascii_lowercase();
    let outcome = if stage == "actor_busy_draining" {
        DispatchBlockedUserFacingOutcome {
            outcome: "queued_behind_owner",
            class: "ok",
            next_action: "wait_for_owner_turn_to_drain",
            unblocker: None,
        }
    } else if stage == "queue_paused" && pause_reason_is_stale_supervisor_churn_stop(reason) {
        DispatchBlockedUserFacingOutcome {
            outcome: "recovered_and_retried",
            class: "recoverable",
            next_action: "continue_after_recovery_retry",
            unblocker: None,
        }
    } else if lower.contains("file cache conflict")
        || lower.contains("component conflict")
        || lower.contains("typed_component_drift")
    {
        DispatchBlockedUserFacingOutcome {
            outcome: "real_component_conflict",
            class: "blocked",
            next_action: "resolve_component_conflict",
            unblocker: None,
        }
    } else if lower.contains("zero drainable")
        || lower.contains("no drainable")
        || lower.contains("undrainable")
    {
        DispatchBlockedUserFacingOutcome {
            outcome: "no_drainable_work",
            class: "ok",
            next_action: "no_agent_action",
            unblocker: None,
        }
    } else if lower.contains("operator-verify")
        || lower.contains("operator proof")
        || lower.contains("manual review")
    {
        DispatchBlockedUserFacingOutcome {
            outcome: "deferred_for_operator_proof",
            class: "operator",
            next_action: "operator_proof_required",
            unblocker: None,
        }
    } else {
        DispatchBlockedUserFacingOutcome {
            outcome: "blocked_with_exact_unblocker",
            class: "blocked",
            next_action: "follow_unblocker",
            unblocker: Some("resume_or_clear_queue_control"),
        }
    };

    outcome.log_fields().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchBlockedProofFacts<'a> {
    pub stage: &'a str,
    pub reason: &'a str,
    pub blocked_head: Option<&'a str>,
    pub trigger: Option<&'a str>,
}

pub fn dispatch_blocked_proof_fields(facts: DispatchBlockedProofFacts<'_>) -> String {
    let mut fields = Vec::new();
    fields.push(dispatch_blocked_user_facing_outcome_fields(
        facts.stage,
        facts.reason,
    ));
    if let Some(head) = facts.blocked_head {
        fields.push(format!("blocked_head_bytes={}", head.len()));
        fields.push(format!("blocked_head_sha256={}", sha256_hex(head)));
    }
    if let Some(trigger) = facts.trigger {
        fields.push(format!("trigger_bytes={}", trigger.len()));
        fields.push(format!("trigger_sha256={}", sha256_hex(trigger)));
    }
    fields.join(" ")
}

pub fn recent_lines_contain_trigger(content: &str, trigger: &str) -> bool {
    let recent_lines: Vec<String> = content.lines().rev().take(8).map(strip_ansi).collect();
    recent_lines
        .iter()
        .any(|line| line_contains_trigger(line, trigger))
        || recent_lines_contain_wrapped_trigger(&recent_lines, trigger)
}

pub fn line_contains_trigger(line: &str, trigger: &str) -> bool {
    let mut offset = 0usize;
    while let Some(found) = line[offset..].find(trigger) {
        let start = offset + found;
        let end = start + trigger.len();
        let prev_ok = line[..start]
            .chars()
            .next_back()
            .map(|ch| ch.is_whitespace() || matches!(ch, '>' | '\u{276f}' | '\u{23f5}'))
            .unwrap_or(true);
        let next_ok = line[end..]
            .chars()
            .next()
            .map(|ch| ch.is_whitespace())
            .unwrap_or(true);
        if prev_ok && next_ok {
            return true;
        }
        offset = start + 1;
    }
    false
}

pub fn compact_trigger_text(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

pub fn strip_leading_prompt_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prompt in ["\u{276f}", ">", "\u{203a}", "\u{23f5}"] {
        if let Some(rest) = trimmed.strip_prefix(prompt) {
            return rest.trim_start();
        }
    }
    trimmed
}

pub fn shares_trigger_prefix(fragment: &str, trigger: &str) -> bool {
    let mut frag = fragment.chars();
    let mut trig = trigger.chars();
    loop {
        match (frag.next(), trig.next()) {
            (Some(left), Some(right)) if left == right => {}
            (Some(_), Some(_)) => return false,
            (None, _) | (_, None) => return true,
        }
    }
}

pub fn recent_lines_contain_wrapped_trigger(recent_lines_rev: &[String], trigger: &str) -> bool {
    let compact_trigger = compact_trigger_text(trigger);
    if compact_trigger.is_empty() {
        return false;
    }
    let lines: Vec<&String> = recent_lines_rev.iter().rev().collect();
    for start in 0..lines.len() {
        let first = compact_trigger_text(strip_leading_prompt_prefix(lines[start]));
        if first.is_empty() || !shares_trigger_prefix(&first, &compact_trigger) {
            continue;
        }
        let mut joined = first;
        if joined.contains(&compact_trigger) {
            return true;
        }
        for next in lines.iter().skip(start + 1).take(3) {
            joined.push_str(&compact_trigger_text(next));
            if joined.contains(&compact_trigger) {
                return true;
            }
            if joined.len() > compact_trigger.len() + 32 {
                break;
            }
        }
    }
    false
}

/// Pure in-content decision for whether the dispatch payload is already
/// pending in the harness's current input.
pub fn dispatch_payload_pending_in_current_input(
    content: &str,
    payload: &str,
    is_dispatch_ready_prompt_line: impl Fn(&str) -> bool,
    is_prompt_line: impl Fn(&str) -> bool,
) -> bool {
    if agent_doc_queue::queue_command::is_context_clear_command(payload) {
        return agent_doc_turn_executor_tmux::context_clear::context_clear_command_visible_in_active_input(
            content,
            payload,
            is_dispatch_ready_prompt_line,
        );
    }
    recent_lines_contain_trigger(content, payload)
        || route_trigger_visible_in_current_draft(content, payload, is_prompt_line)
}

pub fn route_trigger_visible_in_current_draft(
    content: &str,
    trigger: &str,
    is_prompt_line: impl Fn(&str) -> bool,
) -> bool {
    let recent_lines: Vec<String> = content
        .lines()
        .rev()
        .map(strip_ansi)
        .filter(|line| !line.trim().is_empty())
        .take(16)
        .collect();
    let lines: Vec<&String> = recent_lines.iter().rev().collect();
    for start in 0..lines.len() {
        if !line_contains_trigger(lines[start], trigger)
            && !line_contains_equivalent_agent_doc_path_trigger(lines[start], trigger)
            && !wrapped_trigger_starts_at_line(&lines, start, trigger)
        {
            continue;
        }
        let later_has_prompt = lines
            .iter()
            .skip(start + 1)
            .any(|line| is_prompt_line(line));
        return !later_has_prompt;
    }
    false
}

fn line_contains_equivalent_agent_doc_path_trigger(line: &str, trigger: &str) -> bool {
    let Some(trigger_path) = single_agent_doc_path_arg(trigger) else {
        return false;
    };
    let stripped = strip_leading_prompt_prefix(line);
    let tokens: Vec<&str> = stripped.split_whitespace().collect();
    for pair in tokens.windows(2) {
        let [command, path_arg] = pair else {
            continue;
        };
        if is_agent_doc_command_token(command)
            && agent_doc_path_args_equivalent(path_arg, trigger_path)
        {
            return true;
        }
    }
    false
}

fn single_agent_doc_path_arg(command_line: &str) -> Option<&str> {
    let stripped = strip_leading_prompt_prefix(command_line);
    let mut tokens = stripped.split_whitespace();
    let command = tokens.next()?;
    if !is_agent_doc_command_token(command) {
        return None;
    }
    let path_arg = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    Some(path_arg)
}

fn is_agent_doc_command_token(token: &str) -> bool {
    let token = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
    token == "agent-doc" || token == "/agent-doc"
}

#[derive(Debug, PartialEq, Eq)]
struct AgentDocPathArg {
    absolute: bool,
    components: Vec<String>,
}

fn agent_doc_path_arg(token: &str) -> Option<AgentDocPathArg> {
    let trimmed = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    let slash_normalized = trimmed.replace('\\', "/");
    let mut components = Vec::new();
    for component in slash_normalized.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return None;
        }
        components.push(component.to_string());
    }
    if components.is_empty() {
        return None;
    }
    Some(AgentDocPathArg {
        absolute: slash_normalized.starts_with('/'),
        components,
    })
}

fn agent_doc_path_args_equivalent(visible: &str, trigger: &str) -> bool {
    let Some(visible) = agent_doc_path_arg(visible) else {
        return false;
    };
    let Some(trigger) = agent_doc_path_arg(trigger) else {
        return false;
    };
    if visible.components == trigger.components {
        return true;
    }
    if visible.absolute == trigger.absolute {
        return false;
    }
    let (absolute, relative) = if visible.absolute {
        (&visible, &trigger)
    } else {
        (&trigger, &visible)
    };
    absolute.components.ends_with(&relative.components)
}

fn wrapped_trigger_starts_at_line(lines: &[&String], start: usize, trigger: &str) -> bool {
    let compact_trigger = compact_trigger_text(trigger);
    if compact_trigger.is_empty() {
        return false;
    }
    let first = compact_trigger_text(strip_leading_prompt_prefix(lines[start]));
    if first.is_empty() || !shares_trigger_prefix(&first, &compact_trigger) {
        return false;
    }
    let mut joined = first;
    if joined.contains(&compact_trigger) {
        return true;
    }
    for next in lines.iter().skip(start + 1).take(3) {
        joined.push_str(&compact_trigger_text(next));
        if joined.contains(&compact_trigger) {
            return true;
        }
        if joined.len() > compact_trigger.len() + 32 {
            break;
        }
    }
    false
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(next) = chars.next()
                && next == '['
            {
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// `#codex-route-busy-ctrl-g-opens-editor`: pure decision for whether the
/// busy-pane reroute may send `C-g`. `C-g` safely aborts a shell
/// `reverse-i-search` / history-search, but in any other Codex state (normal
/// composer, active turn) it opens the external editor. Any non-shell-search
/// reason, including an unknown timeout (`None`), fails closed.
pub fn is_codex_shell_search_blocker(blocker_reason: Option<&str>) -> bool {
    matches!(
        blocker_reason,
        Some("interactive shell reverse-i-search") | Some("interactive shell history search")
    )
}

pub fn normalize_context_session(context_session: Option<&str>) -> Option<&str> {
    context_session.and_then(|session| {
        let trimmed = session.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

pub fn is_stash_window_name(window_name: &str) -> bool {
    window_name == "stash" || window_name.starts_with("stash-")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchActorState {
    Ready,
    Busy,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RouteDecision {
    ReuseReady,
    WaitForReady,
    FreshRestart,
    StartNew,
    FailClosed,
}

impl RouteDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReuseReady => "reuse_ready",
            Self::WaitForReady => "wait_for_ready",
            Self::FreshRestart => "fresh_restart",
            Self::StartNew => "start_new",
            Self::FailClosed => "fail_closed",
        }
    }
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
pub enum ReopenMode {
    Managed,
    DispatchOnly,
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

pub fn is_interactive_shell_substate_reason(reason: &str) -> bool {
    reason.trim_start().starts_with("interactive shell")
}

pub fn dispatch_only_blocked_guard_reason(blocker_reason: &str) -> RoutedReopenGuardReason {
    if is_interactive_shell_substate_reason(blocker_reason) {
        RoutedReopenGuardReason::BlockedInInteractiveSubstate
    } else {
        RoutedReopenGuardReason::DispatchOnlyBusyActorNotReady
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorLifecycleState {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Closed,
    Blocked,
}

pub fn effective_authoritative_actor_state(
    record_state: ActorLifecycleState,
    runtime_state: Option<ActorLifecycleState>,
) -> ActorLifecycleState {
    if matches!(
        record_state,
        ActorLifecycleState::Blocked | ActorLifecycleState::Closed
    ) {
        return record_state;
    }
    runtime_state.unwrap_or(record_state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupMissRouteFacts {
    pub miss_timestamp: u64,
    pub registered_pane_is_live_owner: bool,
    pub pane_alive: bool,
    pub supervisor_health: DispatchRuntimeHealth,
    pub latest_start_matches_registered_pane: bool,
    pub latest_session_open: bool,
    pub latest_session_closed: bool,
    pub latest_start_timestamp: Option<u64>,
    pub latest_open_run_timestamp: Option<u64>,
}

fn startup_miss_runtime_missing(health: DispatchRuntimeHealth) -> bool {
    matches!(
        health,
        DispatchRuntimeHealth::Unreachable | DispatchRuntimeHealth::NoSocket
    )
}

pub fn startup_miss_requires_fresh_start(facts: StartupMissRouteFacts) -> bool {
    !facts.registered_pane_is_live_owner && startup_miss_runtime_missing(facts.supervisor_health)
}

pub fn startup_miss_superseded_by_later_open_start(facts: StartupMissRouteFacts) -> bool {
    facts.latest_session_open
        && facts.latest_start_matches_registered_pane
        && facts
            .latest_open_run_timestamp
            .is_some_and(|ts| ts > facts.miss_timestamp)
}

pub fn startup_miss_should_restart_live_owner(facts: StartupMissRouteFacts) -> bool {
    facts.registered_pane_is_live_owner
        && facts.latest_session_closed
        && facts.latest_start_matches_registered_pane
        && facts
            .latest_start_timestamp
            .is_some_and(|ts| ts <= facts.miss_timestamp)
}

pub fn startup_miss_should_fail_closed(facts: StartupMissRouteFacts) -> bool {
    facts.pane_alive
        && !facts.registered_pane_is_live_owner
        && startup_miss_runtime_missing(facts.supervisor_health)
        && facts.latest_session_open
}

/// Why a cold-start re-verify refuses an auto-start dispatch. The distinction
/// matters for diagnostics: a starting pane may still become submit-ready, while
/// a dead shell needs operator claim/restart recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoStartDispatchBlock {
    StartingPane,
    DeadShell(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoStartDispatchReadyFacts {
    pub pane_shows_dispatch_ready_prompt: bool,
    pub bare_shell_command: Option<String>,
}

pub fn classify_auto_start_dispatch_ready_block(
    facts: AutoStartDispatchReadyFacts,
) -> Option<AutoStartDispatchBlock> {
    if facts.pane_shows_dispatch_ready_prompt {
        None
    } else if let Some(command) = facts.bare_shell_command {
        Some(AutoStartDispatchBlock::DeadShell(command))
    } else {
        Some(AutoStartDispatchBlock::StartingPane)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshStartAckOutcome {
    CycleAcknowledged,
    IdleNoOpKeep,
    GenuineMissReap,
}

pub const fn fresh_start_ack_outcome(
    cycle_acknowledged: bool,
    pane_dispatch_ready: bool,
) -> FreshStartAckOutcome {
    if cycle_acknowledged {
        FreshStartAckOutcome::CycleAcknowledged
    } else if pane_dispatch_ready {
        FreshStartAckOutcome::IdleNoOpKeep
    } else {
        FreshStartAckOutcome::GenuineMissReap
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutedCycleAckFacts {
    pub baseline_cycle_open: bool,
    pub prompt_bearing_marker_present: bool,
}

pub fn should_require_routed_cycle_ack(facts: RoutedCycleAckFacts) -> bool {
    facts.prompt_bearing_marker_present && !facts.baseline_cycle_open
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingCycleAckFacts<'a> {
    pub harness_binary: &'a str,
    pub live_child_for_file: bool,
}

pub fn should_optimistically_accept_missing_cycle_ack(facts: MissingCycleAckFacts<'_>) -> bool {
    facts.harness_binary == "codex" && facts.live_child_for_file
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteSubmitObservation {
    Accepted,
    TriggerStillVisible,
    CaptureFailed,
    DispatchStartProven,
    AcceptedWithoutDispatchProof,
}

impl RouteSubmitObservation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::TriggerStillVisible => "trigger_still_visible",
            Self::CaptureFailed => "capture_failed",
            Self::DispatchStartProven => "dispatch_start_proven",
            Self::AcceptedWithoutDispatchProof => "accepted_without_dispatch_start_proof",
        }
    }

    pub const fn issue(self) -> Option<&'static str> {
        match self {
            Self::TriggerStillVisible => Some("prompt_not_submitted"),
            Self::CaptureFailed => Some("submit_unverified_capture_failed"),
            Self::AcceptedWithoutDispatchProof => Some("accepted_without_dispatch_start_proof"),
            Self::Accepted | Self::DispatchStartProven => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteSubmitObservationFacts<'a> {
    pub file_display: &'a str,
    pub pane: &'a str,
    pub harness_binary: &'a str,
    pub phase: &'a str,
    pub observation: RouteSubmitObservation,
    pub trigger_visible: Option<bool>,
    pub elapsed_ms: u128,
    pub capture_len: Option<usize>,
    pub capture_hash: Option<&'a str>,
    pub proof: Option<RoutedDispatchStartProof>,
    pub editor_attempt_id: Option<&'a str>,
}

fn append_route_submit_evidence(message: &mut String, facts: RouteSubmitObservationFacts<'_>) {
    if let Some(trigger_visible) = facts.trigger_visible {
        message.push_str(&format!(" trigger_visible={trigger_visible}"));
    }
    if let Some(capture_len) = facts.capture_len {
        message.push_str(&format!(" capture_len={capture_len}"));
    }
    if let Some(capture_hash) = facts.capture_hash {
        message.push_str(&format!(" capture_hash={capture_hash}"));
    }
    if let Some(proof) = facts.proof {
        message.push_str(&format!(" proof={}", proof.dispatch_stage_label()));
    }
    if let Some(editor_attempt_id) = facts.editor_attempt_id {
        message.push_str(&format!(" editor_attempt_id={editor_attempt_id}"));
    }
}

pub fn route_submit_observation_message(facts: RouteSubmitObservationFacts<'_>) -> String {
    let mut message = format!(
        "route_submit_observation file={} pane={} harness={} phase={} result={} elapsed_ms={}",
        facts.file_display,
        facts.pane,
        facts.harness_binary,
        facts.phase,
        facts.observation.label(),
        facts.elapsed_ms
    );
    append_route_submit_evidence(&mut message, facts);
    if let Some(issue) = facts.observation.issue() {
        message.push_str(&format!(" issue={issue}"));
    }
    message
}

pub fn route_submit_issue_message(facts: RouteSubmitObservationFacts<'_>) -> Option<String> {
    let issue = facts.observation.issue()?;
    let mut message = format!(
        "route_submit_issue file={} pane={} harness={} phase={} issue={} result={} elapsed_ms={}",
        facts.file_display,
        facts.pane,
        facts.harness_binary,
        facts.phase,
        issue,
        facts.observation.label(),
        facts.elapsed_ms
    );
    append_route_submit_evidence(&mut message, facts);
    Some(message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutedTriggerPayloadFacts<'a> {
    pub harness_binary: &'a str,
    pub trigger: &'a str,
    pub payload: &'a str,
}

pub fn routed_trigger_payload_rejection(facts: RoutedTriggerPayloadFacts<'_>) -> Option<String> {
    if facts.harness_binary == "codex"
        && (facts.payload != facts.trigger
            || facts.payload.contains('\n')
            || facts.payload.contains('\r'))
    {
        Some(format!(
            "internal route bug: Codex reroute payload must stay the bare `agent-doc <FILE>` reopen; refusing to inject {:?}",
            facts.payload
        ))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchInjectLogFacts<'a> {
    pub file_display: &'a str,
    pub pane: &'a str,
    pub harness_binary: &'a str,
    pub transport: &'a str,
    pub attempt: usize,
}

pub fn dispatch_inject_log_line(facts: DispatchInjectLogFacts<'_>) -> String {
    format!(
        "dispatch_inject file={} pane={} harness={} transport={} attempt={}",
        facts.file_display, facts.pane, facts.harness_binary, facts.transport, facts.attempt
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneResubmitProofFacts<'a> {
    pub file_display: &'a str,
    pub pane: &'a str,
    pub harness_binary: &'a str,
    pub submit_key: &'a str,
    pub status: DirectPaneSubmitStatus,
    pub elapsed_ms: u128,
    pub attempt: usize,
    pub editor_attempt_id: Option<&'a str>,
}

fn direct_pane_resubmit_result_label(status: DirectPaneSubmitStatus) -> &'static str {
    if status == DirectPaneSubmitStatus::Accepted {
        "accepted"
    } else {
        "still_visible"
    }
}

pub fn direct_pane_resubmit_proof_line(facts: DirectPaneResubmitProofFacts<'_>) -> String {
    let mut message = format!(
        "route_submit_resubmit file={} pane={} harness={} action=submit_key key={} result={} elapsed_ms={} attempt={}",
        facts.file_display,
        facts.pane,
        facts.harness_binary,
        facts.submit_key,
        direct_pane_resubmit_result_label(facts.status),
        facts.elapsed_ms,
        facts.attempt
    );
    if let Some(editor_attempt_id) = facts.editor_attempt_id {
        message.push_str(&format!(" editor_attempt_id={editor_attempt_id}"));
    }
    message
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteLatencyStatus {
    Ok,
    OverBudget,
}

impl RouteLatencyStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::OverBudget => "over_budget",
        }
    }
}

pub fn route_latency_status(elapsed_ms: u128, budget_ms: u128) -> RouteLatencyStatus {
    if elapsed_ms >= budget_ms {
        RouteLatencyStatus::OverBudget
    } else {
        RouteLatencyStatus::Ok
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteLatencyFacts<'a> {
    pub phase: &'a str,
    pub elapsed_ms: u128,
    pub budget_ms: u128,
    pub pane: &'a str,
    pub harness_binary: &'a str,
    pub outcome: &'a str,
    pub editor_attempt_id: Option<&'a str>,
}

pub fn route_latency_message(facts: RouteLatencyFacts<'_>) -> String {
    let mut message = format!(
        "route_latency phase={} elapsed_ms={} budget_ms={} status={} pane={} harness={} outcome={}",
        facts.phase,
        facts.elapsed_ms,
        facts.budget_ms,
        route_latency_status(facts.elapsed_ms, facts.budget_ms).label(),
        facts.pane,
        facts.harness_binary,
        facts.outcome
    );
    if let Some(editor_attempt_id) = facts.editor_attempt_id {
        message.push_str(&format!(" editor_attempt_id={editor_attempt_id}"));
    }
    message
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteStartupMissDiagnosticFacts<'a> {
    pub file_display: &'a str,
    pub reason: &'a str,
}

pub fn route_startup_miss_diagnostic_message(facts: RouteStartupMissDiagnosticFacts<'_>) -> String {
    format!(
        "[agent-doc] startup-miss: {}. Run 'agent-doc start {}' to retry.",
        facts.reason, facts.file_display
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteBusyDiagnosticFacts<'a> {
    pub file_display: &'a str,
    pub harness_binary: &'a str,
}

pub fn route_busy_diagnostic_message(facts: RouteBusyDiagnosticFacts<'_>) -> String {
    format!(
        "[agent-doc] routed follow-up for {} is still pending because the live {} session is busy. Finish or interrupt the current task, then rerun `Run Agent Doc` or `agent-doc route {}`.",
        facts.file_display, facts.harness_binary, facts.file_display
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteBusyQueuedDiagnosticFacts<'a> {
    pub file_display: &'a str,
    pub harness_binary: &'a str,
    pub user_outcome_fields: &'a str,
}

pub fn route_busy_queued_diagnostic_message(facts: RouteBusyQueuedDiagnosticFacts<'_>) -> String {
    format!(
        "[agent-doc] turn in progress — the live {} session is busy, so Run Agent Doc for {} was queued and will run when the current turn finishes. No need to rerun. {}",
        facts.harness_binary, facts.file_display, facts.user_outcome_fields
    )
}

/// #route-busy-vs-starting-wording: word the authoritative-actor `FailClosed`
/// wait context. When the live pane shows a harness busy cue the actor is busy
/// on an active turn, not cold-starting, so the "(waited Ns for X startup)"
/// phrasing is misleading.
pub fn failclosed_wait_context(
    harness_binary: &str,
    busy_cue: Option<&str>,
    startup_secs: u64,
) -> String {
    match busy_cue {
        Some(cue) => format!(
            "the pane is busy on an active {harness_binary} turn ({cue}), not cold-starting"
        ),
        None => format!("waited {startup_secs}s for {harness_binary} startup"),
    }
}

pub fn format_busy_existing_pane_error(
    file_display: impl std::fmt::Display,
    pane: &str,
    harness_binary: &str,
    provenance: &str,
    detail: Option<&str>,
    auto_fix_attempted: bool,
) -> String {
    let detail_clause = detail
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    if auto_fix_attempted {
        format!(
            "registered pane {} for {} is still not showing an idle {} prompt{} after automatically applying `agent-doc fix {}` once; refusing to inject a routed trigger into a busy session ({})",
            pane, file_display, harness_binary, detail_clause, file_display, provenance
        )
    } else {
        format!(
            "registered pane {} for {} is not showing an idle {} prompt{}; refusing to inject a routed trigger into a busy session ({})",
            pane, file_display, harness_binary, detail_clause, provenance
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicatePanePolicyErrorFacts<'a> {
    pub session_name: &'a str,
    pub file_path: &'a str,
    pub anchor_pane: Option<&'a str>,
    pub cause: &'a str,
}

pub fn duplicate_pane_policy_error_message(facts: DuplicatePanePolicyErrorFacts<'_>) -> String {
    let mut lines = vec![
        format!(
            "refusing to provision a duplicate tmux pane for {} in session '{}': {}",
            facts.file_path, facts.session_name, facts.cause
        ),
        "Inspect the existing panes first:".to_string(),
        format!(
            "  tmux list-panes -t {}:agent-doc -F '#{{pane_id}} #{{window_name}} #{{pane_current_command}} #{{pane_current_path}}'",
            facts.session_name
        ),
        format!(
            "  tmux list-panes -a -F '#{{session_name}} #{{window_name}} #{{pane_id}} #{{pane_current_command}} #{{pane_current_path}}' | grep ' {}$'",
            facts.file_path
        ),
    ];
    if let Some(anchor_pane) = facts.anchor_pane {
        lines.push(format!(
            "  tmux capture-pane -pt {} | tail -n 80",
            anchor_pane
        ));
        lines.push(format!("  tmux kill-pane -t {}", anchor_pane));
    } else {
        lines.push("  tmux kill-pane -t <pane_id>".to_string());
    }
    lines.push(format!("Then rerun: agent-doc {}", facts.file_path));
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDispatchBugReportItemFacts<'a> {
    pub document_display: &'a str,
    pub document_id: &'a str,
    pub pane: &'a str,
    pub phase: &'a str,
    pub issue: &'a str,
    pub result: &'a str,
    pub elapsed_ms: u128,
    pub actor_generation: Option<u64>,
    pub editor_attempt_id: Option<&'a str>,
    pub dispatch_proof_state: Option<&'a str>,
    pub diagnostic_path: Option<&'a str>,
    pub ops_log_path: Option<&'a str>,
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

fn route_dispatch_bug_report_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "none".to_string()
    } else {
        out
    }
}

pub fn route_dispatch_bug_report_item(
    facts: RouteDispatchBugReportItemFacts<'_>,
) -> Result<String, String> {
    let component = format!("route/{}", route_dispatch_bug_report_field(facts.phase));
    let content_hash = sha256_hex(&format!(
        "{}:{}:{}",
        facts.document_id, facts.phase, facts.issue
    ));
    let symptom_key = agent_doc_element_backlog::backlog::SymptomDedupeKey::new(
        "run_agent_doc_route_dispatch_failure",
        facts.document_id,
        component,
        format!("sha256:{content_hash}"),
    )
    .map_err(|err| err.to_string())?;
    let generation = facts
        .actor_generation
        .map(|generation| generation.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let editor_attempt = facts.editor_attempt_id.unwrap_or("unknown");
    let proof = facts.dispatch_proof_state.unwrap_or("none");
    let diagnostic_path = facts.diagnostic_path.unwrap_or("none");
    let ops_log_path = facts.ops_log_path.unwrap_or("unknown");
    let marker = format!(
        "route_submit_issue(issue={},phase={},result={})",
        facts.issue,
        route_dispatch_bug_report_field(facts.phase),
        route_dispatch_bug_report_field(facts.result)
    );

    Ok(format!(
        "JetBrains Run Agent Doc route/dispatch failed after bounded submit/start proof retries #jbrunautobug #agent-doc-bug failure_class={} document={} stage={} pane={} actor_generation={} editor_attempt_id={} dispatch_proof_state={} elapsed_ms={} diagnostic_path={} ops_log_path={} ops_log_marker={} {}",
        facts.issue,
        facts.document_display,
        facts.phase,
        facts.pane,
        generation,
        editor_attempt,
        proof,
        facts.elapsed_ms,
        diagnostic_path,
        ops_log_path,
        marker,
        symptom_key.marker()
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOnlyReopenDelivery {
    SupervisorIpcOnce,
    DirectPaneSubmit,
}

impl DispatchOnlyReopenDelivery {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SupervisorIpcOnce => "supervisor_ipc_once",
            Self::DirectPaneSubmit => "direct_pane_submit",
        }
    }

    pub const fn submit_mode_for_harness(self, harness_binary: &str) -> &'static str {
        match self {
            Self::SupervisorIpcOnce => "supervisor_normalized_submit",
            Self::DirectPaneSubmit => {
                agent_doc_tmux_commands::tmux_submit_mode_for_harness(harness_binary)
            }
        }
    }
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

pub const fn dispatch_only_should_print_unproven_progress() -> bool {
    true
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchOnlyBusyRefusalFacts<'a> {
    pub generation: u64,
    pub file_display: &'a str,
    pub dispatch_pane: &'a str,
    pub harness_binary: &'a str,
    pub reason: &'a str,
    pub wait_secs: u64,
    pub recovery_hint: &'a str,
    pub active_turn_busy_cue: Option<&'a str>,
    pub blocked_outcome_fields: &'a str,
}

pub fn dispatch_only_busy_refusal_message(facts: DispatchOnlyBusyRefusalFacts<'_>) -> String {
    match facts.active_turn_busy_cue {
        Some(cue) => format!(
            "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because the pane is busy on an active {} turn ({}), not at a dispatch-ready prompt. {} {}",
            facts.generation,
            facts.file_display,
            facts.dispatch_pane,
            facts.harness_binary,
            cue,
            facts.recovery_hint,
            facts.blocked_outcome_fields
        ),
        None => format!(
            "authoritative actor generation {} for {} owns pane {} but dispatch-only route will not inject a new trigger because {} did not return to a dispatch-ready prompt in the current generation after waiting {}s. {} {}",
            facts.generation,
            facts.file_display,
            facts.dispatch_pane,
            facts.reason,
            facts.wait_secs,
            facts.recovery_hint,
            facts.blocked_outcome_fields
        ),
    }
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

pub const DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR: Duration = Duration::from_millis(900);
pub const DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT: usize = 30;

pub fn direct_pane_max_enter_resubmits_from_env_value(value: Option<&str>) -> usize {
    value
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT)
}

pub fn direct_pane_max_enter_resubmits() -> usize {
    let value = std::env::var("AGENT_DOC_DIRECT_PANE_MAX_ENTER_RESUBMITS").ok();
    direct_pane_max_enter_resubmits_from_env_value(value.as_deref())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneDispatchStartProofFacts {
    pub await_start_proof: bool,
    pub submit_status: DirectPaneSubmitStatus,
}

pub fn direct_pane_should_await_dispatch_start_proof(
    facts: DirectPaneDispatchStartProofFacts,
) -> bool {
    facts.await_start_proof || facts.submit_status != DirectPaneSubmitStatus::Accepted
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneAcceptancePollState {
    saw_trigger_visible: bool,
    first_empty_capture_at: Option<Duration>,
}

impl DirectPaneAcceptancePollState {
    pub const fn saw_trigger_visible(self) -> bool {
        self.saw_trigger_visible
    }
}

pub fn direct_pane_acceptance_poll_status(
    state: &mut DirectPaneAcceptancePollState,
    elapsed: Duration,
    trigger_visible: bool,
) -> Option<DirectPaneSubmitStatus> {
    if trigger_visible {
        state.saw_trigger_visible = true;
        state.first_empty_capture_at = None;
        return None;
    }

    if state.saw_trigger_visible {
        return Some(DirectPaneSubmitStatus::Accepted);
    }

    let first_empty_at = state.first_empty_capture_at.get_or_insert(elapsed);
    if elapsed.saturating_sub(*first_empty_at) >= DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR {
        Some(DirectPaneSubmitStatus::Accepted)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneNotDispatchedFacts {
    pub saw_trigger_visible: bool,
    pub pane_idle_dispatch_ready: bool,
}

pub const fn direct_pane_not_dispatched(facts: DirectPaneNotDispatchedFacts) -> bool {
    !facts.saw_trigger_visible && facts.pane_idle_dispatch_ready
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneEnterResubmitFacts {
    pub profile_allows_pending_draft_enter_resubmit: bool,
    pub status: DirectPaneSubmitStatus,
    pub trigger_visible: bool,
}

pub fn direct_pane_needs_enter_resubmit(facts: DirectPaneEnterResubmitFacts) -> bool {
    facts.profile_allows_pending_draft_enter_resubmit
        && facts.status == DirectPaneSubmitStatus::TimedOut
        && facts.trigger_visible
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneEnterResubmitAttemptFacts {
    pub profile_allows_pending_draft_enter_resubmit: bool,
    pub status: DirectPaneSubmitStatus,
    pub trigger_visible: bool,
    pub attempts_sent: usize,
    pub max_attempts: usize,
}

pub fn direct_pane_can_continue_enter_resubmit(facts: DirectPaneEnterResubmitAttemptFacts) -> bool {
    facts.attempts_sent < facts.max_attempts
        && direct_pane_needs_enter_resubmit(DirectPaneEnterResubmitFacts {
            profile_allows_pending_draft_enter_resubmit: facts
                .profile_allows_pending_draft_enter_resubmit,
            status: facts.status,
            trigger_visible: facts.trigger_visible,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneFullResendFacts {
    pub not_dispatched: bool,
    pub attempts_sent: usize,
    pub max_attempts: usize,
}

pub const fn direct_pane_can_full_resend_not_landed(facts: DirectPaneFullResendFacts) -> bool {
    facts.not_dispatched && facts.attempts_sent < facts.max_attempts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectPaneExistingDraftSubmitFacts {
    pub profile_allows_pending_draft_enter_resubmit: bool,
    pub trigger_visible: bool,
}

pub fn direct_pane_can_enter_existing_draft(facts: DirectPaneExistingDraftSubmitFacts) -> bool {
    facts.profile_allows_pending_draft_enter_resubmit && facts.trigger_visible
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseoutBlockDispatchFacts {
    pub recovery_queues_prompt_for_after_closeout: bool,
    pub active_queue_head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutBlockDispatchDecision {
    EnqueuePromptForAfterCloseout,
    WaitForActiveQueueHead { head: String },
    FailClosed,
}

pub fn classify_closeout_block_dispatch(
    facts: CloseoutBlockDispatchFacts,
) -> CloseoutBlockDispatchDecision {
    if facts.recovery_queues_prompt_for_after_closeout {
        return CloseoutBlockDispatchDecision::EnqueuePromptForAfterCloseout;
    }
    if let Some(head) = facts.active_queue_head {
        return CloseoutBlockDispatchDecision::WaitForActiveQueueHead { head };
    }
    CloseoutBlockDispatchDecision::FailClosed
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
    fn recent_lines_contain_trigger_matches_claude_trigger() {
        let content = "\
history line
\x1b[32m\u{276f}\x1b[0m /agent-doc test.md
";
        assert!(recent_lines_contain_trigger(content, "/agent-doc test.md"));
        assert!(!recent_lines_contain_trigger(content, "agent-doc test.md"));
    }

    #[test]
    fn recent_lines_contain_trigger_matches_codex_trigger() {
        let content = "\
history line
> agent-doc test.md
";
        assert!(recent_lines_contain_trigger(content, "agent-doc test.md"));
        assert!(!recent_lines_contain_trigger(content, "/agent-doc test.md"));
    }

    #[test]
    fn recent_lines_contain_trigger_matches_wrapped_codex_trigger() {
        let trigger = "agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md";
        let content = "\
\u{203a} agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claud
escore-3.md
gpt-5.4 high - ~/work/btakita/agent-loop/src/session-share - Context 31% used
";
        assert!(
            recent_lines_contain_trigger(content, trigger),
            "wrapped Codex composer lines must still count as pending input"
        );
    }

    #[test]
    fn dispatch_payload_pending_matches_relative_codex_agent_doc_draft() {
        let payload =
            "agent-doc /home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md";
        let content = "\
› agent-doc tasks/sampleorders.md
agent-doc tasks/sampleorders.md
agent-doc tasks/sampleorders.md
agent-doc tasks/sampleorders.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/sample-app · Context 0% use
";

        assert!(
            dispatch_payload_pending_in_current_input(
                content,
                payload,
                |_| false,
                |line| line.trim_start().starts_with('›')
            ),
            "idle-queue dedupe must recognize equivalent relative Codex drafts before appending another trigger"
        );
    }

    #[test]
    fn dispatch_payload_pending_detects_codex_context_clear_draft() {
        let content = concat!(
            "older output\n",
            "› /clear\n",
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n",
        );

        assert!(
            dispatch_payload_pending_in_current_input(
                content,
                "/clear",
                |line| line.trim() == "›",
                |_| false
            ),
            "idle-queue recovery must see a visible Codex /clear draft and resubmit Enter"
        );
    }

    #[test]
    fn dispatch_payload_pending_ignores_submitted_context_clear_scrollback() {
        let content = concat!(
            "✶ Generating... (3s · esc to interrupt)\n",
            "  ❯ /clear\n",
            "────────────────────\n",
            "❯ Press up to edit queued messages\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:10% ~/work/btakita/agent-loop main brian@host\n",
        );

        assert!(
            !dispatch_payload_pending_in_current_input(
                content,
                "/clear",
                |line| line.trim() == "❯",
                |_| false
            ),
            "a prior submitted /clear in scrollback must not suppress or resubmit the next drain"
        );
    }

    #[test]
    fn dispatch_payload_pending_detects_opencode_new_palette_row() {
        let content = concat!(
            "older output\n",
            "/new        New session\n",
            "/models     Select model\n",
            "> /new\n",
        );

        assert!(dispatch_payload_pending_in_current_input(
            content,
            "/new",
            |line| line.trim() == ">",
            |_| false
        ));
    }

    #[test]
    fn dispatch_payload_pending_detects_opencode_selected_new_session_command() {
        let content = concat!(
            "older output\n",
            "> New session\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );

        assert!(
            dispatch_payload_pending_in_current_input(
                content,
                "/new",
                |line| line.trim() == ">",
                |_| false
            ),
            "OpenCode can replace `/new` with the selected command label before the final submit Enter"
        );

        let structured = concat!(
            "older output\n",
            "> session_new\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );
        assert!(
            dispatch_payload_pending_in_current_input(
                structured,
                "/new",
                |line| line.trim() == ">",
                |_| false
            ),
            "OpenCode can also surface the selected command id before submission"
        );
    }

    #[test]
    fn codex_shell_search_blocker_allows_only_shell_search_reasons() {
        assert!(is_codex_shell_search_blocker(Some(
            "interactive shell reverse-i-search"
        )));
        assert!(is_codex_shell_search_blocker(Some(
            "interactive shell history search"
        )));

        assert!(!is_codex_shell_search_blocker(Some("active codex turn")));
        assert!(!is_codex_shell_search_blocker(Some(
            "queued draft in composer"
        )));
        assert!(!is_codex_shell_search_blocker(Some(
            "active permission prompt"
        )));
        assert!(!is_codex_shell_search_blocker(Some(" reverse-i-search")));
        assert!(!is_codex_shell_search_blocker(None));
    }

    #[test]
    fn normalize_context_session_trims_and_rejects_blank() {
        assert_eq!(
            normalize_context_session(Some("agent-doc")),
            Some("agent-doc")
        );
        assert_eq!(
            normalize_context_session(Some("  work-session \n")),
            Some("work-session")
        );
        assert_eq!(normalize_context_session(Some("")), None);
        assert_eq!(normalize_context_session(Some(" \t\n")), None);
        assert_eq!(normalize_context_session(None), None);
    }

    #[test]
    fn is_stash_window_name_matches_route_stash_windows() {
        assert!(is_stash_window_name("stash"));
        assert!(is_stash_window_name("stash-1"));
        assert!(is_stash_window_name("stash-42"));
        assert!(is_stash_window_name("stash-"));
        assert!(!is_stash_window_name(""));
        assert!(!is_stash_window_name("agent-doc"));
        assert!(!is_stash_window_name("stashed"));
    }

    #[test]
    fn line_contains_trigger_rejects_codex_substring_inside_claude_trigger() {
        assert!(line_contains_trigger(
            "\u{276f} /agent-doc test.md",
            "/agent-doc test.md"
        ));
        assert!(!line_contains_trigger(
            "\u{276f} /agent-doc test.md",
            "agent-doc test.md"
        ));
    }

    #[test]
    fn auto_dispatch_coalesces_only_when_same_cycle_in_flight() {
        assert!(dispatch_should_coalesce_in_flight(true, false));
        assert!(!dispatch_should_coalesce_in_flight(true, true));
        assert!(!dispatch_should_coalesce_in_flight(false, false));
        assert!(!dispatch_should_coalesce_in_flight(false, true));
    }

    #[test]
    fn dispatch_diagnostic_field_trims_field_punctuation() {
        let payload = "stage=queue_paused harness=codex, result=blocked; empty= ;";

        assert_eq!(dispatch_diagnostic_field(payload, "harness"), Some("codex"));
        assert_eq!(
            dispatch_diagnostic_field(payload, "result"),
            Some("blocked")
        );
        assert_eq!(dispatch_diagnostic_field(payload, "empty"), None);
        assert_eq!(dispatch_diagnostic_field(payload, "missing"), None);
    }

    #[test]
    fn append_dispatch_proof_payload_joins_non_empty_parts() {
        assert_eq!(
            append_dispatch_proof_payload("stage=queue_paused", "proof=ready"),
            "stage=queue_paused proof=ready"
        );
        assert_eq!(
            append_dispatch_proof_payload("", "proof=ready"),
            "proof=ready"
        );
        assert_eq!(
            append_dispatch_proof_payload("stage=queue_paused", ""),
            "stage=queue_paused"
        );
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
    fn effective_actor_state_preserves_terminal_record_states() {
        assert_eq!(
            effective_authoritative_actor_state(
                ActorLifecycleState::Blocked,
                Some(ActorLifecycleState::Ready),
            ),
            ActorLifecycleState::Blocked
        );
        assert_eq!(
            effective_authoritative_actor_state(
                ActorLifecycleState::Closed,
                Some(ActorLifecycleState::Ready),
            ),
            ActorLifecycleState::Closed
        );
        assert_eq!(
            effective_authoritative_actor_state(
                ActorLifecycleState::Starting,
                Some(ActorLifecycleState::Ready),
            ),
            ActorLifecycleState::Ready
        );
        assert_eq!(
            effective_authoritative_actor_state(ActorLifecycleState::Busy, None),
            ActorLifecycleState::Busy
        );
    }

    fn startup_miss_facts() -> StartupMissRouteFacts {
        StartupMissRouteFacts {
            miss_timestamp: 10,
            registered_pane_is_live_owner: false,
            pane_alive: true,
            supervisor_health: DispatchRuntimeHealth::NoSocket,
            latest_start_matches_registered_pane: true,
            latest_session_open: true,
            latest_session_closed: false,
            latest_start_timestamp: Some(10),
            latest_open_run_timestamp: Some(10),
        }
    }

    #[test]
    fn startup_miss_requires_fresh_start_only_without_matching_live_owner() {
        assert!(startup_miss_requires_fresh_start(startup_miss_facts()));
        assert!(!startup_miss_requires_fresh_start(StartupMissRouteFacts {
            registered_pane_is_live_owner: true,
            ..startup_miss_facts()
        }));
        assert!(!startup_miss_requires_fresh_start(StartupMissRouteFacts {
            supervisor_health: DispatchRuntimeHealth::Healthy,
            ..startup_miss_facts()
        }));
        assert!(!startup_miss_requires_fresh_start(StartupMissRouteFacts {
            supervisor_health: DispatchRuntimeHealth::Restartable,
            ..startup_miss_facts()
        }));
    }

    #[test]
    fn startup_miss_live_owner_restart_requires_closed_unsuperseded_start() {
        assert!(startup_miss_should_restart_live_owner(
            StartupMissRouteFacts {
                registered_pane_is_live_owner: true,
                latest_session_open: false,
                latest_session_closed: true,
                ..startup_miss_facts()
            }
        ));
        assert!(!startup_miss_should_restart_live_owner(
            StartupMissRouteFacts {
                registered_pane_is_live_owner: true,
                latest_session_open: true,
                latest_session_closed: false,
                latest_open_run_timestamp: Some(11),
                ..startup_miss_facts()
            }
        ));
        assert!(startup_miss_superseded_by_later_open_start(
            StartupMissRouteFacts {
                latest_open_run_timestamp: Some(11),
                ..startup_miss_facts()
            }
        ));
        assert!(!startup_miss_superseded_by_later_open_start(
            StartupMissRouteFacts {
                latest_session_open: false,
                latest_session_closed: true,
                ..startup_miss_facts()
            }
        ));
    }

    #[test]
    fn startup_miss_fail_closed_only_for_alive_open_missing_runtime_sessions() {
        assert!(startup_miss_should_fail_closed(startup_miss_facts()));
        assert!(!startup_miss_should_fail_closed(StartupMissRouteFacts {
            registered_pane_is_live_owner: true,
            ..startup_miss_facts()
        }));
        assert!(!startup_miss_should_fail_closed(StartupMissRouteFacts {
            supervisor_health: DispatchRuntimeHealth::Healthy,
            ..startup_miss_facts()
        }));
        assert!(!startup_miss_should_fail_closed(StartupMissRouteFacts {
            latest_session_open: false,
            latest_session_closed: true,
            ..startup_miss_facts()
        }));
        assert!(!startup_miss_should_fail_closed(StartupMissRouteFacts {
            pane_alive: false,
            ..startup_miss_facts()
        }));
    }

    #[test]
    fn auto_start_dispatch_ready_classification_distinguishes_starting_from_dead_shell() {
        assert_eq!(
            classify_auto_start_dispatch_ready_block(AutoStartDispatchReadyFacts {
                pane_shows_dispatch_ready_prompt: true,
                bare_shell_command: Some("zsh".to_string()),
            }),
            None,
            "a visible dispatch-ready prompt wins over a shell-looking foreground command"
        );
        assert_eq!(
            classify_auto_start_dispatch_ready_block(AutoStartDispatchReadyFacts {
                pane_shows_dispatch_ready_prompt: false,
                bare_shell_command: Some("bash".to_string()),
            }),
            Some(AutoStartDispatchBlock::DeadShell("bash".to_string()))
        );
        assert_eq!(
            classify_auto_start_dispatch_ready_block(AutoStartDispatchReadyFacts {
                pane_shows_dispatch_ready_prompt: false,
                bare_shell_command: None,
            }),
            Some(AutoStartDispatchBlock::StartingPane)
        );
    }

    #[test]
    fn fresh_start_ack_outcome_keeps_idle_no_op_and_reaps_genuine_miss() {
        assert_eq!(
            fresh_start_ack_outcome(true, false),
            FreshStartAckOutcome::CycleAcknowledged,
            "a document cycle ack is a normal fresh start regardless of pane prompt state"
        );
        assert_eq!(
            fresh_start_ack_outcome(false, true),
            FreshStartAckOutcome::IdleNoOpKeep,
            "a no-cycle fresh start that returns to dispatch-ready is a legitimate idle no-op"
        );
        assert_eq!(
            fresh_start_ack_outcome(false, false),
            FreshStartAckOutcome::GenuineMissReap,
            "a no-cycle fresh start without dispatch-ready proof is a genuine startup miss"
        );
    }

    #[test]
    fn routed_cycle_ack_required_only_for_prompt_bearing_closed_baselines() {
        assert!(!should_require_routed_cycle_ack(RoutedCycleAckFacts {
            baseline_cycle_open: false,
            prompt_bearing_marker_present: false,
        }));
        assert!(!should_require_routed_cycle_ack(RoutedCycleAckFacts {
            baseline_cycle_open: true,
            prompt_bearing_marker_present: true,
        }));
        assert!(should_require_routed_cycle_ack(RoutedCycleAckFacts {
            baseline_cycle_open: false,
            prompt_bearing_marker_present: true,
        }));
    }

    #[test]
    fn missing_cycle_ack_optimism_is_codex_live_child_only() {
        assert!(should_optimistically_accept_missing_cycle_ack(
            MissingCycleAckFacts {
                harness_binary: "codex",
                live_child_for_file: true,
            }
        ));
        assert!(!should_optimistically_accept_missing_cycle_ack(
            MissingCycleAckFacts {
                harness_binary: "codex",
                live_child_for_file: false,
            }
        ));
        assert!(!should_optimistically_accept_missing_cycle_ack(
            MissingCycleAckFacts {
                harness_binary: "opencode",
                live_child_for_file: true,
            }
        ));
    }

    fn route_submit_facts(
        observation: RouteSubmitObservation,
    ) -> RouteSubmitObservationFacts<'static> {
        RouteSubmitObservationFacts {
            file_display: "/tmp/run-agent-doc.md",
            pane: "%7",
            harness_binary: "codex",
            phase: "direct_pane_acceptance",
            observation,
            trigger_visible: Some(true),
            elapsed_ms: 5123,
            capture_len: Some(2048),
            capture_hash: Some("abc123def456"),
            proof: None,
            editor_attempt_id: Some("attempt_1_2"),
        }
    }

    #[test]
    fn route_submit_observation_marks_prompt_not_submitted_without_prompt_text() {
        let facts = route_submit_facts(RouteSubmitObservation::TriggerStillVisible);

        let message = route_submit_observation_message(facts);
        assert!(message.contains("route_submit_observation"), "{message}");
        assert!(
            message.contains("result=trigger_still_visible"),
            "{message}"
        );
        assert!(message.contains("trigger_visible=true"), "{message}");
        assert!(message.contains("issue=prompt_not_submitted"), "{message}");
        assert!(message.contains("capture_hash=abc123def456"), "{message}");
        assert!(
            message.contains("editor_attempt_id=attempt_1_2"),
            "{message}"
        );
        assert!(!message.contains("agent-doc "), "{message}");

        let issue =
            route_submit_issue_message(facts).expect("prompt-not-submitted should be an issue");
        assert!(issue.contains("route_submit_issue"), "{issue}");
        assert!(issue.contains("issue=prompt_not_submitted"), "{issue}");
        assert!(issue.contains("result=trigger_still_visible"), "{issue}");
        assert!(issue.contains("editor_attempt_id=attempt_1_2"), "{issue}");
    }

    #[test]
    fn route_submit_observation_marks_dispatch_start_proof_without_issue() {
        let facts = RouteSubmitObservationFacts {
            phase: "dispatch_start_proof",
            observation: RouteSubmitObservation::DispatchStartProven,
            trigger_visible: None,
            elapsed_ms: 800,
            capture_len: None,
            capture_hash: None,
            proof: Some(RoutedDispatchStartProof::HookStateAdvanced),
            editor_attempt_id: None,
            ..route_submit_facts(RouteSubmitObservation::DispatchStartProven)
        };

        let message = route_submit_observation_message(facts);
        assert!(
            message.contains("result=dispatch_start_proven"),
            "{message}"
        );
        assert!(message.contains("proof=submitted"), "{message}");
        assert!(
            route_submit_issue_message(facts).is_none(),
            "dispatch-start proof should not emit an issue"
        );
    }

    #[test]
    fn route_submit_observation_marks_accepted_without_dispatch_proof_as_issue() {
        let facts = RouteSubmitObservationFacts {
            phase: "dispatch_start_proof",
            observation: RouteSubmitObservation::AcceptedWithoutDispatchProof,
            trigger_visible: None,
            elapsed_ms: 10_000,
            capture_len: None,
            capture_hash: None,
            proof: None,
            editor_attempt_id: None,
            ..route_submit_facts(RouteSubmitObservation::AcceptedWithoutDispatchProof)
        };

        let issue = route_submit_issue_message(facts)
            .expect("required dispatch-start proof absence should be an issue");
        assert!(
            issue.contains("issue=accepted_without_dispatch_start_proof"),
            "{issue}"
        );
        assert!(
            issue.contains("result=accepted_without_dispatch_start_proof"),
            "{issue}"
        );
    }

    #[test]
    fn routed_trigger_payload_rejection_accepts_bare_codex_reopen() {
        assert_eq!(
            routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
                harness_binary: "codex",
                trigger: "agent-doc test.md",
                payload: "agent-doc test.md",
            }),
            None
        );
    }

    #[test]
    fn routed_trigger_payload_rejection_rejects_multiline_codex_payload() {
        let rejection = routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
            harness_binary: "codex",
            trigger: "agent-doc test.md",
            payload: "agent-doc test.md\nfollow-up text",
        })
        .expect("Codex reroute payload must fail before injecting extra lines");
        assert!(
            rejection.contains("bare `agent-doc <FILE>` reopen")
                && rejection.contains("follow-up text"),
            "unexpected rejection: {rejection}"
        );
    }

    #[test]
    fn routed_trigger_payload_rejection_rejects_rewritten_codex_payload() {
        let rejection = routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
            harness_binary: "codex",
            trigger: "agent-doc test.md",
            payload: "/agent-doc test.md",
        })
        .expect("Codex reroute payload must stay the bare reopen");
        assert!(
            rejection.contains("refusing to inject \"/agent-doc test.md\""),
            "unexpected rejection: {rejection}"
        );
    }

    #[test]
    fn routed_trigger_payload_rejection_allows_non_codex_payload_shape() {
        assert_eq!(
            routed_trigger_payload_rejection(RoutedTriggerPayloadFacts {
                harness_binary: "claude",
                trigger: "agent-doc test.md",
                payload: "/agent-doc test.md\n",
            }),
            None
        );
    }

    #[test]
    fn dispatch_inject_log_line_records_attempt_and_transport() {
        let message = dispatch_inject_log_line(DispatchInjectLogFacts {
            file_display: "/tmp/plan.md",
            pane: "%42",
            harness_binary: "codex",
            transport: "direct_pane",
            attempt: 2,
        });
        assert_eq!(
            message,
            "dispatch_inject file=/tmp/plan.md pane=%42 harness=codex transport=direct_pane attempt=2"
        );
    }

    #[test]
    fn direct_pane_resubmit_proof_line_is_operator_greppable() {
        let accepted = direct_pane_resubmit_proof_line(DirectPaneResubmitProofFacts {
            file_display: "/tmp/plan.md",
            pane: "%42",
            harness_binary: "codex",
            submit_key: "Enter",
            status: DirectPaneSubmitStatus::Accepted,
            elapsed_ms: 120,
            attempt: 1,
            editor_attempt_id: None,
        });
        assert_eq!(
            accepted,
            "route_submit_resubmit file=/tmp/plan.md pane=%42 harness=codex action=submit_key key=Enter result=accepted elapsed_ms=120 attempt=1"
        );

        let still = direct_pane_resubmit_proof_line(DirectPaneResubmitProofFacts {
            file_display: "/tmp/plan.md",
            pane: "%42",
            harness_binary: "claude",
            submit_key: "Enter",
            status: DirectPaneSubmitStatus::TimedOut,
            elapsed_ms: 300,
            attempt: 3,
            editor_attempt_id: Some("attempt_1_2"),
        });
        assert_eq!(
            still,
            "route_submit_resubmit file=/tmp/plan.md pane=%42 harness=claude action=submit_key key=Enter result=still_visible elapsed_ms=300 attempt=3 editor_attempt_id=attempt_1_2"
        );
        assert_eq!(
            direct_pane_resubmit_result_label(DirectPaneSubmitStatus::Accepted),
            "accepted"
        );
        assert_eq!(
            direct_pane_resubmit_result_label(DirectPaneSubmitStatus::TimedOut),
            "still_visible"
        );
    }

    #[test]
    fn route_latency_status_marks_elapsed_at_budget_as_over_budget() {
        assert_eq!(route_latency_status(999, 1000), RouteLatencyStatus::Ok);
        assert_eq!(
            route_latency_status(1000, 1000),
            RouteLatencyStatus::OverBudget
        );
        assert_eq!(RouteLatencyStatus::Ok.label(), "ok");
        assert_eq!(RouteLatencyStatus::OverBudget.label(), "over_budget");
    }

    #[test]
    fn route_latency_message_includes_budget_status_and_editor_attempt() {
        let ok = route_latency_message(RouteLatencyFacts {
            phase: "dispatch_start_proof",
            elapsed_ms: 999,
            budget_ms: 1000,
            pane: "%1",
            harness_binary: "codex",
            outcome: "submitted",
            editor_attempt_id: None,
        });
        assert!(ok.contains("status=ok"), "{ok}");
        assert!(ok.contains("elapsed_ms=999"), "{ok}");

        let slow = route_latency_message(RouteLatencyFacts {
            phase: "dispatch_start_proof",
            elapsed_ms: 10_000,
            budget_ms: 10_000,
            pane: "%1",
            harness_binary: "codex",
            outcome: "unproven_but_accepted",
            editor_attempt_id: Some("attempt_1_2"),
        });
        assert!(slow.contains("status=over_budget"), "{slow}");
        assert!(slow.contains("outcome=unproven_but_accepted"), "{slow}");
        assert!(slow.contains("editor_attempt_id=attempt_1_2"), "{slow}");
    }

    #[test]
    fn route_startup_miss_diagnostic_names_retry_command() {
        let message = route_startup_miss_diagnostic_message(RouteStartupMissDiagnosticFacts {
            file_display: "tasks/agent-doc/agent-doc-bugs2.md",
            reason: "routed trigger accepted but no document cycle started for pending #smdq",
        });
        assert!(message.contains("[agent-doc] startup-miss:"), "{message}");
        assert!(
            message.contains("agent-doc start tasks/agent-doc/agent-doc-bugs2.md"),
            "{message}"
        );
    }

    #[test]
    fn route_busy_diagnostic_names_rerun_and_busy_harness() {
        let message = route_busy_diagnostic_message(RouteBusyDiagnosticFacts {
            file_display: "plan.md",
            harness_binary: "claude",
        });
        assert!(message.contains("live claude session is busy"), "{message}");
        assert!(message.contains("agent-doc route plan.md"), "{message}");
        assert!(message.contains("rerun `Run Agent Doc`"), "{message}");
    }

    #[test]
    fn route_busy_queued_diagnostic_names_turn_in_progress_and_no_rerun() {
        let message = route_busy_queued_diagnostic_message(RouteBusyQueuedDiagnosticFacts {
            file_display: "plan.md",
            harness_binary: "claude",
            user_outcome_fields: "ui_outcome=queued_behind_owner",
        });
        assert!(message.contains("turn in progress"), "{message}");
        assert!(message.contains("queued"), "{message}");
        assert!(message.contains("plan.md"), "{message}");
        assert!(message.contains("claude"), "{message}");
        assert!(
            message.contains("ui_outcome=queued_behind_owner"),
            "{message}"
        );
        assert!(message.contains("No need to rerun"), "{message}");
        assert!(!message.contains("rerun `Run Agent Doc`"), "{message}");
    }

    #[test]
    fn failclosed_wait_context_distinguishes_busy_turn_from_cold_startup() {
        assert_eq!(
            failclosed_wait_context("claude", None, 12),
            "waited 12s for claude startup"
        );
        assert_eq!(
            failclosed_wait_context("claude", Some("active claude turn"), 12),
            "the pane is busy on an active claude turn (active claude turn), not cold-starting"
        );
    }

    #[test]
    fn busy_existing_pane_error_formats_plain_route_facts() {
        let refused = format_busy_existing_pane_error(
            "plan.md",
            "%42",
            "codex",
            "dispatch_only",
            Some("still shows active turn"),
            false,
        );
        assert!(
            refused.contains("registered pane %42 for plan.md is not showing an idle codex prompt (still shows active turn)"),
            "{refused}"
        );
        assert!(
            refused.contains("busy session (dispatch_only)"),
            "{refused}"
        );

        let after_fix =
            format_busy_existing_pane_error("plan.md", "%42", "codex", "existing_pane", None, true);
        assert!(
            after_fix.contains("after automatically applying `agent-doc fix plan.md` once"),
            "{after_fix}"
        );
        assert!(
            after_fix.contains("busy session (existing_pane)"),
            "{after_fix}"
        );
    }

    #[test]
    fn duplicate_pane_policy_error_includes_manual_tmux_commands() {
        let rendered = duplicate_pane_policy_error_message(DuplicatePanePolicyErrorFacts {
            session_name: "test",
            file_path: "tasks/agent-doc/agent-doc-bugs2.md",
            anchor_pane: Some("%42"),
            cause: "split-window failed alongside pane %42 (too small)",
        });
        assert!(rendered.contains("tmux list-panes -t test:agent-doc"));
        assert!(rendered.contains("tmux kill-pane -t %42"));
        assert!(rendered.contains("agent-doc tasks/agent-doc/agent-doc-bugs2.md"));
        assert!(rendered.contains("split-window failed alongside pane %42"));
    }

    #[test]
    fn route_dispatch_bug_report_item_includes_required_evidence() {
        let item = route_dispatch_bug_report_item(RouteDispatchBugReportItemFacts {
            document_display: "/tmp/run-agent-doc.md",
            document_id: "run-agent-doc",
            pane: "%7",
            phase: "dispatch_start_proof",
            issue: "accepted_without_dispatch_start_proof",
            result: "accepted_without_dispatch_start_proof",
            elapsed_ms: 10_000,
            actor_generation: Some(1),
            editor_attempt_id: Some("attempt_1"),
            dispatch_proof_state: None,
            diagnostic_path: Some("/tmp/.agent-doc/logs/route-submit/snapshot.txt"),
            ops_log_path: Some("/tmp/.agent-doc/logs/ops.log"),
        })
        .unwrap();

        assert!(item.contains("#jbrunautobug"), "{item}");
        assert!(item.contains("#agent-doc-bug"), "{item}");
        assert!(
            item.contains("failure_class=accepted_without_dispatch_start_proof"),
            "{item}"
        );
        assert!(item.contains("stage=dispatch_start_proof"), "{item}");
        assert!(item.contains("pane=%7"), "{item}");
        assert!(item.contains("actor_generation=1"), "{item}");
        assert!(item.contains("dispatch_proof_state=none"), "{item}");
        assert!(item.contains("diagnostic_path="), "{item}");
        assert!(item.contains("ops_log_path="), "{item}");
        assert!(
            item.contains(
                "ops_log_marker=route_submit_issue(issue=accepted_without_dispatch_start_proof"
            ),
            "{item}"
        );
        assert!(
            item.contains("[symptom-key invariant=run_agent_doc_route_dispatch_failure"),
            "{item}"
        );
    }

    #[test]
    fn dispatch_only_proof_policy_accepts_enter_delivery_for_all_harnesses() {
        assert!(dispatch_only_should_print_unproven_progress());
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
        assert!(log.contains("submit_mode=tmux_text_enter"));
        assert!(log.contains("proof=accepted"));
        assert!(log.contains("proof_scope=accepted_only"));

        let console = dispatch_only_sent_console_message(facts);
        assert!(console.contains("accepted proof"));
        assert!(console.contains("accepted-only"));

        let unproven = accepted_only_dispatch_start_log_message(facts);
        assert!(unproven.contains("route_dispatch_only_submit_unproven"));
        assert!(unproven.contains("proof_scope=accepted_only"));

        let refusal = accepted_only_dispatch_start_refusal_message(facts);
        assert!(refusal.contains("tmux_text_enter"));
        assert!(refusal.contains("only pane-input acceptance proof was available"));
        assert!(refusal.contains("treating this as not dispatched"));
    }

    #[test]
    fn dispatch_only_direct_submit_mode_is_harness_specific() {
        assert_eq!(
            DispatchOnlyReopenDelivery::DirectPaneSubmit.submit_mode_for_harness("codex"),
            "tmux_text_enter"
        );
        assert_eq!(
            DispatchOnlyReopenDelivery::DirectPaneSubmit.submit_mode_for_harness("opencode"),
            "tmux_text_enter"
        );
        assert_eq!(
            DispatchOnlyReopenDelivery::DirectPaneSubmit.submit_mode_for_harness("claude"),
            "tmux_text_enter"
        );
        assert_eq!(
            DispatchOnlyReopenDelivery::SupervisorIpcOnce.submit_mode_for_harness("codex"),
            "supervisor_normalized_submit"
        );
    }

    #[test]
    fn interactive_substate_gets_dedicated_guard_reason() {
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
    fn routed_reopen_decision_separates_start_wait_and_queue_paths() {
        let starting = decide_authoritative_reopen(RoutedReopenFacts {
            actor_state: ActorDispatchState::Starting,
            prompt_ready: false,
            has_prompt_bearing_work: true,
            mode: ReopenMode::DispatchOnly,
            degraded_authority: false,
            dispatch_eligible: true,
        });
        assert_eq!(starting.decision, RouteDecision::WaitForReady);
        assert_eq!(starting.reason, "starting_requires_prompt_ready_barrier");

        assert_eq!(
            decide_authoritative_reopen(RoutedReopenFacts {
                actor_state: ActorDispatchState::Busy,
                prompt_ready: false,
                has_prompt_bearing_work: true,
                mode: ReopenMode::DispatchOnly,
                degraded_authority: false,
                dispatch_eligible: true,
            })
            .decision,
            RouteDecision::FailClosed
        );
        assert_eq!(
            decide_authoritative_reopen(RoutedReopenFacts {
                actor_state: ActorDispatchState::Busy,
                prompt_ready: false,
                has_prompt_bearing_work: true,
                mode: ReopenMode::Managed,
                degraded_authority: false,
                dispatch_eligible: true,
            })
            .decision,
            RouteDecision::ReuseReady
        );
    }

    #[test]
    fn busy_projection_repaired_only_with_proven_ready_prompt() {
        assert!(busy_projection_repaired_by_ready_prompt(
            ActorDispatchState::Busy,
            true
        ));
        assert!(!busy_projection_repaired_by_ready_prompt(
            ActorDispatchState::Busy,
            false
        ));
        for state in [
            ActorDispatchState::Ready,
            ActorDispatchState::Starting,
            ActorDispatchState::WaitingInput,
            ActorDispatchState::Blocked,
            ActorDispatchState::Closed,
            ActorDispatchState::Missing,
        ] {
            assert!(!busy_projection_repaired_by_ready_prompt(state, true));
        }
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
        assert!(dispatch_only_focus_only_should_fail_closed(
            ReopenMode::DispatchOnly,
            ActorDispatchState::Busy
        ));
        assert!(!dispatch_only_focus_only_should_fail_closed(
            ReopenMode::Managed,
            ActorDispatchState::Busy
        ));
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
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Busy,
                prompt_ready: true,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Ready
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: ActorDispatchState::Closed,
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
        assert_eq!(routed_dispatch_start_timeout(true), Duration::from_secs(1));
    }

    #[test]
    fn route_ack_timeouts_extend_for_startup_and_live_children() {
        assert_eq!(fresh_route_start_ack_timeout(true), Duration::from_secs(2));
        assert_eq!(
            fresh_route_start_ack_timeout(false),
            Duration::from_secs(30)
        );
        assert_eq!(
            routed_cycle_ack_timeout(false, true),
            Duration::from_secs(1)
        );
        assert_eq!(routed_cycle_ack_timeout(true, true), Duration::from_secs(2));
        assert_eq!(
            routed_cycle_ack_timeout(false, false),
            Duration::from_secs(15)
        );
        assert_eq!(
            routed_cycle_ack_timeout(true, false),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn existing_pane_ready_timeout_is_shortened_in_tests() {
        assert_eq!(existing_pane_ready_timeout(true), Duration::from_secs(2));
        assert_eq!(existing_pane_ready_timeout(false), Duration::from_secs(15));
    }

    #[test]
    fn dispatch_only_busy_refusal_message_distinguishes_active_turn_from_cold_wait() {
        let active = dispatch_only_busy_refusal_message(DispatchOnlyBusyRefusalFacts {
            generation: 282,
            file_display: "/tmp/sampleorders.md",
            dispatch_pane: "%1",
            harness_binary: "claude",
            reason: "actor not ready",
            wait_secs: 8,
            recovery_hint: "Run `agent-doc session interrupt-clear /tmp/sampleorders.md`.",
            active_turn_busy_cue: Some("Working (7m 29s · esc to interrupt)"),
            blocked_outcome_fields: "ui_outcome=blocked_with_exact_unblocker unblocker=wait_for_owner_turn_to_finish",
        });
        assert!(
            active.contains("busy on an active") && active.contains("esc to interrupt"),
            "active-turn refusal must name the busy turn cue: {active}"
        );
        assert!(
            active.contains("ui_outcome=blocked_with_exact_unblocker")
                && active.contains("unblocker=wait_for_owner_turn_to_finish"),
            "active-turn refusal must carry the typed unblocker outcome: {active}"
        );
        assert!(
            !active.contains("after waiting"),
            "active-turn refusal must not claim a ready-wait that was skipped: {active}"
        );

        let cold = dispatch_only_busy_refusal_message(DispatchOnlyBusyRefusalFacts {
            generation: 282,
            file_display: "/tmp/sampleorders.md",
            dispatch_pane: "%1",
            harness_binary: "claude",
            reason: "actor not ready",
            wait_secs: 8,
            recovery_hint: "Run `agent-doc session interrupt-clear /tmp/sampleorders.md`.",
            active_turn_busy_cue: None,
            blocked_outcome_fields: "ui_outcome=blocked_with_exact_unblocker unblocker=wait_for_dispatch_ready_prompt",
        });
        assert!(
            cold.contains("after waiting") && cold.contains("dispatch-ready prompt"),
            "no-cue refusal keeps the cold-start ready-wait wording: {cold}"
        );
        assert!(
            cold.contains("ui_outcome=blocked_with_exact_unblocker")
                && cold.contains("unblocker=wait_for_dispatch_ready_prompt"),
            "cold-wait refusal must carry the typed unblocker outcome: {cold}"
        );
    }

    #[test]
    fn busy_refusal_wait_secs_reports_override_then_default() {
        assert_eq!(
            dispatch_only_busy_refusal_wait_secs(
                Some(Duration::from_secs(60)),
                Duration::from_secs(8),
            ),
            60
        );
        assert_eq!(
            dispatch_only_busy_refusal_wait_secs(None, Duration::from_secs(8)),
            8
        );
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
    fn direct_pane_accepted_dispatch_only_submit_skips_optional_start_proof() {
        assert!(
            !direct_pane_should_await_dispatch_start_proof(DirectPaneDispatchStartProofFacts {
                await_start_proof: false,
                submit_status: DirectPaneSubmitStatus::Accepted,
            }),
            "dispatch-only editor reroutes must not pay the optional proof timeout after accepted input"
        );
        assert!(
            direct_pane_should_await_dispatch_start_proof(DirectPaneDispatchStartProofFacts {
                await_start_proof: false,
                submit_status: DirectPaneSubmitStatus::TimedOut,
            }),
            "when submit acceptance is unobserved, route may still wait for stronger dispatch-start proof"
        );
        assert!(
            direct_pane_should_await_dispatch_start_proof(DirectPaneDispatchStartProofFacts {
                await_start_proof: true,
                submit_status: DirectPaneSubmitStatus::Accepted,
            }),
            "startup dispatch still requires dispatch-start proof after accepted input"
        );
    }

    #[test]
    fn direct_pane_acceptance_waits_for_stable_empty_capture() {
        let mut state = DirectPaneAcceptancePollState::default();
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(0), false),
            None
        );
        assert!(!state.saw_trigger_visible());
        assert_eq!(
            direct_pane_acceptance_poll_status(
                &mut state,
                DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR - Duration::from_millis(1),
                false
            ),
            None
        );
        assert_eq!(
            direct_pane_acceptance_poll_status(
                &mut state,
                DIRECT_PANE_EMPTY_ACCEPTANCE_STABLE_FOR,
                false
            ),
            Some(DirectPaneSubmitStatus::Accepted)
        );
    }

    #[test]
    fn direct_pane_acceptance_accepts_after_visible_draft_disappears() {
        let mut state = DirectPaneAcceptancePollState::default();
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(0), false),
            None
        );
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(150), true),
            None
        );
        assert!(state.saw_trigger_visible());
        assert_eq!(
            direct_pane_acceptance_poll_status(&mut state, Duration::from_millis(300), false),
            Some(DirectPaneSubmitStatus::Accepted)
        );
    }

    #[test]
    fn direct_pane_not_dispatched_requires_empty_idle_never_seen_trigger() {
        assert!(direct_pane_not_dispatched(DirectPaneNotDispatchedFacts {
            saw_trigger_visible: false,
            pane_idle_dispatch_ready: true,
        }));
        assert!(!direct_pane_not_dispatched(DirectPaneNotDispatchedFacts {
            saw_trigger_visible: true,
            pane_idle_dispatch_ready: true,
        }));
        assert!(!direct_pane_not_dispatched(DirectPaneNotDispatchedFacts {
            saw_trigger_visible: false,
            pane_idle_dispatch_ready: false,
        }));
    }

    #[test]
    fn direct_pane_resubmit_only_on_timeout_with_visible_trigger() {
        assert!(direct_pane_needs_enter_resubmit(
            DirectPaneEnterResubmitFacts {
                profile_allows_pending_draft_enter_resubmit: true,
                status: DirectPaneSubmitStatus::TimedOut,
                trigger_visible: true,
            }
        ));
        assert!(!direct_pane_needs_enter_resubmit(
            DirectPaneEnterResubmitFacts {
                profile_allows_pending_draft_enter_resubmit: false,
                status: DirectPaneSubmitStatus::TimedOut,
                trigger_visible: true,
            }
        ));
        assert!(!direct_pane_needs_enter_resubmit(
            DirectPaneEnterResubmitFacts {
                profile_allows_pending_draft_enter_resubmit: true,
                status: DirectPaneSubmitStatus::Accepted,
                trigger_visible: true,
            }
        ));
        assert!(!direct_pane_needs_enter_resubmit(
            DirectPaneEnterResubmitFacts {
                profile_allows_pending_draft_enter_resubmit: true,
                status: DirectPaneSubmitStatus::TimedOut,
                trigger_visible: false,
            }
        ));
    }

    #[test]
    fn direct_pane_resubmit_is_bounded_by_attempt_budget() {
        for attempts_sent in 0..DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT {
            assert!(
                direct_pane_can_continue_enter_resubmit(DirectPaneEnterResubmitAttemptFacts {
                    profile_allows_pending_draft_enter_resubmit: true,
                    status: DirectPaneSubmitStatus::TimedOut,
                    trigger_visible: true,
                    attempts_sent,
                    max_attempts: DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT,
                }),
                "attempt {attempts_sent} should still be eligible while the trigger remains visible"
            );
        }
        assert!(!direct_pane_can_continue_enter_resubmit(
            DirectPaneEnterResubmitAttemptFacts {
                profile_allows_pending_draft_enter_resubmit: true,
                status: DirectPaneSubmitStatus::TimedOut,
                trigger_visible: true,
                attempts_sent: DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT,
                max_attempts: DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT,
            }
        ));
    }

    #[test]
    fn direct_pane_full_resend_not_landed_is_bounded() {
        assert!(direct_pane_can_full_resend_not_landed(
            DirectPaneFullResendFacts {
                not_dispatched: true,
                attempts_sent: 0,
                max_attempts: 1,
            }
        ));
        assert!(!direct_pane_can_full_resend_not_landed(
            DirectPaneFullResendFacts {
                not_dispatched: true,
                attempts_sent: 1,
                max_attempts: 1,
            }
        ));
        assert!(!direct_pane_can_full_resend_not_landed(
            DirectPaneFullResendFacts {
                not_dispatched: false,
                attempts_sent: 0,
                max_attempts: 1,
            }
        ));
    }

    #[test]
    fn direct_pane_max_enter_resubmits_parses_positive_env_value() {
        assert_eq!(
            direct_pane_max_enter_resubmits_from_env_value(Some("42")),
            42
        );
        assert_eq!(
            direct_pane_max_enter_resubmits_from_env_value(Some(" 7 ")),
            7
        );
        assert_eq!(
            direct_pane_max_enter_resubmits_from_env_value(Some("0")),
            DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT
        );
        assert_eq!(
            direct_pane_max_enter_resubmits_from_env_value(Some("nope")),
            DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT
        );
        assert_eq!(
            direct_pane_max_enter_resubmits_from_env_value(None),
            DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT
        );
    }

    #[test]
    fn direct_pane_enter_resubmit_retries_at_least_once_per_second() {
        let timeout = direct_pane_submit_acceptance_timeout();
        assert!(
            timeout <= Duration::from_secs(1),
            "visible drafted triggers should earn another submit key at least once/second; timeout={timeout:?}"
        );

        let default_total_ms =
            timeout.as_millis() * u128::from(DIRECT_PANE_MAX_ENTER_RESUBMITS_DEFAULT as u64);
        assert!(
            default_total_ms >= 30_000,
            "default retry budget should preserve a roughly 30s recovery window"
        );
    }

    #[test]
    fn direct_pane_existing_draft_submit_requires_visible_draft_and_profile() {
        assert!(direct_pane_can_enter_existing_draft(
            DirectPaneExistingDraftSubmitFacts {
                profile_allows_pending_draft_enter_resubmit: true,
                trigger_visible: true,
            }
        ));
        assert!(!direct_pane_can_enter_existing_draft(
            DirectPaneExistingDraftSubmitFacts {
                profile_allows_pending_draft_enter_resubmit: false,
                trigger_visible: true,
            }
        ));
        assert!(!direct_pane_can_enter_existing_draft(
            DirectPaneExistingDraftSubmitFacts {
                profile_allows_pending_draft_enter_resubmit: true,
                trigger_visible: false,
            }
        ));
    }

    #[test]
    fn closeout_block_dispatch_prefers_queued_prompt_context() {
        assert_eq!(
            classify_closeout_block_dispatch(CloseoutBlockDispatchFacts {
                recovery_queues_prompt_for_after_closeout: true,
                active_queue_head: Some("existing-head".to_string()),
            }),
            CloseoutBlockDispatchDecision::EnqueuePromptForAfterCloseout
        );
    }

    #[test]
    fn closeout_block_dispatch_waits_on_existing_active_queue() {
        assert_eq!(
            classify_closeout_block_dispatch(CloseoutBlockDispatchFacts {
                recovery_queues_prompt_for_after_closeout: false,
                active_queue_head: Some("queue-head".to_string()),
            }),
            CloseoutBlockDispatchDecision::WaitForActiveQueueHead {
                head: "queue-head".to_string(),
            }
        );
    }

    #[test]
    fn closeout_block_dispatch_fails_closed_without_prompt_or_queue() {
        assert_eq!(
            classify_closeout_block_dispatch(CloseoutBlockDispatchFacts {
                recovery_queues_prompt_for_after_closeout: false,
                active_queue_head: None,
            }),
            CloseoutBlockDispatchDecision::FailClosed
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
    fn dispatch_blocked_user_facing_outcome_fields_classify_stage_and_reason() {
        assert!(
            dispatch_blocked_user_facing_outcome_fields("actor_busy_draining", "busy")
                .contains("ui_outcome=queued_behind_owner")
        );
        assert!(
            dispatch_blocked_user_facing_outcome_fields(
                "queue_paused",
                "supervisor_binary_stale pid=42"
            )
            .contains("ui_outcome=recovered_and_retried")
        );
        assert!(
            dispatch_blocked_user_facing_outcome_fields("queue_paused", "typed_component_drift")
                .contains("ui_outcome=real_component_conflict")
        );
        assert!(
            dispatch_blocked_user_facing_outcome_fields("queue_paused", "zero drainable head")
                .contains("ui_outcome=no_drainable_work")
        );
        assert!(
            dispatch_blocked_user_facing_outcome_fields("queue_paused", "manual review")
                .contains("ui_outcome=deferred_for_operator_proof")
        );
        assert_eq!(
            dispatch_blocked_user_facing_outcome_fields("queue_paused", "operator pause"),
            "ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=resume_or_clear_queue_control"
        );
    }

    #[test]
    fn dispatch_blocked_proof_fields_use_supplied_io_facts() {
        let head = "abc";
        let trigger = "agent-doc doc.md";

        assert_eq!(
            dispatch_blocked_proof_fields(DispatchBlockedProofFacts {
                stage: "queue_paused",
                reason: "manual review",
                blocked_head: Some(head),
                trigger: Some(trigger),
            }),
            format!(
                "ui_outcome_contract=ui-outcome-v1 ui_outcome=deferred_for_operator_proof ui_outcome_class=operator next_action=operator_proof_required blocked_head_bytes=3 blocked_head_sha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad trigger_bytes=16 trigger_sha256={}",
                sha256_hex(trigger)
            )
        );
    }

    fn codex_prompt_line(line: &str) -> bool {
        line.trim_start().starts_with('\u{203a}')
    }

    #[test]
    fn route_trigger_visible_in_current_draft_enters_only_current_codex_draft() {
        let trigger = "agent-doc tasks/agent-doc/agent-doc-bugs2.md";

        let drafted = "\
history line
› agent-doc tasks/agent-doc/agent-doc-bugs2.md
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert!(
            route_trigger_visible_in_current_draft(drafted, trigger, codex_prompt_line),
            "visible Codex composer draft should be eligible for append-free Enter"
        );

        let accumulated = "\
› agent-doc tasks/agent-doc/agent-doc-bugs2.md agent-doc tasks/agent-doc/agent-doc-bugs2.md
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert!(
            route_trigger_visible_in_current_draft(accumulated, trigger, codex_prompt_line),
            "accumulated duplicate drafts must still be treated as current input"
        );

        let stale_scrollback = "\
› agent-doc tasks/agent-doc/agent-doc-bugs2.md
preflight complete
›
";
        assert!(
            !route_trigger_visible_in_current_draft(stale_scrollback, trigger, codex_prompt_line),
            "an idle prompt below the trigger means it is scrollback, not the active draft"
        );

        let interrupted_with_new_draft = "\
╭─────────────────────────────────────────────╮
│ >_ OpenAI Codex (v0.142.0)                  │
╰─────────────────────────────────────────────╯

› agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/sampleportal.md

■ Conversation interrupted - tell the model what to do differently.

› Use /skills to list available skills

gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% used
";
        assert!(
            !route_trigger_visible_in_current_draft(
                interrupted_with_new_draft,
                "agent-doc /home/brian/work/btakita/agent-loop/tasks/professional/sampleportal.md",
                codex_prompt_line,
            ),
            "a cancelled route trigger in scrollback must not receive Enter when a newer composer draft exists"
        );
    }

    #[test]
    fn route_trigger_visible_in_current_draft_handles_wrapped_codex_path() {
        let trigger =
            "agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md";
        let content = "\
› agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-
doc-bugs2.md
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";

        assert!(
            route_trigger_visible_in_current_draft(content, trigger, codex_prompt_line),
            "wrapped current drafts should be submitted with the profile submit key rather than appended again"
        );
    }

    #[test]
    fn route_trigger_visible_in_current_draft_ignores_codex_blank_padding() {
        let trigger =
            "agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md";
        let content = "\
╭─────────────────────────────────────────────╮
│ >_ OpenAI Codex (v0.141.0)                  │
╰─────────────────────────────────────────────╯

  Tip: Use /side to start a side conversation in a temporary fork without polluting the main thread.


› agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs2.md


  gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% used








";

        assert!(
            route_trigger_visible_in_current_draft(content, trigger, codex_prompt_line),
            "blank-padded Codex composer captures should still expose the current draft for late Enter retry"
        );
    }

    #[test]
    fn route_trigger_visible_in_current_draft_matches_relative_codex_path() {
        let trigger =
            "agent-doc /home/brian/work/btakita/agent-loop/src/sample-app/tasks/sampleorders.md";
        let drafted = "\
› agent-doc tasks/sampleorders.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/sample-app · Context 0% used
";

        assert!(
            route_trigger_visible_in_current_draft(drafted, trigger, codex_prompt_line),
            "a visible relative-path Codex draft for the same target should receive Enter instead of an appended absolute trigger"
        );

        let stale_scrollback = "\
› agent-doc tasks/sampleorders.md
preflight complete
›
";
        assert!(
            !route_trigger_visible_in_current_draft(stale_scrollback, trigger, codex_prompt_line),
            "an idle prompt below an equivalent relative-path draft still proves scrollback"
        );

        let different_target = "\
› agent-doc tasks/sampleportal.md
gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/sample-app · Context 0% used
";
        assert!(
            !route_trigger_visible_in_current_draft(different_target, trigger, codex_prompt_line),
            "relative-path equivalence must not collapse different document names"
        );
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
    fn queue_pause_predates_boot_requires_known_later_boot() {
        assert!(queue_pause_predates_boot(99, Some(100)));
        assert!(!queue_pause_predates_boot(100, Some(100)));
        assert!(!queue_pause_predates_boot(101, Some(100)));
        assert!(!queue_pause_predates_boot(99, None));
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
