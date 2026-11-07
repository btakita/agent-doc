//! Pure supervisor replacement request parsing.

use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SupervisorReplacementRequestFields<'a> {
    pub state: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub diagnostic_payload: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupervisorReplacementMode {
    Continue,
    Fresh,
}

impl SupervisorReplacementMode {
    pub fn parse(raw: &str) -> Result<Self, SupervisorReplacementParseError> {
        let mode = raw.trim();
        match mode {
            "continue" => Ok(Self::Continue),
            "fresh" => Ok(Self::Fresh),
            other => Err(SupervisorReplacementParseError::UnsupportedMode(
                other.to_string(),
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Fresh => "fresh",
        }
    }
}

impl fmt::Display for SupervisorReplacementMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Wire prefix that marks an operator "Restart Agent" intent (`#agentrestartwire`).
///
/// `session_actor_cmd::restart_agent` encodes the operator's harness-replacement
/// intent as `agent:<mode>` so it survives controller transport as a distinct
/// request from a plain supervisor recycle. The supervisor IPC layer
/// (`agent_doc_supervisor_io::ipc::decode_restart_intent`) has always understood
/// it; the controller did not, so every editor "Restart Agent" invocation failed
/// with `unsupported supervisor replacement mode `agent:continue`` before the
/// request ever reached a supervisor.
const RESTART_AGENT_WIRE_PREFIX: &str = "agent:";

/// An operator replacement request: which conversation lineage to keep, and
/// whether the operator explicitly asked to replace the harness child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SupervisorReplacementIntent {
    pub mode: SupervisorReplacementMode,
    /// `true` when the request arrived as `agent:<mode>` — an explicit
    /// "Restart Agent" that exists to re-resolve frontmatter (including an
    /// `agent:` harness switch) and replace the running harness child.
    pub restart_agent: bool,
}

impl SupervisorReplacementIntent {
    pub fn parse(raw: &str) -> Result<Self, SupervisorReplacementParseError> {
        let raw = raw.trim();
        let (mode, restart_agent) = match raw.strip_prefix(RESTART_AGENT_WIRE_PREFIX) {
            Some(mode) => (mode, true),
            None => (raw, false),
        };
        Ok(Self {
            mode: SupervisorReplacementMode::parse(mode)?,
            restart_agent,
        })
    }

    /// The wire form, preserved verbatim so the downstream supervisor IPC still
    /// sees the operator's Restart Agent intent.
    pub fn wire_mode(self) -> String {
        if self.restart_agent {
            format!("{RESTART_AGENT_WIRE_PREFIX}{}", self.mode.as_str())
        } else {
            self.mode.as_str().to_string()
        }
    }

    /// Whether this request authorizes replacing a live harness child that is
    /// still serving this document.
    ///
    /// Fresh mode always does. Continue mode normally preserves the child — but
    /// an explicit Restart Agent request exists precisely to replace it, so
    /// preserving it there would silently discard the operator's intent (and,
    /// with a changed `agent:`, keep the old harness running forever).
    pub const fn replaces_live_harness(self) -> bool {
        self.restart_agent || matches!(self.mode, SupervisorReplacementMode::Fresh)
    }
}

impl fmt::Display for SupervisorReplacementIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.restart_agent {
            f.write_str(RESTART_AGENT_WIRE_PREFIX)?;
        }
        f.write_str(self.mode.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParsedSupervisorReplacementRequest {
    pub mode: SupervisorReplacementMode,
    pub restart_agent: bool,
    pub force: bool,
}

impl ParsedSupervisorReplacementRequest {
    pub const fn intent(self) -> SupervisorReplacementIntent {
        SupervisorReplacementIntent {
            mode: self.mode,
            restart_agent: self.restart_agent,
        }
    }

    pub fn wire_mode(self) -> String {
        self.intent().wire_mode()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorReplacementParseError {
    UnsupportedMode(String),
}

impl fmt::Display for SupervisorReplacementParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMode(mode) => {
                write!(f, "unsupported supervisor replacement mode `{mode}`")
            }
        }
    }
}

impl std::error::Error for SupervisorReplacementParseError {}

/// Result of attempting to hand a supervisor replacement to the currently
/// running supervisor over its IPC socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupervisorReplacementIpcOutcome {
    Accepted,
    Dead,
    Failed,
    /// `#netadv5` R9: the live socket accepted the `restart` request but its
    /// effect receipt did not arrive in time. The supervisor may well be
    /// executing the restart; this is "maybe accepted", never a failure.
    ResponseTimedOut,
}

