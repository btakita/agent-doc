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

/// Replace the status component content with the provided text.
pub fn set<E: StatusWriteEffects + ?Sized>(effects: &E, file: &Path, text: &str) -> Result<()> {
    set_with_options(effects, file, text, false)
}

pub fn set_with_options<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    text: &str,
    force_disk: bool,
) -> Result<()> {
    let full_content = std::fs::read_to_string(file).context("failed to read document")?;
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
        converges: Cell<u32>,
        provenance: Cell<u32>,
        logs: RefCell<Vec<String>>,
    }

    impl TestEffects {
        fn new() -> Self {
            Self {
                converges: Cell::new(0),
                provenance: Cell::new(0),
                logs: RefCell::new(Vec::new()),
            }
        }
    }

    impl StatusWriteEffects for TestEffects {
        fn converge_or_disk_write(
            &self,
            file: &Path,
            _previous: &str,
            updated: &str,
            _phase: &str,
        ) -> Result<()> {
            self.converges.set(self.converges.get() + 1);
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
    }

    #[test]
    fn force_disk_writes_records_provenance_and_logs_hash() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let effects = TestEffects::new();

        set_with_options(&effects, &doc, "new status", true).unwrap();

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(on_disk.contains("new status"));
        assert_eq!(effects.converges.get(), 0);
        assert_eq!(effects.provenance.get(), 1);
        let logs = effects.logs.borrow();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("status_set_writeback"));
        assert!(logs[0].contains("transport=disk_force"));
        assert!(logs[0].contains("reason=force_disk"));
    }
}
