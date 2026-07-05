use anyhow::{Context, Result};
use std::path::Path;

pub mod backlog_guards;
pub mod closeout_guards;
pub mod command;
pub mod detect;
pub mod guard_modes;
pub mod partial_staging;
pub mod pending_capture;
pub mod pending_guards;
pub mod prompt_bearing;
pub mod queue_head_guards;
pub mod queue_head_provenance_guards;
pub mod response_guards;
pub mod write_pending_checks;

pub use backlog_guards::*;
pub use closeout_guards::*;
pub use command::*;
pub use detect::*;
pub use guard_modes::*;
pub use partial_staging::*;
pub use pending_capture::*;
pub use pending_guards::*;
pub use prompt_bearing::*;
pub use queue_head_guards::*;
pub use queue_head_provenance_guards::*;
pub use response_guards::*;
pub use write_pending_checks::*;

pub(crate) fn resolve_current_document(
    file: &Path,
    source: &str,
) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
    agent_doc_document_realtime_io::try_resolve_current_document(file).with_context(|| {
        format!(
            "session-check {source}: resolve current document {}",
            file.display()
        )
    })
}

pub(crate) fn resolve_current_document_with_force_disk(
    file: &Path,
    source: &str,
    force_disk: bool,
) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
    if !force_disk {
        return resolve_current_document(file, source);
    }
    agent_doc_document_realtime_io::resolve_disk_current_document(
        file,
        &format!("session-check {source}"),
    )
}

pub(crate) fn resolve_current_document_content(file: &Path, source: &str) -> Result<String> {
    Ok(resolve_current_document(file, source)?.into_content())
}

pub(crate) fn resolve_current_document_content_with_force_disk(
    file: &Path,
    source: &str,
    force_disk: bool,
) -> Result<String> {
    Ok(resolve_current_document_with_force_disk(file, source, force_disk)?.into_content())
}

pub(crate) fn operator_live_buffer_contains_heading(file: &Path, heading: &str) -> bool {
    let file_key = file.to_string_lossy();
    let heading = heading.trim();
    if heading.is_empty() {
        return false;
    }
    for snapshot in agent_doc_debounce::live_buffer_snapshots(&file_key) {
        if !snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY) {
            continue;
        }
        if !agent_doc_debounce::live_buffer_snapshot_editor_is_live(&snapshot) {
            continue;
        }
        let Some(content) = snapshot.content.as_deref() else {
            continue;
        };
        let content_norm =
            agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content);
        if content_norm.lines().any(|line| line.trim() == heading) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "session_check_committed_response_visible_in_live_buffer file={} heading={:?} editor_id={:?}",
                    file.display(),
                    heading,
                    snapshot.editor_id
                ),
            );
            return true;
        }
    }
    false
}
