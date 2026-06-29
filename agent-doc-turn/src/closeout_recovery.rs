//! Pure closeout recovery policy.
//!
//! Orchestration owns file, git, and sidecar mutation effects. This module owns
//! action-independent turn recovery decisions that can be proven from document
//! content facts.

/// Which side of a metadata-only drift is authoritative for closeout recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataDriftAuthority {
    /// The local side (snapshot for queue metadata drift, visible file for
    /// sidecar-visible drift) is authoritative and can be committed forward.
    Local,
    /// HEAD is authoritative and the local side should be restored from it.
    Head,
    /// Neither side is provably authoritative; recovery must fail closed.
    Ambiguous,
}

/// Why the closeout recovery mutation primitive is changing durable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutRecoveryMutationReason {
    BenignReplayBaseline,
    QueueOnlyReplayBaseline,
    CommitQueueMetadataDrift,
    ResetFromVisible,
    RestoreHeadMetadata,
    RetireWedgedWriteAppliedCapture,
    RetireSupersededCapturedOnlyOrphan,
    RespectManualTailRemoval,
}

impl CloseoutRecoveryMutationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BenignReplayBaseline => "benign_replay_baseline",
            Self::QueueOnlyReplayBaseline => "queue_only_replay_baseline",
            Self::CommitQueueMetadataDrift => "commit_queue_metadata_drift",
            Self::ResetFromVisible => "reset_from_visible",
            Self::RestoreHeadMetadata => "restore_head_metadata",
            Self::RetireWedgedWriteAppliedCapture => "retire_wedged_write_applied_capture",
            Self::RetireSupersededCapturedOnlyOrphan => "retire_superseded_captured_only_orphan",
            Self::RespectManualTailRemoval => "respect_manual_tail_removal",
        }
    }

    pub const fn capture_refresh_event(self) -> &'static str {
        match self {
            Self::QueueOnlyReplayBaseline => "capture_baseline_refreshed_for_queue_only_drift",
            _ => "capture_baseline_refreshed_for_benign_drift",
        }
    }

    pub const fn capture_refresh_message(self) -> &'static str {
        match self {
            Self::QueueOnlyReplayBaseline => "queue-only drift detected",
            _ => "benign drift detected",
        }
    }
}

/// Typed closeout recovery state (`#closeout-repair-churn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutRecoveryState {
    /// No recovery needed.
    Clean,
    /// Cycle still open (preflight_started / response_captured / write_applied).
    OpenCycle,
    /// Committed binary-owned work but the assistant response body is missing
    /// from HEAD (no capture, or a captured body not materialized in HEAD).
    MissingResponseBody,
    /// A visible `### Re:` response was patched directly into the document
    /// outside the binary write path.
    DirectResponsePatchback,
    /// Raw `<!-- agent:NAME -->` component markers were escaped into the
    /// committed exchange instead of applied as `<!-- patch:* -->` blocks.
    EscapedTemplatePatch,
    /// Snapshot differs from HEAD only by agent-doc-generated exchange artifacts
    /// (boundary / `(HEAD)` markers, answered-prompt-prefix canonicalization).
    BoundaryOnlyDrift,
    /// A reaped/closed item left a nested parent submodule pointer uncommitted
    /// while the document itself is clean.
    NestedParentPointerStale,
    /// An empty `preflight_started` cycle with no capture, response, or pending
    /// mutation.
    OpenEmptyPreflight,
    /// Snapshot differs from HEAD only by agent-doc-generated queue/frontmatter
    /// metadata; user/response and tracked-item content is byte-identical.
    QueueMetadataDrift,
    /// The visible/working file is stale relative to its sidecars (or vice versa)
    /// by metadata only, after an accepted metadata change.
    SidecarVisibleDrift,
    /// User-authored prompt/response content drifted vs HEAD.
    UnsafeUserContentDrift,
}

impl CloseoutRecoveryState {
    pub const ALL: [Self; 11] = [
        Self::Clean,
        Self::OpenCycle,
        Self::MissingResponseBody,
        Self::DirectResponsePatchback,
        Self::EscapedTemplatePatch,
        Self::BoundaryOnlyDrift,
        Self::NestedParentPointerStale,
        Self::OpenEmptyPreflight,
        Self::QueueMetadataDrift,
        Self::SidecarVisibleDrift,
        Self::UnsafeUserContentDrift,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::OpenCycle => "open_cycle",
            Self::MissingResponseBody => "missing_response_body",
            Self::DirectResponsePatchback => "direct_response_patchback",
            Self::EscapedTemplatePatch => "escaped_template_patch",
            Self::BoundaryOnlyDrift => "boundary_only_drift",
            Self::NestedParentPointerStale => "nested_parent_pointer_stale",
            Self::OpenEmptyPreflight => "open_empty_preflight",
            Self::QueueMetadataDrift => "queue_metadata_drift",
            Self::SidecarVisibleDrift => "sidecar_visible_drift",
            Self::UnsafeUserContentDrift => "unsafe_user_content_drift",
        }
    }
}

