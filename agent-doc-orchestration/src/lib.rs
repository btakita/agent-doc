//! # agent-doc-orchestration
//!
//! The orchestration layer of agent-doc: routing, git, sessions, IPC,
//! process supervision, and tmux sync. Sits between the CLI shell
//! (`agent-doc`) and the focused domain crates.
//!
//! Extraction tracked under `#adoc-orchestration-crate` / `#bz6s`. See
//! `tasks/agent-doc/plan-agent-doc-orchestration-extraction.md` for the wave
//! plan.
//!
//! Wave 0 (scaffold) + Wave 1a: `ipc_socket` (the one dependency-free leaf).
//! Direction A, increment 2: `env`, `secret_redact` — two more
//! zero-dependency foundation leaves (no `crate::` refs).
//! Direction A, increment 3: global config now lives in `agent-doc-config`.
//! Project config file I/O lives in `agent-doc-project-config-io`.
//! Direction A, increment 4: `ops_log` — best-effort operational logging.
//! Pulled project-root discovery and optional file reads into `agent-doc-fs`
//! so effectful adapters no longer reach through `snapshot`.
//! Direction A, increment 5: `input_diag` — structured tmux/supervisor input
//! diagnostic emission. Pure formatting/hash/gating policy now lives in
//! `agent-doc-tmux-commands`; orchestration keeps only stderr/ops-log adapters.
//! Direction A, increment 6 (big-bang): the entire entangled cluster +
//! sessions/supervisor + neighbors moved in one migration. Orchestration now
//! depends on focused crates directly for extracted document, merge, turn, and
//! realtime policy.
//!
//! The next boundary is to retire this crate as an authority holder. Pure
//! document projection lives in `agent-doc-document`, document authority
//! scheduling should move into `agent-doc-document-realtime`, turn lifecycle
//! state lives in `agent-doc-turn`, shared turn-executor vocabulary in
//! `agent-doc-turn-executor`, shared tmux facts/effects in the
//! `agent-doc-tmux` crate family, and tmux-to-turn readiness in
//! `agent-doc-turn-executor-tmux`. This crate remains a transitional adapter
//! for harness, git, editor, and remaining command ports while those ports are
//! split into narrower crates.

// Foundation utilities (increments 1–5).
pub mod env;
pub mod input_diag;
pub mod ipc_socket;
pub mod ops_log;

// I/O wrappers around focused pure crates.
pub mod frontmatter_io;
pub mod template_io;

// Path/security helpers.
pub mod security;

// The orchestration cluster + sessions/supervisor + neighbors (increment 6).
pub mod admin;
pub mod admit;
pub mod agent;
pub mod autofix;
pub mod backlog_cmd;
pub mod capture;
pub mod checkpoint;
pub mod claim;
pub mod codex_hook;
pub mod compact;
pub mod context_clear_in_flight;
pub mod context_pct;
pub mod convergence_playback;
pub mod crdt_authority;
pub mod crdt_relay;
pub mod crdt_relay_host;
pub mod cycle_state;
pub mod dashboard;
pub mod dedupe;
pub mod diff_io;
pub mod doctor;
pub mod document_watcher;
pub mod drain_stall;
pub mod editor_route_errors;
pub mod flow;
pub mod focus;
pub mod gc;
pub mod git;
pub mod git_sibling;
pub mod graph;
pub mod harness_prompt;
pub mod hooks;
pub mod lint_gate;
pub mod memory_cmd;
pub mod owner_pane_wedge_counter;
pub mod preflight;
pub mod project_controller;
pub mod prompt;
pub mod prompt_cache;
pub mod prompt_context;
pub mod queue_cmd;
pub mod queue_continuation;
pub mod queue_journal;
pub mod realtime_model;
pub mod recycle_inflight;
pub mod recycle_yield;
pub mod repair;
pub mod resync;
pub mod route;
pub mod route_in_flight;
pub mod run;
pub mod session_accretion;
pub mod session_actor;
pub mod session_check;
pub mod sessions;
pub mod snapshot;
pub mod start;
pub mod startup_miss;
pub mod state_backbone;
pub mod state_wire;
pub mod status_cmd;
pub mod stream;
pub mod supervisor;
pub mod supervisor_selfkill;
pub mod sync;
pub mod turn_scope_store;
pub mod watch;
pub mod write;
pub mod write_queue;

// Op-capture merge adapter over the focused merge crate.
pub mod merge;

// Supply side of op-capture / evented-reflection merge (#qnodemerge4wire):
// per-document editor-op sidecar persistence consumed by merge::merge_contents_crdt_with_ops.
pub mod op_capture;

#[cfg(test)]
mod test_support;
