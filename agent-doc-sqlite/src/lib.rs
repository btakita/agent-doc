//! agent-doc-sqlite — SQLite-backed persistence layer for agent-doc.
//!
//! This crate isolates the bundled-SQLite C build (`rusqlite`'s slowest build
//! dependency) from `agent-doc-orchestration` recompiles, so editing
//! orchestration logic no longer pulls the SQLite amalgamation into that
//! crate's build graph.
//!
//! Members:
//! - `answer_ledger_index` — per-document operator answer-ledger cache
//!   (`#answerbyrequestid`): first-seen times, key -> decision ids, pruned
//!   answers, and decision-graph edges. Markdown stays authoritative.
//! - `archive_index` — derived sqlite index over compacted-turn markdown archives.
//! - `context_injection_ledger` — durable per-cycle prompt-context manifests and
//!   duplicate-detection records in the sole controller `state.db`.
//! - `op_log` — operation-log tables (actor + causal/Lamport tagging) in the sole
//!   controller `state.db` for the operation-scoped drift model.
//! - `steering_gate_log` — the steering typing-completion gate's decision
//!   dataset (`#steergatelog`) in the sole controller `state.db`.
//! - `state_store` — project-controller actor/lease/dispatch/queue/cycle/
//!   diagnostic/admin/recovery/layout SQLite state plus storage-specific status
//!   records. Actor domain types live in `agent-doc-controller`.
//! - `write_lock` — `BEGIN IMMEDIATE` write transactions and busy-timeout
//!   openers (`#sqlitebusyupgrade`); every transaction that may write uses it.

pub mod answer_ledger_index;
pub mod archive_index;
pub mod context_injection_ledger;
pub mod op_log;
pub mod reliable_sync_inbox;
pub mod state_store;
pub mod steering_gate_log;
pub mod write_lock;

pub use write_lock::{immediate_transaction, immediate_transaction_unchecked};

pub use state_store::{
    ActorTransitionStatus, ControlPlaneStoreCounts, DispatchAttemptStatus,
    ProjectionDiagnosticStatus, SessionOperatorStatus, SupervisorLeaseStatus,
};
