//! Orchestration adapters for CRDT-authority facts.
//!
//! The pure CRDT-authority policy lives in
//! [`agent_doc_document_realtime::crdt_authority`]. This module owns the
//! orchestration-specific observation points that need plugin-owner sidecar IO or
//! the supervisor backbone projection.

use agent_doc_document_realtime::crdt_authority::{CrdtAuthority, authority_from_liveness};

use crate::state_backbone::{DocumentStateProjection, EventLedger, TransportPatchPhase};
use agent_doc_plugin_owner::ownership_liveness_for_file;

/// Read the live editor-attachment facts for a document from its plugin-owner
/// lease sidecar and resolve the CRDT authority. This is the write-path entry
/// point, mirroring [`ownership_liveness_for_file`]: authority is observed fresh
/// (no persistent authority phase is stored in this rung — it derives from the
/// existing facts on demand).
pub fn authority_for_file(file: &str) -> CrdtAuthority {
    authority_from_liveness(&ownership_liveness_for_file(file))
}

/// Derive the CRDT authority for a single document from a backbone projection,
/// riding the hosting-epoch substrate.
///
/// Authority follows the live editor. A document with a live editor transport
/// (the `EditorIpcBridge` actor has an active generation, and the latest editor
/// transport patch is not a terminal force-disk fallback) is
/// [`CrdtAuthority::MultiReplica`]; otherwise the document is headless and
/// [`CrdtAuthority::GitAuthoritative`].
///
/// This is **per-document by construction**: it consults only the passed
/// document's own projection. A stale-overlay replay for a *different* document
/// (rejected/dropped by the hosting-epoch reset) never reaches this projection, so
/// it cannot flip this document's authority — `#xdocsuper1/3` is the load-bearing
/// isolation here, exactly as the model requires.
pub fn authority_for_projection(projection: &DocumentStateProjection) -> CrdtAuthority {
    if document_has_live_editor_transport(projection) {
        CrdtAuthority::MultiReplica
    } else {
        CrdtAuthority::GitAuthoritative
    }
}

/// Derive the CRDT authority for `document_hash` from the event ledger. Returns
/// [`CrdtAuthority::GitAuthoritative`] for an unknown document (no projection
/// yet) — a document the supervisor has never hosted with an editor is headless
/// until proven otherwise (fail-safe to the cheapest, zero-stale state).
pub fn authority_for_document(ledger: &EventLedger, document_hash: &str) -> CrdtAuthority {
    match ledger.project_document(document_hash) {
        Some(projection) => authority_for_projection(&projection),
        None => CrdtAuthority::GitAuthoritative,
    }
}

/// Whether a document's projection proves a live editor transport replica.
///
/// A live editor is proven when the editor-IPC-bridge transport has an active
/// generation and the latest editor transport patch has not terminally fallen
/// back to a disk write (a force-disk fallback is the supervisor abandoning the
/// editor replica for this turn — the document drops back to the git-authoritative
/// path). A document with no editor transport at all is headless.
fn document_has_live_editor_transport(projection: &DocumentStateProjection) -> bool {
    if projection.transport.editor_generation.is_none() {
        return false;
    }
    // The latest editor transport patch tells us whether the editor replica is
    // still the coordination medium or the supervisor abandoned it to disk.
    match projection.transport.patches.iter().next_back() {
        // A terminal force-disk fallback is the supervisor abandoning the editor
        // replica for this turn — the document drops back to git-authoritative.
        Some((_, patch)) => !matches!(patch.phase, TransportPatchPhase::ForceDiskFallback),
        // An editor generation with no patch yet is a freshly-attached editor.
        None => true,
    }
}
