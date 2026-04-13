//! # Module: supervisor
//!
//! Process supervisor for claude child processes. Owns claude behind a pty,
//! enforces CWD and env invariants, and exposes an IPC socket for lifecycle
//! control (restart, inject, state, stop).
//!
//! See `src/agent-doc/specs/supervisor.md` for the full design.
//!
//! ## Phase 1 status
//! - `cwd` — CWD resolution priority chain (CLI flag > frontmatter > project root > doc parent).
//! - `pty` — pty allocation, child spawn, stdin/stdout forwarding threads.
//!
//! Remaining phase 1 submodules (`resize`, `state`, `ipc`) land in
//! subsequent commits and get wired into `start.rs` last. Until `start.rs`
//! consumes these, the symbols look unused to the compiler — suppress
//! dead-code warnings at the module level rather than littering the
//! individual structs and tests with attributes that would need to be
//! removed later.
#![allow(dead_code)]

pub mod cwd;
pub mod pty;
