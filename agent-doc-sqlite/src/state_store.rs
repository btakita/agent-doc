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
use std::sync::atomic::{AtomicBool, Ordering};
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

fn open_and_init_state_db(path: &Path) -> Result<Connection> {
    let conn =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    conn.busy_timeout(STATE_DB_BUSY_TIMEOUT)?;
    let started = Instant::now();
    loop {
        match initialize_state_db(&conn) {
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
    conn.execute_batch(
        r#"
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
            timestamp INTEGER NOT NULL
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
            timestamp INTEGER NOT NULL
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
    ensure_dispatch_attempt_receipt_columns(conn)?;
    ensure_projection_diagnostic_columns(conn)?;
    ensure_queue_head_columns(conn)?;
    ensure_crash_recovery_marker_columns(conn)?;
    prune_document_authority_observations_once(conn);
    prune_superseded_crdt_recovery_checkpoints_once(conn);
    prune_superseded_document_baselines_once(conn);
    prune_converged_document_write_intents_once(conn);
    retire_removed_state_event_variants(conn)?;
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

/// Guards the once-per-process authority-observation prune so the per-request
/// `open_state_db` path does not rescan `state_events` on every RPC.
static DOCUMENT_AUTHORITY_OBSERVATIONS_PRUNED: AtomicBool = AtomicBool::new(false);

/// Prune the authority-observation ledger once per process, best-effort.
///
/// Retention is maintenance, never a reason to fail state-db open; a failure
/// unlatches the guard so a later open retries instead of giving up for the
/// lifetime of the process.
fn prune_document_authority_observations_once(conn: &Connection) {
    if DOCUMENT_AUTHORITY_OBSERVATIONS_PRUNED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Err(err) =
        prune_document_authority_observations_to(conn, DOCUMENT_AUTHORITY_OBSERVED_MAX_ROWS)
    {
        DOCUMENT_AUTHORITY_OBSERVATIONS_PRUNED.store(false, Ordering::Relaxed);
        eprintln!("[agent-doc] warning: failed to prune document_authority_observed events: {err:#}");
    }
}

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

/// Guards the once-per-process checkpoint prune so the per-request
/// `open_state_db` path does not rescan `state_events` on every RPC.
static CRDT_RECOVERY_CHECKPOINTS_PRUNED: AtomicBool = AtomicBool::new(false);

/// Prune superseded CRDT recovery checkpoints once per process, best-effort.
///
/// Retention is maintenance, never a reason to fail state-db open; a failure
/// unlatches the guard so a later open retries instead of giving up for the
/// lifetime of the process.
fn prune_superseded_crdt_recovery_checkpoints_once(conn: &Connection) {
    if CRDT_RECOVERY_CHECKPOINTS_PRUNED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Err(err) = prune_superseded_crdt_recovery_checkpoints_to(
        conn,
        CRDT_RECOVERY_CHECKPOINTS_KEPT_PER_DOCUMENT,
    ) {
        CRDT_RECOVERY_CHECKPOINTS_PRUNED.store(false, Ordering::Relaxed);
        eprintln!(
            "[agent-doc] warning: failed to prune superseded crdt_recovery_projection_checkpointed events: {err:#}"
        );
    }
}

fn prune_superseded_crdt_recovery_checkpoints_to(
    conn: &Connection,
    keep_per_document: i64,
) -> Result<()> {
    if keep_per_document < 1 {
        return Ok(());
    }
    // A row is superseded when at least `keep_per_document` NEWER checkpoints
    // exist for the same document. The correlated count is driven by
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
                    WHERE e.fact_type = 'crdt_recovery_projection_checkpointed'
                      AND (
                        SELECT COUNT(*) FROM state_events n
                        WHERE n.fact_type = 'crdt_recovery_projection_checkpointed'
                          AND n.document_hash = e.document_hash
                          AND n.id > e.id
                      ) >= ?1
                    LIMIT 2000
                )
                "#,
                [keep_per_document],
            )
            .context("failed to prune superseded crdt recovery checkpoints")?;
        if deleted == 0 {
            break;
        }
    }
    Ok(())
}

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

/// Guards the once-per-process baseline prune so the per-request
/// `open_state_db` path does not rescan `state_events` on every RPC.
static DOCUMENT_BASELINES_PRUNED: AtomicBool = AtomicBool::new(false);

/// Prune superseded document baselines once per process, best-effort.
///
/// Retention is maintenance, never a reason to fail state-db open; a failure
/// unlatches the guard so a later open retries instead of giving up for the
/// lifetime of the process.
fn prune_superseded_document_baselines_once(conn: &Connection) {
    if DOCUMENT_BASELINES_PRUNED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Err(err) =
        prune_superseded_document_baselines_to(conn, DOCUMENT_BASELINES_KEPT_PER_DOCUMENT)
    {
        DOCUMENT_BASELINES_PRUNED.store(false, Ordering::Relaxed);
        eprintln!(
            "[agent-doc] warning: failed to prune superseded document_baseline_checkpointed events: {err:#}"
        );
    }
}

fn prune_superseded_document_baselines_to(conn: &Connection, keep_per_document: i64) -> Result<()> {
    if keep_per_document < 1 {
        return Ok(());
    }
    // Identical shape to `prune_superseded_crdt_recovery_checkpoints_to`: a row
    // is superseded once `keep_per_document` NEWER baselines exist for the same
    // document. Driven by `state_events_document_hash_fact_type_id`, so this
    // needs no additional index. Bounded batches keep a legacy backlog cleanup
    // from ballooning one WAL frame or holding the write lock for a whole scan.
    loop {
        let deleted = conn
            .execute(
                r#"
                DELETE FROM state_events
                WHERE rowid IN (
                    SELECT e.rowid FROM state_events e
                    WHERE e.fact_type = 'document_baseline_checkpointed'
                      AND (
                        SELECT COUNT(*) FROM state_events n
                        WHERE n.fact_type = 'document_baseline_checkpointed'
                          AND n.document_hash = e.document_hash
                          AND n.id > e.id
                      ) >= ?1
                    LIMIT 2000
                )
                "#,
                [keep_per_document],
            )
            .context("failed to prune superseded document baselines")?;
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

/// Guards the once-per-process converged-write-intent prune so the per-request
/// `open_state_db` path does not rescan `state_events` on every RPC.
static CONVERGED_WRITE_INTENTS_PRUNED: AtomicBool = AtomicBool::new(false);

/// Prune converged document write intents once per process, best-effort.
///
/// Retention is maintenance, never a reason to fail state-db open; a failure
/// unlatches the guard so a later open retries instead of giving up for the
/// lifetime of the process.
fn prune_converged_document_write_intents_once(conn: &Connection) {
    if CONVERGED_WRITE_INTENTS_PRUNED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Err(err) = prune_converged_document_write_intents(conn) {
        CONVERGED_WRITE_INTENTS_PRUNED.store(false, Ordering::Relaxed);
        eprintln!(
            "[agent-doc] warning: failed to prune converged document_write_deferred events: {err:#}"
        );
    }
}

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

const RETIRE_PENDING_RESPONSE_FACTS_MIGRATION: &str = "retire_pending_response_fact_variants_v1";

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

fn ensure_dispatch_attempt_receipt_columns(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "dispatch_attempts",
        "result_status",
        "result_status TEXT",
    )?;
    ensure_column(conn, "dispatch_attempts", "proof_scope", "proof_scope TEXT")?;
    ensure_column(
        conn,
        "dispatch_attempts",
        "dispatch_start_proven",
        "dispatch_start_proven INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn ensure_projection_diagnostic_columns(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "projection_diagnostics",
        "source_generation",
        "source_generation INTEGER",
    )?;
    ensure_column(
        conn,
        "projection_diagnostics",
        "intended_hash",
        "intended_hash TEXT",
    )?;
    ensure_column(
        conn,
        "projection_diagnostics",
        "retry_status",
        "retry_status TEXT",
    )?;
    Ok(())
}

fn ensure_queue_head_columns(conn: &Connection) -> Result<()> {
    ensure_column(conn, "queue_heads", "generation", "generation INTEGER")?;
    ensure_column(
        conn,
        "queue_heads",
        "priority",
        "priority INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

/// Maximum retained `crash_recovery_markers` rows. Older markers are an audit
/// trail recovery never consults. A count cap (rather than a time window) keeps
/// the table bounded regardless of write bursts — the failure mode that produced
/// an 11.5M-row / 3.5GB table was a burst that was days, not hours, old.
const CRASH_RECOVERY_MARKER_MAX_ROWS: i64 = 20_000;

/// Guards the once-per-process `crash_recovery_markers` retention prune so the
/// per-request `open_state_db` path does not rescan the table on every RPC.
static CRASH_RECOVERY_MARKERS_PRUNED: AtomicBool = AtomicBool::new(false);

fn ensure_crash_recovery_marker_columns(conn: &Connection) -> Result<()> {
    ensure_column(
        conn,
        "crash_recovery_markers",
        "dedupe_key",
        "dedupe_key TEXT",
    )?;
    conn.execute_batch(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS crash_recovery_markers_dedupe_key
        ON crash_recovery_markers(marker_kind, dedupe_key)
        WHERE dedupe_key IS NOT NULL;
        "#,
    )?;
    // `open_state_db` is called per request handler, so pruning on every open
    // would rescan the table on every RPC. Prune once per process instead: the
    // controller opens this on startup, drains the backlog, then serves cheaply.
    // Retention is best-effort maintenance — never fail state-db open on it — but
    // allow a later open to retry rather than latching the miss.
    if !CRASH_RECOVERY_MARKERS_PRUNED.swap(true, Ordering::Relaxed)
        && let Err(err) = prune_crash_recovery_markers(conn)
    {
        CRASH_RECOVERY_MARKERS_PRUNED.store(false, Ordering::Relaxed);
        eprintln!("[agent-doc] warning: failed to prune crash_recovery_markers: {err:#}");
    }
    Ok(())
}

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
    // Index `timestamp` up front so both the cap lookup and the batched delete run
    // against it. Building it once over a legacy backlog is a bounded one-time
    // cost; afterwards the capped table keeps it cheap to maintain.
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
    let changed = conn.execute(
        r#"
        INSERT OR IGNORE INTO state_events (
            event_id,
            document_hash,
            domain,
            fact_type,
            payload_json,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
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

        prune_superseded_crdt_recovery_checkpoints_to(&conn, 2)?;

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

        prune_superseded_document_baselines_to(&conn, 2)?;

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
        prune_superseded_crdt_recovery_checkpoints_to(&conn, 2)?;
        let remaining: i64 = conn.query_row(
            "SELECT COUNT(*) FROM state_events WHERE fact_type = 'crdt_recovery_projection_checkpointed'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(remaining, 1, "a lone checkpoint must never be pruned");
        Ok(())
    }

}
