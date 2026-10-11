use anyhow::Result;
use std::path::Path;

pub struct RuntimeBacklogCommandEffects;

pub static RUNTIME_BACKLOG_COMMAND_EFFECTS: RuntimeBacklogCommandEffects =
    RuntimeBacklogCommandEffects;

impl agent_doc_element_backlog_io::BacklogCommandEffects for RuntimeBacklogCommandEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            reason,
        )
    }

    fn record_document_write_provenance(&self, file: &Path, content: &str) {
        agent_doc_document_realtime_io::record_document_write_provenance(file, content);
    }

    fn attribute_agent_write_to_baseline(&self, file: &Path, before: &str, after: &str) {
        // Attribution is best-effort bookkeeping on a write that already landed:
        // a failure leaves the baseline untouched (the fail-safe direction) and
        // must not turn the successful command into a failure.
        if let Err(err) = agent_doc_snapshot_io::attribute_agent_write_to_document_baseline(
            file,
            before,
            after,
            agent_doc_ops_log_io::log_op,
        ) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_baseline_agent_write_attribution file={} outcome=error error={}",
                    file.display(),
                    format!("{err:#}").replace('\n', " | ")
                ),
            );
        }
    }
}
