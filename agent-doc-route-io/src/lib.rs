//! Route target session, pane resolution, and startup I/O.
//!
//! This crate owns route tmux/session-registry resolution effects plus focused
//! route startup and dispatch helpers. Document mutation authority stays behind
//! injected write effects; pure queue policy and supervisor process startup stay
//! in focused crates.

pub mod authoritative_actor;
pub mod authoritative_dispatch;
pub mod busy_pane;
pub mod closeout_drain;
pub mod cycle_ack;
pub mod diagnostics;
pub mod direct_pane_dispatch;
pub mod dispatch;
pub mod dispatch_only;
pub mod dispatch_recovery;
pub mod dispatch_start;
pub mod dispatch_target;
pub mod document_prep;
pub mod launch_contract;
pub mod pane_provenance;
pub mod pane_resolution;
pub mod queue_dispatch;
pub mod restart_handoff;
pub mod session_resolution;
pub mod startup;
pub mod startup_debounce;
pub mod startup_harness;
pub mod startup_locks;
pub mod startup_ready;
pub mod startup_sync;
pub mod supervisor_runtime;
