use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::Result;
use std::path::Path;

/// Structural marker stamped on the missing-captured-response refusal below.
///
/// Callers on the far side of a process or crate boundary (the Codex Stop
/// hook classifies the rendered closeout error) must recognize this refusal
/// without matching its prose, the same reason the retained-write refusal
/// carries `AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN`. The refusal itself does
/// not decide who owns the capture; pair it with
/// `agent_doc_capture_io::retained_write_ownership` before treating it as a
/// deferral.
pub const MISSING_CAPTURED_RESPONSE_REFUSAL_TOKEN: &str =
    "refusal=captured_response_not_materialized";

/// The one safe recovery for a captured response stranded after its editor
/// delivery route has disappeared.
///
/// This decision is intentionally narrower than generic snapshot adoption:
/// the exact captured response must already be present in controller
/// authority, it must still be absent from the commit surface, and the caller
/// must prove that adopting authority would not consume a newer edit. The
/// commit coordinator remains responsible for that last document-level proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnownedEditorCaptureRecovery {
    AdoptAuthoritativeCurrent,
    Refuse,
}

pub fn plan_unowned_editor_capture_recovery(
    ownership: agent_doc_turn::write_ownership::RetainedWriteOwnership,
    response_body: &str,
    staged_content: Option<&str>,
    authoritative_current: &str,
    has_unowned_document_edit: bool,
) -> UnownedEditorCaptureRecovery {
    let response_in_commit_surface = staged_content.is_some_and(|content| {
        agent_doc_turn::response_replay::response_materialized_in_content(response_body, content)
    });
    let response_in_authority = agent_doc_turn::response_replay::response_materialized_in_content(
        response_body,
        authoritative_current,
    );

    if ownership.editor_route_unowned
        && ownership.verdict().commit_is_the_named_recovery()
        && !response_in_commit_surface
        && response_in_authority
        && !has_unowned_document_edit
    {
        UnownedEditorCaptureRecovery::AdoptAuthoritativeCurrent
    } else {
        UnownedEditorCaptureRecovery::Refuse
    }
}

/// Whether a rendered error is the missing-captured-response commit refusal.
pub fn is_missing_captured_response_refusal(message: &str) -> bool {
    message.contains(MISSING_CAPTURED_RESPONSE_REFUSAL_TOKEN)
}

pub struct ActiveCaptureMaterialization {
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
    pub terminal: bool,
}

pub trait CaptureMaterializationGuardEffects {
    fn load_active_capture(&self, file: &Path) -> Result<Option<ActiveCaptureMaterialization>>;
    /// Whether anything durable owns a retained write for this document.
    ///
    /// `#commitwritecommitdeadlock`: this guard used to name
    /// `agent-doc write --commit <FILE>` unconditionally. When the cycle is at
    /// `write_applied`, that command answers with the `AwaitingTerminalCommit`
    /// remedy — which names `agent-doc commit <FILE>`, the command whose guard
    /// this is. Two commands, one state, each pointing at the other.
    /// Implementors call `agent_doc_capture_io::retained_write_ownership`.
    fn retained_write_ownership(
        &self,
        file: &Path,
    ) -> agent_doc_turn::write_ownership::RetainedWriteOwnership;
    fn response_materialized_in_referenced_compact_archive(
        &self,
        file: &Path,
        response_body: &str,
        commit_surface: &str,
    ) -> bool;
    fn log_op(&self, file: &Path, message: &str);
    fn log_missing_capture_guard(&self, file: &Path);
}

pub fn ensure_active_capture_materialized_for_head_current_noop(
    effects: &impl CaptureMaterializationGuardEffects,
    file: &Path,
    snapshot_content: Option<&str>,
    head_doc: Option<&str>,
) -> Result<()> {
    ensure_active_capture_materialized_for_commit(
        effects,
        file,
        snapshot_content.or(head_doc),
        "head_current",
    )
}

