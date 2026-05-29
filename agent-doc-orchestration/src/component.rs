//! Component parser shim — re-exports `agent_doc_core::component` so moved
//! orchestration modules' `crate::component::*` call sites resolve unchanged
//! (mirrors the main crate's shim; #bz6s Direction A cluster move).

pub use agent_doc_core::component::*;
