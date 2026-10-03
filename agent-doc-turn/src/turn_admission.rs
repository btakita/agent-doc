//! Closed-cycle turn admission (`#admissionsteeringagree`, GH #118).
//!
//! Two commands, one state, one predicate. Preflight admission and
//! `session-check` both answer "may this turn continue?" for a document whose
//! last agent-doc cycle is closed. They used to answer it with two different
//! compositions: `session-check` let a fresh operator prompt win ("realtime
//! steering — run `agent-doc <FILE>` and answer it") while preflight refused the
//! same document over snapshot/HEAD drift ("without an open or recoverable
//! agent-doc cycle ... then stop") and named `reset --from-current`, which
//! rebuilds the baseline from the visible file and so folds the unanswered prompt
//! into it, after which no cycle ever answers it.
//!
//! The decision lives here, once. The I/O shell is
//! `agent_doc_session_check_io::turn_admission`, the steering observation is
//! `agent_doc_document_realtime::baseline_comparison::closed_cycle_steering_between`,
//! and every site — preflight's drift gates, the drift detector, the
//! `session-check` committed-cycle verdict, the closeout recovery hint, and the
//! recovery executors — derives its behaviour from [`TurnAdmission`].
//! `agent-doc-turn/tests/turn_admission_guard.rs` enforces that the sites call it.

use crate::closeout_recovery::CloseoutRecoveryState;

/// Facts the admission decision needs. Callers observe them; this module decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnAdmissionFacts {
    /// An agent-doc cycle is still open. Open cycles have their own recovery
    /// (finish, replay, or cancel) and are never decided here.
    pub cycle_open: bool,
    /// The visible document carries unanswered operator steering relative to the
    /// closed cycle's baseline (`closed_cycle_steering_between`).
    pub steering_pending: bool,
}

/// The single answer to "may this turn continue?" for a closed cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnAdmission {
    /// An open cycle owns recovery; this predicate makes no claim.
    OpenCycle,
    /// Fresh operator steering is the next turn's prompt. Admit the turn
    /// (preflight) / tell the agent to continue (`session-check`). Closeout drift
    /// is carried by that turn's commit, and no recovery that rebuilds the
    /// baseline from — or restores over — the visible file may run first.
    ContinueWithSteering,
    /// No steering is pending, so closeout drift must be proven clean or recovered
    /// before the turn may be admitted.
    RequireCleanCloseout,
}

impl TurnAdmission {
    pub const fn decide(facts: TurnAdmissionFacts) -> Self {
        if facts.cycle_open {
            Self::OpenCycle
        } else if facts.steering_pending {
            Self::ContinueWithSteering
        } else {
            Self::RequireCleanCloseout
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenCycle => "open_cycle",
            Self::ContinueWithSteering => "continue_with_steering",
            Self::RequireCleanCloseout => "require_clean_closeout",
        }
    }

    /// Whether closed-cycle drift must block admission. `false` for steering:
    /// refusing would strand the operator's prompt behind a recovery that eats it.
    pub const fn requires_clean_closeout(self) -> bool {
        matches!(self, Self::RequireCleanCloseout)
    }

    pub const fn continues_with_steering(self) -> bool {
        matches!(self, Self::ContinueWithSteering)
    }
}

/// Recovery token rendered in place of an absorbing recovery while steering is
/// pending.
pub const PENDING_OPERATOR_STEERING_RECOVERY: &str = "pending_operator_steering";

/// Whether the recovery named for `state` rebuilds the baseline from the visible
/// file, commits the visible file as metadata, or restores over it.
///
/// `RecoveryProjectionVisibleDrift` names `reset --from-current` (baseline :=
/// visible). `BoundaryOnlyDrift` / `QueueMetadataDrift` /
/// `NestedParentPointerStale` / `Clean` name `agent-doc commit`, whose
/// document-only path may adopt the visible document as the new baseline, and
/// `repair --apply-recovery` for the metadata states may restore the visible file
/// from HEAD. Each one, run while an operator prompt is unanswered, makes that
/// prompt stop being a diff — so none of them may be named while steering is
/// pending. Response-preserving states (`write --commit`, `finalize`) keep the
/// operator's edit outside the baseline and stay as they are.
pub const fn recovery_absorbs_visible_document(state: CloseoutRecoveryState) -> bool {
    matches!(
        state,
        CloseoutRecoveryState::Clean
            | CloseoutRecoveryState::BoundaryOnlyDrift
            | CloseoutRecoveryState::NestedParentPointerStale
            | CloseoutRecoveryState::QueueMetadataDrift
            | CloseoutRecoveryState::RecoveryProjectionVisibleDrift
    )
}