pub fn ensure_active_capture_materialized_for_commit(
    effects: &impl CaptureMaterializationGuardEffects,
    file: &Path,
    staged_content: Option<&str>,
    basis: &str,
) -> Result<()> {
    let capture = effects.load_active_capture(file)?;
    let response_in_commit_surface =
        capture
            .as_ref()
            .zip(staged_content)
            .is_some_and(|(capture, materialized)| {
                agent_doc_turn::response_replay::response_materialized_in_content(
                    &capture.response_body,
                    materialized,
                )
            });
    let response_in_referenced_compact_archive =
        capture
            .as_ref()
            .zip(staged_content)
            .is_some_and(|(capture, materialized)| {
                !response_in_commit_surface
                    && effects.response_materialized_in_referenced_compact_archive(
                        file,
                        &capture.response_body,
                        materialized,
                    )
            });
    let decision = agent_doc_workflow::capture::decide_capture_closeout_materialization(
        agent_doc_workflow::capture::CaptureCloseoutMaterializationEvidence {
            active_capture: capture.is_some(),
            capture_terminal: capture.as_ref().is_some_and(|capture| capture.terminal),
            commit_surface_available: staged_content.is_some(),
            response_in_commit_surface,
            response_in_referenced_compact_archive,
        },
    );
    match decision {
        agent_doc_workflow::capture::CaptureCloseoutMaterializationDecision::Allow(
            agent_doc_workflow::capture::CaptureCloseoutMaterializationBasis::ReferencedCompactArchive,
        ) => {
            let capture = capture.as_ref().expect("archive decision requires active capture");
            effects.log_op(
                file,
                &format!(
                    "commit_capture_materialized_in_referenced_compact_archive file={} capture_id={} response_sha256={} basis={}",
                    file.display(),
                    capture.capture_id,
                    capture.response_sha256,
                    basis,
                ),
            );
            return Ok(());
        }
        agent_doc_workflow::capture::CaptureCloseoutMaterializationDecision::Allow(_) => {
            return Ok(());
        }
        agent_doc_workflow::capture::CaptureCloseoutMaterializationDecision::BlockMissingResponse => {}
    }
    let capture = capture.expect("blocked materialization requires active capture");

    effects.log_op(
        file,
        &format!(
            "{} file={} capture_id={} response_sha256={} basis={}",
            OpsLogEvent::CommitBlockedMissingCapturedResponse,
            file.display(),
            capture.capture_id,
            capture.response_sha256,
            basis
        ),
    );
    effects.log_missing_capture_guard(file);

    // The capture loaded above is stronger evidence than a second sidecar read:
    // it proves captured-finalize still owns materialization and terminal
    // commit. Preserve that evidence through the shared verdict so this guard
    // cannot bounce between `commit`, `write --commit`, and a new cycle while
    // the binary-owned worker is already waiting on editor convergence.
    let mut ownership = effects
        .retained_write_ownership(file)
        .with_retained_capture(true);
    if ownership.editor_route_unowned {
        // `commit` was the route-removal recovery only while an exact
        // response-bearing authority cut could still be materialized. We are
        // already inside that command and proved that its commit surface does
        // not contain the capture, so prescribing `commit` again would create
        // a terminal instruction loop. Preserve the durable capture and hand
        // it to its same-intent resume transition instead; do not ask the
        // caller to replay the response or force disk.
        ownership.editor_route_unowned = false;
        ownership.retained_projection = false;
        ownership.capture_resume_unowned = true;
    }
    let remedy = agent_doc_turn::write_ownership::retained_write_remedy(
        ownership,
        &file.display().to_string(),
    );
    anyhow::bail!(
        "captured response body is not present in the staged snapshot for {} even though the snapshot already matches HEAD; refusing already-committed closeout ({}). {remedy}.",
        file.display(),
        MISSING_CAPTURED_RESPONSE_REFUSAL_TOKEN,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::write_ownership::RetainedWriteOwnership;
    use std::cell::RefCell;

    struct Effects {
        ownership: RetainedWriteOwnership,
        logs: RefCell<Vec<String>>,
    }

    impl Effects {
        fn with(ownership: RetainedWriteOwnership) -> Self {
            Self {
                ownership,
                logs: RefCell::new(Vec::new()),
            }
        }
    }

    impl CaptureMaterializationGuardEffects for Effects {
        fn load_active_capture(
            &self,
            _file: &Path,
        ) -> Result<Option<ActiveCaptureMaterialization>> {
            Ok(Some(ActiveCaptureMaterialization {
                capture_id: "cap-1".to_string(),
                response_sha256: "sha".to_string(),
                response_body: "### Re: something — opus-5\n\nbody\n".to_string(),
                terminal: false,
            }))
        }

        fn retained_write_ownership(&self, _file: &Path) -> RetainedWriteOwnership {
            self.ownership
        }

        fn response_materialized_in_referenced_compact_archive(
            &self,
            _file: &Path,
            _response_body: &str,
            _commit_surface: &str,
        ) -> bool {
            false
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.borrow_mut().push(message.to_string());
        }

        fn log_missing_capture_guard(&self, _file: &Path) {}
    }

    fn blocked_error(ownership: RetainedWriteOwnership) -> String {
        let effects = Effects::with(ownership);
        let err = ensure_active_capture_materialized_for_head_current_noop(
            &effects,
            Path::new("plan.md"),
            Some("staged content without the captured response"),
            None,
        )
        .expect_err("a missing captured response must fail closed");
        format!("{err:#}")
    }

    /// At `write_applied`, an active capture still owns materialization and the
    /// terminal commit. This guard must not prescribe any competing mutation.
    #[test]
    fn a_captured_write_applied_cycle_stays_with_binary_owned_finalize() {
        let captured = RetainedWriteOwnership::new_with_phase(true, true, true);
        assert!(
            !captured.verdict().commit_is_the_named_recovery(),
            "a retained capture must keep terminal commit binary-owned"
        );

        let err = blocked_error(captured);
        assert!(err.contains("this exact retained intent"), "{err}");
        assert!(
            err.contains("controller-owned terminal state edge"),
            "{err}"
        );
        assert!(
            !err.contains("Run `agent-doc session-check plan.md`"),
            "a document-wide status check can observe a successor cycle: {err}"
        );
        assert!(
            !err.contains("Replay the captured response with `agent-doc write --commit")
                && !err.contains("Finish it from the pane"),
            "manual recovery races the retained capture: {err}"
        );
    }

    /// Loading the active capture is itself ownership proof even if a racing
    /// secondary read reports an unowned shape.
    #[test]
    fn loaded_capture_refines_a_racing_unowned_read() {
        let unowned = RetainedWriteOwnership::UNOWNED;
        let err = blocked_error(unowned);
        assert!(
            err.contains("this exact retained intent")
                && err.contains("controller-owned terminal state edge")
                && err.contains("deferral, not a lost response"),
            "the loaded capture must retain recovery ownership: {err}"
        );
        assert!(!err.contains("Run `agent-doc session-check plan.md`"));
        assert!(err.contains("captured response body is not present"));
    }

    /// The refusal is classified structurally by callers across a crate or
    /// process boundary (the Codex Stop hook), so it must carry its token.
    #[test]
    fn missing_captured_response_refusal_carries_structural_token() {
        let err = blocked_error(RetainedWriteOwnership::new_with_phase(true, true, false));
        assert!(
            is_missing_captured_response_refusal(&err),
            "refusal must be recognizable without prose matching: {err}"
        );
        assert!(!is_missing_captured_response_refusal(
            "captured response body is not present in the staged snapshot"
        ));
        assert!(!is_missing_captured_response_refusal(
            "recovery=await_editor_replica_no_disk_write"
        ));
    }

    #[test]
    fn unregistered_zero_replica_route_adopts_exact_response_bearing_authority() {
        let response = "### Re: prompt — gpt-5\n\nRecovered answer.\n";
        let staged = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n";
        let authority = "<!-- agent:exchange -->\n❯ prompt\n### Re: prompt — gpt-5\n\nRecovered answer.\n<!-- /agent:exchange -->\n";
        let ownership = RetainedWriteOwnership::new(true, true)
            .with_retained_projection(true)
            .with_delivery_rejected(true)
            .with_editor_route_unowned(true);

        assert_eq!(
            plan_unowned_editor_capture_recovery(
                ownership,
                response,
                Some(staged),
                authority,
                false,
            ),
            UnownedEditorCaptureRecovery::AdoptAuthoritativeCurrent,
        );
    }

    #[test]
    fn unregistered_route_never_adopts_missing_response_or_fresh_prompt() {
        let response = "### Re: prompt — gpt-5\n\nRecovered answer.\n";
        let staged = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n";
        let authority = format!(
            "<!-- agent:exchange -->\n❯ prompt\n{response}❯ later prompt\n<!-- /agent:exchange -->\n"
        );
        let ownership = RetainedWriteOwnership::new(true, true)
            .with_retained_projection(true)
            .with_delivery_rejected(true)
            .with_editor_route_unowned(true);

        assert_eq!(
            plan_unowned_editor_capture_recovery(
                ownership,
                response,
                Some(staged),
                &authority,
                true,
            ),
            UnownedEditorCaptureRecovery::Refuse,
        );
        assert_eq!(
            plan_unowned_editor_capture_recovery(ownership, response, Some(staged), staged, false,),
            UnownedEditorCaptureRecovery::Refuse,
        );
    }

    #[test]
    fn missing_capture_after_route_removal_names_same_capture_resume_not_commit() {
        let ownership = RetainedWriteOwnership::new(true, true)
            .with_retained_projection(true)
            .with_delivery_rejected(true)
            .with_editor_route_unowned(true);
        let err = blocked_error(ownership);

        assert!(err.contains("repair --resume-capture plan.md"), "{err}");
        assert!(!err.contains("Run `agent-doc commit plan.md`"), "{err}");
    }
}
