//! Supervisor coordination I/O for agent-doc.

mod state_events;

pub mod config;
pub mod cwd;
pub mod detection;
pub mod env;
pub mod ipc;
pub mod recycle_request;
pub mod selfkill;
pub mod startup_miss;