/// Controller action after the supervisor IPC attempt.
///
/// An accepted non-forced replacement is owned by the live supervisor. It may
/// legitimately remain pending while an active turn drains, so a foreground
/// proof timeout is never authority to kill that supervisor or its child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupervisorReplacementEscalation {
    AwaitAcceptedInPlace,
    WaitThenEscalate,
    EscalateColdStart,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SupervisorReplacementEscalationFacts {
    pub ipc_outcome: SupervisorReplacementIpcOutcome,
    pub force: bool,
    pub initial_host_stale: bool,
}

/// Controller-owned recovery for a supervisor that predates the durable recycle
/// request protocol. This decision deliberately has no clock input: elapsed
/// time cannot authorize replacing a live process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StaleIdleSupervisorRecovery {
    NotStale,
    DeferTurnActive,
    AlreadyClaimed,
    ReplaceOnce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StaleIdleSupervisorFacts {
    /// Positive `/proc/<pid>/exe` evidence that the mapped executable was
    /// unlinked by an install.
    pub binary_unlinked: bool,
    /// A fresh harness-owned turn marker still names the pane.
    pub turn_active: bool,
    /// This controller generation already dispatched recovery for this exact
    /// supervisor pid.
    pub replacement_claimed: bool,
}

pub const fn decide_stale_idle_supervisor_recovery(
    facts: StaleIdleSupervisorFacts,
) -> StaleIdleSupervisorRecovery {
    if !facts.binary_unlinked {
        StaleIdleSupervisorRecovery::NotStale
    } else if facts.turn_active {
        StaleIdleSupervisorRecovery::DeferTurnActive
    } else if facts.replacement_claimed {
        StaleIdleSupervisorRecovery::AlreadyClaimed
    } else {
        StaleIdleSupervisorRecovery::ReplaceOnce
    }
}

pub const fn decide_supervisor_replacement_escalation(
    facts: SupervisorReplacementEscalationFacts,
) -> SupervisorReplacementEscalation {
    match (facts.ipc_outcome, facts.force, facts.initial_host_stale) {
        (SupervisorReplacementIpcOutcome::Accepted, false, _) => {
            SupervisorReplacementEscalation::AwaitAcceptedInPlace
        }
        (SupervisorReplacementIpcOutcome::Accepted, true, _) => {
            SupervisorReplacementEscalation::WaitThenEscalate
        }
        (SupervisorReplacementIpcOutcome::Dead, _, _) => {
            SupervisorReplacementEscalation::EscalateColdStart
        }
        // `#netadv5` R9: a late receipt is not a refusal. Escalating to a cold
        // start here launched a second supervisor beside one that was already
        // re-exec'ing. Treat it exactly like an accepted request: the live
        // supervisor owns it, and only an operator `force` may later escalate.
        (SupervisorReplacementIpcOutcome::ResponseTimedOut, false, _) => {
            SupervisorReplacementEscalation::AwaitAcceptedInPlace
        }
        (SupervisorReplacementIpcOutcome::ResponseTimedOut, true, _) => {
            SupervisorReplacementEscalation::WaitThenEscalate
        }
        (SupervisorReplacementIpcOutcome::Failed, true, _)
        | (SupervisorReplacementIpcOutcome::Failed, false, true) => {
            SupervisorReplacementEscalation::EscalateColdStart
        }
        (SupervisorReplacementIpcOutcome::Failed, false, false) => {
            SupervisorReplacementEscalation::FailClosed
        }
    }
}

/// `#replacementwithdraw`: whether a pending replacement kill may be withdrawn
/// because the supervisor refreshed itself in place while the request waited.
///
/// Only a replacement that exists to retire a stale binary, and that does not
/// also intend to replace the live harness child, is satisfied by a supervisor
/// that is no longer stale: the install re-exec preserves the child and gives
/// exactly the fresh-binary supervisor the replacement wanted. An operator
/// restart of a fresh supervisor, or a Restart Agent / fresh-mode request,
/// still needs the kill.
pub const fn replacement_goal_satisfied_by_refresh(
    intent: SupervisorReplacementIntent,
    initial_host_stale: bool,
    host_still_stale: bool,
) -> bool {
    initial_host_stale && !host_still_stale && !intent.replaces_live_harness()
}

