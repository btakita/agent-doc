use agent_doc_document::transient_markers::{
    exchange_prompt_prefix_equivalent, strip_head_markers,
};
use anyhow::{Context, Result};
use std::path::Path;

pub trait CommitPreStageRepairEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()>;
    fn log_op(&self, file: &Path, message: &str);
}

pub fn dedupe_snapshot_and_worktree_before_commit(
    effects: &impl CommitPreStageRepairEffects,
    file: &Path,
    snapshot_content: &mut Option<String>,
    file_content: &mut String,
) -> Result<()> {
    let Some(snapshot) = snapshot_content.as_deref() else {
        return Ok(());
    };
    let deduped_snapshot = agent_doc_turn::response_replay::dedupe_responses(snapshot);
    if deduped_snapshot != snapshot {
        eprintln!(
            "[commit] deduped consecutive duplicate response block(s) before staging {}",
            file.display()
        );
        effects.log_op(
            file,
            &format!(
                "commit_pre_stage_dedupe file={} before_commit=true",
                file.display()
            ),
        );
        effects.save_snapshot(file, &deduped_snapshot)?;
        *snapshot_content = Some(deduped_snapshot);
    }

    let deduped_file = agent_doc_turn::response_replay::dedupe_responses(file_content);
    if deduped_file != *file_content {
        effects.atomic_write(file, &deduped_file).with_context(|| {
            format!(
                "failed to repair duplicate response blocks in {}",
                file.display()
            )
        })?;
        effects.log_op(
            file,
            &format!(
                "commit_pre_stage_dedupe_repaired_worktree file={} before_commit=true",
                file.display()
            ),
        );
        *file_content = deduped_file;
    }

    if let Some(snapshot) = snapshot_content.as_deref()
        && let Some(repaired_file) =
            agent_doc_element_exchange_io::repair_commit_prompt_artifacts_against_snapshot_with_log(
                file,
                snapshot,
                file_content,
                |file, message| effects.log_op(file, message),
            )
    {
        let mut snapshot_updated = false;
        if exchange_prompt_prefix_equivalent(snapshot, &repaired_file) {
            let clean_snapshot = strip_head_markers(&repaired_file);
            effects.save_snapshot(file, &clean_snapshot)?;
            *snapshot_content = Some(clean_snapshot);
            snapshot_updated = true;
        }
        if repaired_file != *file_content {
            effects
                .atomic_write(file, &repaired_file)
                .with_context(|| {
                    format!(
                        "failed to repair duplicate prompt artifacts in {}",
                        file.display()
                    )
                })?;
            *file_content = repaired_file;
        }
        effects.log_op(
            file,
            &format!(
                "commit_pre_stage_prompt_duplicate_repaired file={} snapshot_updated={} before_commit=true",
                file.display(),
                snapshot_updated
            ),
        );
    }

    Ok(())
}