/// The steering-preserving recovery: answer the prompt first.
pub fn steering_preserving_recovery(document: &str, steering: &str) -> String {
    format!(
        "Recovery [{PENDING_OPERATOR_STEERING_RECOVERY}]: `agent-doc {document}` — continue the turn and answer the unanswered operator steering, which is the next turn's prompt; that turn's commit carries the remaining drift. Do NOT run `agent-doc commit`, `agent-doc reset --from-current`, or `agent-doc repair --apply-recovery` first: each would fold the unanswered prompt into the baseline (or restore over it) so no cycle ever answers it. Pending steering: {steering}"
    )
}

/// Pick the recovery text a refusal names (`#admissionsteeringagree`).
///
/// `rendered` is the per-state recovery the classifier produced; it is returned
/// unchanged unless steering is pending and that recovery would absorb it.
pub fn steering_safe_recovery(
    document: &str,
    state: CloseoutRecoveryState,
    steering: Option<&str>,
    rendered: String,
) -> String {
    match steering {
        Some(steering) if recovery_absorbs_visible_document(state) => {
            steering_preserving_recovery(document, steering)
        }
        _ => rendered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_after_a_closed_cycle_continues_and_never_requires_clean_closeout() {
        let admission = TurnAdmission::decide(TurnAdmissionFacts {
            cycle_open: false,
            steering_pending: true,
        });
        assert_eq!(admission, TurnAdmission::ContinueWithSteering);
        assert!(!admission.requires_clean_closeout());
        assert!(admission.continues_with_steering());
    }

    #[test]
    fn no_steering_after_a_closed_cycle_requires_clean_closeout() {
        let admission = TurnAdmission::decide(TurnAdmissionFacts {
            cycle_open: false,
            steering_pending: false,
        });
        assert_eq!(admission, TurnAdmission::RequireCleanCloseout);
        assert!(admission.requires_clean_closeout());
    }

    #[test]
    fn an_open_cycle_is_never_decided_here() {
        for steering_pending in [false, true] {
            let admission = TurnAdmission::decide(TurnAdmissionFacts {
                cycle_open: true,
                steering_pending,
            });
            assert_eq!(admission, TurnAdmission::OpenCycle);
            assert!(!admission.requires_clean_closeout());
            assert!(!admission.continues_with_steering());
        }
    }

    #[test]
    fn absorbing_recoveries_are_replaced_while_steering_is_pending() {
        for state in CloseoutRecoveryState::ALL {
            let rendered = format!("rendered:{}", state.as_str());
            let with_steering = steering_safe_recovery(
                "doc.md",
                state,
                Some("content_edit: bug"),
                rendered.clone(),
            );
            let without = steering_safe_recovery("doc.md", state, None, rendered.clone());
            assert_eq!(without, rendered, "no steering keeps {}", state.as_str());
            if recovery_absorbs_visible_document(state) {
                assert!(
                    with_steering.contains(PENDING_OPERATOR_STEERING_RECOVERY)
                        && with_steering.contains("`agent-doc doc.md`")
                        && with_steering.contains("content_edit: bug"),
                    "{}: {with_steering}",
                    state.as_str()
                );
            } else {
                assert_eq!(
                    with_steering,
                    rendered,
                    "{} is response-preserving",
                    state.as_str()
                );
            }
        }
    }

    /// GH #118's exact recovery: `reset --from-current` must never be named while
    /// a prompt is unanswered, because it is the one that folds the prompt away.
    #[test]
    fn reset_from_current_is_never_named_over_pending_steering() {
        let rendered = crate::closeout_recovery::closeout_recovery_command(
            crate::closeout_recovery::CloseoutRecoveryCommandInput {
                document: "doc.md".into(),
                state: CloseoutRecoveryState::RecoveryProjectionVisibleDrift,
                open_cycle: None,
            },
        )
        .unwrap();
        assert!(rendered.contains("reset --from-current"));
        let named = steering_safe_recovery(
            "doc.md",
            CloseoutRecoveryState::RecoveryProjectionVisibleDrift,
            Some("content_edit: bug: when I select the remote terminal"),
            rendered,
        );
        let runnable =
            crate::closeout_recovery::short_recovery_command_from_recommendation(&named).unwrap();
        assert_eq!(runnable, "agent-doc doc.md");
    }
}
