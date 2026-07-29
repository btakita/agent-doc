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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParsedSupervisorReplacementRequest {
    pub mode: SupervisorReplacementMode,
    pub force: bool,
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
    mode: SupervisorReplacementMode,
    facts: SupervisorReplacementPaneFacts,
) -> SupervisorReplacementPaneDecision {
    if !facts.pane_alive {
        return SupervisorReplacementPaneDecision::AutoStartNew;
    }
    if facts.current_command_is_shell {
        return SupervisorReplacementPaneDecision::PreserveExistingShell;
    }
    if facts.runs_document_harness {
        match mode {
            SupervisorReplacementMode::Continue => {
                SupervisorReplacementPaneDecision::PreserveLiveHarness
            }
            SupervisorReplacementMode::Fresh => {
                SupervisorReplacementPaneDecision::RestartLiveHarness
            }
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

    Ok(ParsedSupervisorReplacementRequest {
        mode: SupervisorReplacementMode::parse(mode)?,
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
                SupervisorReplacementMode::Continue,
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
                SupervisorReplacementMode::Fresh,
                SupervisorReplacementPaneFacts {
                    pane_alive: true,
                    current_command_is_shell: false,
                    runs_document_harness: true,
                },
            ),
            SupervisorReplacementPaneDecision::RestartLiveHarness
        );
    }
}
