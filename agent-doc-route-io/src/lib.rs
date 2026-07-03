//! Route target session, pane resolution, and startup I/O.
//!
//! This crate owns route tmux/session-registry resolution effects plus focused
//! route startup helpers. It does not own command dispatch, document mutation,
//! queue policy, or supervisor process startup.

pub mod busy_pane;
pub mod direct_pane_dispatch;
pub mod dispatch_only;
pub mod dispatch_recovery;
pub mod dispatch_start;
pub mod dispatch_target;
pub mod pane_provenance;
pub mod restart_handoff;
pub mod session_resolution;
pub mod startup_debounce;
pub mod startup_harness;
pub mod startup_locks;
pub mod startup_ready;
pub mod startup_sync;
pub mod supervisor_runtime;
