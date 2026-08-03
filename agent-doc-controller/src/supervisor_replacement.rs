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
        (SupervisorReplacementIpcOutcome::Failed, true, _)
        | (SupervisorReplacementIpcOutcome::Failed, false, true) => {
            SupervisorReplacementEscalation::EscalateColdStart
        }
        (SupervisorReplacementIpcOutcome::Failed, false, false) => {
            SupervisorReplacementEscalation::FailClosed
        }
    }
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
}
