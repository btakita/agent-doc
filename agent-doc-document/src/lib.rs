//! Pure document projection rules for agent-doc realtime state.
//!
//! This crate owns rules that can be evaluated from document text and typed
//! facts without touching disk, git, editors, tmux, snapshots, or turn state.

pub mod auto_dag;
pub mod element_models;
pub mod queue_projection;
