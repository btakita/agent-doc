//! # Module: write_queue
//!
//! ## Spec (`#pcpc3` — in-process ordered write queue, realizes `#pcp3b`)
//! Routes every same-process writer of a session document — `write` / `stream`
//! / `finalize` / repair disk writes **and** supervisor idle/file-watch writes —
//! through the document's single session-actor mailbox (`#pcpc1`), so they
//! execute in one serialized order on one owner thread.
//!
//! Today those writers are ordered only by the advisory `flock`
//! (`write::acquire_doc_lock`) plus SQLite CAS. `flock` serializes *across
//! processes* but a single process can still interleave a supervisor
//! idle-write and an agent finalize between lock acquisitions, which is the R6
//! self-race / exit-75 root: a finalize observes a document the supervisor
//! half-rewrote (read-modify-write lost update / torn logical state). With the
//! supervisor now in-process (`#pcpc2`), routing every writer through the
//! session actor's ordered queue removes that interleave at the root. The
//! `flock` stays as a cross-process backstop only.
//!
//! ## Agentic Contracts
//! - `run_serialized` is the queue primitive: it runs a write critical section
//!   (read-modify-write included) on the document's actor owner thread. Two
//!   concurrent critical sections for the same document never overlap, so a
//!   read-modify-write never loses an update.
//! - `serialized_atomic_write` is the disk-write convenience built on it; the
//!   typed [`DocumentWriteQueue`] facade tags the four routed writer classes for
//!   observability while sharing one ordered queue.
//! - The live `write.rs` / `stream.rs` / finalize / repair / supervisor call
//!   sites still call `atomic_write` directly; the cutover that points them at
//!   this queue is the `#pcpc5` 08b gate ladder (shadow → dual-write →
//!   read-switch → authority → removal). Landing the queue first keeps it
//!   independently shippable without disturbing the live write path.
//!
//! ## Evals
//! - `write_queue_serializes_concurrent_read_modify_write_no_lost_update`
//! - `write_queue_finalize_never_observes_half_applied_write`
//! - `write_queue_serialized_atomic_write_persists_content`
//! - `write_queue_typed_writer_classes_share_one_order`

use std::path::Path;

use anyhow::Result;

use agent_doc_document_realtime::session_ops::SessionOpKind;

use crate::session_actor::document_actor_in;

/// Run a document write critical section on the document's session-actor owner
/// thread. Every caller for the same canonical document funnels through one
/// mailbox, so the closures execute in a single serialized order — a
/// read-modify-write inside `job` is therefore atomic against any other
/// `run_serialized` (or typed writer) for the same document.
///
/// `kind` tags the writer class for observability; all classes share the one
/// ordered queue.
pub fn run_serialized<R, F>(base_dir: &Path, file: &str, kind: SessionOpKind, job: F) -> Result<R>
where
    R: Send + 'static,
    F: FnOnce() -> R + Send + 'static,
{
    let actor = document_actor_in(base_dir, file);
    actor.submit(kind, move |_ctx| {
        // Mark the owner thread so a nested `atomic_write` (e.g. the inner
        // `atomic_write_pub` of `serialized_atomic_write`, or any document write
        // performed inside `job`) takes the raw write path instead of
        // re-entering this blocking mailbox and deadlocking the owner thread.
        let _owner_scope = agent_doc_document_realtime::write_authority::owner_scope_guard();
        job()
    })
}

/// Serialized atomic disk write: routes `write::atomic_write_pub` through the
/// document's ordered write queue. Use this instead of a bare `atomic_write`
/// once the `#pcpc5` cutover lands so a supervisor write and an agent finalize
/// can never interleave.
pub fn serialized_atomic_write(
    base_dir: &Path,
    file: &str,
    abs_path: &Path,
    content: &str,
) -> Result<()> {
    let abs_path = abs_path.to_path_buf();
    let content = content.to_string();
    run_serialized(base_dir, file, SessionOpKind::WriteSubmit, move || {
        crate::write::atomic_write_pub(&abs_path, &content)
    })?
}

/// Typed facade over the one ordered write queue. The four methods tag the
/// writer classes the `#pcpc3` spec enumerates (agent finalize/stream/repair vs
/// supervisor idle/file-watch) so logs and metrics can attribute a serialized
/// write without changing its ordering — they all submit to the same mailbox.
pub struct DocumentWriteQueue<'a> {
    base_dir: &'a Path,
    file: &'a str,
}

impl<'a> DocumentWriteQueue<'a> {
    /// Bind a queue facade to a canonical document.
    pub fn new(base_dir: &'a Path, file: &'a str) -> Self {
        Self { base_dir, file }
    }

