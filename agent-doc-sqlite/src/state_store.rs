//! Project-controller SQLite state store.
//!
//! This module owns every `rusqlite::Connection` interaction for the project
//! controller's durable actor/lease/dispatch/diagnostic/layout state, plus the
//! storage and status types those queries read and write. It deliberately
//! depends only on `rusqlite`, `serde_json`, and `std` so that
//! `agent-doc-orchestration` can call into it without pulling the bundled
//! SQLite C build into its own compile graph.
//!
//! Orchestration glue (ops-log, layout projection emission, `read_bootstrap`,
//! drift verification) stays in
//! `agent-doc-orchestration`; this module exposes the SQL primitives those
//! callers stitch together.

use anyhow::{Context, Result};
pub use rusqlite::Connection;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STATE_DB_FILE: &str = "state.db";
const STATE_DB_BUSY_TIMEOUT: Duration = Duration::from_secs(30);
const STATE_DB_SCHEMA_RETRY_INTERVAL: Duration = Duration::from_millis(10);

// ---------------------------------------------------------------------------
// Storage types (moved from agent-doc-orchestration::session_actor).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorState {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Closed,
    Blocked,
}

impl ActorState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Busy => "busy",
            Self::WaitingInput => "waiting_input",
            Self::Closed => "closed",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "starting" => Some(Self::Starting),
            "ready" => Some(Self::Ready),
            "busy" => Some(Self::Busy),
            "waiting_input" => Some(Self::WaitingInput),
            "closed" => Some(Self::Closed),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorLastTransition {
    pub caller: String,
    pub reason: String,
    pub timestamp: u64,
    pub prior_generation: u64,
    pub new_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRecord {
    pub document_id: String,
    pub session_id: String,
    pub generation: u64,
    pub pane_id: String,
    pub window_id: String,
    pub harness: String,
    pub state: ActorState,
    pub last_transition: ActorLastTransition,
}

// ---------------------------------------------------------------------------
// Status types (moved from agent-doc-orchestration::project_controller).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorTransitionStatus {
    pub prior_generation: u64,
    pub new_generation: u64,
    pub caller: String,
    pub reason: String,
    pub old_pane: Option<String>,
    pub new_pane: String,
    pub old_window: Option<String>,
    pub new_window: Option<String>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorLeaseStatus {
    pub generation: u64,
    pub supervisor_pid: Option<u32>,
    pub supervisor_socket: Option<String>,
    pub last_heartbeat: Option<u64>,
    pub runtime_state: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchAttemptStatus {
    pub receipt_id: u64,
    pub generation: u64,
    pub command_kind: String,
    pub accepted_stage: Option<String>,
    pub failed_stage: Option<String>,
    pub diagnostic_payload: Option<String>,
    pub result_status: Option<String>,
    pub proof_scope: Option<String>,
    pub dispatch_start_proven: bool,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchAttemptInsert<'a> {
    pub document_id: &'a str,
    pub generation: u64,
    pub command_kind: &'a str,
    pub accepted_stage: Option<&'a str>,
    pub failed_stage: Option<&'a str>,
    pub diagnostic_payload: &'a str,
    pub result_status: &'a str,
    pub proof_scope: &'a str,
    pub dispatch_start_proven: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionDiagnosticStatus {
    pub projection: String,
    pub message: String,
    pub source_generation: Option<u64>,
    pub intended_hash: Option<String>,
    pub retry_status: Option<String>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueHeadStatus {
    pub document_id: String,
    pub queue_name: String,
    pub generation: Option<u64>,
    pub head_id: Option<String>,
    pub prompt: String,
    pub state: String,
    pub priority: u64,
    pub selected_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueControlStatus {
    pub receipt_id: u64,
    pub scope_kind: String,
    pub scope_id: String,
    pub state: String,
    pub reason: Option<String>,
    pub operation_receipt_id: Option<u64>,
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueControlInsert<'a> {
    pub scope_kind: &'a str,
    pub scope_id: &'a str,
    pub state: &'a str,
    pub reason: Option<&'a str>,
    pub operation_receipt_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueBackpressureStatus {
    pub receipt_id: u64,
    pub document_id: String,
    pub generation: Option<u64>,
    pub command_kind: String,
    pub capacity_class: String,
    pub reason: String,
    pub dispatch_receipt_id: Option<u64>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueBackpressureInsert<'a> {
    pub document_id: &'a str,
    pub generation: Option<u64>,
    pub command_kind: &'a str,
    pub capacity_class: &'a str,
    pub reason: &'a str,
    pub dispatch_receipt_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminOperationStatus {
    pub receipt_id: u64,
    pub operation_kind: String,
    pub document_id: Option<String>,
    pub status: String,
    pub diagnostic_payload: Option<String>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionDiagnosticInsert<'a> {
    pub projection: &'a str,
    pub document_id: &'a str,
    pub message: &'a str,
    pub source_generation: Option<u64>,
    pub intended_hash: Option<&'a str>,
    pub retry_status: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOperatorStatus {
    pub record: Option<ActorRecord>,
    pub transitions: Vec<ActorTransitionStatus>,
    pub supervisor_lease: Option<SupervisorLeaseStatus>,
    pub dispatch_attempts: Vec<DispatchAttemptStatus>,
    pub projection_diagnostics: Vec<ProjectionDiagnosticStatus>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPlaneStoreCounts {
    pub actor_documents: usize,
    pub live_actor_documents: usize,
    pub actor_transitions: usize,
    pub supervisor_leases: usize,
    pub state_events: usize,
    pub dispatch_receipts: usize,
    pub queue_heads: usize,
    pub document_cycles: usize,
    pub pending_mutations: usize,
    pub projection_diagnostics: usize,
    pub admin_operations: usize,
    pub queue_controls: usize,
    pub queue_backpressure: usize,
    pub crash_recovery_markers: usize,
    pub layout_states: usize,
}

impl ControlPlaneStoreCounts {
    pub fn total_authoritative_rows(&self) -> usize {
        self.actor_documents
            + self.actor_transitions
            + self.supervisor_leases
            + self.state_events
            + self.dispatch_receipts
            + self.queue_heads
            + self.document_cycles
            + self.pending_mutations
            + self.projection_diagnostics
            + self.admin_operations
            + self.queue_controls
            + self.queue_backpressure
            + self.crash_recovery_markers
            + self.layout_states
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionActorCloseoutMutation<'a> {
    pub item_id: &'a str,
    pub mutation_kind: &'a str,
    pub status: &'a str,
}

pub fn session_actor_closeout_mutations<'a>(
    pending_done_ids: &'a [String],
    pending_gated_ids: &'a [String],
    pending_kept_open_ids: &'a [String],
    reaped_pending_ids: &'a [String],
) -> Vec<SessionActorCloseoutMutation<'a>> {
    let mutation_groups: [(&'a [String], &'static str); 4] = [
        (pending_done_ids, "done"),
        (pending_gated_ids, "gated"),
        (pending_kept_open_ids, "kept_open"),
        (reaped_pending_ids, "reaped"),
    ];
    let mut mutations = Vec::new();
    for (ids, status) in mutation_groups {
        for item_id in ids.iter().map(String::as_str).filter(|id| !id.is_empty()) {
            mutations.push(SessionActorCloseoutMutation {
                item_id,
                mutation_kind: "backlog_completion",
                status,
            });
        }
    }
    mutations
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionActorCloseoutCommit<'a> {
    pub document_id: &'a str,
    pub cycle_id: &'a str,
    pub cycle_state: &'a str,
    pub queue_name: &'a str,
    pub queue_head_id: Option<&'a str>,
    pub queue_head_prompt: Option<&'a str>,
    pub queue_head_state: &'a str,
    pub response_commit: Option<&'a str>,
    pub mutations: Vec<SessionActorCloseoutMutation<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEventStatus {
    pub sequence: u64,
    pub event_id: String,
    pub document_hash: String,
    pub domain: String,
    pub fact_type: String,
    pub payload_json: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateEventInsert<'a> {
    pub event_id: &'a str,
    pub document_hash: &'a str,
    pub domain: &'a str,
    pub fact_type: &'a str,
    pub payload_json: &'a str,
}

// ---------------------------------------------------------------------------
// Connection + schema.
// ---------------------------------------------------------------------------

pub fn state_db_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(STATE_DB_FILE)
}

pub fn open_state_db(project_root: &Path) -> Result<Connection> {
    let path = state_db_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // state.db is the authoritative state-machine ledger. Never replace it on
    // the normal execution path: doing so would silently discard captured
    // intent and make recovery projections look authoritative. Corruption must
    // fail closed until an explicit repair reconstructs a proven ledger.
    open_and_init_state_db(&path).with_context(|| {
        format!(
            "authoritative state db {} is unavailable; refusing automatic replacement",
            path.display()
        )
    })
}

/// Paths whose schema this process has already converged (`#adopenfast`).
///
/// Schema convergence is idempotent and declared entirely by this binary, so
/// repeating it on every open buys nothing while costing a large `execute_batch`
/// plus eight `PRAGMA table_info` probes. Poll loops (routed-cycle ack at 200ms,
/// `log_op`) open the same db hundreds of times per request, which made that
/// per-open cost the dominant redundant work on the route path. Converge once
/// per path per process; a concurrently-upgraded schema from another binary
/// would carry columns this process does not know about anyway.
static STATE_DB_SCHEMA_CONVERGED: std::sync::Mutex<Option<std::collections::BTreeSet<PathBuf>>> =
    std::sync::Mutex::new(None);

fn schema_already_converged(path: &Path) -> bool {
    STATE_DB_SCHEMA_CONVERGED
        .lock()
        .map(|guard| {
            guard
                .as_ref()
                .is_some_and(|converged| converged.contains(path))
        })
        .unwrap_or(false)
}

fn mark_schema_converged(path: &Path) {
    if let Ok(mut guard) = STATE_DB_SCHEMA_CONVERGED.lock() {
        guard
            .get_or_insert_with(std::collections::BTreeSet::new)
            .insert(path.to_path_buf());
    }
}

/// Forget this process's convergence memo. Tests that rebuild a database under
/// the same path need the next open to re-declare the schema.
pub fn reset_state_db_schema_convergence_memo() {
    if let Ok(mut guard) = STATE_DB_SCHEMA_CONVERGED.lock() {
        *guard = None;
    }
}

/// UNRESOLVED — `state.db` has been observed corrupting itself in the field.
///
/// 2026-07-19, `agent-loop`: a 73MB `state.db` (plus an 8.8MB WAL) failed
/// `PRAGMA integrity_check` with 101 errors, all of the form
/// `Tree N page P cell C: 2nd reference to page Q` — b-tree pages referenced
/// from two parents. Blast radius was total: `.recover` reattributed rows into
/// the wrong tables, `registry_entries` came back with NULL `document_id`
/// primary keys, and every agent-doc command on the project failed.
///
/// **The cause is not known.** The settings below are not obviously implicated:
/// `journal_mode = WAL` with a 30s `busy_timeout` is a sound multi-process
/// configuration, and no crash was correlated with the corruption. Do not
/// assume it was a one-off. If it recurs, these are the untested suspects,
/// roughly in order of how much they would explain a double-referenced page:
///
/// - **`synchronous` is never set**, so it takes SQLite's default. Under WAL, a
///   checkpoint racing power loss or an OS-level crash can tear the main db.
///   Worth pinning explicitly (`NORMAL` is the usual WAL choice; `FULL` is
///   safer and slower) rather than inheriting whatever the build defaults to.
/// - **Many concurrent writers.** Five-plus supervisors plus CLI invocations
///   share one file; the same `.agent-doc/state.db` is also opened by the
///   sibling tmux-router registry code. Cross-process WAL correctness depends
///   on every opener agreeing on locking mode and on the file living on a
///   filesystem with working POSIX advisory locks.
/// - **A killed process mid-checkpoint.** Supervisors are force-killed on some
///   recovery paths; a SIGKILL during checkpoint is a classic source of this
///   exact signature.
///
/// Recovery that worked: `sqlite3 state.db ".recover" | sqlite3 fresh.db`,
/// verify `integrity_check`, drop rows whose primary key came back NULL, then
/// let the registry rebuild from live panes (`agent-doc fix`). Preserve the
/// corrupt original — it is the only evidence for diagnosing this properly.
fn open_and_init_state_db(path: &Path) -> Result<Connection> {
    let conn =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    conn.busy_timeout(STATE_DB_BUSY_TIMEOUT)?;
    let started = Instant::now();
    loop {
        match initialize_state_db_memoizing_shape(&conn, path) {
            Ok(()) => break,
            Err(error)
                if is_state_db_lock_error(&error) && started.elapsed() < STATE_DB_BUSY_TIMEOUT =>
            {
                let remaining = STATE_DB_BUSY_TIMEOUT.saturating_sub(started.elapsed());
                std::thread::sleep(STATE_DB_SCHEMA_RETRY_INTERVAL.min(remaining));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(conn)
}

fn is_state_db_lock_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(sqlite, _))
                if matches!(
                    sqlite.code,
                    rusqlite::ffi::ErrorCode::DatabaseBusy
                        | rusqlite::ffi::ErrorCode::DatabaseLocked
                )
        )
    })
}

pub fn initialize_state_db(conn: &Connection) -> Result<()> {
    declare_canonical_shape(conn)?;
    converge_state_db_data(conn)
}

/// The shape half: table/column/index declarations only.
///
/// This is what the per-process memo may skip — it depends solely on this
/// binary's declarations, so re-running it against a database this process
/// already converged is a guaranteed no-op.
fn declare_canonical_shape(conn: &Connection) -> Result<()> {
    create_canonical_tables(conn)?;
    // One canonical declaration (`#canonicalschema`): `create_canonical_tables`
    // states the full current shape, and these converge an existing database
    // onto it from the same declarations.
    converge_added_columns(conn)?;
    ensure_canonical_indexes(conn)
}

/// The data half: convergence over *rows*, which other processes keep writing.
///
/// This must run on **every** open. Unlike the shape, these steps are not
/// idempotent with respect to time: another binary (or an older one) can append
/// a retired event variant after our first open, and skipping retirement would
/// let that row reach a parser that rejects it.
fn converge_state_db_data(conn: &Connection) -> Result<()> {
    converge_state_event_document_versions(conn)?;
    run_state_event_retention_if_due(conn);
    retire_removed_state_event_variants(conn)?;
    Ok(())
}

/// `#adopenfast`: converge the shape once per path per process, the data always.
fn initialize_state_db_memoizing_shape(conn: &Connection, path: &Path) -> Result<()> {
    if !schema_already_converged(path) {
        declare_canonical_shape(conn)?;
        mark_schema_converged(path);
    }
    converge_state_db_data(conn)
}

/// The canonical schema: every table at its **full current shape**.
///
/// This is the single declaration of what the database looks like
/// (`#canonicalschema`). It is not a historical record — when a column is added,
/// it goes here *and* into [`CANONICAL_ADDED_COLUMNS`] so existing databases
/// converge, and `canonical_schema_declares_every_added_column` fails if the two
/// disagree.
fn create_canonical_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
-- NOTE: `synchronous` is deliberately absent here only because nobody has
-- chosen a value, not because the default was validated. See the unresolved
-- corruption notes on `open_and_init_state_db` before changing these.
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 30000;

        CREATE TABLE IF NOT EXISTS documents (
            document_id TEXT PRIMARY KEY,
            canonical_path TEXT NOT NULL,
            session_id TEXT NOT NULL,
            generation INTEGER NOT NULL,
            pane_id TEXT NOT NULL,
            window_id TEXT NOT NULL,
            harness_id TEXT NOT NULL,
            actor_state TEXT NOT NULL,
            launch_mode TEXT,
            controller_epoch INTEGER,
            last_transition_id INTEGER
        );

        CREATE TABLE IF NOT EXISTS actor_transitions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id TEXT NOT NULL,
            prior_generation INTEGER NOT NULL,
            new_generation INTEGER NOT NULL,
            caller TEXT NOT NULL,
            reason TEXT NOT NULL,
            old_pane TEXT,
            new_pane TEXT NOT NULL,
            old_window TEXT,
            new_window TEXT,
            timestamp INTEGER NOT NULL
        );

CREATE TABLE IF NOT EXISTS supervisor_leases (
            document_id TEXT NOT NULL,
            generation INTEGER NOT NULL,
            supervisor_pid INTEGER,
            supervisor_socket TEXT,
            last_heartbeat INTEGER,
            runtime_state TEXT,
            PRIMARY KEY (document_id, generation)
);

CREATE TABLE IF NOT EXISTS coordination_leases (
scope_kind TEXT NOT NULL,
scope_id TEXT NOT NULL,
holder TEXT NOT NULL,
holder_pid INTEGER,
heartbeat_secs INTEGER NOT NULL,
PRIMARY KEY (scope_kind, scope_id)
);

CREATE TABLE IF NOT EXISTS project_runtime_state (
state_key TEXT PRIMARY KEY,
payload TEXT NOT NULL,
updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS document_runtime_state (
document_hash TEXT NOT NULL,
state_kind TEXT NOT NULL,
canonical_path TEXT NOT NULL,
payload_json TEXT NOT NULL,
updated_at_ms INTEGER NOT NULL,
PRIMARY KEY (document_hash, state_kind)
);

CREATE TABLE IF NOT EXISTS editor_transport_health (
document_hash TEXT PRIMARY KEY,
session_id TEXT NOT NULL,
consecutive_timeouts INTEGER NOT NULL,
degraded INTEGER NOT NULL,
recycle_attempted INTEGER NOT NULL,
last_delivery_id TEXT,
last_transport TEXT NOT NULL,
updated_at_secs INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS editor_op_captures (
    document_hash TEXT PRIMARY KEY,
    canonical_path TEXT NOT NULL,
    base_hash TEXT NOT NULL,
    ops_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

        CREATE TABLE IF NOT EXISTS state_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            document_hash TEXT NOT NULL,
            domain TEXT NOT NULL,
            fact_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            document_version INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS state_schema_migrations (
            migration_id TEXT PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );

CREATE INDEX IF NOT EXISTS state_events_document_hash_id
ON state_events(document_hash, id);

-- Recovery reads recent content-bearing facts by type. Keep those reads
-- proportional to the requested checkpoint window instead of replaying the
-- document's complete event history.
CREATE INDEX IF NOT EXISTS state_events_document_hash_fact_type_id
ON state_events(document_hash, fact_type, id);

-- Closeout/cycle projections never consume document-authority observations.
-- Keep their hot replay index bounded to the durable facts they do consume;
-- authority observations are intentionally excluded from this partial index.
CREATE INDEX IF NOT EXISTS state_events_cycle_projection_document_hash_id
ON state_events(document_hash, id)
WHERE fact_type <> 'document_authority_observed';

        CREATE TABLE IF NOT EXISTS dispatch_attempts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id TEXT NOT NULL,
            generation INTEGER NOT NULL,
            command_kind TEXT NOT NULL,
            accepted_stage TEXT,
            failed_stage TEXT,
            diagnostic_payload TEXT,
            result_status TEXT,
            proof_scope TEXT,
            dispatch_start_proven INTEGER NOT NULL DEFAULT 0,
            timestamp INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS projection_diagnostics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            projection TEXT NOT NULL,
            document_id TEXT NOT NULL,
            message TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            source_generation INTEGER,
            intended_hash TEXT,
            retry_status TEXT
        );

        CREATE TABLE IF NOT EXISTS queue_heads (
            document_id TEXT NOT NULL,
            queue_name TEXT NOT NULL,
            generation INTEGER,
            head_id TEXT,
            prompt TEXT NOT NULL,
            state TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            selected_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (document_id, queue_name)
        );

        -- `#fr79` head provenance: which queue heads the BINARY created by
        -- mirroring a backlog item, as opposed to heads the operator typed
        -- directly.
        --
        -- Needed because "this head's id has no tracked item" cannot by itself
        -- mean "orphaned": a queue head is not required to have a backlog item
        -- at all — operators author `do [#id]` heads directly, and a document
        -- need not carry a backlog component. Striking on absence alone deleted
        -- legitimate queued work and failed 12 preflight tests. Only a head the
        -- mirror created and whose backlog id later vanished is real drift.
        --
        -- Absence of a row means "unknown provenance", which callers must treat
        -- as operator-authored (never strike). That makes the rollout safe by
        -- construction: documents predating this table record nothing, so no
        -- existing head can be struck until the mirror observes it again.
        CREATE TABLE IF NOT EXISTS queue_head_provenance (
            document_id TEXT NOT NULL,
            head_identity TEXT NOT NULL,
            source TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            PRIMARY KEY (document_id, head_identity)
        );

        CREATE TABLE IF NOT EXISTS queue_controls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_kind TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            state TEXT NOT NULL,
            reason TEXT,
            operation_receipt_id INTEGER,
            updated_at INTEGER NOT NULL,
            UNIQUE(scope_kind, scope_id)
        );

CREATE TABLE IF NOT EXISTS queue_backpressure (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id TEXT NOT NULL,
            generation INTEGER,
            command_kind TEXT NOT NULL,
            capacity_class TEXT NOT NULL,
            reason TEXT NOT NULL,
            dispatch_receipt_id INTEGER,
            timestamp INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS queue_document_state (
    document_hash TEXT NOT NULL,
    state_kind TEXT NOT NULL,
    canonical_path TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_secs INTEGER NOT NULL,
    PRIMARY KEY (document_hash, state_kind)
);

CREATE INDEX IF NOT EXISTS queue_document_state_kind
ON queue_document_state(state_kind);

        CREATE TABLE IF NOT EXISTS document_cycles (
            document_id TEXT NOT NULL,
            cycle_id TEXT NOT NULL,
            state TEXT NOT NULL,
            queue_head_id TEXT,
            response_commit TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (document_id, cycle_id)
        );

        CREATE TABLE IF NOT EXISTS pending_mutations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id TEXT NOT NULL,
            cycle_id TEXT NOT NULL,
            item_id TEXT NOT NULL,
            mutation_kind TEXT NOT NULL,
            status TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            UNIQUE(document_id, cycle_id, item_id, mutation_kind)
        );

        CREATE TABLE IF NOT EXISTS admin_operations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            operation_kind TEXT NOT NULL,
            document_id TEXT,
            status TEXT NOT NULL,
            diagnostic_payload TEXT,
            timestamp INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS crash_recovery_markers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            marker_kind TEXT NOT NULL,
            dedupe_key TEXT,
            document_id TEXT,
            generation INTEGER,
            status TEXT NOT NULL,
            diagnostic_payload TEXT,
            timestamp INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS layout_states (
            scope TEXT PRIMARY KEY,
            columns_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS controller_bootstrap (
            scope TEXT PRIMARY KEY,
            bootstrap_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

/// How often a single process re-runs `state_events` retention
/// (`#retentionperiodic`).
///
/// Retention used to be latched once per process. That is correct for a
/// short-lived CLI invocation but wrong for the two processes that write the
/// most: `controller serve` and the supervisor run for days, so they pruned once
/// at startup and then accumulated unbounded until an operator noticed the
/// database had grown again and ran `agent-doc gc` by hand. Re-running on an
/// interval makes retention automatic for long-lived processes while keeping the
/// per-RPC `open_state_db` path free of repeated `state_events` scans.
///
/// This is strictly a superset of the old behavior: the first open in any
/// process still prunes immediately.
const STATE_EVENT_RETENTION_INTERVAL: Duration = Duration::from_secs(900);

/// Monotonic millis of the last retention pass, **keyed by database path**.
///
/// A single process opens more than one project's `state.db`: the cross-project
/// controller sweeps (`reap_orphaned_preparing_controllers_all_projects`) walk
/// every project root in one run. A process-global stamp would let the first
/// project consume the interval window and leave every other project unpruned —
/// the same "one process, one prune" gap this whole change exists to close, just
/// sliced by project instead of by time. A missing entry means "never ran for
/// this database" and is always due.
static STATE_EVENT_RETENTION_LAST_RUN_MS: std::sync::Mutex<Option<BTreeMap<String, u64>>> =
    std::sync::Mutex::new(None);

/// Process start, used as the monotonic origin for the retention interval.
/// `Instant` is not `const`-constructible, so the origin is created lazily.
static STATE_EVENT_RETENTION_ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn state_event_retention_elapsed_ms() -> u64 {
    STATE_EVENT_RETENTION_ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

/// Pure interval policy for [`run_state_event_retention_if_due`]
/// (`#retentionperiodic`).
///
/// `last_run_ms == u64::MAX` is the "never ran in this process" sentinel and is
/// always due. A non-monotonic `now_ms` (clock shim in tests) is treated as due
/// rather than deferring forever.
pub fn state_event_retention_due(last_run_ms: u64, now_ms: u64, interval: Duration) -> bool {
    if last_run_ms == u64::MAX || now_ms < last_run_ms {
        return true;
    }
    now_ms - last_run_ms >= interval.as_millis().min(u64::MAX as u128) as u64
}

/// Run every `state_events` retention pass, best-effort, at most once per
/// [`STATE_EVENT_RETENTION_INTERVAL`] per process.
///
/// Retention is maintenance, never a reason to fail state-db open: each pass
/// logs and continues, and the run stamp only advances on a fully clean pass so
/// a transient failure retries at the next open instead of waiting out the
/// whole interval.
fn run_state_event_retention_if_due(conn: &Connection) {
    let now_ms = state_event_retention_elapsed_ms();
    // Key by database path so each project gets its own interval. `None` (an
    // in-memory or path-less connection) shares one key, which is correct: they
    // are all the same anonymous database from retention's point of view.
    let key = conn.path().unwrap_or_default().to_string();
    // Claim the window before doing the work so concurrent opens in the same
    // process do not all scan `state_events` at once.
    let last_run_ms = {
        let Ok(mut guard) = STATE_EVENT_RETENTION_LAST_RUN_MS.lock() else {
            // A poisoned lock is a maintenance-scheduling concern only; skipping
            // this pass is always safe, and the next open retries.
            return;
        };
        let stamps = guard.get_or_insert_with(BTreeMap::new);
        let last_run_ms = stamps.get(&key).copied().unwrap_or(u64::MAX);
        if !state_event_retention_due(last_run_ms, now_ms, STATE_EVENT_RETENTION_INTERVAL) {
            return;
        }
        stamps.insert(key.clone(), now_ms);
        last_run_ms
    };
    let mut clean = true;
    for (label, outcome) in [
        (
            "document_authority_observed",
            prune_document_authority_observations_to(conn, DOCUMENT_AUTHORITY_OBSERVED_MAX_ROWS),
        ),
        (
            "crdt_recovery_projection_checkpointed",
            prune_superseding_fact_to(
                conn,
                "crdt_recovery_projection_checkpointed",
                CRDT_RECOVERY_CHECKPOINTS_KEPT_PER_DOCUMENT,
            ),
        ),
        (
            "document_baseline_checkpointed",
            prune_superseding_fact_to(
                conn,
                "document_baseline_checkpointed",
                DOCUMENT_BASELINES_KEPT_PER_DOCUMENT,
            ),
        ),
        (
            "response_captured",
            prune_superseding_fact_to(
                conn,
                "response_captured",
                RESPONSE_CAPTURES_KEPT_PER_DOCUMENT,
            ),
        ),
        (
            "turn_intent_checkpointed",
            prune_superseding_fact_to(
                conn,
                "turn_intent_checkpointed",
                TURN_INTENT_CHECKPOINTS_KEPT_PER_DOCUMENT,
            ),
        ),
        (
            "visible_write_commit_candidate_observed",
            prune_superseded_visible_write_commit_candidates(conn),
        ),
        (
            "document_write_deferred",
            prune_converged_document_write_intents(conn),
        ),
        // Not a `state_events` fact, but the same retention concern and the same
        // interval: an append-mostly audit trail on a long-lived controller.
        ("crash_recovery_markers", prune_crash_recovery_markers(conn)),
    ] {
        if let Err(err) = outcome {
            clean = false;
            eprintln!("[agent-doc] warning: failed to prune {label} events: {err:#}");
        }
    }
    if !clean && let Ok(mut guard) = STATE_EVENT_RETENTION_LAST_RUN_MS.lock() {
        // Restore the previous stamp so a transient failure retries at the next
        // open instead of waiting out the whole interval.
        let stamps = guard.get_or_insert_with(BTreeMap::new);
        if last_run_ms == u64::MAX {
            stamps.remove(&key);
        } else {
            stamps.insert(key, last_run_ms);
        }
    }
}

/// Drop rows of a **superseding** `state_events` fact beyond the newest
/// `keep_per_document` for each document.
///
/// Shared by every fact whose `DocumentStateProjection::apply_fact` arm assigns
/// its projection field wholesale (`= Some(..)`), which makes replaying only the
/// newest rows byte-identical to replaying all of them. Callers must verify that
/// property per fact type — an accumulating fact needs projection-mirroring
/// retention instead (see [`prune_converged_document_write_intents`]).
///
/// `fact_type` is a static caller-supplied literal, never user input.
fn prune_superseding_fact_to(
    conn: &Connection,
    fact_type: &'static str,
    keep_per_document: i64,
) -> Result<()> {
    if keep_per_document < 1 {
        return Ok(());
    }
    // A row is superseded when at least `keep_per_document` NEWER rows of the
    // same fact exist for the same document. The correlated count is driven by
    // `state_events_document_hash_fact_type_id`, which already orders by id
    // within (document_hash, fact_type), so this needs no additional index.
    //
    // Bounded batches so a one-time cleanup of a legacy backlog cannot balloon a
    // single WAL frame or hold the write lock for the whole scan.
    loop {
        let deleted = conn
            .execute(
                r#"
                DELETE FROM state_events
                WHERE rowid IN (
                    SELECT e.rowid FROM state_events e
                    WHERE e.fact_type = ?1
                      AND (
                        SELECT COUNT(*) FROM state_events n
                        WHERE n.fact_type = ?1
                          AND n.document_hash = e.document_hash
                          AND n.id > e.id
                      ) >= ?2
                    LIMIT 2000
                )
                "#,
                params![fact_type, keep_per_document],
            )
            .with_context(|| format!("failed to prune superseded {fact_type} events"))?;
        if deleted == 0 {
            break;
        }
    }
    Ok(())
}

/// Retention cap for `document_authority_observed` rows in `state_events`
/// (`#authorityfactretention`).
///
/// This fact is pure high-frequency telemetry: it only ever updates
/// `DocumentProjection::latest_authority`, which is why the cycle-projection
/// reader and the hot replay index both already exclude it (see
/// `load_state_events_for_cycle_projection_from_db` and
/// `state_events_cycle_projection_document_hash_id`). Those exclusions bound the
/// hot READ paths but never bounded the TABLE, so the ledger still grows without
/// limit and inflates every whole-database operation — WAL growth, page-cache
/// pressure, backup/copy, `VACUUM`, and any scan not covered by a partial index.
///
/// Live repro 2026-07-18 (agent-loop): 170,215 of 176,268 `state_events` rows
/// (96.6%) were `document_authority_observed`, giving a 416MB table inside a
/// 477MB `state.db`, alongside multi-second closeout phases
/// (`git_commit:18031ms`, `session_check:12231ms`). Same failure class as
/// [`CRASH_RECOVERY_MARKER_MAX_ROWS`] (`#crashmarkerretention`).
///
/// Only the newest observation per document is ever read, so this cap is
/// generous by orders of magnitude and exists purely to stop unbounded growth.
const DOCUMENT_AUTHORITY_OBSERVED_MAX_ROWS: i64 = 5_000;

fn prune_document_authority_observations_to(conn: &Connection, max_rows: i64) -> Result<()> {
    if max_rows < 1 {
        return Ok(());
    }
    // Cutoff is the id of the `max_rows`-th newest observation; strictly-older
    // rows are dropped. `id` is the primary key, so both the cutoff lookup and
    // the batched delete are index-driven without adding another index.
    let cutoff: Option<i64> = conn
        .query_row(
            r#"
            SELECT id FROM state_events
            WHERE fact_type = 'document_authority_observed'
            ORDER BY id DESC LIMIT 1 OFFSET ?1
            "#,
            [max_rows - 1],
            |row| row.get(0),
        )
        .optional()
        .context("failed to resolve document_authority_observed retention cutoff")?;
    let Some(cutoff) = cutoff else {
        return Ok(());
    };
    // Bounded batches so a one-time cleanup of a legacy backlog cannot balloon a
    // single WAL frame or hold the write lock for the whole scan.
    loop {
        let deleted = conn
            .execute(
                r#"
                DELETE FROM state_events
                WHERE rowid IN (
                    SELECT rowid FROM state_events
                    WHERE fact_type = 'document_authority_observed'
                      AND id < ?1
                    LIMIT 50000
                )
                "#,
                [cutoff],
            )
            .context("failed to prune stale document_authority_observed events")?;
        if deleted == 0 {
            break;
        }
    }
    Ok(())
}

/// How many `crdt_recovery_projection_checkpointed` events to keep per document
/// (`#crdtcheckpointretention`).
///
/// This fact is a **superseding snapshot**, not an accumulating one:
/// `DocumentStateProjection::apply_fact` assigns `crdt_recovery_projection =
/// Some(..)` wholesale, so replaying only the newest checkpoint for a document
/// yields a byte-identical projection to replaying all of them. Every older
/// checkpoint is dead weight the ledger replays and pays for forever.
///
/// It is also the single largest consumer of the ledger. Live measurement
/// 2026-07-18 (agent-loop): 564 rows holding **405MB of the 512MB** total
/// `payload_json` — 79% of the database — at ~736KB per row, because each row
/// embeds a full base64 CRDT projection. One document alone held 4,494 events /
/// 251MB. That bloat is what made per-document ledger reads expensive even after
/// the read was correctly scoped by `document_hash` (`#ledgerdocscope`).
///
/// Same failure class and same remedy shape as
/// [`DOCUMENT_AUTHORITY_OBSERVED_MAX_ROWS`], but the cap must be **per document**
/// rather than global: a global row cap would evict a quiet document's only
/// recovery checkpoint as soon as a busy document produced enough newer ones.
///
/// Keeping more than one is deliberate headroom — the newest is what recovery
/// elects, and a spare bounds the blast radius if the newest is unreadable.
const CRDT_RECOVERY_CHECKPOINTS_KEPT_PER_DOCUMENT: i64 = 2;

/// How many `document_baseline_checkpointed` events to keep per document
/// (`#baselinefactretention`).
///
/// Like [`CRDT_RECOVERY_CHECKPOINTS_KEPT_PER_DOCUMENT`] this fact is a
/// **superseding snapshot**: `DocumentStateProjection::apply_fact` assigns
/// `merge_baseline = Some(..)` wholesale, so replaying only the newest
/// checkpoint per document is byte-identical to replaying all of them. Every
/// older row embeds a full document image the ledger then carries forever.
///
/// Live measurement 2026-07-18 (agent-loop): 816 rows holding **30MB** inside a
/// 200MB `state.db`, second only to `document_write_deferred` (see
/// [`prune_converged_document_write_intents`]). Same failure class and same
/// per-document cap shape as the CRDT checkpoints — a global row cap would evict
/// a quiet document's only baseline as soon as a busy document produced enough
/// newer ones.
const DOCUMENT_BASELINES_KEPT_PER_DOCUMENT: i64 = 2;

/// How many `response_captured` events to keep per document
/// (`#responsecaptureretention`).
///
/// Another **superseding snapshot**: the `StateFact::ResponseCaptured` arm of
/// `apply_fact` assigns `capture_id`, `response_sha256`, and
/// `captured_response` wholesale, so replaying only the newest captures per
/// document reproduces the same closeout projection. Nothing reads the rows as a
/// list — the ledger is folded, never enumerated by this fact type.
///
/// Each row embeds the response body, the full replayable intent body, AND the
/// editor-visible baseline content, so it is the heaviest remaining fact once
/// `#deferredintentretention` and `#baselinefactretention` are in force. Live
/// measurement 2026-07-18 (agent-loop), immediately after those two landed: 658
/// rows holding **14MB** — the largest single consumer of the shrunken ledger.
///
/// Three rather than two: the open cycle's capture plus two prior cycles of
/// headroom, because recovery may reconcile a partially materialized response
/// against the capture that preceded it.
const RESPONSE_CAPTURES_KEPT_PER_DOCUMENT: i64 = 3;

/// How many `turn_intent_checkpointed` events to keep per document
/// (`#turnintentretention`).
///
/// Superseding in the same way: the `apply_fact` arm assigns
/// `closeout.turn_intent_checkpoint = Some(..)` wholesale, and checkpoints are a
/// mid-turn progress projection — only the newest is ever elected. Sequence
/// numbers within a turn make this the highest-**row-count** fact after the
/// authority observations (1,441 rows / 3MB live on 2026-07-18).
const TURN_INTENT_CHECKPOINTS_KEPT_PER_DOCUMENT: i64 = 2;

/// Drop `visible_write_commit_candidate_observed` rows the projection can no
/// longer elect (`#visiblecandidateretention`).
///
/// This fact is superseding, but **not per document** — a keep-newest-N-per-
/// document cap would be wrong. `VisibleWriteProjection::observe_commit_candidate`
/// maintains a MAP keyed by `commit_candidate_hash`, so several distinct
/// candidates stay simultaneously live for one document and evicting by
/// recency would drop a candidate the write state machine can still elect.
///
/// Retention mirrors the map instead. Within one `(document_hash,
/// commit_candidate_hash)` key the projection keeps whichever row survives
/// `if current_revision > model_revision { return }` — that is, the highest
/// `model_revision`, and on a tie the last one replayed (highest `id`). Every
/// other row for that key is provably dead: replaying it cannot change the final
/// map, and nothing enumerates this fact as a list (the only readers are the
/// single `apply_fact` arm and writers).
///
/// `latest_model_revision` is a running max across all candidates, and the
/// document's global maximum is by construction the max of the per-key maxima we
/// keep, so it is preserved exactly too.
///
/// Live measurement 2026-07-18 (agent-loop): 164 rows holding **4MB**, the
/// largest remaining unbounded fact once `#responsecaptureretention` and
/// `#turnintentretention` were in force — each row embeds a full
/// `commit_candidate_content` image.
fn prune_superseded_visible_write_commit_candidates(conn: &Connection) -> Result<()> {
    // Bounded batches so a one-time cleanup of a legacy backlog cannot balloon a
    // single WAL frame or hold the write lock for the whole scan.
    loop {
        let deleted = conn
            .execute(
                r#"
                DELETE FROM state_events
                WHERE rowid IN (
                    SELECT e.rowid FROM state_events e
                    WHERE e.fact_type = 'visible_write_commit_candidate_observed'
                      AND EXISTS (
                        SELECT 1 FROM state_events n
                        WHERE n.fact_type = 'visible_write_commit_candidate_observed'
                          AND n.document_hash = e.document_hash
                          AND json_extract(n.payload_json, '$.fact.commit_candidate_hash')
                              IS json_extract(e.payload_json, '$.fact.commit_candidate_hash')
                          AND (
                            json_extract(n.payload_json, '$.fact.model_revision')
                              > json_extract(e.payload_json, '$.fact.model_revision')
                            OR (
                              json_extract(n.payload_json, '$.fact.model_revision')
                                IS json_extract(e.payload_json, '$.fact.model_revision')
                              AND n.id > e.id
                            )
                          )
                      )
                    LIMIT 2000
                )
                "#,
                [],
            )
            .context("failed to prune superseded visible write commit candidates")?;
        if deleted == 0 {
            break;
        }
    }
    Ok(())
}

/// The one `document_write_deferred` reason that lands in an INDEPENDENT durable
/// lineage (`DocumentStateProjection::pending_external_disk`) instead of the
/// agent-owned `pending_write_journal`. Retention must never mix the two.
const EXTERNAL_DISK_DEFERRAL_REASON: &str = "pending_user_decision_external_disk_vs_editor";

/// Drop `document_write_deferred` events the projection has already drained
/// (`#deferredintentretention`).
///
/// This is the single largest consumer of the ledger. Live measurement
/// 2026-07-18 (agent-loop): 1,091 rows holding **96MB of the 166MB** total
/// `payload_json` at ~90KB per row, because each row embeds both the expected
/// and the target document image.
///
/// Unlike the baseline and CRDT checkpoints this fact is **not** purely
/// superseding, so a "keep N newest per document" cap would be WRONG: the
/// agent-owned lineage ACCUMULATES into `pending_write_journal`, and an intent
/// stays live until its ACK arrives. Retention therefore mirrors the projection
/// exactly instead of approximating it:
///
/// - `StateFact::DocumentWriteConverged` resolves the journal entry matching
///   `(intent_id, target_hash)` and calls `drain(..=index)` — settling that
///   intent AND every older agent intent, because each newer deferred target is
///   composed from the older retained ones. So every `document_write_deferred`
///   row at or below the newest converged intent's row is provably dead.
/// - Rows above it are unconverged retained intents that recovery still replays;
///   they are never touched, however many accumulate.
/// - [`EXTERNAL_DISK_DEFERRAL_REASON`] rows are an independent lineage
///   (`pending_external_disk`, which must "never replace or clear" the other)
///   and are excluded from the drain window on both sides.
fn prune_converged_document_write_intents(conn: &Connection) -> Result<()> {
    // Bounded batches so a one-time cleanup of a legacy backlog cannot balloon a
    // single WAL frame or hold the write lock for the whole scan.
    loop {
        let deleted = conn
            .execute(
                r#"
                DELETE FROM state_events
                WHERE rowid IN (
                    SELECT d.rowid FROM state_events d
                    WHERE d.fact_type = 'document_write_deferred'
                      AND json_extract(d.payload_json, '$.fact.reason') IS NOT ?1
                      AND d.id <= (
                        SELECT MAX(settled.id) FROM state_events settled
                        WHERE settled.fact_type = 'document_write_deferred'
                          AND settled.document_hash = d.document_hash
                          AND json_extract(settled.payload_json, '$.fact.reason') IS NOT ?1
                          AND EXISTS (
                            SELECT 1 FROM state_events c
                            WHERE c.fact_type = 'document_write_converged'
                              AND c.document_hash = settled.document_hash
                              AND c.id > settled.id
                              AND json_extract(c.payload_json, '$.fact.intent_id')
                                  IS json_extract(settled.payload_json, '$.fact.intent_id')
                              AND json_extract(c.payload_json, '$.fact.target_hash')
                                  IS json_extract(settled.payload_json, '$.fact.target_hash')
                          )
                      )
                    LIMIT 2000
                )
                "#,
                [EXTERNAL_DISK_DEFERRAL_REASON],
            )
            .context("failed to prune converged document write intents")?;
        if deleted == 0 {
            break;
        }
    }
    Ok(())
}

/// Fraction of the state-db file that must be reclaimable before
/// [`reclaim_state_db_free_space`] rewrites it (`#statedbvacuum`).
///
/// `VACUUM` rewrites the entire database and takes an exclusive lock, so it is a
/// maintenance operation only — never the hot path. The threshold keeps it from
/// running on a database that is merely a little fragmented.
const STATE_DB_VACUUM_FREE_FRACTION: f64 = 0.25;

/// Reclaim free pages in the project state db, returning the bytes released.
///
/// `#statedbvacuum`: retention pruning (`#authorityfactretention`,
/// `#crdtcheckpointretention`) frees SQLite *pages*, but SQLite never returns
/// them to the filesystem without a `VACUUM`. Live measurement 2026-07-18 after
/// checkpoint GC: payload dropped 512MB -> 119MB while the file stayed at 521MB,
/// so ~400MB remained allocated. That inflates every whole-file operation —
/// backup, copy, page-cache pressure — even though query cost is already fixed.
///
/// Deliberately NOT called from `open_state_db`: a full-file rewrite under an
/// exclusive lock must be an explicit maintenance step (`agent-doc gc`), not
/// something a routine RPC can trigger. Returns `Ok(0)` when the database is not
/// fragmented enough to be worth rewriting.
pub fn reclaim_state_db_free_space(project_root: &Path) -> Result<u64> {
    let path = state_db_path(project_root);
    if !path.exists() {
        return Ok(0);
    }
    let conn = open_state_db(project_root)?;

    let page_count: i64 = conn
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .context("failed to read state db page_count")?;
    let free_count: i64 = conn
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .context("failed to read state db freelist_count")?;

    if page_count <= 0 || free_count <= 0 {
        return Ok(0);
    }
    let free_fraction = free_count as f64 / page_count as f64;
    if free_fraction < STATE_DB_VACUUM_FREE_FRACTION {
        return Ok(0);
    }

    let before = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    conn.execute_batch("VACUUM")
        .context("failed to vacuum state db")?;
    let after = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(before);
    // Saturating: a concurrent write can grow the file between the two stats,
    // and reporting a negative reclaim would be worse than reporting none.
    Ok(before.saturating_sub(after))
}

/// Once-only **data** migrations, and the release each shipped in
/// (`#migrationfloor`).
///
/// Schema shape is declarative and converges on every open — see
/// [`CANONICAL_ADDED_COLUMNS`]. Nothing in that path accumulates. This ledger is
/// the deliberately small exception: work a schema declaration *cannot* express,
/// because it deletes or computes ROWS rather than describing a table. A schema
/// differ, declarative or otherwise, could not generate either entry below.
///
/// **These are not permanent, and should not be treated as such.** Each has a
/// support floor: once no supported release predates the version listed here, an
/// upgrading database is guaranteed to have already applied it, so the record
/// AND its code should be deleted. Pruning an applied migration is the intended
/// end of its life, not a special event — that is what keeps this list from
/// growing for the life of the project. Leaving a migration in place "just in
/// case" is how a two-entry ledger becomes a two-hundred-entry one.
///
/// | migration | shipped in | removable once the supported floor is |
/// |---|---|---|
/// | `retire_pending_response_fact_variants_v1` | 0.34.175 | > 0.34.175 |
///
/// Still in force: the project supports upgrading from releases older than
/// 0.34.175, so that floor has not been crossed yet.
///
/// **`backfill_state_event_document_version_v1` was already retired**, and the
/// reason generalizes. It ran once, recorded itself, and was therefore unable to
/// repair rows that arrived *afterwards* — a concurrently running older binary
/// kept appending events with the column at its `DEFAULT 0` (55 such rows,
/// observed live at ids 205755–205913, newer than the migration record itself).
/// A watermark would have read every one of them as version 0 and deleted them.
/// [`converge_state_event_document_versions`] replaced it with an idempotent
/// convergence pass that repairs stragglers on any open, so there is nothing to
/// record and nothing to accumulate. Prefer convergence to a one-shot migration
/// whenever the invariant can be re-established from the data itself.
const RETIRE_PENDING_RESPONSE_FACTS_MIGRATION: &str = "retire_pending_response_fact_variants_v1";

/// Ledger id of the retired one-shot document-version backfill. Retained only so
/// [`converge_state_event_document_versions`] can delete the stale record from
/// databases that ran it.
const RETIRED_BACKFILL_DOCUMENT_VERSION_MIGRATION: &str =
    "backfill_state_event_document_version_v1";

/// Give `state_events` a first-class monotonic per-document version
/// (`#retentionversion`).
///
/// Retention needs to answer "is this row below the point everyone has moved
/// past?". Today it reconstructs that ordering from three incompatible
/// encodings, none of which is a real version column:
///
/// - `id`, a per-database `AUTOINCREMENT`. It orders correctly *within one
///   database* but is meaningless across peers, so no editor replica and
///   controller can ever agree on "seen through N" using it.
/// - a version smeared into `event_id` strings, in at least three ad-hoc
///   formats: `turn-intent-checkpoint:<doc>:<cycle>:561:<hash>` (a sequence),
///   `document-authority-<doc>-1784424488871912-<source>` (microseconds), and
///   `crdt-recovery:<doc>:17:<hash>` (a generation).
/// - fields inside `payload_json` (`model_revision`, `intent_id` +
///   `target_hash`), reached via `json_extract` in the retention SQL.
///
/// This column is the substrate the per-peer ack watermark needs: a value type
/// a peer can report back as "I have seen this document through version N".
/// Assigning it is phase 1 and deliberately lands on its own — flipping the
/// fact-specific retention rules over to a single generic delete-below-watermark
/// is the follow-up, and it depends on an ack table that does not exist yet.
///
/// Existing rows are backfilled in `id` order per document, which is exactly the
/// order the projection already replays them in, so the backfill cannot
/// reorder history.
///
/// Two invariants, and the difference between them matters to anything built on
/// this column:
///
/// **Guaranteed — a row's version is assigned once and never decreases.**
/// Convergence only ever touches rows still at the `DEFAULT 0` and numbers them
/// *above* their document's current high-water mark. No row can be moved below a
/// version a peer has already acked, so a watermark can never retroactively
/// swallow a row it has not seen. This is the property the whole column exists
/// for.
///
/// **NOT guaranteed — version order does not always match `id` order.** Two
/// things break the correspondence, both deliberately:
///
/// - The sequence is **not gapless**. Retention deletes superseded rows, leaving
///   holes (live ledger: one document at 5,397 rows with a high-water mark of
///   5,408). A watermark only asks "at or below version N", so holes are
///   irrelevant. Do not close them by renumbering — that would violate the
///   guaranteed invariant above.
/// - Repaired stragglers sort **after** rows with higher `id`s. A straggler is
///   numbered above the high-water mark rather than at its `id` position,
///   because the alternative is assigning it a version below one already
///   handed out. Measured on the live ledger: 2 such boundary inversions after
///   repairing 55 stragglers.
///
/// Replay ordering is `id`, not `document_version`, so nothing depends on the
/// correspondence. Do not introduce a dependency on it.
fn converge_state_event_document_versions(conn: &Connection) -> Result<()> {
    // Cheap guard: an index seek on (document_hash, document_version). The
    // repair below is a full per-document ranking, so it must not run on the
    // per-RPC open path unless there is actually something to repair.
    let needs_repair: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM state_events WHERE document_version = 0)",
            [],
            |row| row.get(0),
        )
        .context("probe for unversioned state events")?;
    if !needs_repair {
        return Ok(());
    }
    let tx = conn
        .unchecked_transaction()
        .context("begin document-version convergence")?;
    // `base` is computed before the UPDATE applies, so the high-water mark per
    // document cannot shift underneath the ranking. Unversioned rows are then
    // numbered in `id` order — the projection's replay order — starting above
    // whatever that document has already issued.
    tx.execute(
        r#"
        WITH base AS (
            SELECT document_hash, MAX(document_version) AS high_water
            FROM state_events GROUP BY document_hash
        ),
        ranked AS (
            SELECT e.id AS id,
                   base.high_water + ROW_NUMBER() OVER (
                       PARTITION BY e.document_hash ORDER BY e.id
                   ) AS assigned
            FROM state_events e
            JOIN base ON base.document_hash = e.document_hash
            WHERE e.document_version = 0
        )
        UPDATE state_events
        SET document_version = (SELECT assigned FROM ranked WHERE ranked.id = state_events.id)
        WHERE id IN (SELECT id FROM ranked)
        "#,
        [],
    )
    .context("converge state_events.document_version")?;
    // The one-shot migration this convergence replaced left a record behind in
    // databases that ran it. Drop it so the ledger keeps only migrations that
    // still exist (`#migrationfloor`).
    tx.execute(
        "DELETE FROM state_schema_migrations WHERE migration_id = ?1",
        [RETIRED_BACKFILL_DOCUMENT_VERSION_MIGRATION],
    )?;
    tx.commit()
        .context("commit document-version convergence")?;
    Ok(())
}

/// Remove event variants that no longer exist in the strict state-backbone ABI.
///
/// Older releases dual-wrote `pending_response_*` facts beside the canonical
/// `response_captured` lifecycle facts. The duplicate variants were removed in
/// 0.34.175, so retaining those rows makes strict projection fail before the
/// controller can create its socket. This schema transaction retires the
/// redundant rows once; it does not deserialize, translate, or preserve a
/// compatibility runtime path.
fn retire_removed_state_event_variants(conn: &Connection) -> Result<()> {
    let tx = conn
        .unchecked_transaction()
        .context("begin retired state-event migration")?;
    let already_applied = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM state_schema_migrations WHERE migration_id = ?1)",
        [RETIRE_PENDING_RESPONSE_FACTS_MIGRATION],
        |row| row.get::<_, bool>(0),
    )?;
    if !already_applied {
        let applied_at = sqlite_i64(timestamp_secs(), "state migration applied_at")?;
        tx.execute(
            "DELETE FROM state_events
             WHERE fact_type IN ('pending_response_captured', 'pending_response_cleared')",
            [],
        )?;
        tx.execute(
            "INSERT INTO state_schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
            params![RETIRE_PENDING_RESPONSE_FACTS_MIGRATION, applied_at],
        )?;
    }
    tx.commit()
        .context("commit retired state-event migration")?;
    Ok(())
}

/// Durable, project-scoped coordination lease stored in `state.db`.
///
/// These leases coordinate processes; they are not document content and must
/// never be projected through a live-buffer filesystem sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationLeaseRecord {
    pub scope_kind: String,
    pub scope_id: String,
    pub holder: String,
    pub holder_pid: Option<u32>,
    pub heartbeat_secs: u64,
}

pub fn upsert_coordination_lease_in_db(
    conn: &Connection,
    lease: &CoordinationLeaseRecord,
) -> Result<()> {
    conn.execute(
        "INSERT INTO coordination_leases \
         (scope_kind, scope_id, holder, holder_pid, heartbeat_secs) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(scope_kind, scope_id) DO UPDATE SET \
           holder = excluded.holder, \
           holder_pid = excluded.holder_pid, \
           heartbeat_secs = excluded.heartbeat_secs",
        params![
            lease.scope_kind,
            lease.scope_id,
            lease.holder,
            lease.holder_pid.map(i64::from),
            lease.heartbeat_secs as i64,
        ],
    )?;
    Ok(())
}

pub fn load_coordination_lease_from_db(
    conn: &Connection,
    scope_kind: &str,
    scope_id: &str,
) -> Result<Option<CoordinationLeaseRecord>> {
    conn.query_row(
        "SELECT scope_kind, scope_id, holder, holder_pid, heartbeat_secs \
         FROM coordination_leases WHERE scope_kind = ?1 AND scope_id = ?2",
        params![scope_kind, scope_id],
        |row| {
            let holder_pid: Option<i64> = row.get(3)?;
            let heartbeat_secs: i64 = row.get(4)?;
            Ok(CoordinationLeaseRecord {
                scope_kind: row.get(0)?,
                scope_id: row.get(1)?,
                holder: row.get(2)?,
                holder_pid: holder_pid.and_then(|pid| u32::try_from(pid).ok()),
                heartbeat_secs: u64::try_from(heartbeat_secs).unwrap_or_default(),
            })
        },
    )
    .optional()
    .context("load coordination lease")
}

pub fn clear_coordination_lease_in_db(
    conn: &Connection,
    scope_kind: &str,
    scope_id: &str,
) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM coordination_leases WHERE scope_kind = ?1 AND scope_id = ?2",
        params![scope_kind, scope_id],
    )? > 0)
}

/// Durable health of the PID-scoped editor transport for one document.
/// This is control-plane state, not a live-document replica or fallback route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTransportHealthRecord {
    pub document_hash: String,
    pub session_id: String,
    pub consecutive_timeouts: u64,
    pub degraded: bool,
    pub recycle_attempted: bool,
    pub last_delivery_id: Option<String>,
    pub last_transport: String,
    pub updated_at_secs: u64,
}

