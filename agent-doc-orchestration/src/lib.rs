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
//! Direction A, increment 3: `config`, `project_config_io`.
//! Direction A, increment 4: `ops_log` — best-effort operational logging.
//! Pulled `find_project_root` (a pure path walk) down into `fs_util` so
//! `ops_log` no longer reaches back into the main crate's `snapshot` module;
//! `snapshot::find_project_root` is now a re-export shim.
//! Direction A, increment 5: `input_diag` — structured tmux/supervisor input
//! diagnostics (production-dep on `ops_log` only). Brings a self-contained
//! `#[cfg(test)] test_support` (crate-local `TEST_ENV_LOCK`) for its
//! env-mutating tests, since orchestration's test binary is independent of the
//! main crate's.
//! Direction A, increment 6 (big-bang): the entire entangled cluster +
//! sessions/supervisor + neighbors moves in one migration. Orchestration is now
//! self-contained over `agent_doc_core`; the main crate becomes a CLI shell
//! re-exporting these modules via `pub use` shims. Core-backed lib modules
//! (`component`/`crdt`/`frontmatter`/`project_config`/`template`) are mirrored
//! here as shims so moved bodies need no `crate::` rewriting.

// Core-backed shims (mirror the main-crate shims).
pub mod component;
pub mod crdt;
pub mod frontmatter;
pub mod project_config;
pub mod template;

// Foundation utilities (increments 1–5).
pub mod config;
pub mod env;
pub mod fs_util;
pub mod input_diag;
pub mod ipc_socket;
pub mod ops_log;
pub mod project_config_io;
pub mod secret_redact;

// I/O wrappers for the core-backed shims.
pub mod frontmatter_io;
pub mod template_io;

// Path/security helpers.
pub mod security;

// The orchestration cluster + sessions/supervisor + neighbors (increment 6).
pub mod agent;
/// Re-export of the archive index, now owned by `agent-doc-sqlite`, so existing
/// `crate::archive_index::*` and `agent_doc_orchestration::archive_index::*`
/// call sites keep working after the SQLite-layer extraction.
pub use agent_doc_sqlite::archive_index;
pub mod admin;
pub mod boundary;
pub mod callback;
pub mod capture;
pub mod checkpoint;
pub mod claim;
pub mod codex_hook;
pub mod compact;
pub mod context_pct;
pub mod cycle_state;
pub mod dashboard;
pub mod debounce;
pub mod dedupe;
pub mod diff;
pub mod diff_io;
pub mod document_watcher;
pub mod editor_route_errors;
pub mod flow;
pub mod focus;
pub mod gc;
pub mod git;
pub mod git_sibling;
pub mod graph;
pub mod harness;
pub mod harness_prompt;
pub mod turn_status;
// Relocated to `agent-doc-core` (#bz6s follow-up #adoc-pure-to-core — pure,
// zero-dep). Re-exported so `crate::heuristics::*` call sites resolve unchanged.
pub use agent_doc_core::heuristics;
pub mod hooks;
// Relocated to `agent-doc-core` (#ipcfullprompt-recur2 — pure, zero-dep forensic
// detector). Re-exported so `crate::ipc_corruption::*` call sites resolve.
pub use agent_doc_core::ipc_corruption;
// Relocated to `agent-doc-core` (#optverify — pure gate-verify predicate scan).
// Re-exported so `crate::gate_verify::*` call sites resolve unchanged.
pub use agent_doc_core::gate_verify;
pub mod auto_dag;
pub mod lint_gate;
pub mod memory_cmd;
pub mod pending;
pub mod pending_cmd;
pub mod preflight;
pub mod project_controller;
pub mod prompt;
pub mod prompt_cache;
pub mod prompt_context;
pub mod prompt_contract;
pub mod queue;
pub mod queue_cmd;
pub mod queue_command;
pub mod queue_continuation;
pub mod queue_preemption;
pub mod recguard_wedge;
pub mod repair;
// Relocated to `agent-doc-core` (#adoc-pure-to-core — only uses core::template).
pub use agent_doc_core::replay_guard;
pub mod response_toc;
pub mod resync;
pub mod route;
pub mod run;
pub mod session_accretion;
pub mod session_actor;
pub mod session_check;
pub mod sessions;
pub mod snapshot;
pub mod start;
pub mod startup_miss;
pub mod status_cmd;
pub mod stream;
pub mod supervisor;
pub mod supervisor_authority;
pub mod sync;
pub mod turn_scope_store;
pub mod watch;
pub mod write;
pub mod write_authority;
pub mod write_queue;

// Core-backed shim for the CRDT merge path (merge -> crdt).
pub mod merge;

#[cfg(test)]
mod test_support;
