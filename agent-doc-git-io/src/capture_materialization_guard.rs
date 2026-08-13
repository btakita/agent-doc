use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::Result;
use std::path::Path;

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
    let ownership = effects
        .retained_write_ownership(file)
        .with_retained_capture(true);
    let remedy = agent_doc_turn::write_ownership::retained_write_remedy(
        ownership,
        &file.display().to_string(),
    );
    anyhow::bail!(
        "captured response body is not present in the staged snapshot for {} even though the snapshot already matches HEAD; refusing already-committed closeout. {remedy}.",
        file.display()
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
        assert!(err.contains("`agent-doc session-check plan.md`"));
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
            err.contains("`agent-doc session-check plan.md`")
                && err.contains("deferral, not a lost response"),
            "the loaded capture must retain recovery ownership: {err}"
        );
        assert!(err.contains("captured response body is not present"));
    }
}