pub fn upsert_editor_transport_health_in_db(
    conn: &Connection,
    health: &EditorTransportHealthRecord,
) -> Result<()> {
    conn.execute(
        "INSERT INTO editor_transport_health \
         (document_hash, session_id, consecutive_timeouts, degraded, recycle_attempted, \
          last_delivery_id, last_transport, updated_at_secs) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(document_hash) DO UPDATE SET \
          session_id = excluded.session_id, \
          consecutive_timeouts = excluded.consecutive_timeouts, \
          degraded = excluded.degraded, \
          recycle_attempted = excluded.recycle_attempted, \
          last_delivery_id = excluded.last_delivery_id, \
          last_transport = excluded.last_transport, \
          updated_at_secs = excluded.updated_at_secs",
        params![
            health.document_hash,
            health.session_id,
            health.consecutive_timeouts as i64,
            i64::from(health.degraded),
            i64::from(health.recycle_attempted),
            health.last_delivery_id,
            health.last_transport,
            health.updated_at_secs as i64,
        ],
    )?;
    Ok(())
}

pub fn load_editor_transport_health_from_db(
    conn: &Connection,
    document_hash: &str,
) -> Result<Option<EditorTransportHealthRecord>> {
    conn.query_row(
        "SELECT document_hash, session_id, consecutive_timeouts, degraded, recycle_attempted, \
                last_delivery_id, last_transport, updated_at_secs \
         FROM editor_transport_health WHERE document_hash = ?1",
        params![document_hash],
        |row| {
            let timeouts: i64 = row.get(2)?;
            let degraded: i64 = row.get(3)?;
            let recycle_attempted: i64 = row.get(4)?;
            let updated_at_secs: i64 = row.get(7)?;
            Ok(EditorTransportHealthRecord {
                document_hash: row.get(0)?,
                session_id: row.get(1)?,
                consecutive_timeouts: u64::try_from(timeouts).unwrap_or_default(),
                degraded: degraded != 0,
                recycle_attempted: recycle_attempted != 0,
                last_delivery_id: row.get(5)?,
                last_transport: row.get(6)?,
                updated_at_secs: u64::try_from(updated_at_secs).unwrap_or_default(),
            })
        },
    )
    .optional()
    .context("load editor transport health")
}

