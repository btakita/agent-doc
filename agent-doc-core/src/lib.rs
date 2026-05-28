//! agent-doc-core — pure document data layer for agent-doc.
//!
//! No filesystem, git, tmux, IPC, or harness probing. Consumers pass
//! parsed content in and receive parsed/mutated content back.
//!
//! This crate is currently a **stub**. Module bodies will be populated
//! wave-by-wave per
//! `tasks/agent-doc/plan-agent-doc-core-extraction.md`:
//!
//! - Wave 1: `id`, `crdt`, `model_tier`, `pending`, `component`
//! - Wave 2: `frontmatter`, `project_config`
//! - Wave 3: `template`
//! - Wave 4: `diff` (after `#rtx6` seam decision — recommended Option 1:
//!   split `diff.rs` into `diff_data` (pure, here) + `diff_io`
//!   (snapshot-backed, in main crate))
//!
//! Pure deps only: `anyhow`, `serde`, `serde_yaml`, `uuid`,
//! `pulldown-cmark`, `yrs`, `similar`. No git/tmux/tokio/IPC.
