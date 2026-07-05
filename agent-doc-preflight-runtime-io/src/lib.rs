//! Runtime adapters for preflight maintenance writes.

use anyhow::Result;
use std::path::Path;

pub struct RuntimePreflightMaintenanceWriteEffects;

pub static PREFLIGHT_MAINTENANCE_WRITE_EFFECTS: RuntimePreflightMaintenanceWriteEffects =
    RuntimePreflightMaintenanceWriteEffects;

impl agent_doc_preflight_io::PreflightMaintenanceWriteEffects
    for RuntimePreflightMaintenanceWriteEffects
{
    fn record_document_write_provenance(&self, file: &Path, content: &str) {
        agent_doc_document_realtime_io::record_document_write_provenance(file, content);
    }

    fn guard_visible_write_idle_and_current(
        &self,
        file: &Path,
        source: &str,
        expected_current: &str,
    ) -> Result<()> {
        agent_doc_document_realtime_io::guard_visible_write_idle_and_current(
            file,
            source,
            expected_current,
        )
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        source: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            source,
        )
    }
}
