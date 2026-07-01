//! # Module: supervisor
//!
//! Process supervisor for claude child processes. Owns claude behind a pty,
//! enforces CWD and env invariants, and exposes an IPC socket for lifecycle
//! control (restart, inject, state, stop).
//!
//! See `src/agent-doc/specs/supervisor.md` for the full design.
//!
//! ## Submodules
//! - `env` — parent-env cascade + frontmatter overlay + unset, resolved once
//!   per supervisor lifetime and reused across every restart.
//! - `pty` — pty allocation, child spawn, stdin/stdout forwarding threads.
//! - `screen` — alacritty_terminal-backed screen state for owned PTY output.
//! - `ipc` — per-session Unix-domain socket for lifecycle control.

pub mod env;
pub mod in_process;
pub mod ipc;
pub mod pty;
pub mod screen;
