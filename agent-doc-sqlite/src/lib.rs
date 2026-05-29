//! agent-doc-sqlite — SQLite-backed persistence layer for agent-doc.
//!
//! This crate isolates the bundled-SQLite C build (`rusqlite`'s slowest build
//! dependency) from `agent-doc-orchestration` recompiles. It depends only on
//! `agent-doc-core` (pure document data layer), so editing orchestration logic
//! no longer pulls the SQLite amalgamation into that crate's build graph.
//!
//! Members:
//! - `archive_index` — derived sqlite index over compacted-turn markdown archives.

pub mod archive_index;
