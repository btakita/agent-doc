//! CRDT foundation — moved to `agent_doc_core::crdt` (Wave 1 of #adcr extraction).
//!
//! This file is now a thin re-export shim. Existing `agent_doc_core::crdt::CrdtDoc`,
//! `agent_doc_core::crdt::merge`, `agent_doc_core::crdt::compact`, and `agent_doc_core::crdt::dedup_adjacent_blocks`
//! call sites in `merge.rs`, `stream.rs`, `write.rs`, `git.rs`, etc. resolve
//! unchanged.

pub use agent_doc_core::crdt::*;
