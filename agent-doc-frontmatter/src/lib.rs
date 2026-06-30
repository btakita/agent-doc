//! Pure frontmatter and project configuration parsing.
//!
//! This crate owns document frontmatter schema, project config types, and pure
//! parsing/serialization helpers. File-backed IO stays in orchestration
//! adapters.

pub mod frontmatter;
pub mod project_config;
pub mod security_review;

pub use frontmatter::*;
