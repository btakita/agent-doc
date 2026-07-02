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
}
