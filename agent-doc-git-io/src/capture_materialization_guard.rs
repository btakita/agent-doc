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
            "commit_blocked_missing_captured_response file={} capture_id={} response_sha256={} basis={}",
            file.display(),
            capture.capture_id,
            capture.response_sha256,
            basis
        ),
    );
    effects.log_missing_capture_guard(file);
    anyhow::bail!(
        "captured response body is not present in the staged snapshot for {} even though the snapshot already matches HEAD; refusing already-committed closeout. Replay the captured response with `agent-doc write --commit {}` before marking the cycle committed.",
        file.display(),
        file.display()
    );
}