/// Input facts that are already known at a closeout recovery call site.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CloseoutRecoveryDecisionInput<'a> {
    /// A routed/JB prompt is waiting and should not be typed over an unresolved
    /// closeout.
    pub prompt_context_available: bool,
    /// Low-level blocker text from the caller, retained only as evidence on the
    /// typed decision boundary.
    pub blocker_reason: Option<&'a str>,
    /// Positive proof that the active capture is stale and superseded by visible
    /// exchange content, so retiring it will not drop the user's intended answer.
    pub stale_capture_supersession_proof: Option<&'a str>,
}

/// Typed closeout recovery policy boundary (`#smcloseoutdecision`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutRecoveryDecision {
    /// No closeout recovery remains.
    AlreadyCommitted,
    /// The existing response/cycle can be safely replayed or completed by the
    /// binary without choosing between competing user-authored contents.
    ReplaySafe {
        state: CloseoutRecoveryState,
        command: String,
    },
    /// A stale capture can be retired because superseding visible content proves
    /// the captured body should not be replayed.
    RetireStaleCapture {
        state: CloseoutRecoveryState,
        proof: String,
    },
    /// Sidecars are stale relative to the visible markdown and can be rebuilt
    /// from the visible file.
    ResetSidecarsFromVisible {
        state: CloseoutRecoveryState,
        command: String,
    },
    /// A new routed prompt must wait behind the unresolved closeout instead of
    /// being submitted to the pane.
    QueuePromptForAfterCloseout {
        state: CloseoutRecoveryState,
        reason: String,
    },
    /// Recovery is not safe because a required proof is missing.
    Blocked {
        state: CloseoutRecoveryState,
        missing_proof: String,
        recommended: String,
    },
}

impl CloseoutRecoveryDecision {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AlreadyCommitted => "already_committed",
            Self::ReplaySafe { .. } => "replay_safe",
            Self::RetireStaleCapture { .. } => "retire_stale_capture",
            Self::ResetSidecarsFromVisible { .. } => "reset_sidecars_from_visible",
            Self::QueuePromptForAfterCloseout { .. } => "queue_prompt_for_after_closeout",
            Self::Blocked { .. } => "blocked",
        }
    }

    pub const fn state(&self) -> Option<CloseoutRecoveryState> {
        match self {
            Self::AlreadyCommitted => None,
            Self::ReplaySafe { state, .. }
            | Self::RetireStaleCapture { state, .. }
            | Self::ResetSidecarsFromVisible { state, .. }
            | Self::QueuePromptForAfterCloseout { state, .. }
            | Self::Blocked { state, .. } => Some(*state),
        }
    }

    pub fn route_terminal_reason(&self) -> String {
        match self {
            Self::AlreadyCommitted => "closeout recovery already_committed".to_string(),
            Self::ReplaySafe { state, command } => format!(
                "closeout recovery replay_safe [{}]: {}",
                state.as_str(),
                command
            ),
            Self::RetireStaleCapture { state, proof } => format!(
                "closeout recovery retire_stale_capture [{}]: proof: {}",
                state.as_str(),
                proof
            ),
            Self::ResetSidecarsFromVisible { state, command } => format!(
                "closeout recovery reset_sidecars_from_visible [{}]: {}",
                state.as_str(),
                command
            ),
            Self::QueuePromptForAfterCloseout { state, .. } => format!(
                "closeout recovery queue_prompt_for_after_closeout [{}]: routed prompt queued behind unresolved closeout",
                state.as_str()
            ),
            Self::Blocked {
                state,
                missing_proof,
                recommended,
            } => format!(
                "closeout recovery blocked [{}]: missing proof: {}; recommended: {}",
                state.as_str(),
                missing_proof,
                recommended
            ),
        }
    }
}

