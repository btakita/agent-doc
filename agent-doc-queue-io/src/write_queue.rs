//! Document write queue serialization API.
//!
//! This module owns the queue-side serialization contract and typed writer
//! classes. The runtime mailbox remains an adapter supplied by orchestration
//! until the session actor itself moves to a focused crate.

use std::path::Path;

use agent_doc_document_realtime::session_ops::SessionOpKind;
use anyhow::Result;

/// Submit document write work to a single ordered owner for one document.
///
/// Implementations own the effectful mailbox. This crate owns the policy that
/// every submitted job runs under the document write-authority owner scope and
/// that all writer classes share the same serialized path.
pub trait DocumentWriteQueueSubmitter {
    fn submit<R, F>(&self, base_dir: &Path, file: &str, kind: SessionOpKind, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static;
}

/// Run a document write critical section through the provided ordered owner.
///
/// The owner-scope guard is set while `job` executes so nested raw document
/// writes do not re-enter the blocking write queue.
pub fn run_serialized_with<S, R, F>(
    submitter: &S,
    base_dir: &Path,
    file: &str,
    kind: SessionOpKind,
    job: F,
) -> Result<R>
where
    S: DocumentWriteQueueSubmitter + ?Sized,
    R: Send + 'static,
    F: FnOnce() -> R + Send + 'static,
{
    submitter.submit(base_dir, file, kind, move || {
        let _owner_scope = agent_doc_document_realtime::write_authority::owner_scope_guard();
        job()
    })
}

/// Serialized atomic disk write convenience.
///
/// The caller supplies the raw write function because the low-level document
/// write primitive still lives outside this focused crate.
pub fn serialized_atomic_write_with<S, W>(
    submitter: &S,
    base_dir: &Path,
    file: &str,
    abs_path: &Path,
    content: &str,
    write_raw: W,
) -> Result<()>
where
    S: DocumentWriteQueueSubmitter + ?Sized,
    W: FnOnce(&Path, &str) -> Result<()> + Send + 'static,
{
    let abs_path = abs_path.to_path_buf();
    let content = content.to_string();
    run_serialized_with(
        submitter,
        base_dir,
        file,
        SessionOpKind::WriteSubmit,
        move || write_raw(&abs_path, &content),
    )?
}

/// Typed API over the one ordered write queue.
///
/// The writer classes tag the routed write for observability, but all methods
/// submit to the same queue supplied by `submitter`.
pub struct DocumentWriteQueue<'a, S: DocumentWriteQueueSubmitter + ?Sized> {
    submitter: &'a S,
    base_dir: &'a Path,
    file: &'a str,
}

impl<'a, S: DocumentWriteQueueSubmitter + ?Sized> DocumentWriteQueue<'a, S> {
    pub fn new(submitter: &'a S, base_dir: &'a Path, file: &'a str) -> Self {
        Self {
            submitter,
            base_dir,
            file,
        }
    }

