//! agent-doc-core — pure document data layer for agent-doc.
//!
//! No filesystem, git, tmux, IPC, or harness probing. Consumers pass
//! parsed content in and receive parsed/mutated content back.
//!
//! Wave plan (see `tasks/agent-doc/plan-agent-doc-core-extraction.md`):
//!
//! - Wave 1 (in progress): `id`, `crdt`, `model_tier`, `pending`,
//!   `component`. This crate currently carries the first two.
//! - Wave 2: `frontmatter`, `project_config`
//! - Wave 3: `template`
//! - Wave 4: `diff` (pure half only, per `#rtx6` Option 1)
//!
//! Pure deps only: `anyhow`, `serde`, `serde_yaml`, `uuid`,
//! `pulldown-cmark`, `yrs`, `similar`. No git/tmux/tokio/IPC.

pub mod crdt;
pub mod id;

pub use crdt::CrdtDoc;
pub use id::{BOUNDARY_ID_LEN, format_boundary_marker, new_boundary_id, new_boundary_id_with_summary};
