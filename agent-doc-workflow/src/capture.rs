//! Pure response-capture lifecycle policy.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    Captured,
    WriteApplied,
    Replayed,
    Committed,
    Discarded,
}

pub const fn capture_state_rank(state: CaptureState) -> u8 {
    match state {
        CaptureState::Captured => 0,
        CaptureState::WriteApplied => 1,
        CaptureState::Replayed => 2,
        CaptureState::Committed => 3,
        CaptureState::Discarded => 4,
    }
}

pub const fn capture_state_can_advance(from: CaptureState, to: CaptureState) -> bool {
    capture_state_rank(to) >= capture_state_rank(from)
}

pub const fn capture_state_is_repairable(state: CaptureState) -> bool {
    matches!(
        state,
        CaptureState::Captured | CaptureState::WriteApplied | CaptureState::Replayed
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleCaptureRetirementDecision {
    Keep,
    RetireWedgedWriteApplied,
    RetireSupersededCapturedOnlyOrphan,
}

/// Side-effect-free stale-capture retirement policy.
///
/// A drifted `WriteApplied` capture may be retired only after the captured body
/// is missing from the live document: the write was already attempted, but
/// replaying onto a drifted baseline risks duplication or reordering. A
/// drifted `Captured`-only orphan is more conservative because the write never
/// ran, so retirement also requires proof that the captured heading is already
/// answered by a superseding live exchange turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleCaptureRetirementEvidence {
    pub state: CaptureState,
    pub replay_baseline_drifted: bool,
    pub captured_response_body_missing: bool,
    pub captured_response_heading_answered: bool,
}

pub const fn decide_stale_capture_retirement(
    evidence: StaleCaptureRetirementEvidence,
) -> StaleCaptureRetirementDecision {
    if !evidence.replay_baseline_drifted || !evidence.captured_response_body_missing {
        return StaleCaptureRetirementDecision::Keep;
    }

    match evidence.state {
        CaptureState::WriteApplied => StaleCaptureRetirementDecision::RetireWedgedWriteApplied,
        CaptureState::Captured if evidence.captured_response_heading_answered => {
            StaleCaptureRetirementDecision::RetireSupersededCapturedOnlyOrphan
        }
        _ => StaleCaptureRetirementDecision::Keep,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairTemplateChangeKind {
    DuplicateOpener,
    DuplicateClose,
    DuplicateScaffold,
    ConversationTail,
    CompletedTurnBoundary,
    ResponsePromptOrder,
    PromptPrefixes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RepairTemplateChanges {
    pub duplicate_opener: bool,
    pub duplicate_close: bool,
    pub duplicate_scaffold: bool,
    pub conversation_tail: bool,
    pub completed_turn_boundary: bool,
    pub response_prompt_order: bool,
    pub prompt_prefixes: bool,
}

impl RepairTemplateChanges {
    const LOG_ORDER: [RepairTemplateChangeKind; 7] = [
        RepairTemplateChangeKind::DuplicateOpener,
        RepairTemplateChangeKind::DuplicateClose,
        RepairTemplateChangeKind::DuplicateScaffold,
        RepairTemplateChangeKind::ConversationTail,
        RepairTemplateChangeKind::CompletedTurnBoundary,
        RepairTemplateChangeKind::ResponsePromptOrder,
        RepairTemplateChangeKind::PromptPrefixes,
    ];

    pub const fn contains(self, kind: RepairTemplateChangeKind) -> bool {
        match kind {
            RepairTemplateChangeKind::DuplicateOpener => self.duplicate_opener,
            RepairTemplateChangeKind::DuplicateClose => self.duplicate_close,
            RepairTemplateChangeKind::DuplicateScaffold => self.duplicate_scaffold,
            RepairTemplateChangeKind::ConversationTail => self.conversation_tail,
            RepairTemplateChangeKind::CompletedTurnBoundary => self.completed_turn_boundary,
            RepairTemplateChangeKind::ResponsePromptOrder => self.response_prompt_order,
            RepairTemplateChangeKind::PromptPrefixes => self.prompt_prefixes,
        }
    }

    pub const fn should_persist(self) -> bool {
        self.duplicate_opener
            || self.duplicate_close
            || self.duplicate_scaffold
            || self.conversation_tail
            || self.completed_turn_boundary
            || self.response_prompt_order
            || self.prompt_prefixes
    }

    pub const fn should_log(self, kind: RepairTemplateChangeKind) -> bool {
        self.contains(kind)
    }

    pub fn changed_kinds(self) -> impl Iterator<Item = RepairTemplateChangeKind> {
        Self::LOG_ORDER
            .into_iter()
            .filter(move |kind| self.should_log(*kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_state_rank_orders_lifecycle_forward() {
        assert!(
            capture_state_rank(CaptureState::Captured)
                < capture_state_rank(CaptureState::WriteApplied)
        );
        assert!(
            capture_state_rank(CaptureState::WriteApplied)
                < capture_state_rank(CaptureState::Replayed)
        );
        assert!(
            capture_state_rank(CaptureState::Replayed)
                < capture_state_rank(CaptureState::Committed)
        );
        assert!(
            capture_state_rank(CaptureState::Committed)
                < capture_state_rank(CaptureState::Discarded)
        );
    }

    #[test]
    fn capture_state_can_advance_rejects_backward_transitions() {
        assert!(capture_state_can_advance(
            CaptureState::Captured,
            CaptureState::Committed
        ));
        assert!(!capture_state_can_advance(
            CaptureState::Committed,
            CaptureState::Replayed
        ));
    }

    #[test]
    fn capture_state_is_repairable_accepts_unfinished_states() {
        assert!(capture_state_is_repairable(CaptureState::Captured));
        assert!(capture_state_is_repairable(CaptureState::WriteApplied));
        assert!(capture_state_is_repairable(CaptureState::Replayed));
    }

    #[test]
    fn capture_state_is_repairable_rejects_terminal_states() {
        assert!(!capture_state_is_repairable(CaptureState::Committed));
        assert!(!capture_state_is_repairable(CaptureState::Discarded));
    }

    #[test]
    fn stale_capture_retirement_retires_drifted_write_applied_capture() {
        let decision = decide_stale_capture_retirement(StaleCaptureRetirementEvidence {
            state: CaptureState::WriteApplied,
            replay_baseline_drifted: true,
            captured_response_body_missing: true,
            captured_response_heading_answered: false,
        });

        assert_eq!(
            decision,
            StaleCaptureRetirementDecision::RetireWedgedWriteApplied
        );
    }

    #[test]
    fn stale_capture_retirement_keeps_write_applied_without_baseline_drift() {
        let decision = decide_stale_capture_retirement(StaleCaptureRetirementEvidence {
            state: CaptureState::WriteApplied,
            replay_baseline_drifted: false,
            captured_response_body_missing: true,
            captured_response_heading_answered: true,
        });

        assert_eq!(decision, StaleCaptureRetirementDecision::Keep);
    }

    #[test]
    fn stale_capture_retirement_keeps_materialized_response_body() {
        let decision = decide_stale_capture_retirement(StaleCaptureRetirementEvidence {
            state: CaptureState::WriteApplied,
            replay_baseline_drifted: true,
            captured_response_body_missing: false,
            captured_response_heading_answered: true,
        });

        assert_eq!(decision, StaleCaptureRetirementDecision::Keep);
    }

    #[test]
    fn stale_capture_retirement_requires_supersession_for_captured_only_orphan() {
        let no_supersession = decide_stale_capture_retirement(StaleCaptureRetirementEvidence {
            state: CaptureState::Captured,
            replay_baseline_drifted: true,
            captured_response_body_missing: true,
            captured_response_heading_answered: false,
        });
        assert_eq!(no_supersession, StaleCaptureRetirementDecision::Keep);

        let superseded = decide_stale_capture_retirement(StaleCaptureRetirementEvidence {
            state: CaptureState::Captured,
            replay_baseline_drifted: true,
            captured_response_body_missing: true,
            captured_response_heading_answered: true,
        });
        assert_eq!(
            superseded,
            StaleCaptureRetirementDecision::RetireSupersededCapturedOnlyOrphan
        );
    }

    #[test]
    fn stale_capture_retirement_does_not_retire_replayed_or_terminal_states() {
        for state in [
            CaptureState::Replayed,
            CaptureState::Committed,
            CaptureState::Discarded,
        ] {
            let decision = decide_stale_capture_retirement(StaleCaptureRetirementEvidence {
                state,
                replay_baseline_drifted: true,
                captured_response_body_missing: true,
                captured_response_heading_answered: true,
            });
            assert_eq!(decision, StaleCaptureRetirementDecision::Keep);
        }
    }

    #[test]
    fn repair_template_changes_do_not_persist_or_log_when_empty() {
        let changes = RepairTemplateChanges::default();

        assert!(!changes.should_persist());
        assert!(changes.changed_kinds().collect::<Vec<_>>().is_empty());
    }

    #[test]
    fn repair_template_changes_persist_and_log_each_changed_kind() {
        let changes = RepairTemplateChanges {
            duplicate_opener: true,
            duplicate_close: false,
            duplicate_scaffold: true,
            conversation_tail: false,
            completed_turn_boundary: true,
            response_prompt_order: false,
            prompt_prefixes: true,
        };

        assert!(changes.should_persist());
        assert!(changes.should_log(RepairTemplateChangeKind::DuplicateOpener));
        assert!(!changes.should_log(RepairTemplateChangeKind::DuplicateClose));
        assert!(changes.should_log(RepairTemplateChangeKind::DuplicateScaffold));
        assert!(!changes.should_log(RepairTemplateChangeKind::ConversationTail));
        assert!(changes.should_log(RepairTemplateChangeKind::CompletedTurnBoundary));
        assert!(!changes.should_log(RepairTemplateChangeKind::ResponsePromptOrder));
        assert!(changes.should_log(RepairTemplateChangeKind::PromptPrefixes));
    }

    #[test]
    fn repair_template_changes_keep_existing_log_order() {
        let changes = RepairTemplateChanges {
            duplicate_opener: true,
            duplicate_close: true,
            duplicate_scaffold: true,
            conversation_tail: true,
            completed_turn_boundary: true,
            response_prompt_order: true,
            prompt_prefixes: true,
        };

        assert_eq!(
            changes.changed_kinds().collect::<Vec<_>>(),
            vec![
                RepairTemplateChangeKind::DuplicateOpener,
                RepairTemplateChangeKind::DuplicateClose,
                RepairTemplateChangeKind::DuplicateScaffold,
                RepairTemplateChangeKind::ConversationTail,
                RepairTemplateChangeKind::CompletedTurnBoundary,
                RepairTemplateChangeKind::ResponsePromptOrder,
                RepairTemplateChangeKind::PromptPrefixes,
            ]
        );
    }
}