/// What a cold supervisor-replacement path may do with the recorded owner pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupervisorReplacementPaneDecision {
    PreserveExistingShell,
    AutoStartNew,
    /// Continue-mode found this document's harness still alive. Preserve it;
    /// an unowned live child is more valuable than a replacement supervisor.
    PreserveLiveHarness,
    /// Fresh-mode explicitly authorizes replacing this document's harness.
    RestartLiveHarness,
    BlockLiveNonShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SupervisorReplacementPaneFacts {
    pub pane_alive: bool,
    pub current_command_is_shell: bool,
    pub runs_document_harness: bool,
}

pub const fn decide_supervisor_replacement_pane(
    intent: SupervisorReplacementIntent,
    facts: SupervisorReplacementPaneFacts,
) -> SupervisorReplacementPaneDecision {
    if !facts.pane_alive {
        return SupervisorReplacementPaneDecision::AutoStartNew;
    }
    if facts.current_command_is_shell {
        return SupervisorReplacementPaneDecision::PreserveExistingShell;
    }
    if facts.runs_document_harness {
        if intent.replaces_live_harness() {
            SupervisorReplacementPaneDecision::RestartLiveHarness
        } else {
            SupervisorReplacementPaneDecision::PreserveLiveHarness
        }
    } else {
        SupervisorReplacementPaneDecision::BlockLiveNonShell
    }
}

/// `#stalesupervisorhidden`: which witness named the supervisor a replacement
/// targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupervisorReplacementTargetSource {
    /// The recorded owner pane's own process tree.
    PaneProcessTree,
    /// The global `/proc` scan for a `start --route-owned` supervisor.
    RouteOwnedScan,
    /// The authoritative actor binding's supervisor lease.
    ActorLease,
}

impl SupervisorReplacementTargetSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PaneProcessTree => "pane_process_tree",
            Self::RouteOwnedScan => "route_owned_scan",
            Self::ActorLease => "actor_lease",
        }
    }
}

/// `#stalesupervisorhidden`: the candidate supervisor pids a replacement may
/// target, each already VERIFIED by the IO layer as a live direct
/// `agent-doc start` process serving exactly this document (route-owned or
/// operator-started), not a fork image of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SupervisorReplacementTargetFacts {
    pub pane_tree_pid: Option<u32>,
    pub route_owned_scan_pid: Option<u32>,
    pub lease_pid: Option<u32>,
}

/// Continue-mode cold start found no exact `resume:` id for the dead harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MissingResumeColdStart {
    /// Launch the route-owned supervisor without an exact conversation id.
    LaunchFresh,
    /// Do not launch. A fresh conversation would discard a thread this harness
    /// cannot mint back (`#restartresume`).
    Refuse,
}

/// Whether a continue-mode cold start may launch when the editor document has
/// no exact resume id for `harness_binary`.
///
/// Codex and Claude stay refused: their thread is the recorded id, and
/// starting fresh here is how a dead supervisor loses the conversation. Grok's
/// id is assigned by the supervisor (`--session-id`). A route-owned Grok pane
/// whose child is already gone has no harness to resume and no operator at the
/// restart prompt. Refusing leaves that pane at a bare shell; the watchdog
/// records the loss and stops. Launch a fresh child instead.
pub fn missing_resume_cold_start(harness_binary: &str) -> MissingResumeColdStart {
    match harness_binary.trim() {
        "grok" | "grok-build" => MissingResumeColdStart::LaunchFresh,
        _ => MissingResumeColdStart::Refuse,
    }
}

/// `#stalesupervisorhidden`: the supervisor a replacement stops.
///
/// The route-owned `/proc` scan used to be the only witness. It never sees an
/// operator-started `agent-doc start <FILE>` supervisor (no `--route-owned` on
/// its command line), so the 2026-11-05 replacement of `sdk-dev.md` read
/// `initial_pid=none`, recorded `cleared_no_pid`, killed nothing
/// (`NoSupervisor`), and then failed closed on the live harness that very
/// supervisor was still serving in the pane. The supervisor lease named the
/// pid all along, and the pane's process tree held it.
///
/// The pane tree outranks the global scan: the cold start that follows types
/// into THAT pane, so stopping a same-document supervisor elsewhere would leave
/// this pane live and the replacement failed closed anyway. The lease is the
/// last resort, for a replacement with no recorded pane.
pub const fn resolve_supervisor_replacement_target(
    facts: SupervisorReplacementTargetFacts,
) -> Option<(u32, SupervisorReplacementTargetSource)> {
    if let Some(pid) = facts.pane_tree_pid {
        Some((pid, SupervisorReplacementTargetSource::PaneProcessTree))
    } else if let Some(pid) = facts.route_owned_scan_pid {
        Some((pid, SupervisorReplacementTargetSource::RouteOwnedScan))
    } else if let Some(pid) = facts.lease_pid {
        Some((pid, SupervisorReplacementTargetSource::ActorLease))
    } else {
        None
    }
}

