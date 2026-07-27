use std::path::Path;

use anyhow::Result;

pub struct RuntimeDirectRunEffects;

pub static DIRECT_RUN_EFFECTS: RuntimeDirectRunEffects = RuntimeDirectRunEffects;

impl agent_doc_run_io::DirectRunEffects for RuntimeDirectRunEffects {
    fn guard_no_exchange_compaction_request_for_diff(
        &self,
        file: &Path,
        diff_text: &str,
    ) -> Result<()> {
        agent_doc_run_io::guard_no_exchange_compaction_request_for_diff(file, diff_text)
    }

    fn commit(&self, file: &Path) -> Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn normalize_template_structure_or_fail(&self, content: &str, file: &Path) -> Result<String> {
        agent_doc_template_io::normalize_template_structure_or_fail(content, file)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn consume_queue_prompts_for_done_ids_with_outcome(
        &self,
        file: &Path,
        done_ids: &[String],
        force_disk: bool,
    ) -> Result<Option<agent_doc_queue_io::queue_consume::QueueConsumptionOutcome>> {
        if force_disk {
            agent_doc_queue_io::queue_consume::consume_queue_prompts_with_outcome(
                file,
                done_ids,
                true,
                &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
            )
        } else {
            agent_doc_queue_io::queue_consume::consume_queue_prompts_for_done_ids_with_outcome(
                file,
                done_ids,
                &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
            )
        }
    }

    fn complete_required_closeout(&self, file: &Path, force_disk: bool) -> Result<()> {
        agent_doc_closeout_runtime_io::complete_required_closeout(file, force_disk).map(|_| ())
    }

    fn abandon_recursive_cycle(&self, file: &Path, event: &str, diagnostic: &str) -> Result<()> {
        agent_doc_run_io::abandon_run_recursive_cycle(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            diagnostic,
        )
    }
}
