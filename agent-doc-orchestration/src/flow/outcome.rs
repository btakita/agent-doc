use serde::{Deserialize, Serialize};
use std::fmt;

pub const BINARY_OUTCOME_CONTRACT_VERSION: &str = "binary-outcome-v1";
pub const USER_FACING_OUTCOME_CONTRACT_VERSION: &str = "ui-outcome-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOutcomeClass {
    Ok,
    Recoverable,
    Blocked,
    Operator,
}

impl BinaryOutcomeClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Recoverable => "recoverable",
            Self::Blocked => "blocked",
            Self::Operator => "operator",
        }
    }
}

impl fmt::Display for BinaryOutcomeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserFacingOutcomeKind {
    QueuedBehindOwner,
    RecoveredAndRetried,
    DeferredForOperatorProof,
    /// The in-session loop has no drainable head, but a `[focused-cycle]` head
    /// remains that the SUPERVISOR clear-and-continue path will drain (force
    /// `/clear` + re-dispatch to a fresh context). No operator action is required;
    /// the in-session agent simply ends its turn so the supervisor takes over
    /// (`#qfocsup`).
    DeferredForSupervisorDrain,
    NoDrainableWork,
    RealComponentConflict,
    BlockedWithExactUnblocker,
}

impl UserFacingOutcomeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueuedBehindOwner => "queued_behind_owner",
            Self::RecoveredAndRetried => "recovered_and_retried",
            Self::DeferredForOperatorProof => "deferred_for_operator_proof",
            Self::DeferredForSupervisorDrain => "deferred_for_supervisor_drain",
            Self::NoDrainableWork => "no_drainable_work",
            Self::RealComponentConflict => "real_component_conflict",
            Self::BlockedWithExactUnblocker => "blocked_with_exact_unblocker",
        }
    }

    pub const fn class(self) -> BinaryOutcomeClass {
        match self {
            Self::QueuedBehindOwner
            | Self::NoDrainableWork
            | Self::DeferredForSupervisorDrain => BinaryOutcomeClass::Ok,
            Self::RecoveredAndRetried => BinaryOutcomeClass::Recoverable,
            Self::DeferredForOperatorProof => BinaryOutcomeClass::Operator,
            Self::RealComponentConflict | Self::BlockedWithExactUnblocker => {
                BinaryOutcomeClass::Blocked
            }
        }
    }

    pub const fn next_action(self) -> &'static str {
        match self {
            Self::QueuedBehindOwner => "wait_for_owner_turn_to_drain",
            Self::RecoveredAndRetried => "continue_after_recovery_retry",
            Self::DeferredForOperatorProof => "operator_proof_required",
            Self::DeferredForSupervisorDrain => "yield_to_supervisor_clear_and_continue",
            Self::NoDrainableWork => "no_agent_action",
            Self::RealComponentConflict => "resolve_component_conflict",
            Self::BlockedWithExactUnblocker => "follow_unblocker",
        }
    }
}

impl fmt::Display for UserFacingOutcomeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserFacingOutcome {
    pub contract_version: String,
    pub outcome: UserFacingOutcomeKind,
    pub class: BinaryOutcomeClass,
    pub next_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unblocker: Option<String>,
}

impl UserFacingOutcome {
    pub fn new(outcome: UserFacingOutcomeKind) -> Result<Self, BinaryOutcomeError> {
        if outcome == UserFacingOutcomeKind::BlockedWithExactUnblocker {
            return Err(BinaryOutcomeError::EmptyField { field: "unblocker" });
        }
        Ok(Self {
            contract_version: USER_FACING_OUTCOME_CONTRACT_VERSION.to_string(),
            outcome,
            class: outcome.class(),
            next_action: outcome.next_action().to_string(),
            unblocker: None,
        })
    }

    pub fn with_unblocker(
        outcome: UserFacingOutcomeKind,
        unblocker: impl Into<String>,
    ) -> Result<Self, BinaryOutcomeError> {
        let unblocker = validate_token("unblocker", unblocker.into())?;
        Ok(Self {
            contract_version: USER_FACING_OUTCOME_CONTRACT_VERSION.to_string(),
            outcome,
            class: outcome.class(),
            next_action: outcome.next_action().to_string(),
            unblocker: Some(unblocker),
        })
    }

