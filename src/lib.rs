//! agent-doc library — shared components for CLI, FFI, and editor plugins.
//!
//! Re-exports the core document manipulation modules that are shared between
//! the CLI binary and native plugin bindings (JNI, napi-rs).

pub mod component;
pub mod crdt;
pub mod ffi;
pub mod frontmatter;
pub mod merge;
pub mod template;
