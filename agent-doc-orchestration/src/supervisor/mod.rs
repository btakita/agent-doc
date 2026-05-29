//! # Module: supervisor
//!
//! Process supervisor for claude child processes. Owns claude behind a pty,
//! enforces CWD and env invariants, and exposes an IPC socket for lifecycle
//! control (restart, inject, state, stop).
//!
//! See `src/agent-doc/specs/supervisor.md` for the full design.
//!
//! ## Submodules
//! - `cwd` — CWD resolution priority chain (CLI flag > frontmatter > project root > doc parent).
//! - `env` — parent-env cascade + frontmatter overlay + unset, resolved once
//!   per supervisor lifetime and reused across every restart.
//! - `pty` — pty allocation, child spawn, stdin/stdout forwarding threads.
//! - `screen` — alacritty_terminal-backed screen state for owned PTY output.
//! - `resize` — SIGWINCH handling (Unix) for terminal resize propagation.
//! - `state` — crash classifier, restart history ring buffer, state machine.
//! - `ipc` — per-session Unix-domain socket for lifecycle control.

pub mod cwd;
pub mod env;
pub mod ipc;
pub mod pty;
pub mod resize;
pub mod screen;
pub mod state;
