//! Route target session and pane resolution I/O.
//!
//! This crate owns tmux/session-registry route resolution effects. It does not
//! own command dispatch, document mutation, queue policy, or supervisor startup.

pub mod session_resolution;