    /// Agent `finalize` / `write` / `stream` disk write.
    pub fn agent_write<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        run_serialized_with(
            self.submitter,
            self.base_dir,
            self.file,
            SessionOpKind::WriteSubmit,
            job,
        )
    }

    /// Repair disk write.
    pub fn repair_write<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        run_serialized_with(
            self.submitter,
            self.base_dir,
            self.file,
            SessionOpKind::Closeout,
            job,
        )
    }

    /// Supervisor idle / file-watch write.
    pub fn supervisor_write<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        run_serialized_with(
            self.submitter,
            self.base_dir,
            self.file,
            SessionOpKind::Lifecycle,
            job,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    #[derive(Default)]
    struct SerialSubmitter {
        lock: Mutex<()>,
    }

    impl DocumentWriteQueueSubmitter for SerialSubmitter {
        fn submit<R, F>(
            &self,
            _base_dir: &Path,
            _file: &str,
            _kind: SessionOpKind,
            job: F,
        ) -> Result<R>
        where
            R: Send + 'static,
            F: FnOnce() -> R + Send + 'static,
        {
            let _guard = self.lock.lock().unwrap();
            Ok(job())
        }
    }

    fn seed(dir: &tempfile::TempDir, rel: &str, content: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join(rel);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, content).unwrap();
        file
    }

    fn read_count(path: &Path) -> u64 {
        std::fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    fn write_raw(path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content)?;
        Ok(())
    }

    #[test]
    fn write_queue_serializes_concurrent_read_modify_write_no_lost_update() {
        let dir = tempfile::TempDir::new().unwrap();
        let abs = seed(&dir, "doc.md", "0\n");
        let base = dir.path().to_path_buf();
        let submitter = Arc::new(SerialSubmitter::default());

        const N: u64 = 50;
        let barrier = Arc::new(Barrier::new(N as usize));
        let mut threads = Vec::new();
        for _ in 0..N {
            let barrier = barrier.clone();
            let base = base.clone();
            let abs = abs.clone();
            let submitter = submitter.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                run_serialized_with(
                    submitter.as_ref(),
                    &base,
                    "doc.md",
                    SessionOpKind::WriteSubmit,
                    move || {
                        let cur = read_count(&abs);
                        thread::yield_now();
                        write_raw(&abs, &format!("{}\n", cur + 1)).unwrap();
                    },
                )
                .unwrap();
            }));
        }
        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(read_count(&abs), N);
    }

    #[test]
    fn write_queue_finalize_never_observes_half_applied_write() {
        let dir = tempfile::TempDir::new().unwrap();
        let complete_a = "AAAA-complete\n";
        let complete_b = "BBBB-complete\n";
        let abs = seed(&dir, "doc.md", complete_a);
        let base = dir.path().to_path_buf();
        let submitter = Arc::new(SerialSubmitter::default());
        let barrier = Arc::new(Barrier::new(3));

        let sup = {
            let base = base.clone();
            let abs = abs.clone();
            let barrier = barrier.clone();
            let submitter = submitter.clone();
            thread::spawn(move || {
                let q = DocumentWriteQueue::new(submitter.as_ref(), &base, "doc.md");
                barrier.wait();
                for i in 0..200 {
                    let content = if i % 2 == 0 { complete_a } else { complete_b };
                    let abs = abs.clone();
                    q.supervisor_write(move || write_raw(&abs, content).unwrap())
                        .unwrap();
                }
            })
        };

        let agent = {
            let base = base.clone();
            let abs = abs.clone();
            let barrier = barrier.clone();
            let submitter = submitter.clone();
            thread::spawn(move || {
                let q = DocumentWriteQueue::new(submitter.as_ref(), &base, "doc.md");
                barrier.wait();
                for _ in 0..200 {
                    let abs = abs.clone();
                    q.agent_write(move || {
                        let observed = std::fs::read_to_string(&abs).unwrap();
                        assert!(
                            observed == complete_a || observed == complete_b,
                            "finalize observed a half-applied write: {observed:?}"
                        );
                        write_raw(&abs, complete_a).unwrap();
                    })
                    .unwrap();
                }
            })
        };

        barrier.wait();
        sup.join().unwrap();
        agent.join().unwrap();

        let final_doc = std::fs::read_to_string(&abs).unwrap();
        assert!(final_doc == complete_a || final_doc == complete_b);
    }

    #[test]
    fn write_queue_serialized_atomic_write_persists_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let abs = seed(&dir, "doc.md", "old\n");
        let submitter = SerialSubmitter::default();

        serialized_atomic_write_with(
            &submitter,
            dir.path(),
            "doc.md",
            &abs,
            "new content\n",
            write_raw,
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&abs).unwrap(), "new content\n");
    }

    #[test]
    fn run_serialized_marks_owner_scope_and_does_not_leak() {
        let dir = tempfile::TempDir::new().unwrap();
        seed(&dir, "doc.md", "x");
        let submitter = SerialSubmitter::default();

        assert!(!agent_doc_document_realtime::write_authority::within_owner_scope());
        let inside = run_serialized_with(
            &submitter,
            dir.path(),
            "doc.md",
            SessionOpKind::WriteSubmit,
            agent_doc_document_realtime::write_authority::within_owner_scope,
        )
        .unwrap();
        assert!(inside);
        assert!(!agent_doc_document_realtime::write_authority::within_owner_scope());
    }

    #[test]
    fn write_queue_typed_writer_classes_share_one_order() {
        let dir = tempfile::TempDir::new().unwrap();
        seed(&dir, "doc.md", "");
        let submitter = SerialSubmitter::default();
        let q = DocumentWriteQueue::new(&submitter, dir.path(), "doc.md");
        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let o = order.clone();
        q.agent_write(move || o.lock().unwrap().push("agent"))
            .unwrap();
        let o = order.clone();
        q.supervisor_write(move || o.lock().unwrap().push("sup"))
            .unwrap();
        let o = order.clone();
        q.repair_write(move || o.lock().unwrap().push("repair"))
            .unwrap();

        assert_eq!(*order.lock().unwrap(), vec!["agent", "sup", "repair"]);
    }
}
