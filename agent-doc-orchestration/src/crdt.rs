//! CRDT shim — re-exports `agent_doc_core::crdt` so moved orchestration
//! modules' `crate::crdt::*` call sites resolve unchanged (#bz6s Direction A).

pub use agent_doc_core::crdt::*;