pub fn closeout_recovery_decision_from_state(
    state: CloseoutRecoveryState,
    input: CloseoutRecoveryDecisionInput<'_>,
    recovery_command: Option<&str>,
) -> CloseoutRecoveryDecision {
    if input.prompt_context_available {
        return CloseoutRecoveryDecision::QueuePromptForAfterCloseout {
            state,
            reason: input
                .blocker_reason
                .unwrap_or_else(|| state.as_str())
                .to_string(),
        };
    }

    if state == CloseoutRecoveryState::Clean {
        return CloseoutRecoveryDecision::AlreadyCommitted;
    }

    if let Some(proof) = input.stale_capture_supersession_proof
        && matches!(
            state,
            CloseoutRecoveryState::MissingResponseBody
                | CloseoutRecoveryState::UnsafeUserContentDrift
        )
    {
        return CloseoutRecoveryDecision::RetireStaleCapture {
            state,
            proof: proof.to_string(),
        };
    }

    let command = || recovery_command.unwrap_or_default().to_string();
    match state {
        CloseoutRecoveryState::Clean => CloseoutRecoveryDecision::AlreadyCommitted,
        CloseoutRecoveryState::DirectResponsePatchback
        | CloseoutRecoveryState::BoundaryOnlyDrift
        | CloseoutRecoveryState::NestedParentPointerStale
        | CloseoutRecoveryState::OpenEmptyPreflight
        | CloseoutRecoveryState::QueueMetadataDrift => CloseoutRecoveryDecision::ReplaySafe {
            state,
            command: command(),
        },
        CloseoutRecoveryState::SidecarVisibleDrift => {
            CloseoutRecoveryDecision::ResetSidecarsFromVisible {
                state,
                command: command(),
            }
        }
        CloseoutRecoveryState::OpenCycle => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "open cycle must finish, be replayed, or be explicitly queued behind"
                .to_string(),
            recommended: command(),
        },
        CloseoutRecoveryState::MissingResponseBody => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "captured response body presence or supersession proof".to_string(),
            recommended: command(),
        },
        CloseoutRecoveryState::EscapedTemplatePatch => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "unescaped patchback blocks that can be applied safely".to_string(),
            recommended: command(),
        },
        CloseoutRecoveryState::UnsafeUserContentDrift => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "proof that visible user-authored content is metadata-only drift"
                .to_string(),
            recommended: command(),
        },
    }
}

