//! Response replay file-effect adapters.
//!
//! Pure replay/deduplication policy lives in `agent-doc-turn`. This crate owns
//! the file-facing dedupe command flow: read the document, apply the focused
//! response replay policy, write the deduped document through an injected writer,
//! update the injected snapshot store, and remove stale patch sidecars.

use agent_doc_turn::response_replay::dedupe_responses;
use anyhow::Result;
use std::path::Path;

/// Effects required by the response-dedupe command.
pub trait DedupeEffects {
    fn write_deduped_document(&self, file: &Path, previous: &str, deduped: &str) -> Result<()>;
    fn save_snapshot(&self, file: &Path, deduped: &str) -> Result<()>;
}

fn current_document_content(file: &Path) -> Result<String> {
    agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "response_replay_dedupe",
    )
}

/// Detect and remove duplicate consecutive response blocks from a document.
pub fn run<E: DedupeEffects + ?Sized>(effects: &E, file: &Path) -> Result<()> {
    let content = current_document_content(file)?;

    let result = dedupe_responses(&content);

    if result == content {
        eprintln!("[dedupe] no duplicates found in {}", file.display());
        return Ok(());
    }

    let removed = content.len() - result.len();
    effects.write_deduped_document(file, &content, &result)?;
    effects.save_snapshot(file, &result)?;
    clean_stale_patch_file(file);

    eprintln!(
        "[dedupe] removed {} bytes of duplicate content from {}",
        removed,
        file.display()
    );

    Ok(())
}

fn clean_stale_patch_file(file: &Path) {
    if let Ok(hash) = agent_doc_fs::document_state_hash(file)
        && let Some(project_root) = agent_doc_fs::find_project_root(file)
    {
        let patch_file = project_root
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        if patch_file.exists() {
            eprintln!(
                "[dedupe] cleaning stale patch file: {}",
                patch_file.display()
            );
            if let Err(e) = std::fs::remove_file(&patch_file) {
                eprintln!("[dedupe] WARNING: failed to remove stale patch file: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct TestEffects {
        writes: Cell<u32>,
        snapshots: Cell<u32>,
    }

    impl TestEffects {
        fn new() -> Self {
            Self {
                writes: Cell::new(0),
                snapshots: Cell::new(0),
            }
        }
    }

    impl DedupeEffects for TestEffects {
        fn write_deduped_document(
            &self,
            file: &Path,
            _previous: &str,
            deduped: &str,
        ) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            std::fs::write(file, deduped)?;
            Ok(())
        }

        fn save_snapshot(&self, file: &Path, deduped: &str) -> Result<()> {
            self.snapshots.set(self.snapshots.get() + 1);
            let path = agent_doc_fs::snapshot_path_for(file)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, deduped)?;
            Ok(())
        }
    }

    fn duplicate_doc() -> String {
        "### Re: topic\nanswer\n### Re: topic\nanswer\n".to_string()
    }

    #[test]
    fn run_removes_duplicate_updates_snapshot_and_cleans_patch() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/patches")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, duplicate_doc()).unwrap();
        let hash = agent_doc_fs::document_state_hash(&file).unwrap();
        let patch_file = dir
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        std::fs::write(&patch_file, "{}").unwrap();

        let effects = TestEffects::new();
        run(&effects, &file).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content.matches("### Re: topic").count(), 1);
        assert_eq!(effects.writes.get(), 1);
        assert_eq!(effects.snapshots.get(), 1);
        assert!(!patch_file.exists());
        let snapshot = std::fs::read_to_string(agent_doc_fs::snapshot_path_for(&file).unwrap())
            .expect("snapshot should be updated");
        assert_eq!(snapshot, content);
    }

    #[test]
    fn run_noops_when_no_duplicate_response() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "### Re: topic\n\nanswer\n").unwrap();

        let effects = TestEffects::new();
        run(&effects, &file).unwrap();

        assert_eq!(effects.writes.get(), 0);
        assert_eq!(effects.snapshots.get(), 0);
    }
}
