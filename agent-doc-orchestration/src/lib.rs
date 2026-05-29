//! # agent-doc-orchestration
//!
//! The orchestration layer of agent-doc: routing, git, sessions, IPC,
//! process supervision, and tmux sync. Sits between the CLI shell
//! (`agent-doc`) and the pure data layer (`agent-doc-core`), completing a
//! 3-layer architecture (CLI → orchestration → core).
//!
//! Extraction tracked under `#adoc-orchestration-crate` / `#bz6s`. See
//! `tasks/agent-doc/plan-agent-doc-orchestration-extraction.md` for the wave
//! plan. The main `agent-doc` crate re-exports these modules via `pub use`
//! shims so existing call sites resolve unchanged during the migration.
//!
//! Wave 0 (scaffold) + Wave 1a: `ipc_socket` (the one dependency-free leaf).
//! Direction A, increment 2: `env`, `fs_util`, `secret_redact` — three more
//! zero-dependency foundation leaves (no `crate::` refs).

pub mod config;
pub mod env;
pub mod fs_util;
pub mod ipc_socket;
pub mod project_config_io;
pub mod secret_redact;
