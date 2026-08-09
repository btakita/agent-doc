//! Response replay file-effect adapters.
//!
//! Pure replay/deduplication policy lives in `agent-doc-turn`. This crate owns
//! the file-facing dedupe command flow: read the document, apply the focused
//! response replay policy, write the deduped document through an injected writer,
//! and update the injected cold snapshot projection.

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

/// How many times `run` re-reads the authority and reapplies the cut before it
/// reports the repair as unconverged.
///
/// `#dedupepresettle`: the write path may accept a repair and return before the
/// deferred delivery projection has published it, so a single pass can report
/// "removed N bytes" over a document that still holds the duplicate. Observed
/// 2026-08-08 on `tasks/agent-doc/agent-doc-bugs2.md`: the first `agent-doc
/// dedupe` printed `removed 3313 bytes` and exited 0 with both response copies
/// still present, and a byte-identical rerun actually converged. Dedupe is a
/// repair command whose entire contract is "the duplicate is gone", so it must
/// prove the fixpoint itself instead of inheriting the ordinary write path's
/// deferred-delivery success.
const DEDUPE_CONVERGENCE_ATTEMPTS: u32 = 5;

/// Detect and remove duplicate consecutive response blocks from a document.
pub fn run<E: DedupeEffects + ?Sized>(effects: &E, file: &Path) -> Result<()> {
    let original = current_document_content(file)?;

    if dedupe_responses(&original) == original {
        eprintln!("[dedupe] no duplicates found in {}", file.display());
        return Ok(());
    }

    let mut content = original.clone();
    for attempt in 1..=DEDUPE_CONVERGENCE_ATTEMPTS {
        let result = dedupe_responses(&content);
        if result == content {
            let removed = original.len().saturating_sub(content.len());
            eprintln!(
                "[dedupe] removed {} bytes of duplicate content from {}",
                removed,
                file.display()
            );
            return Ok(());
        }

        effects.write_deduped_document(file, &content, &result)?;
        effects.save_snapshot(file, &result)?;

        // Re-read the authority rather than trusting the cut we just handed the
        // write path: an accepted-but-undelivered repair leaves the duplicate in
        // place, and reporting removed bytes off `result` would claim a repair
        // that never became visible.
        content = current_document_content(file)?;
        if attempt < DEDUPE_CONVERGENCE_ATTEMPTS && dedupe_responses(&content) != content {
            eprintln!(
                "[dedupe] repair not yet visible in {} (attempt {}/{}); reapplying against current authority",
                file.display(),
                attempt,
                DEDUPE_CONVERGENCE_ATTEMPTS,
            );
        }
    }

    anyhow::bail!(
        "[dedupe] duplicate response content is still present in {} after {} write attempts; \
         the repair was accepted but has not converged. Do not force a disk write — re-run \
         `agent-doc dedupe` once the editor/CRDT delivery settles",
        file.display(),
        DEDUPE_CONVERGENCE_ATTEMPTS,
    )
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
    fn run_removes_duplicate_and_updates_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, duplicate_doc()).unwrap();

        let effects = TestEffects::new();
        run(&effects, &file).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content.matches("### Re: topic").count(), 1);
        assert_eq!(effects.writes.get(), 1);
        assert_eq!(effects.snapshots.get(), 1);
        let snapshot = std::fs::read_to_string(agent_doc_fs::snapshot_path_for(&file).unwrap())
            .expect("snapshot should be updated");
        assert_eq!(snapshot, content);
    }

    /// `#dedupepresettle`: a write the delivery projection accepted but has not
    /// published leaves the duplicate in place. `run` must reapply against the
    /// re-read authority instead of reporting removed bytes off the cut it just
    /// handed the write path.
    struct DeferredFirstWriteEffects {
        writes: Cell<u32>,
        defer_writes: u32,
    }

    impl DedupeEffects for DeferredFirstWriteEffects {
        fn write_deduped_document(
            &self,
            file: &Path,
            _previous: &str,
            deduped: &str,
        ) -> Result<()> {
            self.writes.set(self.writes.get() + 1);
            if self.writes.get() > self.defer_writes {
                std::fs::write(file, deduped)?;
            }
            Ok(())
        }

        fn save_snapshot(&self, _file: &Path, _deduped: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn run_reapplies_until_the_repair_is_visible() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, duplicate_doc()).unwrap();

        let effects = DeferredFirstWriteEffects {
            writes: Cell::new(0),
            defer_writes: 1,
        };
        run(&effects, &file).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content.matches("### Re: topic").count(), 1);
        assert_eq!(
            effects.writes.get(),
            2,
            "the accepted-but-undelivered first write must be reapplied"
        );
    }

    #[test]
    fn run_fails_when_the_repair_never_converges() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, duplicate_doc()).unwrap();

        let effects = DeferredFirstWriteEffects {
            writes: Cell::new(0),
            defer_writes: u32::MAX,
        };
        let err = run(&effects, &file).unwrap_err().to_string();

        assert!(
            err.contains("has not converged"),
            "an unconverged repair must not report success: {err}"
        );
        assert!(
            err.contains("Do not force a disk write"),
            "the refusal must keep the operator off the disk-write escape hatch: {err}"
        );
        assert_eq!(effects.writes.get(), DEDUPE_CONVERGENCE_ATTEMPTS);
        assert_eq!(
            std::fs::read_to_string(&file)
                .unwrap()
                .matches("### Re: topic")
                .count(),
            2,
        );
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