pub fn clear_editor_transport_health_in_db(conn: &Connection, document_hash: &str) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM editor_transport_health WHERE document_hash = ?1",
        params![document_hash],
    )? > 0)
}

/// Ordered editor operations captured against one exact Lazily base.
///
/// This is state-machine input in the single project ledger. It must never be
/// projected to a per-document file because a stale file can replay deleted
/// operator text after reconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorOpCaptureRecord {
    pub document_hash: String,
    pub canonical_path: String,
    pub base_hash: String,
    pub ops_json: String,
    pub updated_at_ms: u64,
}

pub fn upsert_editor_op_capture_in_db(
    conn: &Connection,
    capture: &EditorOpCaptureRecord,
) -> Result<()> {
    conn.execute(
        "INSERT INTO editor_op_captures \
         (document_hash, canonical_path, base_hash, ops_json, updated_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(document_hash) DO UPDATE SET \
           canonical_path = excluded.canonical_path, \
           base_hash = excluded.base_hash, \
           ops_json = excluded.ops_json, \
           updated_at_ms = excluded.updated_at_ms",
        params![
            capture.document_hash,
            capture.canonical_path,
            capture.base_hash,
            capture.ops_json,
            capture.updated_at_ms as i64,
        ],
    )?;
    Ok(())
}

pub fn load_editor_op_capture_from_db(
    conn: &Connection,
    document_hash: &str,
) -> Result<Option<EditorOpCaptureRecord>> {
    conn.query_row(
        "SELECT document_hash, canonical_path, base_hash, ops_json, updated_at_ms \
         FROM editor_op_captures WHERE document_hash = ?1",
        params![document_hash],
        |row| {
            let updated_at_ms: i64 = row.get(4)?;
            Ok(EditorOpCaptureRecord {
                document_hash: row.get(0)?,
                canonical_path: row.get(1)?,
                base_hash: row.get(2)?,
                ops_json: row.get(3)?,
                updated_at_ms: u64::try_from(updated_at_ms).unwrap_or_default(),
            })
        },
    )
    .optional()
    .context("load editor op capture")
}

pub fn clear_editor_op_capture_in_db(conn: &Connection, document_hash: &str) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM editor_op_captures WHERE document_hash = ?1",
        params![document_hash],
    )? > 0)
}

pub fn gc_editor_op_captures_in_db(conn: &Connection, cutoff_ms: u64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM editor_op_captures WHERE updated_at_ms = 0 OR updated_at_ms < ?1",
        params![cutoff_ms as i64],
    )?)
}

pub fn upsert_project_runtime_state_in_db(
    conn: &Connection,
    state_key: &str,
    payload: &str,
    updated_at_ms: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO project_runtime_state (state_key, payload, updated_at_ms) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(state_key) DO UPDATE SET \
           payload = excluded.payload, updated_at_ms = excluded.updated_at_ms",
        params![state_key, payload, updated_at_ms as i64],
    )?;
    Ok(())
}

pub fn load_project_runtime_state_from_db(
    conn: &Connection,
    state_key: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT payload FROM project_runtime_state WHERE state_key = ?1",
        params![state_key],
        |row| row.get(0),
    )
    .optional()
    .context("load project runtime state")
}

