//! # Module: lib (agent_doc)
//!
//! ## Spec
//! - Exposes the public API surface consumed by the CLI binary, FFI layer, and editor plugins.
//! - `agent_doc_element::element::strip_comments(content)` is the shared entry point for comment stripping,
//!   usable by both the binary (`diff::compute`) and external crates (`eval-runner`).
//!
//! ## Agentic Contracts
//! - All public symbols are safe to call from FFI consumers (JNA, napi-rs) via `ffi` module.
//!
//! ## Evals
//! - FFI functions return stable C ABI result structs and JSON envelopes.

pub mod ffi;
pub mod ffi_lossless_tree;