    pub fn log_fields(&self) -> String {
        let fields = format!(
            "ui_outcome_contract={} ui_outcome={} ui_outcome_class={} next_action={}",
            self.contract_version,
            self.outcome.as_str(),
            self.class.as_str(),
            self.next_action
        );
        match self.unblocker.as_deref() {
            Some(unblocker) => format!("{fields} unblocker={unblocker}"),
            None => fields,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryOutcome {
    pub contract_version: String,
    pub class: BinaryOutcomeClass,
    pub invariant_id: String,
    pub proof_marker: String,
    pub next_action: String,
}

impl BinaryOutcome {
    pub fn new(
        class: BinaryOutcomeClass,
        invariant_id: impl Into<String>,
        proof_marker: impl Into<String>,
        next_action: impl Into<String>,
    ) -> Result<Self, BinaryOutcomeError> {
        let invariant_id = validate_token("invariant_id", invariant_id.into())?;
        let proof_marker = validate_token("proof_marker", proof_marker.into())?;
        let next_action = validate_token("next_action", next_action.into())?;
        Ok(Self {
            contract_version: BINARY_OUTCOME_CONTRACT_VERSION.to_string(),
            class,
            invariant_id,
            proof_marker,
            next_action,
        })
    }

    pub fn ok(
        invariant_id: impl Into<String>,
        proof_marker: impl Into<String>,
        next_action: impl Into<String>,
    ) -> Result<Self, BinaryOutcomeError> {
        Self::new(
            BinaryOutcomeClass::Ok,
            invariant_id,
            proof_marker,
            next_action,
        )
    }

    pub fn recoverable(
        invariant_id: impl Into<String>,
        proof_marker: impl Into<String>,
        next_action: impl Into<String>,
    ) -> Result<Self, BinaryOutcomeError> {
        Self::new(
            BinaryOutcomeClass::Recoverable,
            invariant_id,
            proof_marker,
            next_action,
        )
    }

    pub fn blocked(
        invariant_id: impl Into<String>,
        proof_marker: impl Into<String>,
        next_action: impl Into<String>,
    ) -> Result<Self, BinaryOutcomeError> {
        Self::new(
            BinaryOutcomeClass::Blocked,
            invariant_id,
            proof_marker,
            next_action,
        )
    }

    pub fn operator(
        invariant_id: impl Into<String>,
        proof_marker: impl Into<String>,
        next_action: impl Into<String>,
    ) -> Result<Self, BinaryOutcomeError> {
        Self::new(
            BinaryOutcomeClass::Operator,
            invariant_id,
            proof_marker,
            next_action,
        )
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOutcomeError {
    EmptyField { field: &'static str },
    InvalidToken { field: &'static str, value: String },
}

impl fmt::Display for BinaryOutcomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { field } => write!(f, "{field} must not be empty"),
            Self::InvalidToken { field, value } => {
                write!(f, "{field} must be a single field-safe token: {value}")
            }
        }
    }
}

impl std::error::Error for BinaryOutcomeError {}

fn validate_token(field: &'static str, value: String) -> Result<String, BinaryOutcomeError> {
    if value.trim().is_empty() {
        return Err(BinaryOutcomeError::EmptyField { field });
    }
    if value.trim() != value || !value.chars().all(is_token_char) {
        return Err(BinaryOutcomeError::InvalidToken { field, value });
    }
    Ok(value)
}

fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_outcome_records_required_contract_fields() {
        let outcome = BinaryOutcome::recoverable(
            "supervisor_freshness",
            "supervisor_binary_stale_self_recycled",
            "restart_supervisor_once_and_retry",
        )
        .unwrap();

        assert_eq!(outcome.contract_version, "binary-outcome-v1");
        assert_eq!(outcome.class, BinaryOutcomeClass::Recoverable);
        assert_eq!(outcome.invariant_id, "supervisor_freshness");
        assert_eq!(
            outcome.log_fields(),
            "binary_outcome=recoverable invariant=supervisor_freshness proof_marker=supervisor_binary_stale_self_recycled next_action=restart_supervisor_once_and_retry"
        );
    }

    #[test]
    fn binary_outcome_classes_are_stable_snake_case() {
        assert_eq!(BinaryOutcomeClass::Ok.as_str(), "ok");
        assert_eq!(BinaryOutcomeClass::Recoverable.as_str(), "recoverable");
        assert_eq!(BinaryOutcomeClass::Blocked.as_str(), "blocked");
        assert_eq!(BinaryOutcomeClass::Operator.as_str(), "operator");

        let json = serde_json::to_value(
            BinaryOutcome::operator(
                "two_editor_convergence",
                "missing_live_vscode_ack",
                "request_operator_live_proof",
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(json["class"], "operator");
        assert_eq!(json["next_action"], "request_operator_live_proof");
    }

    #[test]
    fn user_facing_outcome_vocabulary_is_stable_and_typed() {
        use UserFacingOutcomeKind as Kind;

        let cases = [
            (
                Kind::QueuedBehindOwner,
                "queued_behind_owner",
                BinaryOutcomeClass::Ok,
                "wait_for_owner_turn_to_drain",
            ),
            (
                Kind::RecoveredAndRetried,
                "recovered_and_retried",
                BinaryOutcomeClass::Recoverable,
                "continue_after_recovery_retry",
            ),
            (
                Kind::DeferredForOperatorProof,
                "deferred_for_operator_proof",
                BinaryOutcomeClass::Operator,
                "operator_proof_required",
            ),
            (
                Kind::DeferredForSupervisorDrain,
                "deferred_for_supervisor_drain",
                BinaryOutcomeClass::Ok,
                "yield_to_supervisor_clear_and_continue",
            ),
            (
                Kind::NoDrainableWork,
                "no_drainable_work",
                BinaryOutcomeClass::Ok,
                "no_agent_action",
            ),
            (
                Kind::RealComponentConflict,
                "real_component_conflict",
                BinaryOutcomeClass::Blocked,
                "resolve_component_conflict",
            ),
            (
                Kind::BlockedWithExactUnblocker,
                "blocked_with_exact_unblocker",
                BinaryOutcomeClass::Blocked,
                "follow_unblocker",
            ),
        ];

        for (kind, token, class, next_action) in cases {
            assert_eq!(kind.as_str(), token);
            assert_eq!(kind.class(), class);
            assert_eq!(kind.next_action(), next_action);
        }

        let json = serde_json::to_value(
            UserFacingOutcome::new(Kind::QueuedBehindOwner)
                .expect("queued outcome does not require an unblocker"),
        )
        .unwrap();
        assert_eq!(json["contract_version"], "ui-outcome-v1");
        assert_eq!(json["outcome"], "queued_behind_owner");
        assert_eq!(json["class"], "ok");
        assert_eq!(json["next_action"], "wait_for_owner_turn_to_drain");
    }

    #[test]
    fn blocked_user_facing_outcome_requires_exact_unblocker() {
        use UserFacingOutcomeKind as Kind;

        assert_eq!(
            UserFacingOutcome::new(Kind::BlockedWithExactUnblocker).unwrap_err(),
            BinaryOutcomeError::EmptyField { field: "unblocker" }
        );

        let outcome = UserFacingOutcome::with_unblocker(
            Kind::BlockedWithExactUnblocker,
            "restore_idle_prompt",
        )
        .unwrap();
        assert_eq!(
            outcome.log_fields(),
            "ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=follow_unblocker unblocker=restore_idle_prompt"
        );
        assert!(matches!(
            UserFacingOutcome::with_unblocker(
                Kind::BlockedWithExactUnblocker,
                "restore idle prompt"
            )
            .unwrap_err(),
            BinaryOutcomeError::InvalidToken {
                field: "unblocker",
                ..
            }
        ));
    }

    #[test]
    fn binary_outcome_rejects_ambiguous_or_multi_action_fields() {
        assert_eq!(
            BinaryOutcome::ok("", "proof", "continue").unwrap_err(),
            BinaryOutcomeError::EmptyField {
                field: "invariant_id"
            }
        );

        assert!(matches!(
            BinaryOutcome::blocked("queue head", "proof", "stop").unwrap_err(),
            BinaryOutcomeError::InvalidToken {
                field: "invariant_id",
                ..
            }
        ));

        assert!(matches!(
            BinaryOutcome::recoverable("queue_head", "proof", "retry,clear").unwrap_err(),
            BinaryOutcomeError::InvalidToken {
                field: "next_action",
                ..
            }
        ));
    }
}