pub fn list_project_runtime_state_from_db(
    conn: &Connection,
    state_key_prefix: &str,
) -> Result<Vec<(String, String, u64)>> {
    let mut stmt = conn.prepare(
        "SELECT state_key, payload, updated_at_ms FROM project_runtime_state \
         WHERE state_key LIKE ?1 ORDER BY updated_at_ms DESC",
    )?;
    let pattern = format!("{state_key_prefix}%");
    let rows = stmt.query_map(params![pattern], |row| {
        let updated_at_ms: i64 = row.get(2)?;
        Ok((
            row.get(0)?,
            row.get(1)?,
            u64::try_from(updated_at_ms).unwrap_or_default(),
        ))
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("list project runtime state")
}

pub fn clear_project_runtime_state_in_db(conn: &Connection, state_key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM project_runtime_state WHERE state_key = ?1",
        params![state_key],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRuntimeStateRecord {
    pub document_hash: String,
    pub state_kind: String,
    pub canonical_path: String,
    pub payload_json: String,
    pub updated_at_ms: u64,
}

pub fn upsert_document_runtime_state_in_db(
    conn: &Connection,
    state: &DocumentRuntimeStateRecord,
) -> Result<()> {
    conn.execute(
        "INSERT INTO document_runtime_state \
         (document_hash, state_kind, canonical_path, payload_json, updated_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(document_hash, state_kind) DO UPDATE SET \
           canonical_path = excluded.canonical_path, \
           payload_json = excluded.payload_json, \
           updated_at_ms = excluded.updated_at_ms",
        params![
            state.document_hash,
            state.state_kind,
            state.canonical_path,
            state.payload_json,
            state.updated_at_ms as i64,
        ],
    )?;
    Ok(())
}

pub fn load_document_runtime_state_from_db(
    conn: &Connection,
    document_hash: &str,
    state_kind: &str,
) -> Result<Option<DocumentRuntimeStateRecord>> {
    conn.query_row(
        "SELECT document_hash, state_kind, canonical_path, payload_json, updated_at_ms \
         FROM document_runtime_state WHERE document_hash = ?1 AND state_kind = ?2",
        params![document_hash, state_kind],
        |row| {
            let updated_at_ms: i64 = row.get(4)?;
            Ok(DocumentRuntimeStateRecord {
                document_hash: row.get(0)?,
                state_kind: row.get(1)?,
                canonical_path: row.get(2)?,
                payload_json: row.get(3)?,
                updated_at_ms: u64::try_from(updated_at_ms).unwrap_or_default(),
            })
        },
    )
    .optional()
    .context("load document runtime state")
}

pub fn list_document_runtime_state_kind_from_db(
    conn: &Connection,
    state_kind: &str,
) -> Result<Vec<DocumentRuntimeStateRecord>> {
    let mut statement = conn.prepare(
        "SELECT document_hash, state_kind, canonical_path, payload_json, updated_at_ms \
         FROM document_runtime_state WHERE state_kind = ?1 ORDER BY document_hash",
    )?;
    let rows = statement.query_map(params![state_kind], |row| {
        let updated_at_ms: i64 = row.get(4)?;
        Ok(DocumentRuntimeStateRecord {
            document_hash: row.get(0)?,
            state_kind: row.get(1)?,
            canonical_path: row.get(2)?,
            payload_json: row.get(3)?,
            updated_at_ms: u64::try_from(updated_at_ms).unwrap_or_default(),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("list document runtime state by kind")
}

pub fn clear_document_runtime_state_in_db(
    conn: &Connection,
    document_hash: &str,
    state_kind: &str,
) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM document_runtime_state WHERE document_hash = ?1 AND state_kind = ?2",
        params![document_hash, state_kind],
    )? > 0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueDocumentStateRecord {
    pub document_hash: String,
    pub state_kind: String,
    pub canonical_path: String,
    pub payload_json: String,
    pub updated_at_secs: u64,
}

pub fn upsert_queue_document_state_in_db(
    conn: &Connection,
    state: &QueueDocumentStateRecord,
) -> Result<()> {
    conn.execute(
        "INSERT INTO queue_document_state \
         (document_hash, state_kind, canonical_path, payload_json, updated_at_secs) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(document_hash, state_kind) DO UPDATE SET \
           canonical_path = excluded.canonical_path, \
           payload_json = excluded.payload_json, \
           updated_at_secs = excluded.updated_at_secs",
        params![
            state.document_hash,
            state.state_kind,
            state.canonical_path,
            state.payload_json,
            state.updated_at_secs as i64,
        ],
    )?;
    Ok(())
}

pub fn load_queue_document_state_from_db(
    conn: &Connection,
    document_hash: &str,
    state_kind: &str,
) -> Result<Option<QueueDocumentStateRecord>> {
    conn.query_row(
        "SELECT document_hash, state_kind, canonical_path, payload_json, updated_at_secs \
         FROM queue_document_state WHERE document_hash = ?1 AND state_kind = ?2",
        params![document_hash, state_kind],
        |row| {
            let updated_at_secs: i64 = row.get(4)?;
            Ok(QueueDocumentStateRecord {
                document_hash: row.get(0)?,
                state_kind: row.get(1)?,
                canonical_path: row.get(2)?,
                payload_json: row.get(3)?,
                updated_at_secs: u64::try_from(updated_at_secs).unwrap_or_default(),
            })
        },
    )
    .optional()
    .context("load queue document state")
}

pub fn list_queue_document_state_from_db(
    conn: &Connection,
    state_kind: &str,
) -> Result<Vec<QueueDocumentStateRecord>> {
    let mut stmt = conn.prepare(
        "SELECT document_hash, state_kind, canonical_path, payload_json, updated_at_secs \
         FROM queue_document_state WHERE state_kind = ?1 ORDER BY document_hash",
    )?;
    let rows = stmt.query_map(params![state_kind], |row| {
        let updated_at_secs: i64 = row.get(4)?;
        Ok(QueueDocumentStateRecord {
            document_hash: row.get(0)?,
            state_kind: row.get(1)?,
            canonical_path: row.get(2)?,
            payload_json: row.get(3)?,
            updated_at_secs: u64::try_from(updated_at_secs).unwrap_or_default(),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("list queue document state")
}

pub fn clear_queue_document_state_in_db(
    conn: &Connection,
    document_hash: &str,
    state_kind: &str,
) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM queue_document_state WHERE document_hash = ?1 AND state_kind = ?2",
        params![document_hash, state_kind],
    )? > 0)
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let existing: String = row.get(1)?;
        if existing == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn sqlite_duplicate_column_error(err: &rusqlite::Error) -> bool {
    err.to_string().contains("duplicate column name")
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }
    match conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {definition}"), []) {
        Ok(_) => Ok(()),
        Err(err) if sqlite_duplicate_column_error(&err) && column_exists(conn, table, column)? => {
            Ok(())
        }
        Err(err) => Err(err.into()),
    }
}

/// Columns added to a table after its original release (`#canonicalschema`).
///
/// Every entry is `(table, column, column definition)`. This list is the SINGLE
/// declaration of those columns; both schema paths derive from it:
///
/// - a **new** database gets them because the canonical `CREATE TABLE` in
///   [`initialize_state_db`] carries them, and
///   `canonical_schema_declares_every_added_column` fails the build if it does
///   not;
/// - an **existing** database gets them because [`converge_added_columns`] walks
///   this list and issues `ALTER TABLE ... ADD COLUMN` for whatever is missing.
///
/// Keeping the two in sync by hand did not work. Before this list existed, four
/// of the twelve added columns were absent from their own canonical
/// `CREATE TABLE` — `projection_diagnostics.{source_generation, intended_hash,
/// retry_status}` and `state_events.document_version` — so a freshly created
/// database only ever got them through the `ALTER` path and the declared schema
/// did not describe reality. That is silent for as long as both paths run, and
/// it is exactly the drift a canonical declaration is supposed to make
/// impossible.
///
/// SQLite only supports `ADD COLUMN` (plus `RENAME`, and a narrow `DROP COLUMN`
/// since 3.35), so this convergence is additive by construction. A destructive
/// change would need the 12-step table rebuild, which is deliberately out of
/// scope: `state.db` is the authoritative ledger and must never be rewritten on
/// the normal open path.
const CANONICAL_ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    ("dispatch_attempts", "result_status", "result_status TEXT"),
    ("dispatch_attempts", "proof_scope", "proof_scope TEXT"),
    (
        "dispatch_attempts",
        "dispatch_start_proven",
        "dispatch_start_proven INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "projection_diagnostics",
        "source_generation",
        "source_generation INTEGER",
    ),
    ("projection_diagnostics", "intended_hash", "intended_hash TEXT"),
    ("projection_diagnostics", "retry_status", "retry_status TEXT"),
    ("queue_heads", "generation", "generation INTEGER"),
    (
        "queue_heads",
        "priority",
        "priority INTEGER NOT NULL DEFAULT 0",
    ),
    ("crash_recovery_markers", "dedupe_key", "dedupe_key TEXT"),
    (
        "state_events",
        "document_version",
        "document_version INTEGER NOT NULL DEFAULT 0",
    ),
];

/// Bring an existing database up to [`CANONICAL_ADDED_COLUMNS`].
///
/// A no-op on a database created from the current canonical `CREATE TABLE`
/// statements, which is what the anti-drift test asserts.
fn converge_added_columns(conn: &Connection) -> Result<()> {
    for (table, column, definition) in CANONICAL_ADDED_COLUMNS {
        ensure_column(conn, table, column, definition)
            .with_context(|| format!("converge canonical column {table}.{column}"))?;
    }
    Ok(())
}

/// Indexes added after their table's original release (`#canonicalschema`).
///
/// The counterpart to [`CANONICAL_ADDED_COLUMNS`]: indexes that ship with a
/// table live beside its `CREATE TABLE` in [`initialize_state_db`], and ones
/// added later live here rather than being scattered through whichever function
/// happened to introduce them. `CREATE INDEX IF NOT EXISTS` is already
/// idempotent, so this needs no version gate.
fn ensure_canonical_indexes(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS state_events_document_hash_document_version
        ON state_events(document_hash, document_version);

        CREATE UNIQUE INDEX IF NOT EXISTS crash_recovery_markers_dedupe_key
        ON crash_recovery_markers(marker_kind, dedupe_key)
        WHERE dedupe_key IS NOT NULL;

        CREATE INDEX IF NOT EXISTS crash_recovery_markers_timestamp
        ON crash_recovery_markers(timestamp);
        "#,
    )
    .context("ensure canonical indexes")?;
    Ok(())
}

/// Maximum retained `crash_recovery_markers` rows. Older markers are an audit
/// trail recovery never consults. A count cap (rather than a time window) keeps
/// the table bounded regardless of write bursts — the failure mode that produced
/// an 11.5M-row / 3.5GB table was a burst that was days, not hours, old.
const CRASH_RECOVERY_MARKER_MAX_ROWS: i64 = 20_000;


/// `#crashmarkerretention`: crash-recovery markers are an append-mostly audit
/// trail (dispatch-receipt / supervisor-lease / controller-restart reconciles).
/// Without retention they grow unbounded — a live controller accumulated 11.5M
/// rows / 3.5GB, after which the
/// `SELECT COUNT(*), MAX(...) WHERE marker_kind = 'dispatch_receipt_reconcile'`
/// aggregate scan took tens of seconds, stalling every controller read and
/// wedging editor/idle-watch supervisors on model-read timeouts. Cap the trail to
/// the newest [`CRASH_RECOVERY_MARKER_MAX_ROWS`] rows on open. This runs against
/// the freshly opened connection the controller owns, so there is no
/// cross-process lock contention, and the `crash_recovery_markers_timestamp`
/// index keeps the cap lookup and delete cheap. Delete in bounded batches so the
/// one-time cleanup of a legacy backlog cannot balloon a single WAL frame or hold
/// the write lock for the entire scan.
fn prune_crash_recovery_markers(conn: &Connection) -> Result<()> {
    prune_crash_recovery_markers_to(conn, CRASH_RECOVERY_MARKER_MAX_ROWS)
}

fn prune_crash_recovery_markers_to(conn: &Connection, max_rows: i64) -> Result<()> {
    // `crash_recovery_markers_timestamp` is declared in
    // `ensure_canonical_indexes`, but this function is also reachable from tests
    // that build the table directly, so keep the idempotent guard here too.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS crash_recovery_markers_timestamp ON crash_recovery_markers(timestamp);",
    )
    .context("failed to index crash_recovery_markers.timestamp")?;
    // The retention cutoff is the timestamp of the `max_rows`-th newest marker
    // (0-based `OFFSET max_rows - 1`); rows strictly older are dropped, keeping the
    // newest `max_rows` (plus any tie at the boundary). A non-positive cap or
    // fewer than `max_rows` rows → nothing to prune.
    if max_rows < 1 {
        return Ok(());
    }
    let cutoff: Option<i64> = conn
        .query_row(
            "SELECT timestamp FROM crash_recovery_markers ORDER BY timestamp DESC LIMIT 1 OFFSET ?1",
            [max_rows - 1],
            |row| row.get(0),
        )
        .optional()
        .context("failed to resolve crash_recovery_markers retention cutoff")?;
    let Some(cutoff) = cutoff else {
        return Ok(());
    };
    loop {
        let deleted = conn
            .execute(
                r#"
                DELETE FROM crash_recovery_markers
                WHERE rowid IN (
                    SELECT rowid FROM crash_recovery_markers
                    WHERE timestamp < ?1
                    LIMIT 50000
                )
                "#,
                [cutoff],
            )
            .context("failed to prune stale crash_recovery_markers")?;
        if deleted == 0 {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Conversion + time helpers.
// ---------------------------------------------------------------------------

pub fn sqlite_i64(value: u64, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} is too large for sqlite INTEGER"))
}

pub fn sqlite_u64(value: i64, name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{name} is negative in sqlite state"))
}

pub fn timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn count_rows(conn: &Connection, sql: &str, label: &str) -> Result<usize> {
    let count: i64 = conn
        .query_row(sql, [], |row| row.get(0))
        .with_context(|| format!("failed to count {label} in controller state"))?;
    usize::try_from(count).with_context(|| format!("{label} count is negative"))
}

pub fn load_control_plane_store_counts(conn: &Connection) -> Result<ControlPlaneStoreCounts> {
    Ok(ControlPlaneStoreCounts {
        actor_documents: count_rows(conn, "SELECT COUNT(*) FROM documents", "actor documents")?,
        live_actor_documents: count_rows(
            conn,
            "SELECT COUNT(*) FROM documents WHERE actor_state != 'closed'",
            "live actor documents",
        )?,
        actor_transitions: count_rows(
            conn,
            "SELECT COUNT(*) FROM actor_transitions",
            "actor transitions",
        )?,
        supervisor_leases: count_rows(
            conn,
            "SELECT COUNT(*) FROM supervisor_leases",
            "supervisor leases",
        )?,
        state_events: count_rows(conn, "SELECT COUNT(*) FROM state_events", "state events")?,
        dispatch_receipts: count_rows(
            conn,
            "SELECT COUNT(*) FROM dispatch_attempts",
            "dispatch attempts",
        )?,
        queue_heads: count_rows(conn, "SELECT COUNT(*) FROM queue_heads", "queue heads")?,
        document_cycles: count_rows(
            conn,
            "SELECT COUNT(*) FROM document_cycles",
            "document cycles",
        )?,
        pending_mutations: count_rows(
            conn,
            "SELECT COUNT(*) FROM pending_mutations",
            "pending mutations",
        )?,
        projection_diagnostics: count_rows(
            conn,
            "SELECT COUNT(*) FROM projection_diagnostics",
            "projection diagnostics",
        )?,
        admin_operations: count_rows(
            conn,
            "SELECT COUNT(*) FROM admin_operations",
            "admin operations",
        )?,
        queue_controls: count_rows(
            conn,
            "SELECT COUNT(*) FROM queue_controls",
            "queue controls",
        )?,
        queue_backpressure: count_rows(
            conn,
            "SELECT COUNT(*) FROM queue_backpressure",
            "queue backpressure",
        )?,
        // This ledger can grow into the millions on crash-looping projects. Status
        // only needs a scale signal, so use the rowid high-water mark instead of
        // forcing every controller health poll to scan the full table.
        crash_recovery_markers: count_rows(
            conn,
            "SELECT COALESCE(MAX(id), 0) FROM crash_recovery_markers",
            "crash recovery markers",
        )?,
        layout_states: count_rows(conn, "SELECT COUNT(*) FROM layout_states", "layout states")?,
    })
}

// ---------------------------------------------------------------------------
// Actor reads.
// ---------------------------------------------------------------------------

pub fn actor_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActorRecord> {
    let actor_state: String = row.get("actor_state")?;
    let state = ActorState::parse(&actor_state).ok_or(rusqlite::Error::InvalidQuery)?;
    let generation: i64 = row.get("generation")?;
    let transition_prior: i64 = row.get("prior_generation")?;
    let transition_new: i64 = row.get("new_generation")?;
    let transition_timestamp: i64 = row.get("timestamp")?;
    Ok(ActorRecord {
        document_id: row.get("document_id")?,
        session_id: row.get("session_id")?,
        generation: sqlite_u64(generation, "generation")
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        pane_id: row.get("pane_id")?,
        window_id: row.get("window_id")?,
        harness: row.get("harness_id")?,
        state,
        last_transition: ActorLastTransition {
            caller: row.get("caller")?,
            reason: row.get("reason")?,
            timestamp: sqlite_u64(transition_timestamp, "transition timestamp")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            prior_generation: sqlite_u64(transition_prior, "transition prior_generation")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            new_generation: sqlite_u64(transition_new, "transition new_generation")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        },
    })
}

pub fn load_actor_record_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<Option<ActorRecord>> {
    conn.query_row(
        r#"
        SELECT
            d.document_id,
            d.session_id,
            d.generation,
            d.pane_id,
            d.window_id,
            d.harness_id,
            d.actor_state,
            t.caller,
            t.reason,
            t.timestamp,
            t.prior_generation,
            t.new_generation
        FROM documents d
        JOIN actor_transitions t ON t.id = d.last_transition_id
        WHERE d.document_id = ?1
        "#,
        params![document_id],
        actor_record_from_row,
    )
    .optional()
    .context("failed to load actor record from controller state")
}

pub fn load_actor_store_from_db(conn: &Connection) -> Result<BTreeMap<String, ActorRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            d.document_id,
            d.session_id,
            d.generation,
            d.pane_id,
            d.window_id,
            d.harness_id,
            d.actor_state,
            t.caller,
            t.reason,
            t.timestamp,
            t.prior_generation,
            t.new_generation
        FROM documents d
        JOIN actor_transitions t ON t.id = d.last_transition_id
        ORDER BY d.document_id
        "#,
    )?;
    let mut store = BTreeMap::new();
    for row in stmt.query_map([], actor_record_from_row)? {
        let record = row?;
        store.insert(record.document_id.clone(), record);
    }
    Ok(store)
}

pub fn load_actor_transitions_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<Vec<ActorTransitionStatus>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            prior_generation,
            new_generation,
            caller,
            reason,
            old_pane,
            new_pane,
            old_window,
            new_window,
            timestamp
        FROM actor_transitions
        WHERE document_id = ?1
        ORDER BY id
        "#,
    )?;
    let mut transitions = Vec::new();
    for row in stmt.query_map(params![document_id], |row| {
        let prior_generation: i64 = row.get("prior_generation")?;
        let new_generation: i64 = row.get("new_generation")?;
        let timestamp: i64 = row.get("timestamp")?;
        Ok(ActorTransitionStatus {
            prior_generation: sqlite_u64(prior_generation, "transition prior_generation")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            new_generation: sqlite_u64(new_generation, "transition new_generation")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            caller: row.get("caller")?,
            reason: row.get("reason")?,
            old_pane: row.get("old_pane")?,
            new_pane: row.get("new_pane")?,
            old_window: row.get("old_window")?,
            new_window: row.get("new_window")?,
            timestamp: sqlite_u64(timestamp, "transition timestamp")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })? {
        transitions.push(row?);
    }
    Ok(transitions)
}

pub fn load_supervisor_lease_from_db(
    conn: &Connection,
    document_id: &str,
    generation: u64,
) -> Result<Option<SupervisorLeaseStatus>> {
    conn.query_row(
        r#"
        SELECT
            generation,
            supervisor_pid,
            supervisor_socket,
            last_heartbeat,
            runtime_state
        FROM supervisor_leases
        WHERE document_id = ?1 AND generation = ?2
        "#,
        params![document_id, sqlite_i64(generation, "generation")?],
        |row| {
            let generation: i64 = row.get("generation")?;
            let supervisor_pid: Option<i64> = row.get("supervisor_pid")?;
            let last_heartbeat: Option<i64> = row.get("last_heartbeat")?;
            Ok(SupervisorLeaseStatus {
                generation: sqlite_u64(generation, "supervisor lease generation")
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                supervisor_pid: supervisor_pid.and_then(|pid| u32::try_from(pid).ok()),
                supervisor_socket: row.get("supervisor_socket")?,
                last_heartbeat: last_heartbeat
                    .map(|value| sqlite_u64(value, "supervisor last heartbeat"))
                    .transpose()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                runtime_state: row.get("runtime_state")?,
            })
        },
    )
    .optional()
    .context("failed to load supervisor lease from controller state")
}

pub fn load_dispatch_attempts_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<Vec<DispatchAttemptStatus>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            id,
            generation,
            command_kind,
            accepted_stage,
            failed_stage,
            diagnostic_payload,
            result_status,
            proof_scope,
            dispatch_start_proven,
            timestamp
        FROM dispatch_attempts
        WHERE document_id = ?1
        ORDER BY id DESC
        LIMIT 10
        "#,
    )?;
    let mut attempts = Vec::new();
    for row in stmt.query_map(params![document_id], |row| {
        let receipt_id: i64 = row.get("id")?;
        let generation: i64 = row.get("generation")?;
        let dispatch_start_proven: i64 = row.get("dispatch_start_proven")?;
        let timestamp: i64 = row.get("timestamp")?;
        Ok(DispatchAttemptStatus {
            receipt_id: sqlite_u64(receipt_id, "dispatch receipt id")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            generation: sqlite_u64(generation, "dispatch generation")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            command_kind: row.get("command_kind")?,
            accepted_stage: row.get("accepted_stage")?,
            failed_stage: row.get("failed_stage")?,
            diagnostic_payload: row.get("diagnostic_payload")?,
            result_status: row.get("result_status")?,
            proof_scope: row.get("proof_scope")?,
            dispatch_start_proven: dispatch_start_proven != 0,
            timestamp: sqlite_u64(timestamp, "dispatch timestamp")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })? {
        attempts.push(row?);
    }
    attempts.reverse();
    Ok(attempts)
}

pub fn load_projection_diagnostics_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<Vec<ProjectionDiagnosticStatus>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT projection, message, source_generation, intended_hash, retry_status, timestamp
        FROM projection_diagnostics
        WHERE document_id = ?1
        ORDER BY id DESC
        LIMIT 10
        "#,
    )?;
    let mut diagnostics = Vec::new();
    for row in stmt.query_map(params![document_id], |row| {
        let timestamp: i64 = row.get("timestamp")?;
        let source_generation: Option<i64> = row.get("source_generation")?;
        Ok(ProjectionDiagnosticStatus {
            projection: row.get("projection")?,
            message: row.get("message")?,
            source_generation: source_generation
                .map(|generation| sqlite_u64(generation, "projection source generation"))
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            intended_hash: row.get("intended_hash")?,
            retry_status: row.get("retry_status")?,
            timestamp: sqlite_u64(timestamp, "projection diagnostic timestamp")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })? {
        diagnostics.push(row?);
    }
    diagnostics.reverse();
    Ok(diagnostics)
}

pub fn load_queue_head_from_db(
    conn: &Connection,
    document_id: &str,
    queue_name: &str,
) -> Result<Option<QueueHeadStatus>> {
    conn.query_row(
        r#"
        SELECT
            document_id,
            queue_name,
            generation,
            head_id,
            prompt,
            state,
            priority,
            selected_at,
            updated_at
        FROM queue_heads
        WHERE document_id = ?1 AND queue_name = ?2
        "#,
        params![document_id, queue_name],
        |row| {
            let generation: Option<i64> = row.get("generation")?;
            let priority: i64 = row.get("priority")?;
            let selected_at: i64 = row.get("selected_at")?;
            let updated_at: i64 = row.get("updated_at")?;
            Ok(QueueHeadStatus {
                document_id: row.get("document_id")?,
                queue_name: row.get("queue_name")?,
                generation: generation
                    .map(|generation| sqlite_u64(generation, "queue head generation"))
                    .transpose()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                head_id: row.get("head_id")?,
                prompt: row.get("prompt")?,
                state: row.get("state")?,
                priority: sqlite_u64(priority, "queue head priority")
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                selected_at: sqlite_u64(selected_at, "queue head selected_at")
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                updated_at: sqlite_u64(updated_at, "queue head updated_at")
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        },
    )
    .optional()
    .context("failed to load queue head from controller state")
}

pub fn load_queue_control_from_db(
    conn: &Connection,
    scope_kind: &str,
    scope_id: &str,
) -> Result<Option<QueueControlStatus>> {
    conn.query_row(
        r#"
        SELECT
            id,
            scope_kind,
            scope_id,
            state,
            reason,
            operation_receipt_id,
            updated_at
        FROM queue_controls
        WHERE scope_kind = ?1 AND scope_id = ?2
        "#,
        params![scope_kind, scope_id],
        |row| {
            let receipt_id: i64 = row.get("id")?;
            let operation_receipt_id: Option<i64> = row.get("operation_receipt_id")?;
            let updated_at: i64 = row.get("updated_at")?;
            Ok(QueueControlStatus {
                receipt_id: sqlite_u64(receipt_id, "queue control receipt id")
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                scope_kind: row.get("scope_kind")?,
                scope_id: row.get("scope_id")?,
                state: row.get("state")?,
                reason: row.get("reason")?,
                operation_receipt_id: operation_receipt_id
                    .map(|value| sqlite_u64(value, "queue control operation receipt id"))
                    .transpose()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                updated_at: sqlite_u64(updated_at, "queue control updated_at")
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        },
    )
    .optional()
    .context("failed to load queue control from controller state")
}

pub fn load_effective_queue_control_from_db(
    conn: &Connection,
    document_id: &str,
    project_scope_id: &str,
) -> Result<Option<QueueControlStatus>> {
    if let Some(control) = load_queue_control_from_db(conn, "document", document_id)?
        && control.state != "resumed"
    {
        return Ok(Some(control));
    }
    let project = load_queue_control_from_db(conn, "project", project_scope_id)?;
    Ok(project.filter(|control| control.state != "resumed"))
}

pub fn load_admin_operations_from_db(
    conn: &Connection,
    document_id: Option<&str>,
    limit: usize,
) -> Result<Vec<AdminOperationStatus>> {
    let limit = i64::try_from(limit.max(1)).context("admin operation limit too large")?;
    let mut operations = Vec::new();
    if let Some(document_id) = document_id {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, operation_kind, document_id, status, diagnostic_payload, timestamp
            FROM admin_operations
            WHERE document_id = ?1
            ORDER BY id DESC
            LIMIT ?2
            "#,
        )?;
        for row in stmt.query_map(params![document_id, limit], admin_operation_from_row)? {
            operations.push(row?);
        }
    } else {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, operation_kind, document_id, status, diagnostic_payload, timestamp
            FROM admin_operations
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        for row in stmt.query_map(params![limit], admin_operation_from_row)? {
            operations.push(row?);
        }
    }
    operations.reverse();
    Ok(operations)
}

pub fn load_queue_backpressure_from_db(
    conn: &Connection,
    document_id: &str,
    limit: usize,
) -> Result<Vec<QueueBackpressureStatus>> {
    let limit = i64::try_from(limit.max(1)).context("queue backpressure limit too large")?;
    let mut stmt = conn.prepare(
        r#"
        SELECT
            id,
            document_id,
            generation,
            command_kind,
            capacity_class,
            reason,
            dispatch_receipt_id,
            timestamp
        FROM queue_backpressure
        WHERE document_id = ?1
        ORDER BY id DESC
        LIMIT ?2
        "#,
    )?;
    let mut receipts = Vec::new();
    for row in stmt.query_map(params![document_id, limit], |row| {
        let receipt_id: i64 = row.get("id")?;
        let generation: Option<i64> = row.get("generation")?;
        let dispatch_receipt_id: Option<i64> = row.get("dispatch_receipt_id")?;
        let timestamp: i64 = row.get("timestamp")?;
        Ok(QueueBackpressureStatus {
            receipt_id: sqlite_u64(receipt_id, "queue backpressure receipt id")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            document_id: row.get("document_id")?,
            generation: generation
                .map(|generation| sqlite_u64(generation, "queue backpressure generation"))
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            command_kind: row.get("command_kind")?,
            capacity_class: row.get("capacity_class")?,
            reason: row.get("reason")?,
            dispatch_receipt_id: dispatch_receipt_id
                .map(|receipt_id| sqlite_u64(receipt_id, "queue backpressure dispatch receipt id"))
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            timestamp: sqlite_u64(timestamp, "queue backpressure timestamp")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })? {
        receipts.push(row?);
    }
    receipts.reverse();
    Ok(receipts)
}

fn admin_operation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdminOperationStatus> {
    let receipt_id: i64 = row.get("id")?;
    let timestamp: i64 = row.get("timestamp")?;
    Ok(AdminOperationStatus {
        receipt_id: sqlite_u64(receipt_id, "admin operation receipt id")
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        operation_kind: row.get("operation_kind")?,
        document_id: row.get("document_id")?,
        status: row.get("status")?,
        diagnostic_payload: row.get("diagnostic_payload")?,
        timestamp: sqlite_u64(timestamp, "admin operation timestamp")
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

pub fn load_session_operator_status_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<SessionOperatorStatus> {
    let record = load_actor_record_from_db(conn, document_id)?;
    let supervisor_lease = match &record {
        Some(record) => load_supervisor_lease_from_db(conn, document_id, record.generation)?,
        None => None,
    };
    Ok(SessionOperatorStatus {
        record,
        transitions: load_actor_transitions_from_db(conn, document_id)?,
        supervisor_lease,
        dispatch_attempts: load_dispatch_attempts_from_db(conn, document_id)?,
        projection_diagnostics: load_projection_diagnostics_from_db(conn, document_id)?,
    })
}

// ---------------------------------------------------------------------------
// State-event ledger reads.
// ---------------------------------------------------------------------------

fn state_event_status_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StateEventStatus> {
    let sequence: i64 = row.get("id")?;
    let timestamp: i64 = row.get("timestamp")?;
    Ok(StateEventStatus {
        sequence: sqlite_u64(sequence, "state event sequence")
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        event_id: row.get("event_id")?,
        document_hash: row.get("document_hash")?,
        domain: row.get("domain")?,
        fact_type: row.get("fact_type")?,
        payload_json: row.get("payload_json")?,
        timestamp: sqlite_u64(timestamp, "state event timestamp")
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

pub fn load_state_events_from_db(
    conn: &Connection,
    document_hash: Option<&str>,
) -> Result<Vec<StateEventStatus>> {
    let mut events = Vec::new();
    if let Some(document_hash) = document_hash {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, event_id, document_hash, domain, fact_type, payload_json, timestamp
            FROM state_events
            WHERE document_hash = ?1
            ORDER BY id
            "#,
        )?;
        for row in stmt.query_map(params![document_hash], state_event_status_from_row)? {
            events.push(row?);
        }
    } else {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, event_id, document_hash, domain, fact_type, payload_json, timestamp
            FROM state_events
            ORDER BY id
            "#,
        )?;
        for row in stmt.query_map([], state_event_status_from_row)? {
            events.push(row?);
        }
    }
    Ok(events)
}

/// Load the durable facts consumed by cycle closeout and proof projections.
///
/// `document_authority_observed` is high-frequency current-document telemetry;
/// it only updates `DocumentProjection::latest_authority` and cannot affect the
/// closeout/proof fields read by `agent-doc-cycle-state-io`. Excluding it keeps
/// idle supervisor polls proportional to lifecycle facts instead of the age of
/// the authority-observation ledger.
pub fn load_state_events_for_cycle_projection_from_db(
    conn: &Connection,
    document_hash: &str,
) -> Result<Vec<StateEventStatus>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, event_id, document_hash, domain, fact_type, payload_json, timestamp
        FROM state_events
        WHERE document_hash = ?1
          AND fact_type <> 'document_authority_observed'
        ORDER BY id
        "#,
    )?;
    let mut events = Vec::new();
    for row in stmt.query_map(params![document_hash], state_event_status_from_row)? {
        events.push(row?);
    }
    Ok(events)
}

/// Load a bounded newest-first window of one durable fact type for a document.
///
/// This is intentionally narrower than projection replay: history-aware
/// recovery needs a few prior content-bearing checkpoints, not every fact the
/// document has accumulated.
pub fn load_recent_state_events_by_fact_type_from_db(
    conn: &Connection,
    document_hash: &str,
    fact_type: &str,
    limit: usize,
) -> Result<Vec<StateEventStatus>> {
    let limit = i64::try_from(limit.max(1)).context("state event history limit too large")?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, event_id, document_hash, domain, fact_type, payload_json, timestamp
        FROM state_events
        WHERE document_hash = ?1
          AND fact_type = ?2
        ORDER BY id DESC
        LIMIT ?3
        "#,
    )?;
    let mut events = Vec::new();
    for row in stmt.query_map(
        params![document_hash, fact_type, limit],
        state_event_status_from_row,
    )? {
        events.push(row?);
    }
    Ok(events)
}

// ---------------------------------------------------------------------------
// Actor writes.
// ---------------------------------------------------------------------------

pub fn insert_actor_transition(
    conn: &Connection,
    previous: Option<&ActorRecord>,
    record: &ActorRecord,
) -> Result<i64> {
    conn.execute(
        r#"
        INSERT INTO actor_transitions (
            document_id,
            prior_generation,
            new_generation,
            caller,
            reason,
            old_pane,
            new_pane,
            old_window,
            new_window,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            record.document_id,
            sqlite_i64(record.last_transition.prior_generation, "prior_generation")?,
            sqlite_i64(record.last_transition.new_generation, "new_generation")?,
            record.last_transition.caller,
            record.last_transition.reason,
            previous.map(|prior| prior.pane_id.as_str()),
            record.pane_id,
            previous.map(|prior| prior.window_id.as_str()),
            record.window_id,
            sqlite_i64(record.last_transition.timestamp, "transition timestamp")?,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Upsert the authoritative `documents` row for an actor.
///
/// `launch_mode` and `controller_epoch` are the lifted orchestration tendril:
/// the orchestration caller computes them from `read_bootstrap` (launch mode
/// short string + bootstrap epoch) and passes them in. Pass `None`/`None` when
/// no bootstrap state is available.
pub fn upsert_actor_document(
    conn: &Connection,
    record: &ActorRecord,
    transition_id: i64,
    launch_mode: Option<String>,
    controller_epoch: Option<i64>,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO documents (
            document_id,
            canonical_path,
            session_id,
            generation,
            pane_id,
            window_id,
            harness_id,
            actor_state,
            launch_mode,
            controller_epoch,
            last_transition_id
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(document_id) DO UPDATE SET
            canonical_path = excluded.canonical_path,
            session_id = excluded.session_id,
            generation = excluded.generation,
            pane_id = excluded.pane_id,
            window_id = excluded.window_id,
            harness_id = excluded.harness_id,
            actor_state = excluded.actor_state,
            launch_mode = excluded.launch_mode,
            controller_epoch = excluded.controller_epoch,
            last_transition_id = excluded.last_transition_id
        "#,
        params![
            record.document_id,
            record.document_id,
            record.session_id,
            sqlite_i64(record.generation, "generation")?,
            record.pane_id,
            record.window_id,
            record.harness,
            record.state.as_str(),
            launch_mode,
            controller_epoch,
            transition_id,
        ],
    )?;
    Ok(())
}

pub fn upsert_supervisor_lease_in_db(
    conn: &Connection,
    record: &ActorRecord,
    supervisor_pid: Option<u32>,
    supervisor_socket: Option<&str>,
    runtime_state: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO supervisor_leases (
            document_id,
            generation,
            supervisor_pid,
            supervisor_socket,
            last_heartbeat,
            runtime_state
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(document_id, generation) DO UPDATE SET
            supervisor_pid = COALESCE(excluded.supervisor_pid, supervisor_leases.supervisor_pid),
            supervisor_socket = COALESCE(excluded.supervisor_socket, supervisor_leases.supervisor_socket),
            last_heartbeat = excluded.last_heartbeat,
            runtime_state = excluded.runtime_state
        "#,
        params![
            record.document_id,
            sqlite_i64(record.generation, "generation")?,
            supervisor_pid.map(i64::from),
            supervisor_socket,
            sqlite_i64(timestamp_secs(), "supervisor heartbeat timestamp")?,
            runtime_state,
        ],
    )?;
    Ok(())
}

pub fn insert_state_event_in_db(conn: &Connection, event: &StateEventInsert<'_>) -> Result<bool> {
    // `document_version` (`#retentionversion`) is assigned inside the same
    // statement as the insert. SQLite serializes writers, so the `MAX(..) + 1`
    // subquery and the insert cannot interleave with another connection's
    // append — there is no read-modify-write window to lose. It is deliberately
    // NOT a UNIQUE constraint: `INSERT OR IGNORE` targets the `event_id`
    // uniqueness, and a second unique index here could silently drop a
    // legitimate event instead of the intended duplicate.
    let changed = conn.execute(
        r#"
        INSERT OR IGNORE INTO state_events (
            event_id,
            document_hash,
            domain,
            fact_type,
            payload_json,
            timestamp,
            document_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, (
            SELECT COALESCE(MAX(existing.document_version), 0) + 1
            FROM state_events existing
            WHERE existing.document_hash = ?2
        ))
        "#,
        params![
            event.event_id,
            event.document_hash,
            event.domain,
            event.fact_type,
            event.payload_json,
            sqlite_i64(timestamp_secs(), "state event timestamp")?,
        ],
    )?;
    Ok(changed > 0)
}

/// `#qflood`: is a dispatch already in flight (accepted, not yet consumed) for this
/// document at `generation`? Mirrors the open-dispatch shape the restart reconciler
/// keys on. An open dispatch means the current turn already has work queued/running,
/// so an auto re-fire would only pile a redundant trigger into the busy pane.
pub fn has_open_in_flight_dispatch(
    conn: &Connection,
    document_id: &str,
    generation: u64,
) -> Result<bool> {
    let count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM dispatch_attempts
        WHERE document_id = ?1
          AND generation = ?2
          AND failed_stage IS NULL
          AND COALESCE(result_status, '') IN ('accepted', 'queued', 'running')
          AND dispatch_start_proven = 0
        "#,
        params![document_id, generation as i64],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// How long an unproven dispatch may pin the controller's idle self-recycle gate
/// (`#dispatchgateleak`).
///
/// An unproven row is only cleared by `mark_open_dispatches_consumed` on an actor
/// `Ready` transition. A dispatch that never reached `Ready` — a killed pane, a
/// crashed harness, a supervisor that died mid-turn — leaves its row open forever,
/// and because [`has_any_open_in_flight_dispatch`] is project-wide, ONE such row
/// pins the recycle gate false for every document in the project, permanently.
///
/// Live repro 2026-07-18: 18 unproven rows aged 34-50h kept a stale `controller
/// serve` alive across two days of cycles, so the freshly-installed binary could
/// never be promoted. No real dispatch stays in flight for hours; anything past
/// this horizon is leaked state, not in-flight work.
pub const OPEN_DISPATCH_IN_FLIGHT_HORIZON_SECS: i64 = 30 * 60;

/// `#ctlrecycle`: is ANY dispatch in flight across every document/generation? The
/// controller's idle self-recycle (R1) uses this as its idle proof — it must never
/// exit while a turn is mid-dispatch for any session it coordinates. Same open-set
/// definition as [`has_open_in_flight_dispatch`] without the document/generation
/// filter.
///
/// Bounded by [`OPEN_DISPATCH_IN_FLIGHT_HORIZON_SECS`] (`#dispatchgateleak`) so a
/// leaked unproven row cannot wedge the recycle gate forever. This only ever makes
/// the gate MORE permissive for provably-stale rows; a genuinely in-flight dispatch
/// is minutes old at most and still pins the gate exactly as before.
pub fn has_any_open_in_flight_dispatch(conn: &Connection) -> Result<bool> {
    has_any_open_in_flight_dispatch_as_of(conn, timestamp_secs() as i64)
}

/// [`has_any_open_in_flight_dispatch`] with an injectable clock so the staleness
/// horizon is testable without sleeping.
pub fn has_any_open_in_flight_dispatch_as_of(conn: &Connection, now_secs: i64) -> Result<bool> {
    let count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM dispatch_attempts
        WHERE failed_stage IS NULL
          AND COALESCE(result_status, '') IN ('accepted', 'queued', 'running')
          AND dispatch_start_proven = 0
          AND timestamp > ?1
        "#,
        params![now_secs - OPEN_DISPATCH_IN_FLIGHT_HORIZON_SECS],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// `#qflood`: mark every open in-flight dispatch for this document consumed. Called
/// when the actor transitions to `Ready` (the turn finished → its dispatch is done),
/// keeping the open-dispatch set accurate for the next busy episode's coalescing and
/// for restart recovery. Returns the number of receipts released.
pub fn mark_open_dispatches_consumed(conn: &Connection, document_id: &str) -> Result<usize> {
    let released = conn.execute(
        r#"
        UPDATE dispatch_attempts
        SET dispatch_start_proven = 1
        WHERE document_id = ?1
          AND failed_stage IS NULL
          AND COALESCE(result_status, '') IN ('accepted', 'queued', 'running')
          AND dispatch_start_proven = 0
        "#,
        params![document_id],
    )?;
    Ok(released)
}

pub fn insert_dispatch_attempt_in_db(
    conn: &Connection,
    attempt: &DispatchAttemptInsert<'_>,
) -> Result<u64> {
    conn.execute(
        r#"
        INSERT INTO dispatch_attempts (
            document_id,
            generation,
            command_kind,
            accepted_stage,
            failed_stage,
            diagnostic_payload,
            result_status,
            proof_scope,
            dispatch_start_proven,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            attempt.document_id,
            sqlite_i64(attempt.generation, "generation")?,
            attempt.command_kind,
            attempt.accepted_stage,
            attempt.failed_stage,
            attempt.diagnostic_payload,
            attempt.result_status,
            attempt.proof_scope,
            if attempt.dispatch_start_proven {
                1_i64
            } else {
                0_i64
            },
            sqlite_i64(timestamp_secs(), "dispatch attempt timestamp")?,
        ],
    )?;
    sqlite_u64(conn.last_insert_rowid(), "dispatch receipt id")
}

pub fn insert_projection_diagnostic(
    conn: &Connection,
    projection: &str,
    document_id: &str,
    message: &str,
) -> Result<()> {
    insert_projection_diagnostic_with_metadata(
        conn,
        &ProjectionDiagnosticInsert {
            projection,
            document_id,
            message,
            source_generation: None,
            intended_hash: None,
            retry_status: "retry_pending",
        },
    )
}

pub fn insert_projection_diagnostic_with_metadata(
    conn: &Connection,
    diagnostic: &ProjectionDiagnosticInsert<'_>,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO projection_diagnostics (
            projection,
            document_id,
            message,
            source_generation,
            intended_hash,
            retry_status,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            diagnostic.projection,
            diagnostic.document_id,
            diagnostic.message,
            diagnostic
                .source_generation
                .map(|generation| sqlite_i64(generation, "projection source generation"))
                .transpose()?,
            diagnostic.intended_hash,
            diagnostic.retry_status,
            sqlite_i64(timestamp_secs(), "projection diagnostic timestamp")?
        ],
    )?;
    Ok(())
}

fn evict_cross_document_actor_pane_bindings_tx(
    conn: &Connection,
    owner_document_id: &str,
    pane_id: &str,
    caller: &str,
    timestamp: u64,
    launch_mode: Option<String>,
    controller_epoch: Option<i64>,
) -> Result<Vec<String>> {
    if pane_id.is_empty() {
        return Ok(Vec::new());
    }
    let store = load_actor_store_from_db(conn)?;
    let mut evicted = Vec::new();
    for prior in store.values() {
        if prior.document_id == owner_document_id || prior.pane_id != pane_id {
            continue;
        }
        let mut next = prior.clone();
        next.state = ActorState::Closed;
        next.pane_id.clear();
        next.window_id.clear();
        next.last_transition = ActorLastTransition {
            caller: caller.to_string(),
            reason: format!("evicted_cross_document_pane owner={owner_document_id} pane={pane_id}"),
            timestamp,
            prior_generation: prior.generation,
            new_generation: prior.generation,
        };
        let transition_id = insert_actor_transition(conn, Some(prior), &next)?;
        upsert_actor_document(
            conn,
            &next,
            transition_id,
            launch_mode.clone(),
            controller_epoch,
        )?;
        evicted.push(prior.document_id.clone());
    }
    Ok(evicted)
}

/// Run the compare-and-swap actor write transaction.
///
/// Mirrors the prior `project_controller::store_actor_record` transaction body:
/// load the previous record, enforce the optional CAS expectation, reject
/// generation regressions, recover cross-document pane aliases, insert the
/// transition, then upsert the document row using the lifted
/// `launch_mode`/`controller_epoch` bootstrap tendril.
pub fn store_actor_record_tx(
    conn: &mut Connection,
    expected_prior_generation: Option<u64>,
    record: &ActorRecord,
    launch_mode: Option<String>,
    controller_epoch: Option<i64>,
) -> Result<Vec<String>> {
    // This transaction reads the current actor generation before writing the
    // replacement record. A deferred SQLite transaction can let two starters
    // acquire read snapshots and then fail one read-to-write upgrade with
    // SQLITE_BUSY immediately; busy_timeout cannot resolve that upgrade
    // deadlock. Reserve the writer slot before reading so concurrent starts
    // serialize through the configured busy timeout instead.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let previous = load_actor_record_from_db(&tx, &record.document_id)?;
    let prior_generation = previous
        .as_ref()
        .map(|prior| prior.generation)
        .unwrap_or(record.last_transition.prior_generation);
    if let Some(expected) = expected_prior_generation
        && prior_generation != expected
    {
        anyhow::bail!(
            "controller actor generation compare-and-swap failed for {}: expected {}, found {}",
            record.document_id,
            expected,
            prior_generation
        );
    }
    if record.generation < prior_generation {
        anyhow::bail!(
            "controller actor generation regression for {}: attempted {}, current {}",
            record.document_id,
            record.generation,
            prior_generation
        );
    }
    let evicted_document_ids = if record.state != ActorState::Closed {
        evict_cross_document_actor_pane_bindings_tx(
            &tx,
            &record.document_id,
            &record.pane_id,
            &record.last_transition.caller,
            record.last_transition.timestamp,
            launch_mode.clone(),
            controller_epoch,
        )?
    } else {
        Vec::new()
    };
    let transition_id = insert_actor_transition(&tx, previous.as_ref(), record)?;
    upsert_actor_document(&tx, record, transition_id, launch_mode, controller_epoch)?;
    tx.commit()?;
    Ok(evicted_document_ids)
}

/// `#actorprune`: hard-delete a dead actor record and its history/lease rows.
///
/// `close_stale_starting_actors` only TRANSITIONS `Starting` actors to `Closed`;
/// nothing removes long-dead `Closed` rows, so the `documents` projection (and
/// `admin list`) grows without bound. This removes the `documents` row plus its
/// `actor_transitions` history and `supervisor_leases` rows for one document_id
/// in a single transaction. Returns the number of `documents` rows removed (0 or
/// 1).
pub fn delete_actor_document_tx(conn: &mut Connection, document_id: &str) -> Result<usize> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM actor_transitions WHERE document_id = ?1",
        params![document_id],
    )?;
    tx.execute(
        "DELETE FROM supervisor_leases WHERE document_id = ?1",
        params![document_id],
    )?;
    let removed = tx.execute(
        "DELETE FROM documents WHERE document_id = ?1",
        params![document_id],
    )?;
    tx.commit()?;
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Control-plane durable facts.
// ---------------------------------------------------------------------------

pub fn upsert_queue_head_in_db(
    conn: &Connection,
    document_id: &str,
    queue_name: &str,
    head_id: Option<&str>,
    prompt: &str,
    state: &str,
) -> Result<()> {
    let now = sqlite_i64(timestamp_secs(), "queue head timestamp")?;
    conn.execute(
        r#"
        INSERT INTO queue_heads (
            document_id,
            queue_name,
            head_id,
            prompt,
            state,
            selected_at,
            updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
        ON CONFLICT(document_id, queue_name) DO UPDATE SET
            head_id = excluded.head_id,
            prompt = excluded.prompt,
            state = excluded.state,
            updated_at = excluded.updated_at
        "#,
        params![document_id, queue_name, head_id, prompt, state, now],
    )?;
    Ok(())
}

/// Provenance of a queue head (`#fr79`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueHeadSource {
    /// The binary created this head by mirroring a backlog item. If its backlog
    /// id later vanishes, the head is real drift and may be struck.
    BacklogMirror,
}

impl QueueHeadSource {
    pub fn label(self) -> &'static str {
        match self {
            QueueHeadSource::BacklogMirror => "backlog_mirror",
        }
    }
}

/// Record that the backlog mirror created these queue head identities.
///
/// Idempotent and additive: re-recording an identity only refreshes its
/// timestamp. Nothing here ever marks a head operator-authored — that is the
/// DEFAULT, expressed as the absence of a row (see the table comment).
pub fn record_mirrored_queue_heads_in_db(
    conn: &Connection,
    document_id: &str,
    head_identities: &[String],
) -> Result<()> {
    if head_identities.is_empty() {
        return Ok(());
    }
    let now = sqlite_i64(timestamp_secs(), "queue head provenance timestamp")?;
    for identity in head_identities {
        conn.execute(
            r#"
            INSERT INTO queue_head_provenance (
                document_id,
                head_identity,
                source,
                recorded_at
            )
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(document_id, head_identity) DO UPDATE SET
                recorded_at = excluded.recorded_at
            "#,
            params![
                document_id,
                identity,
                QueueHeadSource::BacklogMirror.label(),
                now
            ],
        )?;
    }
    Ok(())
}

/// Head identities this document's backlog mirror is known to have created.
///
/// An identity absent from this set has UNKNOWN provenance and must be treated
/// as operator-authored — never struck (`#qauthorder`).
pub fn load_mirrored_queue_head_identities_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT head_identity FROM queue_head_provenance \
         WHERE document_id = ?1 AND source = ?2",
    )?;
    let rows = stmt.query_map(
        params![document_id, QueueHeadSource::BacklogMirror.label()],
        |row| row.get::<_, String>(0),
    )?;
    let mut out = std::collections::HashSet::new();
    for row in rows {
        out.insert(row?);
    }
    Ok(out)
}

pub fn upsert_queue_control_in_db(
    conn: &Connection,
    control: &QueueControlInsert<'_>,
) -> Result<QueueControlStatus> {
    let now = sqlite_i64(timestamp_secs(), "queue control timestamp")?;
    conn.execute(
        r#"
        INSERT INTO queue_controls (
            scope_kind,
            scope_id,
            state,
            reason,
            operation_receipt_id,
            updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(scope_kind, scope_id) DO UPDATE SET
            state = excluded.state,
            reason = excluded.reason,
            operation_receipt_id = excluded.operation_receipt_id,
            updated_at = excluded.updated_at
        "#,
        params![
            control.scope_kind,
            control.scope_id,
            control.state,
            control.reason,
            control
                .operation_receipt_id
                .map(|receipt_id| sqlite_i64(receipt_id, "queue control operation receipt id"))
                .transpose()?,
            now
        ],
    )?;
    load_queue_control_from_db(conn, control.scope_kind, control.scope_id)?
        .context("missing queue control after upsert")
}

pub fn insert_queue_backpressure_in_db(
    conn: &Connection,
    backpressure: &QueueBackpressureInsert<'_>,
) -> Result<QueueBackpressureStatus> {
    conn.execute(
        r#"
        INSERT INTO queue_backpressure (
            document_id,
            generation,
            command_kind,
            capacity_class,
            reason,
            dispatch_receipt_id,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            backpressure.document_id,
            backpressure
                .generation
                .map(|generation| sqlite_i64(generation, "queue backpressure generation"))
                .transpose()?,
            backpressure.command_kind,
            backpressure.capacity_class,
            backpressure.reason,
            backpressure
                .dispatch_receipt_id
                .map(|receipt_id| sqlite_i64(receipt_id, "queue backpressure dispatch receipt id"))
                .transpose()?,
            sqlite_i64(timestamp_secs(), "queue backpressure timestamp")?
        ],
    )?;
    let receipt_id = sqlite_u64(conn.last_insert_rowid(), "queue backpressure receipt id")?;
    Ok(QueueBackpressureStatus {
        receipt_id,
        document_id: backpressure.document_id.to_string(),
        generation: backpressure.generation,
        command_kind: backpressure.command_kind.to_string(),
        capacity_class: backpressure.capacity_class.to_string(),
        reason: backpressure.reason.to_string(),
        dispatch_receipt_id: backpressure.dispatch_receipt_id,
        timestamp: timestamp_secs(),
    })
}

pub fn upsert_document_cycle_state_in_db(
    conn: &Connection,
    document_id: &str,
    cycle_id: &str,
    state: &str,
    queue_head_id: Option<&str>,
    response_commit: Option<&str>,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO document_cycles (
            document_id,
            cycle_id,
            state,
            queue_head_id,
            response_commit,
            updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(document_id, cycle_id) DO UPDATE SET
            state = excluded.state,
            queue_head_id = excluded.queue_head_id,
            response_commit = excluded.response_commit,
            updated_at = excluded.updated_at
        "#,
        params![
            document_id,
            cycle_id,
            state,
            queue_head_id,
            response_commit,
            sqlite_i64(timestamp_secs(), "document cycle timestamp")?
        ],
    )?;
    Ok(())
}

pub fn commit_session_actor_closeout_in_db(
    conn: &mut Connection,
    closeout: &SessionActorCloseoutCommit<'_>,
) -> Result<()> {
    let now = sqlite_i64(timestamp_secs(), "session actor closeout timestamp")?;
    let tx = conn.transaction()?;

    if let Some(prompt) = closeout.queue_head_prompt {
        tx.execute(
            r#"
            INSERT INTO queue_heads (
                document_id,
                queue_name,
                head_id,
                prompt,
                state,
                selected_at,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
            ON CONFLICT(document_id, queue_name) DO UPDATE SET
                head_id = excluded.head_id,
                prompt = excluded.prompt,
                state = excluded.state,
                updated_at = excluded.updated_at
            "#,
            params![
                closeout.document_id,
                closeout.queue_name,
                closeout.queue_head_id,
                prompt,
                closeout.queue_head_state,
                now
            ],
        )?;
    }

    tx.execute(
        r#"
        INSERT INTO document_cycles (
            document_id,
            cycle_id,
            state,
            queue_head_id,
            response_commit,
            updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(document_id, cycle_id) DO UPDATE SET
            state = excluded.state,
            queue_head_id = excluded.queue_head_id,
            response_commit = excluded.response_commit,
            updated_at = excluded.updated_at
        "#,
        params![
            closeout.document_id,
            closeout.cycle_id,
            closeout.cycle_state,
            closeout.queue_head_id,
            closeout.response_commit,
            now
        ],
    )?;

    for mutation in &closeout.mutations {
        tx.execute(
            r#"
            INSERT INTO pending_mutations (
                document_id,
                cycle_id,
                item_id,
                mutation_kind,
                status,
                recorded_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(document_id, cycle_id, item_id, mutation_kind) DO UPDATE SET
                status = excluded.status,
                recorded_at = excluded.recorded_at
            "#,
            params![
                closeout.document_id,
                closeout.cycle_id,
                mutation.item_id,
                mutation.mutation_kind,
                mutation.status,
                now
            ],
        )?;
    }

    tx.commit()?;
    Ok(())
}

pub fn insert_admin_operation_in_db(
    conn: &Connection,
    operation_kind: &str,
    document_id: Option<&str>,
    status: &str,
    diagnostic_payload: Option<&str>,
) -> Result<u64> {
    conn.execute(
        r#"
        INSERT INTO admin_operations (
            operation_kind,
            document_id,
            status,
            diagnostic_payload,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        params![
            operation_kind,
            document_id,
            status,
            diagnostic_payload,
            sqlite_i64(timestamp_secs(), "admin operation timestamp")?
        ],
    )?;
    sqlite_u64(conn.last_insert_rowid(), "admin operation receipt id")
}

pub fn insert_crash_recovery_marker_in_db(
    conn: &Connection,
    marker_kind: &str,
    document_id: Option<&str>,
    generation: Option<u64>,
    status: &str,
    diagnostic_payload: Option<&str>,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO crash_recovery_markers (
            marker_kind,
            document_id,
            generation,
            status,
            diagnostic_payload,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![
            marker_kind,
            document_id,
            generation
                .map(|value| sqlite_i64(value, "crash recovery marker generation"))
                .transpose()?,
            status,
            diagnostic_payload,
            sqlite_i64(timestamp_secs(), "crash recovery marker timestamp")?
        ],
    )?;
    Ok(())
}

pub fn upsert_crash_recovery_marker_in_db(
    conn: &Connection,
    marker_kind: &str,
    dedupe_key: &str,
    document_id: Option<&str>,
    generation: Option<u64>,
    status: &str,
    diagnostic_payload: Option<&str>,
) -> Result<()> {
    let generation = generation
        .map(|value| sqlite_i64(value, "crash recovery marker generation"))
        .transpose()?;
    let timestamp = sqlite_i64(timestamp_secs(), "crash recovery marker timestamp")?;
    let updated = conn.execute(
        r#"
        UPDATE crash_recovery_markers
        SET document_id = ?3,
            generation = ?4,
            status = ?5,
            diagnostic_payload = ?6,
            timestamp = ?7
        WHERE marker_kind = ?1
          AND dedupe_key = ?2
        "#,
        params![
            marker_kind,
            dedupe_key,
            document_id,
            generation,
            status,
            diagnostic_payload,
            timestamp
        ],
    )?;
    if updated > 0 {
        return Ok(());
    }
    let inserted = conn.execute(
        r#"
        INSERT OR IGNORE INTO crash_recovery_markers (
            marker_kind,
            dedupe_key,
            document_id,
            generation,
            status,
            diagnostic_payload,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            marker_kind,
            dedupe_key,
            document_id,
            generation,
            status,
            diagnostic_payload,
            timestamp
        ],
    )?;
    if inserted == 0 {
        conn.execute(
            r#"
            UPDATE crash_recovery_markers
            SET document_id = ?3,
                generation = ?4,
                status = ?5,
                diagnostic_payload = ?6,
                timestamp = ?7
            WHERE marker_kind = ?1
              AND dedupe_key = ?2
            "#,
            params![
                marker_kind,
                dedupe_key,
                document_id,
                generation,
                status,
                diagnostic_payload,
                timestamp
            ],
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Controller bootstrap state.
// ---------------------------------------------------------------------------

pub fn load_controller_bootstrap_json_from_db(
    conn: &Connection,
    scope: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT bootstrap_json FROM controller_bootstrap WHERE scope = ?1",
        params![scope],
        |row| row.get(0),
    )
    .optional()
    .context("failed to load controller bootstrap from sqlite")
}

pub fn store_controller_bootstrap_json_in_db(
    conn: &Connection,
    scope: &str,
    bootstrap_json: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO controller_bootstrap (scope, bootstrap_json, updated_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(scope) DO UPDATE SET
            bootstrap_json = excluded.bootstrap_json,
            updated_at = excluded.updated_at
        "#,
        params![
            scope,
            bootstrap_json,
            sqlite_i64(timestamp_secs(), "controller bootstrap timestamp")?
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Layout state.
// ---------------------------------------------------------------------------

pub fn load_layout_state_from_db(conn: &Connection, scope: &str) -> Result<Vec<String>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT columns_json FROM layout_states WHERE scope = ?1",
            params![scope],
            |row| row.get(0),
        )
        .optional()?;
    match raw {
        Some(raw) => serde_json::from_str(&raw).context("failed to parse layout state from sqlite"),
        None => Ok(Vec::new()),
    }
}

pub fn store_layout_state_in_db(conn: &Connection, scope: &str, columns: &[String]) -> Result<()> {
    let columns_json = serde_json::to_string(columns)?;
    conn.execute(
        r#"
        INSERT INTO layout_states (scope, columns_json, updated_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(scope) DO UPDATE SET
            columns_json = excluded.columns_json,
            updated_at = excluded.updated_at
        "#,
        params![
            scope,
            columns_json,
            sqlite_i64(timestamp_secs(), "layout state timestamp")?
        ],
    )?;
    Ok(())
}

pub fn layout_scope_exists(conn: &Connection, scope: &str) -> Result<bool> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM layout_states WHERE scope = ?1",
            params![scope],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#adopenfast`: the second open of the same path must skip schema
    /// convergence, and a path this process has never converged must not
    /// inherit another path's memo.
    #[test]
    fn repeat_open_skips_schema_convergence_but_still_converges_each_new_path() -> Result<()> {
        // Unique temp paths are the cache key, so no global reset is needed —
        // and a reset would race the other tests in this process.
        let dir = tempfile::TempDir::new()?;
        let first = dir.path().join("first");
        let second = dir.path().join("second");

        let path = state_db_path(&first);
        assert!(!schema_already_converged(&path), "no memo before first open");
        let conn = open_state_db(&first)?;
        drop(conn);
        assert!(
            schema_already_converged(&path),
            "first open must record convergence for this path"
        );

        // A repeat open reuses the memo and must still hand back a usable,
        // fully-converged connection.
        let reopened = open_state_db(&first)?;
        let documents: i64 =
            reopened.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
        assert_eq!(documents, 0);

        // The memo covers the *shape* only. Row-level convergence must still
        // run on every open, because another process can append a retired event
        // variant after we first converged this path. Simulate that: rewind the
        // retirement migration and plant a legacy fact, then reopen.
        reopened.execute(
            "DELETE FROM state_schema_migrations WHERE migration_id = ?1",
            [RETIRE_PENDING_RESPONSE_FACTS_MIGRATION],
        )?;
        reopened.execute(
            "INSERT INTO state_events
                 (event_id, document_hash, domain, fact_type, payload_json, timestamp)
             VALUES ('legacy-1', 'doc', 'response', 'pending_response_captured', '{}', 0)",
            [],
        )?;
        drop(reopened);

        let after_reopen = open_state_db(&first)?;
        let legacy: i64 = after_reopen.query_row(
            "SELECT COUNT(*) FROM state_events WHERE fact_type = 'pending_response_captured'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            legacy, 0,
            "a memoized-shape reopen must still retire removed event variants"
        );
        drop(after_reopen);

        // A different database is a different key: it must converge on its own.
        assert!(!schema_already_converged(&state_db_path(&second)));
        let other = open_state_db(&second)?;
        let documents: i64 =
            other.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
        assert_eq!(documents, 0);
        assert!(schema_already_converged(&state_db_path(&second)));
        Ok(())
    }

    #[test]
    fn concurrent_state_db_schema_initializers_converge_without_replacing_authority() -> Result<()>
    {
        let dir = tempfile::TempDir::new()?;
        let project_root = std::sync::Arc::new(dir.path().to_path_buf());
        let workers = 16;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(workers));
        let handles = (0..workers)
            .map(|_| {
                let project_root = std::sync::Arc::clone(&project_root);
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || -> Result<()> {
                    barrier.wait();
                    let conn = open_state_db(&project_root)?;
                    let documents: i64 =
                        conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
                    anyhow::ensure!(documents == 0, "new state authority must be empty");
                    Ok(())
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle
                .join()
                .expect("concurrent state-db initializer must not panic")?;
        }

        assert!(state_db_path(&project_root).is_file());
        let state_files = std::fs::read_dir(project_root.join(".agent-doc"))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        assert!(
            state_files.iter().all(|name| {
                matches!(
                    name.to_str(),
                    Some("state.db" | "state.db-wal" | "state.db-shm")
                )
            }),
            "concurrent initialization must not replace or quarantine authority: {state_files:?}"
        );
        Ok(())
    }

    #[test]
    fn store_actor_record_waits_for_concurrent_writer_before_reading_cas_state() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let mut lock_conn = open_state_db(dir.path())?;
        let mut store_conn = open_state_db(dir.path())?;
        store_conn.busy_timeout(std::time::Duration::from_secs(2))?;

        let writer =
            lock_conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let record = ActorRecord {
            document_id: "tasks/concurrent-start.md".to_string(),
            session_id: "session-concurrent-start".to_string(),
            generation: 1,
            pane_id: "%41".to_string(),
            window_id: "@41".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: ActorLastTransition {
                caller: "test".to_string(),
                reason: "concurrent_start".to_string(),
                timestamp: timestamp_secs(),
                prior_generation: 0,
                new_generation: 1,
            },
        };
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let store = std::thread::spawn(move || -> Result<()> {
            started_tx
                .send(())
                .expect("start signal receiver remains live");
            store_actor_record_tx(&mut store_conn, None, &record, None, None)?;
            Ok(())
        });

        started_rx.recv()?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        writer.commit()?;
        store.join().expect("store thread must not panic")?;

        let verify = open_state_db(dir.path())?;
        assert!(load_actor_record_from_db(&verify, "tasks/concurrent-start.md")?.is_some());
        Ok(())
    }

    #[test]
    fn open_state_db_fails_closed_without_replacing_a_corrupt_authority() -> Result<()> {
        // state.db owns the state-machine ledger. A corrupt image must remain in
        // place for explicit recovery; normal opens cannot erase captured intent
        // by manufacturing an empty authority.
        let dir = tempfile::TempDir::new()?;
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc"))?;
        let path = state_db_path(root);
        let corrupt = b"this is not a sqlite database at all\n";
        std::fs::write(&path, corrupt)?;

        let err = open_state_db(root).unwrap_err().to_string();
        assert!(err.contains("refusing automatic replacement"), "{err}");
        assert_eq!(std::fs::read(&path)?, corrupt);
        assert_eq!(
            std::fs::read_dir(root.join(".agent-doc"))?.count(),
            1,
            "normal open must not create a replacement or quarantine sidecar"
        );
        Ok(())
    }

    #[test]
    fn open_state_db_transactionally_retires_removed_state_event_variants() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc"))?;
        let legacy = Connection::open(state_db_path(root))?;
        legacy.execute_batch(
            r#"
            CREATE TABLE state_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT NOT NULL UNIQUE,
                document_hash TEXT NOT NULL,
                domain TEXT NOT NULL,
                fact_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            INSERT INTO state_events (
                event_id, document_hash, domain, fact_type, payload_json, timestamp
            ) VALUES
                ('legacy-captured', 'doc-hash', 'closeout', 'pending_response_captured', '{}', 1),
                ('legacy-cleared', 'doc-hash', 'closeout', 'pending_response_cleared', '{}', 2),
                ('current-captured', 'doc-hash', 'closeout', 'response_captured', '{}', 3);
            "#,
        )?;
        drop(legacy);

        let conn = open_state_db(root)?;
        let remaining: Vec<String> = conn
            .prepare("SELECT fact_type FROM state_events ORDER BY id")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(remaining, vec!["response_captured"]);
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM state_schema_migrations WHERE migration_id = ?1",
                [RETIRE_PENDING_RESPONSE_FACTS_MIGRATION],
                |row| row.get::<_, i64>(0),
            )?,
            1
        );

        initialize_state_db(&conn)?;
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM state_events", [], |row| row
                .get::<_, i64>(0))?,
            1,
            "reopening the canonical schema must leave current facts intact"
        );
        Ok(())
    }

    #[test]
    fn session_actor_closeout_mutations_filter_empty_ids_and_preserve_status_order() {
        let pending_done_ids = vec!["done-1".to_string(), String::new(), "done-2".to_string()];
        let pending_gated_ids = vec![String::new(), "gated-1".to_string()];
        let pending_kept_open_ids = vec!["kept-1".to_string(), String::new()];
        let reaped_pending_ids = vec![String::new(), "reaped-1".to_string()];

        let mutations = session_actor_closeout_mutations(
            &pending_done_ids,
            &pending_gated_ids,
            &pending_kept_open_ids,
            &reaped_pending_ids,
        );

        assert_eq!(
            mutations,
            vec![
                SessionActorCloseoutMutation {
                    item_id: "done-1",
                    mutation_kind: "backlog_completion",
                    status: "done",
                },
                SessionActorCloseoutMutation {
                    item_id: "done-2",
                    mutation_kind: "backlog_completion",
                    status: "done",
                },
                SessionActorCloseoutMutation {
                    item_id: "gated-1",
                    mutation_kind: "backlog_completion",
                    status: "gated",
                },
                SessionActorCloseoutMutation {
                    item_id: "kept-1",
                    mutation_kind: "backlog_completion",
                    status: "kept_open",
                },
                SessionActorCloseoutMutation {
                    item_id: "reaped-1",
                    mutation_kind: "backlog_completion",
                    status: "reaped",
                },
            ]
        );
    }

    // `#dispatchgateleak`: an unproven dispatch row is only cleared by
    // `mark_open_dispatches_consumed` on an actor `Ready` transition, so a killed
    // pane / crashed harness leaves it open forever. Because the gate is
    // project-wide, ONE leaked row pinned the controller's idle self-recycle false
    // for every document — live repro 2026-07-18: 18 rows aged 34-50h kept a stale
    // `controller serve` alive across two days, so a freshly-installed binary could
    // never be promoted.
    #[test]
    fn stale_unproven_dispatch_does_not_pin_the_recycle_gate() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            r#"
            CREATE TABLE dispatch_attempts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id TEXT NOT NULL,
                generation INTEGER NOT NULL,
                command_kind TEXT NOT NULL,
                accepted_stage TEXT,
                failed_stage TEXT,
                diagnostic_payload TEXT,
                result_status TEXT,
                proof_scope TEXT,
                dispatch_start_proven INTEGER NOT NULL DEFAULT 0,
                timestamp INTEGER NOT NULL
            );
            "#,
        )?;
        let now = 1_000_000i64;
        let insert = |ts: i64| -> Result<()> {
            conn.execute(
                "INSERT INTO dispatch_attempts \
                 (document_id, generation, command_kind, result_status, dispatch_start_proven, timestamp) \
                 VALUES ('doc', 1, 'run', 'accepted', 0, ?1)",
                [ts],
            )?;
            Ok(())
        };

        // Leaked row well past the horizon must NOT pin the gate.
        insert(now - OPEN_DISPATCH_IN_FLIGHT_HORIZON_SECS - 1)?;
        assert!(
            !has_any_open_in_flight_dispatch_as_of(&conn, now)?,
            "a dispatch older than the in-flight horizon is leaked state, not in-flight work"
        );

        // A genuinely in-flight dispatch inside the horizon still pins it.
        insert(now - 5)?;
        assert!(
            has_any_open_in_flight_dispatch_as_of(&conn, now)?,
            "a recent unproven dispatch must still block the controller idle self-recycle"
        );
        Ok(())
    }

    // `#authorityfactretention`: `document_authority_observed` is pure telemetry
    // that only updates `latest_authority`. The hot read paths already exclude it,
    // but nothing bounded the TABLE — live repro 2026-07-18 had 170,215 of 176,268
    // rows (96.6%) as this fact, a 416MB table in a 477MB state.db.
    #[test]
    fn authority_observation_prune_caps_telemetry_and_keeps_durable_facts() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            r#"
            CREATE TABLE state_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT NOT NULL,
                document_hash TEXT NOT NULL,
                domain TEXT NOT NULL,
                fact_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            "#,
        )?;
        for i in 0..50 {
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, 'doc', 'document', 'document_authority_observed', '{}', 0)",
                [format!("authority-{i}")],
            )?;
        }
        // Durable lifecycle facts are interleaved and must survive untouched.
        for i in 0..3 {
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, 'doc', 'document', 'response_captured', '{}', 0)",
                [format!("captured-{i}")],
            )?;
        }

        prune_document_authority_observations_to(&conn, 10)?;

        let observations: i64 = conn.query_row(
            "SELECT COUNT(*) FROM state_events WHERE fact_type = 'document_authority_observed'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(observations, 10, "telemetry must be capped to the retention limit");

        let durable: i64 = conn.query_row(
            "SELECT COUNT(*) FROM state_events WHERE fact_type = 'response_captured'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(durable, 3, "retention must never drop durable lifecycle facts");

        // The NEWEST observations are the ones kept — `latest_authority` reads the tail.
        let newest: String = conn.query_row(
            "SELECT event_id FROM state_events \
             WHERE fact_type = 'document_authority_observed' ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(newest, "authority-49");

        // Idempotent: a second pass over an already-capped table is a no-op.
        prune_document_authority_observations_to(&conn, 10)?;
        let after: i64 = conn.query_row(
            "SELECT COUNT(*) FROM state_events WHERE fact_type = 'document_authority_observed'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(after, 10);
        Ok(())
    }

    #[test]
    fn ensure_column_tolerates_duplicate_column_after_race() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        conn.execute(
            "CREATE TABLE projection_diagnostics (id INTEGER PRIMARY KEY, intended_hash TEXT)",
            [],
        )?;
        let err = conn
            .execute(
                "ALTER TABLE projection_diagnostics ADD COLUMN intended_hash TEXT",
                [],
            )
            .unwrap_err();
        assert!(sqlite_duplicate_column_error(&err));
        ensure_column(
            &conn,
            "projection_diagnostics",
            "intended_hash",
            "intended_hash TEXT",
        )
    }

    #[test]
    fn crash_recovery_marker_upsert_dedupes_by_marker_kind_and_key() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;
        upsert_crash_recovery_marker_in_db(
            &conn,
            "dispatch_receipt_reconcile",
            "receipt:1",
            Some("tasks/doc.md"),
            Some(1),
            "retryable",
            Some("first"),
        )?;
        upsert_crash_recovery_marker_in_db(
            &conn,
            "dispatch_receipt_reconcile",
            "receipt:1",
            Some("tasks/doc.md"),
            Some(2),
            "blocked",
            Some("second"),
        )?;
        let row: (i64, i64, String, String) = conn.query_row(
            "SELECT COUNT(*), MAX(generation), MAX(status), MAX(diagnostic_payload) FROM crash_recovery_markers WHERE marker_kind = 'dispatch_receipt_reconcile'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(row, (1, 2, "blocked".to_string(), "second".to_string()));
        Ok(())
    }

    #[test]
    fn crash_recovery_markers_prune_caps_to_newest_rows() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;
        let base = sqlite_i64(timestamp_secs(), "base")?;
        // Seed 100 markers with strictly increasing timestamps (oldest..newest).
        for i in 0..100 {
            conn.execute(
                "INSERT INTO crash_recovery_markers (marker_kind, status, timestamp) VALUES ('dispatch_receipt_reconcile', 'seed', ?1)",
                [base + i],
            )?;
        }
        // Call the cap directly: the per-process once-guard in
        // `ensure_crash_recovery_marker_columns` may already be latched by an
        // earlier `open_state_db` in this test binary. Cap to the newest 10.
        prune_crash_recovery_markers_to(&conn, 10)?;
        let remaining: i64 =
            conn.query_row("SELECT COUNT(*) FROM crash_recovery_markers", [], |r| {
                r.get(0)
            })?;
        assert_eq!(remaining, 10, "only the newest `max_rows` markers survive");
        let oldest_kept: i64 = conn.query_row(
            "SELECT MIN(timestamp) FROM crash_recovery_markers",
            [],
            |r| r.get(0),
        )?;
        // Newest 10 of 0..100 are timestamps base+90 .. base+99.
        assert_eq!(oldest_kept, base + 90);
        // A second pass on an already-capped table is a no-op.
        prune_crash_recovery_markers_to(&conn, 10)?;
        let after: i64 =
            conn.query_row("SELECT COUNT(*) FROM crash_recovery_markers", [], |r| {
                r.get(0)
            })?;
        assert_eq!(after, 10);
        Ok(())
    }

    #[test]
    fn control_plane_store_counts_extended_fact_categories() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let mut conn = open_state_db(dir.path())?;
        let record = ActorRecord {
            document_id: "tasks/control-plane.md".to_string(),
            session_id: "session-control-plane".to_string(),
            generation: 1,
            pane_id: "%1".to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: ActorLastTransition {
                caller: "test".to_string(),
                reason: "store_actor_categories".to_string(),
                timestamp: timestamp_secs(),
                prior_generation: 0,
                new_generation: 1,
            },
        };

        store_actor_record_tx(
            &mut conn,
            None,
            &record,
            Some("managed".to_string()),
            Some(7),
        )?;
        upsert_supervisor_lease_in_db(
            &conn,
            &record,
            Some(1234),
            Some("supervisor.sock"),
            "ready",
        )?;
        assert!(insert_state_event_in_db(
            &conn,
            &StateEventInsert {
                event_id: "state-event-1",
                document_hash: &record.document_id,
                domain: "document",
                fact_type: "file_watch_change_observed",
                payload_json: r#"{"event_id":"state-event-1"}"#,
            },
        )?);
        assert!(!insert_state_event_in_db(
            &conn,
            &StateEventInsert {
                event_id: "state-event-1",
                document_hash: &record.document_id,
                domain: "document",
                fact_type: "file_watch_change_observed",
                payload_json: r#"{"event_id":"state-event-1"}"#,
            },
        )?);
        insert_dispatch_attempt_in_db(
            &conn,
            &DispatchAttemptInsert {
                document_id: &record.document_id,
                generation: record.generation,
                command_kind: "managed_reopen",
                accepted_stage: Some("accepted"),
                failed_stage: None,
                diagnostic_payload: "store actor count test",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )?;
        upsert_queue_head_in_db(
            &conn,
            &record.document_id,
            "agent:queue",
            Some("ctrlplane-storeactor"),
            "do [#ctrlplane-storeactor]",
            "selected",
        )?;
        let queue_head =
            load_queue_head_from_db(&conn, &record.document_id, "agent:queue")?.unwrap();
        assert_eq!(queue_head.generation, None);
        assert_eq!(queue_head.priority, 0);
        upsert_document_cycle_state_in_db(
            &conn,
            &record.document_id,
            "cycle-storeactor",
            "preflight_started",
            Some("ctrlplane-storeactor"),
            None,
        )?;
        commit_session_actor_closeout_in_db(
            &mut conn,
            &SessionActorCloseoutCommit {
                document_id: &record.document_id,
                cycle_id: "cycle-storeactor",
                cycle_state: "committed",
                queue_name: "agent:queue",
                queue_head_id: Some("ctrlplane-storeactor"),
                queue_head_prompt: Some("do [#ctrlplane-storeactor]"),
                queue_head_state: "consumed",
                response_commit: Some("commit-storeactor"),
                mutations: vec![SessionActorCloseoutMutation {
                    item_id: "ctrlplane-storeactor",
                    mutation_kind: "backlog_completion",
                    status: "done",
                }],
            },
        )?;
        insert_projection_diagnostic(
            &conn,
            "controller-state",
            &record.document_id,
            "diagnostic lag",
        )?;
        insert_admin_operation_in_db(
            &conn,
            "projection_repair",
            Some(&record.document_id),
            "accepted",
            Some("store actor count test"),
        )?;
        let queue_control = upsert_queue_control_in_db(
            &conn,
            &QueueControlInsert {
                scope_kind: "document",
                scope_id: &record.document_id,
                state: "paused",
                reason: Some("store actor count test"),
                operation_receipt_id: Some(1),
            },
        )?;
        insert_queue_backpressure_in_db(
            &conn,
            &QueueBackpressureInsert {
                document_id: &record.document_id,
                generation: Some(record.generation),
                command_kind: "managed_reopen",
                capacity_class: "queue_paused",
                reason: "store actor count test",
                dispatch_receipt_id: Some(queue_control.receipt_id),
            },
        )?;
        insert_crash_recovery_marker_in_db(
            &conn,
            "startup_reconcile",
            Some(&record.document_id),
            Some(record.generation),
            "pending",
            Some("store actor count test"),
        )?;
        store_layout_state_in_db(&conn, "default", &["%1".to_string()])?;

        let counts = load_control_plane_store_counts(&conn)?;
        assert_eq!(counts.actor_documents, 1);
        assert_eq!(counts.live_actor_documents, 1);
        assert_eq!(counts.actor_transitions, 1);
        assert_eq!(counts.supervisor_leases, 1);
        assert_eq!(counts.state_events, 1);
        assert_eq!(counts.dispatch_receipts, 1);
        assert_eq!(counts.queue_heads, 1);
        assert_eq!(counts.document_cycles, 1);
        assert_eq!(counts.pending_mutations, 1);
        assert_eq!(counts.projection_diagnostics, 1);
        assert_eq!(counts.admin_operations, 1);
        assert_eq!(counts.queue_controls, 1);
        assert_eq!(counts.queue_backpressure, 1);
        assert_eq!(counts.crash_recovery_markers, 1);
        assert_eq!(counts.layout_states, 1);
        assert_eq!(counts.total_authoritative_rows(), 14);

        let state_events = load_state_events_from_db(&conn, Some(&record.document_id))?;
        assert_eq!(state_events.len(), 1);
        assert_eq!(state_events[0].event_id, "state-event-1");
        assert_eq!(state_events[0].domain, "document");
        assert_eq!(state_events[0].fact_type, "file_watch_change_observed");

        Ok(())
    }

    #[test]
    fn control_plane_store_counts_use_marker_high_water_mark() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        initialize_state_db(&conn)?;
        insert_crash_recovery_marker_in_db(
            &conn,
            "startup_reconcile",
            Some("doc.md"),
            Some(1),
            "pending",
            Some("first marker"),
        )?;
        insert_crash_recovery_marker_in_db(
            &conn,
            "watchdog_restart",
            Some("doc.md"),
            Some(2),
            "pending",
            Some("second marker"),
        )?;
        conn.execute("DELETE FROM crash_recovery_markers WHERE id = 1", [])?;

        let counts = load_control_plane_store_counts(&conn)?;
        assert_eq!(
            counts.crash_recovery_markers, 2,
            "controller status should use the marker high-water mark instead of an exact full-table scan"
        );

        Ok(())
    }

    #[test]
    fn cycle_projection_event_load_excludes_authority_observations() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        initialize_state_db(&conn)?;
        insert_state_event_in_db(
            &conn,
            &StateEventInsert {
                event_id: "authority-1",
                document_hash: "doc-hash",
                domain: "document",
                fact_type: "document_authority_observed",
                payload_json: r#"{"event_id":"authority-1","fact":{"type":"document_authority_observed"}}"#,
            },
        )?;
        insert_state_event_in_db(
            &conn,
            &StateEventInsert {
                event_id: "closeout-1",
                document_hash: "doc-hash",
                domain: "closeout",
                fact_type: "commit_observed",
                payload_json: r#"{"event_id":"closeout-1","fact":{"type":"commit_observed"}}"#,
            },
        )?;

        let all = load_state_events_from_db(&conn, Some("doc-hash"))?;
        assert_eq!(all.len(), 2);
        let projected = load_state_events_for_cycle_projection_from_db(&conn, "doc-hash")?;
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].event_id, "closeout-1");
        Ok(())
    }

    /// `#crdtcheckpointretention`: superseded CRDT recovery checkpoints are the
    /// largest consumer of the ledger (measured: 564 rows / 405MB of a 512MB
    /// database, ~736KB each). They are safe to drop because
    /// `apply_fact` ASSIGNS `crdt_recovery_projection` wholesale rather than
    /// accumulating, so replaying the newest per document is equivalent.
    #[test]
    fn superseded_crdt_recovery_checkpoints_are_pruned_per_document() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;

        // Two documents with different checkpoint counts, plus an interleaved
        // durable fact that must survive untouched.
        for i in 0..6 {
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, 'docA', 'document', 'crdt_recovery_projection_checkpointed', '{}', 0)",
                [format!("a-ckpt-{i}")],
            )?;
        }
        for i in 0..3 {
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, 'docB', 'document', 'crdt_recovery_projection_checkpointed', '{}', 0)",
                [format!("b-ckpt-{i}")],
            )?;
        }
        conn.execute(
            "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
             VALUES ('keep-me', 'docA', 'document', 'response_captured', '{}', 0)",
            [],
        )?;

        prune_superseding_fact_to(&conn, "crdt_recovery_projection_checkpointed", 2)?;

        let count_for = |doc: &str| -> Result<i64> {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM state_events \
                 WHERE fact_type = 'crdt_recovery_projection_checkpointed' AND document_hash = ?1",
                [doc],
                |row| row.get(0),
            )?)
        };
        assert_eq!(count_for("docA")?, 2, "each document keeps its own newest N");
        assert_eq!(
            count_for("docB")?,
            2,
            "a quiet document must not be evicted by a busy one — the cap is per document"
        );

        // The survivors must be the NEWEST, since recovery elects the latest.
        let newest: String = conn.query_row(
            "SELECT event_id FROM state_events \
             WHERE fact_type = 'crdt_recovery_projection_checkpointed' AND document_hash = 'docA' \
             ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(newest, "a-ckpt-5", "the newest checkpoint must survive");

        let durable: i64 = conn.query_row(
            "SELECT COUNT(*) FROM state_events WHERE fact_type = 'response_captured'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(durable, 1, "durable lifecycle facts must survive untouched");
        Ok(())
    }

    /// `#baselinefactretention`: `document_baseline_checkpointed` assigns
    /// `merge_baseline` wholesale, so only the newest per document is ever read.
    #[test]
    fn superseded_document_baselines_are_pruned_per_document() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;

        for i in 0..6 {
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, 'docA', 'document', 'document_baseline_checkpointed', '{}', 0)",
                [format!("a-base-{i}")],
            )?;
        }
        conn.execute(
            "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
             VALUES ('b-base-0', 'docB', 'document', 'document_baseline_checkpointed', '{}', 0)",
            [],
        )?;

        prune_superseding_fact_to(&conn, "document_baseline_checkpointed", 2)?;

        let count_for = |doc: &str| -> Result<i64> {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM state_events \
                 WHERE fact_type = 'document_baseline_checkpointed' AND document_hash = ?1",
                [doc],
                |row| row.get(0),
            )?)
        };
        assert_eq!(count_for("docA")?, 2, "each document keeps its own newest N");
        assert_eq!(
            count_for("docB")?,
            1,
            "a quiet document below the cap is untouched by a busy one"
        );

        let newest: String = conn.query_row(
            "SELECT event_id FROM state_events \
             WHERE fact_type = 'document_baseline_checkpointed' AND document_hash = 'docA' \
             ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(newest, "a-base-5", "the newest baseline must survive");
        Ok(())
    }

    /// `#responsecaptureretention`: `response_captured` embeds the response
    /// body, the replayable intent body, and the editor-visible baseline, and
    /// `apply_fact` assigns all of them wholesale. Only the newest few per
    /// document can ever be elected.
    #[test]
    fn superseded_response_captures_are_pruned_per_document() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;

        for i in 0..8 {
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, 'docA', 'closeout', 'response_captured', '{}', 0)",
                [format!("a-cap-{i}")],
            )?;
        }
        conn.execute(
            "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
             VALUES ('b-cap-0', 'docB', 'closeout', 'response_captured', '{}', 0)",
            [],
        )?;

        prune_superseding_fact_to(&conn, "response_captured", RESPONSE_CAPTURES_KEPT_PER_DOCUMENT)?;

        let count_for = |doc: &str| -> Result<i64> {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM state_events \
                 WHERE fact_type = 'response_captured' AND document_hash = ?1",
                [doc],
                |row| row.get(0),
            )?)
        };
        assert_eq!(
            count_for("docA")?,
            RESPONSE_CAPTURES_KEPT_PER_DOCUMENT,
            "the open cycle's capture plus recovery headroom survives"
        );
        assert_eq!(
            count_for("docB")?,
            1,
            "a quiet document below the cap is untouched by a busy one"
        );

        let newest: String = conn.query_row(
            "SELECT event_id FROM state_events \
             WHERE fact_type = 'response_captured' AND document_hash = 'docA' \
             ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(newest, "a-cap-7", "the newest capture must survive");
        Ok(())
    }

    /// `#turnintentretention`: mid-turn intent checkpoints supersede each other
    /// within a turn, so they are the highest-row-count fact after the authority
    /// observations and only the newest is ever elected.
    #[test]
    fn superseded_turn_intent_checkpoints_are_pruned_per_document() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;

        for i in 0..5 {
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, 'docA', 'closeout', 'turn_intent_checkpointed', '{}', 0)",
                [format!("a-turn-{i}")],
            )?;
        }

        prune_superseding_fact_to(
            &conn,
            "turn_intent_checkpointed",
            TURN_INTENT_CHECKPOINTS_KEPT_PER_DOCUMENT,
        )?;

        let remaining: Vec<String> = conn
            .prepare(
                "SELECT event_id FROM state_events \
                 WHERE fact_type = 'turn_intent_checkpointed' ORDER BY id",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            remaining,
            vec!["a-turn-3".to_string(), "a-turn-4".to_string()],
            "only the newest checkpoints survive, newest last"
        );
        Ok(())
    }

    /// `#visiblecandidateretention`: `observe_commit_candidate` maintains a MAP
    /// keyed by `commit_candidate_hash`, so several candidates stay live for one
    /// document at once. Retention must mirror that map — keep the highest
    /// `model_revision` per key (last replayed on a tie) — and must NOT evict by
    /// per-document recency, which would drop a still-electable candidate.
    #[test]
    fn superseded_visible_write_commit_candidates_are_pruned_per_candidate_hash() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;

        let observe = |event_id: &str, doc: &str, candidate: &str, revision: u64| {
            let payload = format!(
                r#"{{"fact":{{"commit_candidate_hash":"{candidate}","model_revision":{revision}}}}}"#
            );
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, ?2, 'document', 'visible_write_commit_candidate_observed', ?3, 0)",
                rusqlite::params![event_id, doc, payload],
            )
        };

        // One candidate observed three times at rising revisions: only the
        // newest revision survives.
        observe("a-c1-r1", "docA", "cand-1", 1)?;
        observe("a-c1-r2", "docA", "cand-1", 2)?;
        observe("a-c1-r3", "docA", "cand-1", 3)?;
        // A SECOND candidate for the same document, at a LOWER revision than
        // cand-1's newest. Per-document recency would evict it; the map keeps it.
        observe("a-c2-r1", "docA", "cand-2", 1)?;
        // Tie on revision: the last row replayed wins.
        observe("a-c3-r5-old", "docA", "cand-3", 5)?;
        observe("a-c3-r5-new", "docA", "cand-3", 5)?;
        // A quiet second document is untouched.
        observe("b-c1-r1", "docB", "cand-1", 1)?;

        prune_superseded_visible_write_commit_candidates(&conn)?;

        let remaining: Vec<String> = conn
            .prepare(
                "SELECT event_id FROM state_events \
                 WHERE fact_type = 'visible_write_commit_candidate_observed' ORDER BY id",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            remaining,
            vec![
                "a-c1-r3".to_string(),
                "a-c2-r1".to_string(),
                "a-c3-r5-new".to_string(),
                "b-c1-r1".to_string(),
            ],
            "one row survives per (document, commit_candidate_hash): highest revision, last on a tie"
        );
        Ok(())
    }

    /// `#canonicalschema`: the canonical `CREATE TABLE` statements must already
    /// declare every column in `CANONICAL_ADDED_COLUMNS`, so a freshly created
    /// database never depends on the `ALTER TABLE` convergence path to be
    /// correct.
    ///
    /// This is the assertion that makes the drift unrepeatable. Before the
    /// canonical list existed, four of the twelve added columns were missing
    /// from their own `CREATE TABLE` and only ever arrived via `ALTER` —
    /// silently, because both paths ran on every open.
    #[test]
    fn canonical_schema_declares_every_added_column() -> Result<()> {
        // Run ONLY the `CREATE TABLE` path. Asserting against a fully
        // initialized database would prove nothing: convergence would already
        // have added any missing column by `ALTER`, and SQLite rewrites
        // `sqlite_master.sql` on `ADD COLUMN`, so even the recorded DDL would
        // look correct. The declaration is only testable in isolation.
        let conn = Connection::open_in_memory()?;
        create_canonical_tables(&conn)?;

        for (table, column, _) in CANONICAL_ADDED_COLUMNS {
            assert!(
                column_exists(&conn, table, column)?,
                "{table}.{column} is in CANONICAL_ADDED_COLUMNS but missing from the \
                 canonical CREATE TABLE, so a fresh database would only get it through \
                 the ALTER convergence path — exactly the drift this list exists to prevent"
            );
        }
        Ok(())
    }

    /// `#canonicalschema`: convergence must be a no-op on a fresh database. A
    /// column that only ever arrives by `ALTER` is drift even when the end state
    /// is correct, because the declared schema stops describing reality.
    #[test]
    fn converging_a_fresh_canonical_database_changes_nothing() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        create_canonical_tables(&conn)?;

        let columns_of = |table: &str| -> Result<Vec<String>> {
            Ok(conn
                .prepare(&format!("PRAGMA table_info({table})"))?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<std::result::Result<_, _>>()?)
        };
        let tables: Vec<&str> = {
            let mut seen: Vec<&str> = CANONICAL_ADDED_COLUMNS
                .iter()
                .map(|(table, _, _)| *table)
                .collect();
            seen.dedup();
            seen
        };
        let before: Vec<Vec<String>> = tables
            .iter()
            .map(|table| columns_of(table))
            .collect::<Result<_>>()?;

        converge_added_columns(&conn)?;

        let after: Vec<Vec<String>> = tables
            .iter()
            .map(|table| columns_of(table))
            .collect::<Result<_>>()?;
        assert_eq!(
            before, after,
            "convergence added a column the canonical CREATE TABLE should already declare"
        );
        Ok(())
    }

    /// `#retentionversion`: every append gets the next per-document version, and
    /// documents advance independently — a busy document must not push a quiet
    /// one's next version forward, or a peer watermark keyed on it would skip
    /// rows the quiet document never produced.
    #[test]
    fn document_version_is_assigned_monotonically_per_document() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;

        let append = |event_id: &str, doc: &str| -> Result<()> {
            insert_state_event_in_db(
                &conn,
                &StateEventInsert {
                    event_id,
                    document_hash: doc,
                    domain: "document",
                    fact_type: "write_applied",
                    payload_json: "{}",
                },
            )?;
            Ok(())
        };
        append("a-1", "docA")?;
        append("b-1", "docB")?;
        append("a-2", "docA")?;
        append("a-3", "docA")?;
        append("b-2", "docB")?;

        let versions: Vec<(String, i64)> = conn
            .prepare("SELECT event_id, document_version FROM state_events ORDER BY event_id")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            versions,
            vec![
                ("a-1".to_string(), 1),
                ("a-2".to_string(), 2),
                ("a-3".to_string(), 3),
                ("b-1".to_string(), 1),
                ("b-2".to_string(), 2),
            ],
            "each document numbers its own appends from 1, independently of other documents"
        );

        // `INSERT OR IGNORE` on a duplicate `event_id` must not advance the
        // document's version — a burned version would leave a hole a watermark
        // could never reach.
        append("a-3", "docA")?;
        let highest: i64 = conn.query_row(
            "SELECT MAX(document_version) FROM state_events WHERE document_hash = 'docA'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(highest, 3, "an ignored duplicate does not burn a version");
        Ok(())
    }

    /// `#retentionversion`: the backfill numbers pre-existing rows in `id` order
    /// per document — the same order the projection replays them in — so
    /// migrating an existing ledger cannot reorder history.
    #[test]
    fn document_version_backfill_follows_existing_replay_order() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        std::fs::create_dir_all(dir.path().join(".agent-doc"))?;
        let legacy = Connection::open(state_db_path(dir.path()))?;
        legacy.execute_batch(
            r#"
            CREATE TABLE state_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT NOT NULL UNIQUE,
                document_hash TEXT NOT NULL,
                domain TEXT NOT NULL,
                fact_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) VALUES
                ('a-old', 'docA', 'document', 'write_applied', '{}', 1),
                ('b-old', 'docB', 'document', 'write_applied', '{}', 2),
                ('a-mid', 'docA', 'document', 'write_applied', '{}', 3),
                ('a-new', 'docA', 'document', 'write_applied', '{}', 4);
            "#,
        )?;
        drop(legacy);

        let conn = open_state_db(dir.path())?;
        let versions: Vec<(String, i64)> = conn
            .prepare("SELECT event_id, document_version FROM state_events ORDER BY id")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            versions,
            vec![
                ("a-old".to_string(), 1),
                ("b-old".to_string(), 1),
                ("a-mid".to_string(), 2),
                ("a-new".to_string(), 3),
            ],
            "backfilled versions follow id order within each document"
        );

        // A post-migration append continues from the backfilled high-water mark
        // rather than restarting at 1.
        insert_state_event_in_db(
            &conn,
            &StateEventInsert {
                event_id: "a-fresh",
                document_hash: "docA",
                domain: "document",
                fact_type: "write_applied",
                payload_json: "{}",
            },
        )?;
        let fresh: i64 = conn.query_row(
            "SELECT document_version FROM state_events WHERE event_id = 'a-fresh'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(fresh, 4, "appends resume above the backfilled high-water mark");
        Ok(())
    }

    /// `#retentionversion`: convergence must repair rows that arrive AFTER the
    /// first pass. This is what a one-shot migration could not do — a still
    /// running older binary kept appending events at the column's `DEFAULT 0`
    /// (55 such rows observed live, newer than the migration record itself), and
    /// a watermark would have read every one as version 0 and deleted them.
    #[test]
    fn document_version_convergence_repairs_stragglers_from_an_older_writer() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;
        for event_id in ["a-1", "a-2"] {
            insert_state_event_in_db(
                &conn,
                &StateEventInsert {
                    event_id,
                    document_hash: "docA",
                    domain: "document",
                    fact_type: "write_applied",
                    payload_json: "{}",
                },
            )?;
        }

        // An older binary appends without knowing about the column, so it lands
        // at the `DEFAULT 0`.
        conn.execute(
            "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
             VALUES ('a-legacy-1', 'docA', 'document', 'write_applied', '{}', 0)",
            [],
        )?;
        conn.execute(
            "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
             VALUES ('a-legacy-2', 'docA', 'document', 'write_applied', '{}', 0)",
            [],
        )?;

        converge_state_event_document_versions(&conn)?;

        let versions: Vec<(String, i64)> = conn
            .prepare("SELECT event_id, document_version FROM state_events ORDER BY id")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            versions,
            vec![
                ("a-1".to_string(), 1),
                ("a-2".to_string(), 2),
                ("a-legacy-1".to_string(), 3),
                ("a-legacy-2".to_string(), 4),
            ],
            "stragglers are numbered in id order ABOVE the document's high-water mark, \
             never left at 0 where a watermark would treat them as already superseded"
        );

        // Idempotent: a second pass with nothing to repair changes nothing.
        converge_state_event_document_versions(&conn)?;
        let after: Vec<(String, i64)> = conn
            .prepare("SELECT event_id, document_version FROM state_events ORDER BY id")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(versions, after, "convergence is idempotent");
        Ok(())
    }

    /// `#retentionperiodic`: the interval stamp is keyed by database path. A
    /// single process opens several projects' `state.db` (the cross-project
    /// controller sweeps walk every root), so a process-global stamp would let
    /// the first project consume the window and leave every other project
    /// unpruned — the same one-prune-per-process gap, sliced by project.
    #[test]
    fn retention_interval_is_tracked_per_database_not_per_process() -> Result<()> {
        // Build one over-cap database, then clone the FILE into two fresh
        // project roots. Retention runs on open, so the rows must already exist
        // before the first `open_state_db` for each path.
        let seed = tempfile::TempDir::new()?;
        {
            let conn = open_state_db(seed.path())?;
            for i in 0..(DOCUMENT_AUTHORITY_OBSERVED_MAX_ROWS + 25) {
                conn.execute(
                    "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                     VALUES (?1, 'docA', 'document', 'document_authority_observed', '{}', 0)",
                    [format!("obs-{i}")],
                )?;
            }
        }

        let clone_seed = |root: &Path| -> Result<()> {
            let target = state_db_path(root);
            std::fs::create_dir_all(target.parent().expect("state db has a parent"))?;
            std::fs::copy(state_db_path(seed.path()), &target)?;
            Ok(())
        };
        let observations = |conn: &Connection| -> Result<i64> {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM state_events WHERE fact_type = 'document_authority_observed'",
                [],
                |row| row.get(0),
            )?)
        };

        let first = tempfile::TempDir::new()?;
        let second = tempfile::TempDir::new()?;
        clone_seed(first.path())?;
        clone_seed(second.path())?;

        // Both opens land well inside one interval. Per-database keying means the
        // SECOND project still prunes rather than inheriting the first's
        // freshly-claimed window.
        let first_conn = open_state_db(first.path())?;
        assert_eq!(
            observations(&first_conn)?,
            DOCUMENT_AUTHORITY_OBSERVED_MAX_ROWS,
            "the first database is pruned to its cap on open"
        );
        let second_conn = open_state_db(second.path())?;
        assert_eq!(
            observations(&second_conn)?,
            DOCUMENT_AUTHORITY_OBSERVED_MAX_ROWS,
            "a second database opened in the same process must not inherit the first's interval window"
        );
        Ok(())
    }

    /// `#retentionperiodic`: retention used to latch once per process, so a
    /// long-lived `controller serve` pruned at startup and then accumulated
    /// forever. The first pass is always due; later passes wait out the interval.
    #[test]
    fn state_event_retention_is_due_first_then_only_after_the_interval() {
        let interval = Duration::from_secs(900);
        assert!(
            state_event_retention_due(u64::MAX, 0, interval),
            "the first open in a process must always prune"
        );
        assert!(
            !state_event_retention_due(1_000, 1_000 + 899_999, interval),
            "a per-RPC open inside the interval must not rescan state_events"
        );
        assert!(
            state_event_retention_due(1_000, 1_000 + 900_000, interval),
            "a long-lived process must re-prune once the interval elapses"
        );
        assert!(
            state_event_retention_due(5_000, 1_000, interval),
            "a non-monotonic clock is treated as due rather than deferring forever"
        );
    }

    /// `#deferredintentretention`: the agent-owned deferral lineage ACCUMULATES
    /// into `pending_write_journal`, so retention must follow the projection's
    /// own `drain(..=index)` rule rather than a keep-newest-N cap. Unconverged
    /// intents and the independent external-disk lineage must both survive.
    #[test]
    fn converged_document_write_intents_are_pruned_but_live_intents_survive() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;

        let deferred = |event_id: &str, doc: &str, intent: &str, target: &str, reason: &str| {
            let payload = format!(
                r#"{{"fact":{{"intent_id":"{intent}","target_hash":"{target}","reason":"{reason}"}}}}"#
            );
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, ?2, 'document', 'document_write_deferred', ?3, 0)",
                rusqlite::params![event_id, doc, payload],
            )
        };
        let converged = |event_id: &str, doc: &str, intent: &str, target: &str| {
            let payload =
                format!(r#"{{"fact":{{"intent_id":"{intent}","target_hash":"{target}"}}}}"#);
            conn.execute(
                "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
                 VALUES (?1, ?2, 'document', 'document_write_converged', ?3, 0)",
                rusqlite::params![event_id, doc, payload],
            )
        };

        // docA: i1 and i2 both deferred; only i2 later converges. `drain(..=i2)`
        // settles the whole prefix, so BOTH die. i3 raced ahead of the ACK and
        // must survive.
        deferred("a-d1", "docA", "i1", "t1", "crdt_delivery_ack_pending")?;
        deferred("a-d2", "docA", "i2", "t2", "crdt_delivery_ack_pending")?;
        converged("a-c2", "docA", "i2", "t2")?;
        deferred("a-d3", "docA", "i3", "t3", "crdt_delivery_ack_pending")?;
        // The external-disk lineage is independent — never drained by an
        // agent-lineage convergence, even though it is older than a-c2.
        deferred("a-ext", "docA", "iext", "text", EXTERNAL_DISK_DEFERRAL_REASON)?;
        // docB never converges anything: a busy neighbour must not drain it.
        deferred("b-d1", "docB", "j1", "u1", "crdt_delivery_ack_pending")?;

        prune_converged_document_write_intents(&conn)?;

        let mut survivors: Vec<String> = conn
            .prepare(
                "SELECT event_id FROM state_events \
                 WHERE fact_type = 'document_write_deferred' ORDER BY event_id",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        survivors.sort();
        assert_eq!(
            survivors,
            vec![
                "a-d3".to_string(),
                "a-ext".to_string(),
                "b-d1".to_string()
            ],
            "converged prefix drops; unconverged, external-disk, and other documents survive"
        );

        // A converged row that matches on intent but NOT on target_hash is not
        // the one the projection drained, so it must not license a delete.
        deferred("c-d1", "docC", "k1", "v1", "crdt_delivery_ack_pending")?;
        converged("c-c1", "docC", "k1", "v-other")?;
        prune_converged_document_write_intents(&conn)?;
        let doc_c: i64 = conn.query_row(
            "SELECT COUNT(*) FROM state_events \
             WHERE fact_type = 'document_write_deferred' AND document_hash = 'docC'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            doc_c, 1,
            "convergence must match (intent_id, target_hash) exactly, as the projection does"
        );
        Ok(())
    }

    #[test]
    fn crdt_recovery_checkpoint_prune_is_a_no_op_below_the_cap() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        let conn = open_state_db(dir.path())?;
        conn.execute(
            "INSERT INTO state_events (event_id, document_hash, domain, fact_type, payload_json, timestamp) \
             VALUES ('only', 'docA', 'document', 'crdt_recovery_projection_checkpointed', '{}', 0)",
            [],
        )?;
        prune_superseding_fact_to(&conn, "crdt_recovery_projection_checkpointed", 2)?;
        let remaining: i64 = conn.query_row(
            "SELECT COUNT(*) FROM state_events WHERE fact_type = 'crdt_recovery_projection_checkpointed'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(remaining, 1, "a lone checkpoint must never be pruned");
        Ok(())
    }

}
