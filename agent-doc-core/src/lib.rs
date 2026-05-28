//! agent-doc-core — pure document data layer for agent-doc.
//!
//! No filesystem, git, tmux, IPC, or harness probing. Consumers pass
//! parsed content in and receive parsed/mutated content back.
//!
//! Wave plan (see `tasks/agent-doc/plan-agent-doc-core-extraction.md`):
//!
//! - Wave 1 (done): `id`, `crdt`, `component`, `model_tier`, `pending`.
//! - Wave 2: `frontmatter`, `project_config`
//! - Wave 3: `template`
//! - Wave 4: `diff` (pure half only, per `#rtx6` Option 1)
//!
//! Pure deps only: `anyhow`, `serde`, `serde_yaml`, `uuid`,
//! `pulldown-cmark`, `yrs`, `similar`. No git/tmux/tokio/IPC.

pub mod component;
pub mod crdt;
pub mod id;
pub mod model_tier;
pub mod pending;

pub use component::Component;
pub use crdt::CrdtDoc;
pub use id::{BOUNDARY_ID_LEN, format_boundary_marker, new_boundary_id, new_boundary_id_with_summary};
