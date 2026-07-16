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
pub struct CaptureCloseoutMaterializationEvidence {
    pub active_capture: bool,
    pub capture_terminal: bool,
    pub commit_surface_available: bool,
    pub response_in_commit_surface: bool,
    pub response_in_referenced_compact_archive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureCloseoutMaterializationBasis {
    NoActiveCapture,
    TerminalCapture,
    CommitSurfaceUnavailable,
    InlineCommitSurface,
    ReferencedCompactArchive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureCloseoutMaterializationDecision {
    Allow(CaptureCloseoutMaterializationBasis),
    BlockMissingResponse,
}

/// Decide whether a capture is materially preserved by the commit surface.
///
/// Compact Exchange intentionally removes response bodies from the session
/// document. The response remains materialized when the staged/HEAD document
/// references a compact archive under `.agent-doc/archives` that contains the
/// exact captured response. Adapters own parsing content and reading archives;
/// this function owns the exhaustive closeout policy.
pub const fn decide_capture_closeout_materialization(
    evidence: CaptureCloseoutMaterializationEvidence,
) -> CaptureCloseoutMaterializationDecision {
    if !evidence.active_capture {
        return CaptureCloseoutMaterializationDecision::Allow(
            CaptureCloseoutMaterializationBasis::NoActiveCapture,
        );
    }
    if evidence.capture_terminal {
        return CaptureCloseoutMaterializationDecision::Allow(
            CaptureCloseoutMaterializationBasis::TerminalCapture,
        );
    }
    if !evidence.commit_surface_available {
        return CaptureCloseoutMaterializationDecision::Allow(
            CaptureCloseoutMaterializationBasis::CommitSurfaceUnavailable,
        );
    }
    if evidence.response_in_commit_surface {
        return CaptureCloseoutMaterializationDecision::Allow(
            CaptureCloseoutMaterializationBasis::InlineCommitSurface,
        );
    }
    if evidence.response_in_referenced_compact_archive {
        return CaptureCloseoutMaterializationDecision::Allow(
            CaptureCloseoutMaterializationBasis::ReferencedCompactArchive,
        );
    }
    CaptureCloseoutMaterializationDecision::BlockMissingResponse
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleCaptureRetirementDecision {
    Keep,
    RetireSupersededCapturedOnlyOrphan,
}

/// Side-effect-free stale-capture retirement policy.
///
/// A durable response is never retired merely because a `WriteApplied` attempt
/// raced with later authoritative document state. The response remains replayable
/// after a safe baseline rebase, or retained for an explicit conflict recovery.
/// A `Captured`-only orphan may be retired only when its heading is already
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
    if !evidence.captured_response_body_missing {
        return StaleCaptureRetirementDecision::Keep;
    }

    match evidence.state {
        CaptureState::Captured if evidence.captured_response_heading_answered => {
            StaleCaptureRetirementDecision::RetireSupersededCapturedOnlyOrphan
        }
        _ => StaleCaptureRetirementDecision::Keep,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativeReplayRebaseEvidence {
    pub capture_repairable: bool,
    pub replay_baseline_drifted: bool,
    pub authoritative_current: bool,
    pub matching_open_cycle: bool,
    pub captured_response_body_missing: bool,
    pub captured_response_heading_answered: bool,
    pub current_monotonically_extends_baseline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthoritativeReplayRebaseDecision {
    KeepBaseline,
    RebaseToAuthoritativeCurrent,
    BlockConflict,
}

/// Decide whether an interrupted response may adopt a newer authoritative cut.
///
/// The monotonic-extension proof is deliberately strict: every baseline line
/// must remain in order and only new lines may have been inserted. That admits
/// later user steering without treating arbitrary edits, deletions, or reorders
/// as benign. A matching open cycle binds the retained response to the cut.
pub const fn decide_authoritative_replay_rebase(
    evidence: AuthoritativeReplayRebaseEvidence,
) -> AuthoritativeReplayRebaseDecision {
    if !evidence.replay_baseline_drifted {
        return AuthoritativeReplayRebaseDecision::KeepBaseline;
    }
    if evidence.capture_repairable
        && evidence.authoritative_current
        && evidence.matching_open_cycle
        && evidence.captured_response_body_missing
        && !evidence.captured_response_heading_answered
        && evidence.current_monotonically_extends_baseline
    {
        return AuthoritativeReplayRebaseDecision::RebaseToAuthoritativeCurrent;
    }
    AuthoritativeReplayRebaseDecision::BlockConflict
}

/// True when `current` preserves every baseline line in order and differs only
/// by inserted lines. This is the small, pure proof used before rebasing a
/// retained response onto concurrent operator steering.
pub fn current_monotonically_extends_baseline(baseline: &str, current: &str) -> bool {
    if baseline == current {
        return true;
    }
    let mut current_lines = current.lines();
    baseline
        .lines()
        .all(|baseline_line| current_lines.by_ref().any(|line| line == baseline_line))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginGenerationRefreshEvidence {
    pub preflight_generation: u128,
    pub live_generation: u128,
    pub live_registration_observed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginGenerationRefreshDecision {
    KeepPreflight,
    AdoptLive,
}

/// A registered editor generation observed at replay/ACK time supersedes the
/// turn-start generation when it is newer. Native-library reloads do not enter
/// this decision because they do not replace editor-plugin code.
pub const fn decide_plugin_generation_refresh(
    evidence: PluginGenerationRefreshEvidence,
) -> PluginGenerationRefreshDecision {
    if evidence.live_registration_observed
        && evidence.live_generation > evidence.preflight_generation
    {
        PluginGenerationRefreshDecision::AdoptLive
    } else {
        PluginGenerationRefreshDecision::KeepPreflight
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
    fn capture_closeout_materialization_decision_covers_the_boolean_state_table() {
        for active_capture in [false, true] {
            for capture_terminal in [false, true] {
                for commit_surface_available in [false, true] {
                    for response_in_commit_surface in [false, true] {
                        for response_in_referenced_compact_archive in [false, true] {
                            let evidence = CaptureCloseoutMaterializationEvidence {
                                active_capture,
                                capture_terminal,
                                commit_surface_available,
                                response_in_commit_surface,
                                response_in_referenced_compact_archive,
                            };
                            let decision = decide_capture_closeout_materialization(evidence);
                            let should_block = active_capture
                                && !capture_terminal
                                && commit_surface_available
                                && !response_in_commit_surface
                                && !response_in_referenced_compact_archive;
                            assert_eq!(
                                matches!(
                                    decision,
                                    CaptureCloseoutMaterializationDecision::BlockMissingResponse
                                ),
                                should_block,
                                "unexpected decision for {evidence:?}: {decision:?}",
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn capture_closeout_materialization_prefers_inline_then_archive_evidence() {
        let base = CaptureCloseoutMaterializationEvidence {
            active_capture: true,
            capture_terminal: false,
            commit_surface_available: true,
            response_in_commit_surface: false,
            response_in_referenced_compact_archive: false,
        };
        assert_eq!(
            decide_capture_closeout_materialization(CaptureCloseoutMaterializationEvidence {
                response_in_commit_surface: true,
                response_in_referenced_compact_archive: true,
                ..base
            }),
            CaptureCloseoutMaterializationDecision::Allow(
                CaptureCloseoutMaterializationBasis::InlineCommitSurface,
            ),
        );
        assert_eq!(
            decide_capture_closeout_materialization(CaptureCloseoutMaterializationEvidence {
                response_in_referenced_compact_archive: true,
                ..base
            }),
            CaptureCloseoutMaterializationDecision::Allow(
                CaptureCloseoutMaterializationBasis::ReferencedCompactArchive,
            ),
        );
    }

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
    fn stale_capture_retirement_retains_drifted_write_applied_capture() {
        let decision = decide_stale_capture_retirement(StaleCaptureRetirementEvidence {
            state: CaptureState::WriteApplied,
            replay_baseline_drifted: true,
            captured_response_body_missing: true,
            captured_response_heading_answered: false,
        });

        assert_eq!(decision, StaleCaptureRetirementDecision::Keep);
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
            replay_baseline_drifted: false,
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
    fn authoritative_rebase_accepts_new_steering_on_the_matching_open_cycle() {
        let decision = decide_authoritative_replay_rebase(AuthoritativeReplayRebaseEvidence {
            capture_repairable: true,
            replay_baseline_drifted: true,
            authoritative_current: true,
            matching_open_cycle: true,
            captured_response_body_missing: true,
            captured_response_heading_answered: false,
            current_monotonically_extends_baseline: true,
        });
        assert_eq!(
            decision,
            AuthoritativeReplayRebaseDecision::RebaseToAuthoritativeCurrent
        );
    }

    #[test]
    fn authoritative_rebase_blocks_conflicts_and_non_authoritative_cuts() {
        for (authoritative_current, monotonic) in [(false, true), (true, false)] {
            let decision = decide_authoritative_replay_rebase(AuthoritativeReplayRebaseEvidence {
                capture_repairable: true,
                replay_baseline_drifted: true,
                authoritative_current,
                matching_open_cycle: true,
                captured_response_body_missing: true,
                captured_response_heading_answered: false,
                current_monotonically_extends_baseline: monotonic,
            });
            assert_eq!(decision, AuthoritativeReplayRebaseDecision::BlockConflict);
        }
    }

    #[test]
    fn monotonic_extension_allows_insertions_but_not_edits_or_reorders() {
        let baseline = "first\nsecond\nthird\n";
        assert!(current_monotonically_extends_baseline(
            baseline,
            "first\nnew steering\nsecond\nthird\n"
        ));
        assert!(!current_monotonically_extends_baseline(
            baseline,
            "first\nchanged\nthird\n"
        ));
        assert!(!current_monotonically_extends_baseline(
            baseline,
            "second\nfirst\nthird\n"
        ));
    }

    #[test]
    fn live_registered_plugin_generation_supersedes_preflight() {
        assert_eq!(
            decide_plugin_generation_refresh(PluginGenerationRefreshEvidence {
                preflight_generation: 10,
                live_generation: 20,
                live_registration_observed: true,
            }),
            PluginGenerationRefreshDecision::AdoptLive
        );
        assert_eq!(
            decide_plugin_generation_refresh(PluginGenerationRefreshEvidence {
                preflight_generation: 10,
                live_generation: 20,
                live_registration_observed: false,
            }),
            PluginGenerationRefreshDecision::KeepPreflight
        );
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