pub fn parse_supervisor_replacement_request(
    fields: SupervisorReplacementRequestFields<'_>,
) -> Result<ParsedSupervisorReplacementRequest, SupervisorReplacementParseError> {
    let payload = fields
        .diagnostic_payload
        .and_then(|payload| serde_json::from_str::<Value>(payload).ok());
    let mode = match fields.state {
        Some(state) => state.trim(),
        None => payload
            .as_ref()
            .and_then(|value| value.get("mode"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("continue"),
    };
    let force = payload
        .as_ref()
        .and_then(|value| value.get("force"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| fields.reason.is_some_and(|reason| reason.contains("force")));

    let intent = SupervisorReplacementIntent::parse(mode)?;

    Ok(ParsedSupervisorReplacementRequest {
        mode: intent.mode,
        restart_agent: intent.restart_agent,
        force,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crashed route-owned Grok child whose editor document has no
    /// `resume.grok` must still be cold-started. Codex and Claude keep the
    /// `#restartresume` refusal.
    #[test]
    fn grok_child_gone_without_resume_id_still_cold_starts() {
        assert_eq!(
            missing_resume_cold_start("grok"),
            MissingResumeColdStart::LaunchFresh
        );
        assert_eq!(
            missing_resume_cold_start("grok-build"),
            MissingResumeColdStart::LaunchFresh
        );
        assert_eq!(
            missing_resume_cold_start("codex"),
            MissingResumeColdStart::Refuse
        );
        assert_eq!(
            missing_resume_cold_start("claude"),
            MissingResumeColdStart::Refuse
        );
        assert_eq!(
            missing_resume_cold_start("opencode"),
            MissingResumeColdStart::Refuse
        );
    }

    /// `#replacementwithdraw` regression (2026-10-20 `agent-doc-bugs.md`,
    /// receipt 2172): a stale-binary `continue` replacement escalated three
    /// seconds before the supervisor re-exec'd onto the installed binary in
    /// place. Once the host is fresh, the continue replacement's goal is met.
    #[test]
    fn stale_continue_replacement_is_satisfied_by_an_in_place_refresh() {
        let intent = SupervisorReplacementIntent::parse("continue").unwrap();
        assert!(replacement_goal_satisfied_by_refresh(intent, true, false));
        assert!(!replacement_goal_satisfied_by_refresh(intent, true, true));
        // A replacement of a supervisor that was never stale is an operator
        // restart: freshness never satisfies it.
        assert!(!replacement_goal_satisfied_by_refresh(intent, false, false));
        // Restart Agent and fresh mode exist to replace the harness child.
        for raw in ["agent:continue", "fresh", "agent:fresh"] {
            let intent = SupervisorReplacementIntent::parse(raw).unwrap();
            assert!(
                !replacement_goal_satisfied_by_refresh(intent, true, false),
                "{raw}"
            );
        }
    }

    #[test]
    fn gh136c_stale_idle_recovery_is_total_and_clock_free() {
        for binary_unlinked in [false, true] {
            for turn_active in [false, true] {
                for replacement_claimed in [false, true] {
                    let actual = decide_stale_idle_supervisor_recovery(StaleIdleSupervisorFacts {
                        binary_unlinked,
                        turn_active,
                        replacement_claimed,
                    });
                    let expected = if !binary_unlinked {
                        StaleIdleSupervisorRecovery::NotStale
                    } else if turn_active {
                        StaleIdleSupervisorRecovery::DeferTurnActive
                    } else if replacement_claimed {
                        StaleIdleSupervisorRecovery::AlreadyClaimed
                    } else {
                        StaleIdleSupervisorRecovery::ReplaceOnce
                    };
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    fn supervisor_replacement_defaults_to_continue_without_force() {
        let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: None,
            reason: None,
            diagnostic_payload: None,
        })
        .unwrap();

        assert_eq!(
            parsed,
            ParsedSupervisorReplacementRequest {
                mode: SupervisorReplacementMode::Continue,
                restart_agent: false,
                force: false,
            }
        );
    }

    #[test]
    fn supervisor_replacement_parses_json_payload_mode_and_force() {
        let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: None,
            reason: None,
            diagnostic_payload: Some(r#"{"mode":"fresh","force":true}"#),
        })
        .unwrap();

        assert_eq!(
            parsed,
            ParsedSupervisorReplacementRequest {
                mode: SupervisorReplacementMode::Fresh,
                restart_agent: false,
                force: true,
            }
        );
    }

    #[test]
    fn supervisor_replacement_state_field_takes_mode_precedence() {
        let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: Some(" continue "),
            reason: None,
            diagnostic_payload: Some(r#"{"mode":"fresh","force":false}"#),
        })
        .unwrap();

        assert_eq!(
            parsed,
            ParsedSupervisorReplacementRequest {
                mode: SupervisorReplacementMode::Continue,
                restart_agent: false,
                force: false,
            }
        );
    }

    #[test]
    fn supervisor_replacement_json_force_overrides_reason_text() {
        let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: Some("fresh"),
            reason: Some("operator_force_request"),
            diagnostic_payload: Some(r#"{"force":false}"#),
        })
        .unwrap();

        assert_eq!(
            parsed,
            ParsedSupervisorReplacementRequest {
                mode: SupervisorReplacementMode::Fresh,
                restart_agent: false,
                force: false,
            }
        );
    }

    #[test]
    fn supervisor_replacement_falls_back_to_reason_force_for_non_json_payload() {
        let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: Some("continue"),
            reason: Some("operator_force_request"),
            diagnostic_payload: Some("not json"),
        })
        .unwrap();

        assert!(parsed.force);
    }

    #[test]
    fn supervisor_replacement_rejects_unsupported_mode() {
        let err = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: Some("restart"),
            reason: None,
            diagnostic_payload: None,
        })
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "unsupported supervisor replacement mode `restart`"
        );
    }

    #[test]
    fn supervisor_replacement_rejects_explicit_blank_mode() {
        let err = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: Some("   "),
            reason: None,
            diagnostic_payload: Some(r#"{"mode":"fresh"}"#),
        })
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "unsupported supervisor replacement mode ``"
        );
    }

    #[test]
    fn accepted_stale_replacement_waits_for_in_place_drain_without_escalating() {
        assert_eq!(
            decide_supervisor_replacement_escalation(SupervisorReplacementEscalationFacts {
                ipc_outcome: SupervisorReplacementIpcOutcome::Accepted,
                force: false,
                initial_host_stale: true,
            }),
            SupervisorReplacementEscalation::AwaitAcceptedInPlace
        );
    }

    /// `#netadv5` R9: a restart receipt that arrives after the 10s effect
    /// budget must never escalate to a cold start (a duplicate supervisor),
    /// stale host or not.
    #[test]
    fn late_restart_receipt_never_escalates_to_cold_start() {
        for initial_host_stale in [false, true] {
            assert_eq!(
                decide_supervisor_replacement_escalation(SupervisorReplacementEscalationFacts {
                    ipc_outcome: SupervisorReplacementIpcOutcome::ResponseTimedOut,
                    force: false,
                    initial_host_stale,
                }),
                SupervisorReplacementEscalation::AwaitAcceptedInPlace
            );
        }
        assert_eq!(
            decide_supervisor_replacement_escalation(SupervisorReplacementEscalationFacts {
                ipc_outcome: SupervisorReplacementIpcOutcome::ResponseTimedOut,
                force: true,
                initial_host_stale: true,
            }),
            SupervisorReplacementEscalation::WaitThenEscalate,
            "only an operator force may escalate, and only after the wait"
        );
        // An explicit refusal (positive evidence) on a stale host still escalates.
        assert_eq!(
            decide_supervisor_replacement_escalation(SupervisorReplacementEscalationFacts {
                ipc_outcome: SupervisorReplacementIpcOutcome::Failed,
                force: false,
                initial_host_stale: true,
            }),
            SupervisorReplacementEscalation::EscalateColdStart
        );
    }

    #[test]
    fn forced_accepted_replacement_may_escalate_after_wait() {
        assert_eq!(
            decide_supervisor_replacement_escalation(SupervisorReplacementEscalationFacts {
                ipc_outcome: SupervisorReplacementIpcOutcome::Accepted,
                force: true,
                initial_host_stale: true,
            }),
            SupervisorReplacementEscalation::WaitThenEscalate
        );
    }

    #[test]
    fn continue_mode_preserves_live_document_harness() {
        assert_eq!(
            decide_supervisor_replacement_pane(
                SupervisorReplacementIntent {
                    mode: SupervisorReplacementMode::Continue,
                    restart_agent: false,
                },
                SupervisorReplacementPaneFacts {
                    pane_alive: true,
                    current_command_is_shell: false,
                    runs_document_harness: true,
                },
            ),
            SupervisorReplacementPaneDecision::PreserveLiveHarness
        );
        assert_eq!(
            decide_supervisor_replacement_pane(
                SupervisorReplacementIntent {
                    mode: SupervisorReplacementMode::Fresh,
                    restart_agent: false,
                },
                SupervisorReplacementPaneFacts {
                    pane_alive: true,
                    current_command_is_shell: false,
                    runs_document_harness: true,
                },
            ),
            SupervisorReplacementPaneDecision::RestartLiveHarness
        );
    }

    /// `#agentrestartwire`: the editor "Restart Agent" action encodes its intent
    /// as `agent:<mode>`. The controller rejected the whole request with
    /// `unsupported supervisor replacement mode `agent:continue``, so no
    /// supervisor was ever asked and the harness never changed.
    #[test]
    fn supervisor_replacement_accepts_the_restart_agent_wire_prefix() {
        let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: Some("agent:continue"),
            reason: None,
            diagnostic_payload: None,
        })
        .unwrap();

        assert_eq!(
            parsed,
            ParsedSupervisorReplacementRequest {
                mode: SupervisorReplacementMode::Continue,
                restart_agent: true,
                force: false,
            }
        );

        let parsed_fresh =
            parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
                state: Some(" agent:fresh "),
                reason: None,
                diagnostic_payload: None,
            })
            .unwrap();

        assert_eq!(
            parsed_fresh,
            ParsedSupervisorReplacementRequest {
                mode: SupervisorReplacementMode::Fresh,
                restart_agent: true,
                force: false,
            }
        );
    }

    /// The prefix must survive controller transport: the supervisor's own
    /// `decode_restart_intent` is what keeps a Restart Agent from being
    /// downgraded to an in-place re-exec when the serving binary is stale.
    #[test]
    fn restart_agent_wire_mode_round_trips_through_the_controller() {
        for raw in ["agent:continue", "agent:fresh", "continue", "fresh"] {
            let parsed = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
                state: Some(raw),
                reason: None,
                diagnostic_payload: None,
            })
            .unwrap();
            assert_eq!(parsed.wire_mode(), raw, "wire mode must round-trip: {raw}");
        }
    }

    #[test]
    fn restart_agent_intent_replaces_a_live_harness_even_in_continue_mode() {
        let facts = SupervisorReplacementPaneFacts {
            pane_alive: true,
            current_command_is_shell: false,
            runs_document_harness: true,
        };

        assert_eq!(
            decide_supervisor_replacement_pane(
                SupervisorReplacementIntent {
                    mode: SupervisorReplacementMode::Continue,
                    restart_agent: true,
                },
                facts,
            ),
            SupervisorReplacementPaneDecision::RestartLiveHarness
        );
    }

    #[test]
    fn restart_agent_prefix_still_rejects_an_unsupported_mode() {
        let err = parse_supervisor_replacement_request(SupervisorReplacementRequestFields {
            state: Some("agent:restart"),
            reason: None,
            diagnostic_payload: None,
        })
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "unsupported supervisor replacement mode `restart`"
        );
    }

    /// `#stalesupervisorhidden`: a deterministic world for one replacement of
    /// the 2026-11-05 `sdk-dev.md` pane `%10`: zsh `1423217` -> supervisor
    /// `2612299` (operator-started `agent-doc start <FILE>`, no
    /// `--route-owned`) -> `grok` `1977436`, whose lease names `2612299`.
    struct ReplacementSimWorld {
        /// The supervisor, while alive, and whether it was route-owned.
        supervisor: Option<(u32, bool)>,
        /// The harness lives on the supervisor's PTY: it gets SIGHUP and
        /// exits with the supervisor.
        harness_alive: bool,
        lease_pid: Option<u32>,
        /// The pane id recorded by the replacement, `None` for a paneless one.
        pane_recorded: bool,
    }

    impl ReplacementSimWorld {
        fn sdk_dev(route_owned: bool) -> Self {
            Self {
                supervisor: Some((2_612_299, route_owned)),
                harness_alive: true,
                lease_pid: Some(2_612_299),
                pane_recorded: true,
            }
        }

        /// The IO layer's verified witnesses. Each one names only a live
        /// direct `agent-doc start` for this document.
        fn facts(&self) -> SupervisorReplacementTargetFacts {
            let live = self.supervisor;
            SupervisorReplacementTargetFacts {
                pane_tree_pid: live.filter(|_| self.pane_recorded).map(|(pid, _)| pid),
                route_owned_scan_pid: live
                    .filter(|(_, route_owned)| *route_owned)
                    .map(|(pid, _)| pid),
                lease_pid: self
                    .lease_pid
                    .filter(|pid| live.is_some_and(|(live_pid, _)| live_pid == *pid)),
            }
        }

        /// Kill the target (verified again at signal time), then classify the
        /// pane for the continue-mode cold start.
        fn replace(&mut self, target: Option<u32>) -> SupervisorReplacementPaneDecision {
            if let (Some(pid), Some((live, _))) = (target, self.supervisor)
                && pid == live
            {
                self.supervisor = None;
                self.harness_alive = false;
            }
            let shell = self.supervisor.is_none() && !self.harness_alive;
            decide_supervisor_replacement_pane(
                SupervisorReplacementIntent::parse("continue").unwrap(),
                SupervisorReplacementPaneFacts {
                    pane_alive: true,
                    current_command_is_shell: shell,
                    runs_document_harness: !shell,
                },
            )
        }
    }

    /// The incident, replayed: the route-owned scan alone found nothing, so
    /// nothing was killed and continue mode failed closed on the live grok.
    /// The pane tree names the supervisor, so the kill lands, the PTY child
    /// exits, and the pane is a shell the resumed start is typed into.
    #[test]
    fn operator_started_supervisor_is_replaced_from_the_pane_tree() {
        let mut pre_fix = ReplacementSimWorld::sdk_dev(false);
        let route_owned_only = pre_fix.facts().route_owned_scan_pid;
        assert_eq!(route_owned_only, None, "cleared_no_pid / NoSupervisor");
        assert_eq!(
            pre_fix.replace(route_owned_only),
            SupervisorReplacementPaneDecision::PreserveLiveHarness
        );

        let mut world = ReplacementSimWorld::sdk_dev(false);
        let target = resolve_supervisor_replacement_target(world.facts());
        assert_eq!(
            target,
            Some((
                2_612_299,
                SupervisorReplacementTargetSource::PaneProcessTree
            ))
        );
        assert_eq!(
            world.replace(target.map(|(pid, _)| pid)),
            SupervisorReplacementPaneDecision::PreserveExistingShell
        );
    }

    /// Every world shape: a live supervisor is always targeted whatever
    /// witness survives, the pane tree wins whenever it has one, and a world
    /// with no live supervisor targets nothing (never a stale lease pid).
    #[test]
    fn replacement_target_resolution_is_total_over_the_world_shapes() {
        for route_owned in [false, true] {
            for pane_recorded in [false, true] {
                for lease in [None, Some(2_612_299), Some(7)] {
                    for alive in [false, true] {
                        let mut world = ReplacementSimWorld::sdk_dev(route_owned);
                        world.pane_recorded = pane_recorded;
                        world.lease_pid = lease;
                        if !alive {
                            world.supervisor = None;
                            world.harness_alive = false;
                        }
                        let target = resolve_supervisor_replacement_target(world.facts());
                        let reachable =
                            alive && (pane_recorded || route_owned || lease == Some(2_612_299));
                        assert_eq!(
                            target.map(|(pid, _)| pid),
                            reachable.then_some(2_612_299),
                            "route_owned={route_owned} pane={pane_recorded} lease={lease:?} alive={alive}"
                        );
                        if alive && pane_recorded {
                            assert_eq!(
                                target.map(|(_, source)| source),
                                Some(SupervisorReplacementTargetSource::PaneProcessTree)
                            );
                        }
                        if reachable {
                            assert_eq!(
                                world.replace(target.map(|(pid, _)| pid)),
                                SupervisorReplacementPaneDecision::PreserveExistingShell
                            );
                        }
                    }
                }
            }
        }
    }
}
