//! CRDT foundation — moved to `agent_doc_core::crdt` (Wave 1 of #adcr extraction).
//!
//! This file is now a thin re-export shim. Existing `crate::crdt::CrdtDoc`,
//! `crate::crdt::merge`, `crate::crdt::compact`, and `crate::crdt::dedup_adjacent_blocks`
//! call sites in `merge.rs`, `stream.rs`, `write.rs`, `git.rs`, etc. resolve
//! unchanged.

pub use agent_doc_core::crdt::*;
