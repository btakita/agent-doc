//! CLI adapter for response replay dedupe effects.
//!
//! `agent-doc-response-replay-io` owns the file-facing dedupe flow. The CLI
//! supplies the remaining editor-aware write and snapshot effects directly so
//! `agent-doc-orchestration` does not keep a dedupe facade module.

use anyhow::Result;
use std::path::Path;

struct CliDedupeEffects;

impl agent_doc_response_replay_io::DedupeEffects for CliDedupeEffects {
    fn write_deduped_document(&self, file: &Path, previous: &str, deduped: &str) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            previous,
            deduped,
            "dedupe",
        )
    }

    fn save_snapshot(&self, file: &Path, deduped: &str) -> Result<()> {
        agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            deduped,
            agent_doc_ops_log_io::log_op,
        )
    }
}

const EFFECTS: CliDedupeEffects = CliDedupeEffects;

pub fn run(file: &Path) -> Result<()> {
    agent_doc_response_replay_io::run(&EFFECTS, file)
}
