//! Document session operation vocabulary.
//!
//! This module owns pure classifications shared by realtime write/watch
//! routing. Runtime queues, actor mailboxes, and effectful dispatch remain in
//! orchestration.

/// Classification of a document-scoped op. Runtime mailboxes execute arbitrary
/// job closures; the kind is carried for ordering/observability and to mirror
/// the op taxonomy `08b` §"Document write and watch authority" enumerates
/// (write submit, closeout, queue-head selection, lifecycle transition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOpKind {
    /// A `write`/`stream`/`finalize`/repair disk write submission (#pcpc3).
    WriteSubmit,
    /// Cycle closeout / commit.
    Closeout,
    /// Filesystem watcher event that should reconcile/submit through the actor.
    FileWatch,
    /// Queue-head selection / advance.
    QueueHead,
    /// Ownership / state lifecycle transition (the CAS ops above).
    Lifecycle,
}