    /// Agent `finalize` / `write` / `stream` disk write.
    pub fn agent_write<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        run_serialized(self.base_dir, self.file, SessionOpKind::WriteSubmit, job)
    }

    /// Repair disk write (orphaned-response / missed-patchback recovery).
    pub fn repair_write<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        run_serialized(self.base_dir, self.file, SessionOpKind::Closeout, job)
    }

    /// Supervisor idle / file-watch write.
    pub fn supervisor_write<R, F>(&self, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        run_serialized(self.base_dir, self.file, SessionOpKind::Lifecycle, job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn seed(dir: &tempfile::TempDir, rel: &str, content: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join(rel);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, content).unwrap();
        file
    }

    /// Read-modify-write a u64 counter stored as the file's text.
    fn read_count(path: &Path) -> u64 {
        std::fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    #[test]
    fn write_queue_serializes_concurrent_read_modify_write_no_lost_update() {
        // SimWorld: N threads each read the counter, increment, and write it
        // back — all through the ordered queue for ONE document. Serialization
        // means no lost updates: final == initial + N. Without the queue these
        // unsynchronized RMW cycles would race and drop increments.
        let dir = tempfile::TempDir::new().unwrap();
        let abs = seed(&dir, "doc.md", "0\n");
        let base = dir.path().to_path_buf();

        const N: u64 = 50;
        let barrier = Arc::new(Barrier::new(N as usize));
        let mut threads = Vec::new();
        for _ in 0..N {
            let barrier = barrier.clone();
            let base = base.clone();
            let abs = abs.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                run_serialized(&base, "doc.md", SessionOpKind::WriteSubmit, move || {
                    let cur = read_count(&abs);
                    // Widen the RMW window so an unserialized run would lose updates.
                    thread::yield_now();
                    crate::write::atomic_write_pub(&abs, &format!("{}\n", cur + 1)).unwrap();
                })
                .unwrap();
            }));
        }
        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(
            read_count(&abs),
            N,
            "serialized read-modify-write must not lose any increment"
        );
    }

    #[test]
    fn write_queue_finalize_never_observes_half_applied_write() {
        // SimWorld: a "supervisor" writer and an "agent finalize" writer race on
        // one document. Every write goes through the queue and writes a COMPLETE
        // tagged document. A finalize critical section reads the file first and
        // asserts it always sees a complete prior document (one writer's full
        // content), never a torn/half-applied mix.
        let dir = tempfile::TempDir::new().unwrap();
        let complete_a = "AAAA-complete\n";
        let complete_b = "BBBB-complete\n";
        let abs = seed(&dir, "doc.md", complete_a);
        let base = dir.path().to_path_buf();

        let barrier = Arc::new(Barrier::new(3));

        // Supervisor writer thread: flips between two complete documents.
        let sup = {
            let base = base.clone();
            let abs = abs.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let q = DocumentWriteQueue::new(&base, "doc.md");
                barrier.wait();
                for i in 0..200 {
                    let content = if i % 2 == 0 { complete_a } else { complete_b };
                    let abs = abs.clone();
                    q.supervisor_write(move || {
                        crate::write::atomic_write_pub(&abs, content).unwrap()
                    })
                    .unwrap();
                }
            })
        };

        // Agent finalize thread: reads-then-writes; the read must always be a
        // complete document.
        let agent = {
            let base = base.clone();
            let abs = abs.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let q = DocumentWriteQueue::new(&base, "doc.md");
                barrier.wait();
                for _ in 0..200 {
                    let abs = abs.clone();
                    q.agent_write(move || {
                        let observed = std::fs::read_to_string(&abs).unwrap();
                        assert!(
                            observed == complete_a || observed == complete_b,
                            "finalize observed a half-applied write: {observed:?}"
                        );
                        crate::write::atomic_write_pub(&abs, complete_a).unwrap();
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
        serialized_atomic_write(dir.path(), "doc.md", &abs, "new content\n").unwrap();
        assert_eq!(std::fs::read_to_string(&abs).unwrap(), "new content\n");
    }

    #[test]
    fn run_serialized_marks_owner_scope_and_does_not_leak() {
        // #pcpc5cut re-entrancy guard: while a job runs on the owner thread, the
        // owner scope is active so a nested `atomic_write` takes the raw path
        // instead of re-entering this blocking mailbox (which would deadlock).
        // The scope must not leak back to the caller thread.
        let dir = tempfile::TempDir::new().unwrap();
        seed(&dir, "doc.md", "x");
        let base = dir.path().to_path_buf();

        assert!(!agent_doc_document_realtime::write_authority::within_owner_scope());
        let inside = run_serialized(&base, "doc.md", SessionOpKind::WriteSubmit, || {
            agent_doc_document_realtime::write_authority::within_owner_scope()
        })
        .unwrap();
        assert!(
            inside,
            "owner thread must be marked in-scope so nested atomic_write stays raw"
        );
        assert!(
            !agent_doc_document_realtime::write_authority::within_owner_scope(),
            "owner scope must not leak to the submitting thread"
        );
    }

    #[test]
    fn write_queue_typed_writer_classes_share_one_order() {
        // The three typed methods must funnel into the SAME ordered queue, so a
        // sequence submitted across classes executes in submission order.
        let dir = tempfile::TempDir::new().unwrap();
        seed(&dir, "doc.md", "");
        let base = dir.path().to_path_buf();
        let q = DocumentWriteQueue::new(&base, "doc.md");
        let order: Arc<std::sync::Mutex<Vec<&'static str>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));

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