/// Decide the authoritative side of a content-equal metadata-only drift between
/// a `local` document string (the candidate to commit) and the committed `head`.
///
/// The decision turns on the live auto-queue continuation signal
/// (`#recovery-drift-authoritative-side`). Because the caller has already proven
/// the content components are byte-identical, the only durable state the diff can
/// destroy is an active queue continuation. Legitimate consumption of a queue
/// head always shows up as response/content drift, so a continuation that exists
/// in HEAD but is gone or re-headed in a metadata-only local drift cannot have
/// been legitimately consumed.
pub fn metadata_drift_authority(local: &str, head: &str) -> MetadataDriftAuthority {
    let local_head = agent_doc_queue::queue_continuation::live_continuation_head(local);
    let head_head = agent_doc_queue::queue_continuation::live_continuation_head(head);
    match (local_head, head_head) {
        // HEAD carries a live continuation that the local side dropped entirely
        // (deactivated / drained / fenced) with no consuming response.
        (None, Some(_)) => MetadataDriftAuthority::Head,
        // Both sides carry a live continuation but with different ready heads,
        // and content equality proves no response consumed the old head.
        (Some(local_id), Some(head_id)) if local_id != head_id => MetadataDriftAuthority::Ambiguous,
        // Same live head, HEAD has no live continuation at risk, or neither side
        // does.
        _ => MetadataDriftAuthority::Local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_drift_authority_head_when_local_drops_live_continuation() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("queue_active: true", "queue_active: false");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Head
        );
    }

    #[test]
    fn metadata_drift_authority_local_when_no_live_head_continuation() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Local
        );
    }

    #[test]
    fn metadata_drift_authority_ambiguous_when_live_heads_diverge() {
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]", "- do [#z]");
        assert_eq!(
            metadata_drift_authority(&local, head),
            MetadataDriftAuthority::Ambiguous
        );
    }

    #[test]
    fn closeout_recovery_mutation_reason_labels_are_stable() {
        assert_eq!(
            CloseoutRecoveryMutationReason::BenignReplayBaseline.as_str(),
            "benign_replay_baseline"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline.as_str(),
            "queue_only_replay_baseline"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::CommitQueueMetadataDrift.as_str(),
            "commit_queue_metadata_drift"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::ResetFromVisible.as_str(),
            "reset_from_visible"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RestoreHeadMetadata.as_str(),
            "restore_head_metadata"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RetireWedgedWriteAppliedCapture.as_str(),
            "retire_wedged_write_applied_capture"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RetireSupersededCapturedOnlyOrphan.as_str(),
            "retire_superseded_captured_only_orphan"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::RespectManualTailRemoval.as_str(),
            "respect_manual_tail_removal"
        );
    }

    #[test]
    fn closeout_recovery_mutation_reason_owns_capture_refresh_labels() {
        assert_eq!(
            CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline.capture_refresh_event(),
            "capture_baseline_refreshed_for_queue_only_drift"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::QueueOnlyReplayBaseline.capture_refresh_message(),
            "queue-only drift detected"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::BenignReplayBaseline.capture_refresh_event(),
            "capture_baseline_refreshed_for_benign_drift"
        );
        assert_eq!(
            CloseoutRecoveryMutationReason::BenignReplayBaseline.capture_refresh_message(),
            "benign drift detected"
        );
    }

    #[test]
    fn closeout_recovery_state_labels_are_stable() {
        use CloseoutRecoveryState::*;
        let cases = [
            (Clean, "clean"),
            (OpenCycle, "open_cycle"),
            (MissingResponseBody, "missing_response_body"),
            (DirectResponsePatchback, "direct_response_patchback"),
            (EscapedTemplatePatch, "escaped_template_patch"),
            (BoundaryOnlyDrift, "boundary_only_drift"),
            (NestedParentPointerStale, "nested_parent_pointer_stale"),
            (OpenEmptyPreflight, "open_empty_preflight"),
            (QueueMetadataDrift, "queue_metadata_drift"),
            (SidecarVisibleDrift, "sidecar_visible_drift"),
            (UnsafeUserContentDrift, "unsafe_user_content_drift"),
        ];
        assert_eq!(cases.len(), CloseoutRecoveryState::ALL.len());
        for (state, label) in cases {
            assert_eq!(state.as_str(), label);
        }
    }

    #[test]
    fn recovery_decision_maps_states_to_typed_outcomes() {
        use CloseoutRecoveryDecision::*;
        use CloseoutRecoveryState::*;
        let command = Some("agent-doc recover tasks/doc.md");

        let default_cases = [
            (Clean, "already_committed"),
            (OpenCycle, "blocked"),
            (MissingResponseBody, "blocked"),
            (DirectResponsePatchback, "replay_safe"),
            (EscapedTemplatePatch, "blocked"),
            (BoundaryOnlyDrift, "replay_safe"),
            (NestedParentPointerStale, "replay_safe"),
            (OpenEmptyPreflight, "replay_safe"),
            (QueueMetadataDrift, "replay_safe"),
            (SidecarVisibleDrift, "reset_sidecars_from_visible"),
            (UnsafeUserContentDrift, "blocked"),
        ];
        assert_eq!(default_cases.len(), CloseoutRecoveryState::ALL.len());

        for (state, expected) in default_cases {
            let decision = closeout_recovery_decision_from_state(
                state,
                CloseoutRecoveryDecisionInput::default(),
                command,
            );
            assert_eq!(
                decision.as_str(),
                expected,
                "unexpected default decision for {state:?}: {decision:?}"
            );
            assert_eq!(
                decision.state(),
                if state == Clean { None } else { Some(state) },
                "decision should retain its source state for {state:?}: {decision:?}"
            );
            match decision {
                AlreadyCommitted => {}
                ReplaySafe {
                    command: rendered, ..
                }
                | ResetSidecarsFromVisible {
                    command: rendered, ..
                } => {
                    assert_eq!(rendered, command.unwrap());
                }
                Blocked {
                    missing_proof,
                    recommended,
                    ..
                } => {
                    assert!(
                        !missing_proof.is_empty(),
                        "blocked decision should name missing proof for {state:?}"
                    );
                    assert_eq!(recommended, command.unwrap());
                }
                other => panic!("default path unexpectedly produced {other:?} for {state:?}"),
            }
        }

        for state in CloseoutRecoveryState::ALL {
            assert_eq!(
                closeout_recovery_decision_from_state(
                    state,
                    CloseoutRecoveryDecisionInput {
                        prompt_context_available: true,
                        blocker_reason: Some("active closeout"),
                        stale_capture_supersession_proof: Some("superseded"),
                    },
                    command,
                ),
                QueuePromptForAfterCloseout {
                    state,
                    reason: "active closeout".to_string(),
                },
                "prompt context must take priority for {state:?}"
            );
            assert_eq!(
                closeout_recovery_decision_from_state(
                    state,
                    CloseoutRecoveryDecisionInput {
                        prompt_context_available: true,
                        blocker_reason: None,
                        stale_capture_supersession_proof: None,
                    },
                    None,
                ),
                QueuePromptForAfterCloseout {
                    state,
                    reason: state.as_str().to_string(),
                },
                "prompt context fallback reason should be the state name for {state:?}"
            );
        }

        for state in [MissingResponseBody, UnsafeUserContentDrift] {
            assert_eq!(
                closeout_recovery_decision_from_state(
                    state,
                    CloseoutRecoveryDecisionInput {
                        stale_capture_supersession_proof: Some("heading already answered"),
                        ..CloseoutRecoveryDecisionInput::default()
                    },
                    command,
                ),
                RetireStaleCapture {
                    state,
                    proof: "heading already answered".to_string(),
                }
            );
        }
    }
}
