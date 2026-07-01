//! Adapter from the focused queue-io write queue contract to the in-process
//! orchestration session actor.

use std::path::Path;

use agent_doc_document_realtime::session_ops::SessionOpKind;
use agent_doc_queue_io::write_queue::{DocumentWriteQueueSubmitter, serialized_atomic_write_with};
use anyhow::Result;

use crate::session_actor::document_actor_in;

struct SessionActorWriteQueueSubmitter;

impl DocumentWriteQueueSubmitter for SessionActorWriteQueueSubmitter {
    fn submit<R, F>(&self, base_dir: &Path, file: &str, kind: SessionOpKind, job: F) -> Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        let actor = document_actor_in(base_dir, file);
        actor.submit(kind, move |_ctx| job())
    }
}

static SESSION_ACTOR_WRITE_QUEUE: SessionActorWriteQueueSubmitter = SessionActorWriteQueueSubmitter;

/// Serialize an editor-visible document write through the document session
/// actor, using the focused queue-io policy for owner-scope handling and writer
/// class tagging.
pub fn serialized_atomic_write(
    base_dir: &Path,
    file: &str,
    abs_path: &Path,
    content: &str,
) -> Result<()> {
    serialized_atomic_write_with(
        &SESSION_ACTOR_WRITE_QUEUE,
        base_dir,
        file,
        abs_path,
        content,
        crate::write::atomic_write_pub,
    )
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

    fn read_count(path: &Path) -> u64 {
        std::fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    #[test]
    fn orchestration_adapter_uses_session_actor_order() {
        let dir = tempfile::TempDir::new().unwrap();
        let abs = seed(&dir, "doc.md", "0\n");
        let base = dir.path().to_path_buf();

        const N: u64 = 25;
        let barrier = Arc::new(Barrier::new(N as usize));
        let mut threads = Vec::new();
        for _ in 0..N {
            let barrier = barrier.clone();
            let base = base.clone();
            let abs = abs.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                agent_doc_queue_io::write_queue::run_serialized_with(
                    &SESSION_ACTOR_WRITE_QUEUE,
                    &base,
                    "doc.md",
                    SessionOpKind::WriteSubmit,
                    {
                        let abs = abs.clone();
                        move || {
                            let cur = read_count(&abs);
                            thread::yield_now();
                            crate::write::atomic_write_pub(&abs, &format!("{}\n", cur + 1))
                                .unwrap();
                        }
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
    fn orchestration_adapter_marks_owner_scope() {
        let dir = tempfile::TempDir::new().unwrap();
        seed(&dir, "doc.md", "x");
        let base = dir.path().to_path_buf();

        assert!(!agent_doc_document_realtime::write_authority::within_owner_scope());
        let inside = agent_doc_queue_io::write_queue::run_serialized_with(
            &SESSION_ACTOR_WRITE_QUEUE,
            &base,
            "doc.md",
            SessionOpKind::WriteSubmit,
            agent_doc_document_realtime::write_authority::within_owner_scope,
        )
        .unwrap();

        assert!(inside);
        assert!(!agent_doc_document_realtime::write_authority::within_owner_scope());
    }
}
