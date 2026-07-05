use anyhow::Result;
use std::path::Path;

pub fn route_write_document(
    file: &Path,
    next_content: &str,
    previous_content: &str,
    reason: &str,
) -> Result<()> {
    if crate::invocation::force_disk_route_writes() {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, next_content)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{}_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                reason,
                file.display(),
                next_content.len(),
                agent_doc_hash::content_hash(next_content)
            ),
        );
        Ok(())
    } else {
        agent_doc_write_converge_io::converge_document_or_disk(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            next_content,
            previous_content,
            reason,
        )
    }
}
