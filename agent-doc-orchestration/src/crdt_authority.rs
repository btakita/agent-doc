//! Orchestration adapters for CRDT-authority facts.
//!
//! The pure CRDT-authority policy lives in
//! [`agent_doc_document_realtime::crdt_authority`]. This module owns the
//! orchestration-specific observation points that need plugin-owner sidecar IO or
//! the supervisor-side plugin-owner liveness probe.

use agent_doc_document_realtime::crdt_authority::{CrdtAuthority, authority_from_liveness};

use agent_doc_plugin_owner::ownership_liveness_for_file;

/// Read the live editor-attachment facts for a document from its plugin-owner
/// lease sidecar and resolve the CRDT authority. This is the write-path entry
/// point, mirroring [`ownership_liveness_for_file`]: authority is observed fresh
/// (no persistent authority phase is stored in this rung — it derives from the
/// existing facts on demand).
pub fn authority_for_file(file: &str) -> CrdtAuthority {
    authority_from_liveness(&ownership_liveness_for_file(file))
}
