//! Status component writeback adapters.
//!
//! Pure status projection lives in `agent-doc-document`. This crate owns the
//! command flow for `write --status`: read the document, apply the focused
//! projection, choose forced disk write vs editor-aware convergence, and emit
//! the status-write audit marker through injected orchestration effects.

use anyhow::{Context, Result};
use std::path::Path;

/// Effects required by status writeback.
pub trait StatusWriteEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_or_disk_write(
        &self,
        file: &Path,
        previous: &str,
        updated: &str,
        phase: &str,
    ) -> Result<()>;

    fn record_document_write_provenance(&self, file: &Path, updated: &str);

    fn log_op(&self, file: &Path, message: &str);
}

pub struct RuntimeStatusWriteEffects;

pub static RUNTIME_STATUS_WRITE_EFFECTS: RuntimeStatusWriteEffects = RuntimeStatusWriteEffects;

impl StatusWriteEffects for RuntimeStatusWriteEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        previous: &str,
        updated: &str,
        phase: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            previous,
            updated,
            phase,
        )
    }

    fn record_document_write_provenance(&self, file: &Path, updated: &str) {
        agent_doc_document_realtime_io::record_document_write_provenance(file, updated);
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

/// Replace the status component content with the provided text.
pub fn set<E: StatusWriteEffects + ?Sized>(effects: &E, file: &Path, text: &str) -> Result<()> {
    set_with_options(effects, file, text, false)
}

pub fn set_with_runtime_options(file: &Path, text: &str, force_disk: bool) -> Result<()> {
    set_with_options(&RUNTIME_STATUS_WRITE_EFFECTS, file, text, force_disk)
}

pub fn set_with_options<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    text: &str,
    force_disk: bool,
) -> Result<()> {
    let full_content = if force_disk {
        effects.force_disk_document_content(file, "status_set")
    } else {
        effects.current_document_content(file, "status_set")
    }
    .context("failed to read document")?;
    let new_doc =
        agent_doc_document::status_projection::replace_status_content(&full_content, text)?;
    if force_disk {
        std::fs::write(file, &new_doc)
            .with_context(|| format!("status_set: failed to write {}", file.display()))?;
        effects.record_document_write_provenance(file, &new_doc);
        effects.log_op(
            file,
            &format!(
                "status_set_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                new_doc.len(),
                agent_doc_hash::content_hash(&new_doc)
            ),
        );
        return Ok(());
    }
    effects.converge_or_disk_write(file, &full_content, &new_doc, "status_set")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct TestEffects {
        current_content: RefCell<Option<String>>,
        disk_content: RefCell<Option<String>>,
        previous: RefCell<Option<String>>,
        updated: RefCell<Option<String>>,
        current_reads: Cell<u32>,
        disk_reads: Cell<u32>,
        converges: Cell<u32>,
        provenance: Cell<u32>,
        logs: RefCell<Vec<String>>,
    }

    impl TestEffects {
        fn new() -> Self {
            Self {
                current_content: RefCell::new(None),
                disk_content: RefCell::new(None),
                previous: RefCell::new(None),
                updated: RefCell::new(None),
                current_reads: Cell::new(0),
                disk_reads: Cell::new(0),
                converges: Cell::new(0),
                provenance: Cell::new(0),
                logs: RefCell::new(Vec::new()),
            }
        }

        fn with_current_content(self, content: impl Into<String>) -> Self {
            *self.current_content.borrow_mut() = Some(content.into());
            self
        }

        fn with_disk_content(self, content: impl Into<String>) -> Self {
            *self.disk_content.borrow_mut() = Some(content.into());
            self
        }
    }

    impl StatusWriteEffects for TestEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            self.current_reads.set(self.current_reads.get() + 1);
            if let Some(content) = self.current_content.borrow().clone() {
                return Ok(content);
            }
            Ok(std::fs::read_to_string(file)?)
        }

        fn force_disk_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            self.disk_reads.set(self.disk_reads.get() + 1);
            if let Some(content) = self.disk_content.borrow().clone() {
                return Ok(content);
            }
            Ok(std::fs::read_to_string(file)?)
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            previous: &str,
            updated: &str,
            _phase: &str,
        ) -> Result<()> {
            self.converges.set(self.converges.get() + 1);
            *self.previous.borrow_mut() = Some(previous.to_string());
            *self.updated.borrow_mut() = Some(updated.to_string());
            std::fs::write(file, updated)?;
            Ok(())
        }

        fn record_document_write_provenance(&self, _file: &Path, _updated: &str) {
            self.provenance.set(self.provenance.get() + 1);
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.borrow_mut().push(message.to_string());
        }
    }

    fn write_status_doc(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let doc = dir.path().join("plan.md");
        std::fs::write(
            &doc,
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "old status\n",
                "<!-- /agent:status -->\n",
            ),
        )
        .unwrap();
        doc
    }

    #[test]
    fn set_uses_converge_write_by_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let effects = TestEffects::new();

        set(&effects, &doc, "new status").unwrap();

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(on_disk.contains("new status"));
        assert!(!on_disk.contains("old status"));
        assert_eq!(effects.converges.get(), 1);
        assert_eq!(effects.provenance.get(), 0);
        assert!(effects.logs.borrow().is_empty());
        assert_eq!(effects.current_reads.get(), 1);
        assert_eq!(effects.disk_reads.get(), 0);
    }

    #[test]
    fn set_projects_from_current_document_not_stale_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let current = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "editor status\n",
            "<!-- /agent:status -->\n",
            "\noperator note\n",
        );
        let effects = TestEffects::new().with_current_content(current);

        set(&effects, &doc, "new status").unwrap();

        let previous = effects.previous.borrow().clone().unwrap();
        assert!(previous.contains("editor status"));
        assert!(previous.contains("operator note"));
        let updated = effects.updated.borrow().clone().unwrap();
        assert!(updated.contains("new status"));
        assert!(updated.contains("operator note"));
        assert!(!updated.contains("old status"));
        assert_eq!(effects.current_reads.get(), 1);
        assert_eq!(effects.disk_reads.get(), 0);
    }

    #[test]
    fn force_disk_writes_records_provenance_and_logs_hash() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let effects = TestEffects::new().with_disk_content(
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "disk status\n",
                "<!-- /agent:status -->\n",
            )
            .to_string(),
        );

        set_with_options(&effects, &doc, "new status", true).unwrap();

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(on_disk.contains("new status"));
        assert!(!on_disk.contains("disk status"));
        assert_eq!(effects.current_reads.get(), 0);
        assert_eq!(effects.disk_reads.get(), 1);
        assert_eq!(effects.converges.get(), 0);
        assert_eq!(effects.provenance.get(), 1);
        let logs = effects.logs.borrow();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("status_set_writeback"));
        assert!(logs[0].contains("transport=disk_force"));
        assert!(logs[0].contains("reason=force_disk"));
    }
}
