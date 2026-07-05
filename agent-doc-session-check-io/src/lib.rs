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

pub(crate) fn resolve_current_document_content(file: &Path, source: &str) -> Result<String> {
    Ok(resolve_current_document(file, source)?.into_content())
}
