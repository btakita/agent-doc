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
    let Some(capture) = effects.load_active_capture(file)? else {
        return Ok(());
    };
    if capture.terminal {
        return Ok(());
    }
    let Some(materialized) = staged_content else {
        return Ok(());
    };
    if agent_doc_turn::response_replay::response_materialized_in_content(
        &capture.response_body,
        materialized,
    ) {
        return Ok(());
    }

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
