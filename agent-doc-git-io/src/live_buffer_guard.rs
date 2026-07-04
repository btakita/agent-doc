use anyhow::Result;
use std::path::Path;

use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;

pub trait LiveBufferGuardEffects {
    fn live_buffer_diverges_from_content(
        &self,
        file: &Path,
        file_content: &str,
    ) -> Option<agent_doc_debounce::LiveBufferSnapshot>;
    fn log_op(&self, file: &Path, message: &str);
    fn log_live_buffer_guard_blocked(&self, file: &Path);
}

pub fn ensure_no_live_editor_buffer_ahead_of_disk(
    effects: &impl LiveBufferGuardEffects,
    file: &Path,
    file_content: &str,
    basis: &str,
    staged_content: Option<&str>,
) -> Result<()> {
    let Some(snapshot) = effects.live_buffer_diverges_from_content(file, file_content) else {
        return Ok(());
    };
    let editor_id = snapshot.editor_id.as_deref().unwrap_or("unknown");
    if let Some(staged) = staged_content
        && live_buffer_snapshot_matches_content(&snapshot, staged)
        && snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY)
        && snapshot.edit_epoch <= snapshot.last_synced_epoch
    {
        effects.log_op(
            file,
            &format!(
                "commit_live_buffer_ahead_of_disk_allowed file={} basis={} editor_id={} edit_epoch={} last_synced_epoch={} buffer_len={} disk_len={} reason=staged_snapshot_matches_synced_operator_buffer",
                file.display(),
                basis,
                editor_id,
                snapshot.edit_epoch,
                snapshot.last_synced_epoch,
                snapshot.len,
                file_content.len()
            ),
        );
        return Ok(());
    }
    if let Some(staged) = staged_content
        && snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY)
        && live_buffer_insertions_are_materialized_in_file(&snapshot, staged, file_content)
    {
        effects.log_op(
            file,
            &format!(
                "commit_live_buffer_ahead_of_disk_allowed file={} basis={} editor_id={} edit_epoch={} last_synced_epoch={} buffer_len={} disk_len={} allowance=staged_snapshot_excludes_materialized_operator_buffer",
                file.display(),
                basis,
                editor_id,
                snapshot.edit_epoch,
                snapshot.last_synced_epoch,
                snapshot.len,
                file_content.len()
            ),
        );
        return Ok(());
    }
    effects.log_op(
        file,
        &format!(
            "commit_blocked_live_buffer_ahead_of_disk file={} basis={} editor_id={} edit_epoch={} last_synced_epoch={} buffer_len={} disk_len={}",
            file.display(),
            basis,
            editor_id,
            snapshot.edit_epoch,
            snapshot.last_synced_epoch,
            snapshot.len,
            file_content.len()
        ),
    );
    effects.log_live_buffer_guard_blocked(file);
    anyhow::bail!(
        "live editor buffer has unflushed changes ahead of disk for {}; refusing to commit from stale disk (editor_id={}, edit_epoch={}, last_synced_epoch={})",
        file.display(),
        editor_id,
        snapshot.edit_epoch,
        snapshot.last_synced_epoch
    );
}

pub fn live_buffer_snapshot_matches_content(
    snapshot: &agent_doc_debounce::LiveBufferSnapshot,
    content: &str,
) -> bool {
    if snapshot.len == content.len()
        && snapshot
            .hash
            .eq_ignore_ascii_case(&agent_doc_hash::content_hash(content))
    {
        return true;
    }
    snapshot.content.as_ref().is_some_and(|editor_text| {
        normalize_transient_agent_doc_markers(editor_text)
            == normalize_transient_agent_doc_markers(content)
    })
}

pub fn live_buffer_insertions_are_materialized_in_file(
    snapshot: &agent_doc_debounce::LiveBufferSnapshot,
    staged_content: &str,
    file_content: &str,
) -> bool {
    let Some(editor_text) = snapshot.content.as_deref() else {
        return false;
    };

    let normalized_file = normalize_transient_agent_doc_markers(file_content);
    let normalized_staged = normalize_transient_agent_doc_markers(staged_content);
    let diff = similar::TextDiff::from_lines(staged_content, editor_text);
    let mut saw_insert = false;
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let inserted = change.value().trim_end_matches('\n');
        let normalized_inserted = normalize_transient_agent_doc_markers(inserted);
        let trimmed = normalized_inserted.trim();
        if trimmed.is_empty() || trimmed == "(HEAD)" || trimmed.starts_with("<!-- agent:boundary:")
        {
            continue;
        }
        saw_insert = true;
        if normalized_staged.contains(trimmed) {
            continue;
        }
        if !normalized_file.contains(trimmed) {
            return false;
        }
    }
    saw_insert
}
