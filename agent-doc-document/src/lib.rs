//! Pure document projection rules for agent-doc realtime state.
//!
//! This crate owns rules that can be evaluated from document text and typed
//! facts without touching disk, git, editors, tmux, snapshots, or turn state.

pub mod active_identity;
pub mod commit_normalization;
pub mod element_models;
pub mod outline_projection;
pub mod queue_projection;
pub mod singleton_repair;
pub mod status_projection;
pub mod transient_markers;
pub mod write_normalization;
