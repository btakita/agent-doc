//! Plugin-owner adapters for CRDT-authority facts.
//!
//! The pure CRDT-authority policy lives in
//! [`agent_doc_document_realtime::crdt_authority`]. This module owns the
//! plugin-owner observation point that needs lease-sidecar IO.

use agent_doc_document_realtime::crdt_authority::{CrdtAuthority, authority_from_liveness};
use agent_doc_document_realtime::editor_attach::editor_attach;

use crate::ownership_liveness_for_file;

/// Resolve the CRDT authority for a document — the editor-attached gate at the top of
/// every CRDT op.
///
/// **S4b (`#s4b-liveness-cell`): reactive on the hot path, lease on a cold miss.**
///
/// When the process-global editor-attach registry has ever recorded this document
/// ([`is_tracked`](agent_doc_document_realtime::editor_attach::EditorAttach::is_tracked)),
/// the decision is a **pure reactive read** with zero filesystem: a live attached editor
/// (some registered replica whose process is alive) is
/// [`MultiReplica`](CrdtAuthority::MultiReplica); otherwise
/// [`GitAuthoritative`](CrdtAuthority::GitAuthoritative). Crucially this reads truthfully
/// through an editor **crash** — the controller's OS process-exit watcher
/// (`process_exit_watcher`) flips the reactive `alive` cell on process death, which a
/// crashed editor's (absent) `deregister` never could.
///
/// On a **cold miss** — a document this process never recorded: a short-lived CLI that
/// installs no watcher, or the controller right after a recycle before any editor event
/// re-seeded the in-memory authority — it falls back to the durable plugin-owner **lease**
/// (the pid-liveness crash backstop), exactly the pre-S4b behavior. A watcher-less process
/// therefore always takes this lease path, so its authority stays crash-safe without a
/// reactive watcher. The reactive state is seeded by the editor replica lifecycle
/// (register/update → attach, deregister → detach) in `agent-doc-crdt-relay-io`.
pub fn authority_for_file(file: &str) -> CrdtAuthority {
    let registry = editor_attach();
    if registry.is_tracked(file) {
        if registry.is_attached(file) {
            CrdtAuthority::MultiReplica
        } else {
            CrdtAuthority::GitAuthoritative
        }
    } else {
        authority_from_liveness(&ownership_liveness_for_file(file))
    }
}
