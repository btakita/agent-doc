//! # Module: supervisor
//!
//! Process supervisor for claude child processes. Owns claude behind a pty,
//! enforces CWD and env invariants, and exposes an IPC socket for lifecycle
//! control (restart, inject, state, stop).
//!
//! See `src/agent-doc/specs/supervisor.md` for the full design.
//!
//! ## Submodules
//! - `pty` — pty allocation, child spawn, stdin/stdout forwarding threads.

pub mod in_process;
pub mod pty;
