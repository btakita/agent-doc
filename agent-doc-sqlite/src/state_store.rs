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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STATE_DB_FILE: &str = "state.db";
const STATE_DB_BUSY_TIMEOUT: Duration = Duration::from_secs(30);

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
    match open_and_init_state_db(&path) {
        Ok(conn) => Ok(conn),
        Err(e) if is_corrupt_state_db_error(&e) => {
            // #statedbgc: a corrupt / non-SQLite state.db (truncated, or grown into a
            // non-database blob — observed live at 4.3GB with no SQLite header) must
            // self-heal instead of hard-erroring every caller (preflight actor-gc,
            // closeout projection, session status, ...). Quarantine it aside with its
            // -wal/-shm siblings and rebuild a fresh DB; the JSON sidecars remain the
            // authoritative fallback the state backbone rebuilds from, so this loses no
            // durable authority — only the derived projection cache.
            eprintln!("[state-db] state.db is corrupt ({e:#}); quarantining and rebuilding fresh");
            quarantine_corrupt_state_db(&path);
            open_and_init_state_db(&path).with_context(|| {
                format!(
                    "failed to reopen a fresh state db after quarantining corrupt {}",
                    path.display()
                )
            })
        }
        Err(e) => Err(e),
    }
}

fn open_and_init_state_db(path: &Path) -> Result<Connection> {
    let conn =
        Connection::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    conn.busy_timeout(STATE_DB_BUSY_TIMEOUT)?;
    initialize_state_db(&conn)?;
    Ok(conn)
}

/// True when a state.db open/init failure means the file is not a usable SQLite
/// database (corrupt, truncated, or a non-database blob) and should be
/// quarantined + rebuilt rather than propagated.
fn is_corrupt_state_db_error(e: &anyhow::Error) -> bool {
    let msg = format!("{e:#}").to_ascii_lowercase();
    msg.contains("file is not a database")
        || msg.contains("not a database")
        || msg.contains("database disk image is malformed")
        || msg.contains("file is encrypted")
}

/// Rename a corrupt `state.db` (and its `-wal`/`-shm` siblings) aside so a fresh
/// database is created on the next open, while preserving the corrupt image for
/// forensics. Best-effort: a rename failure is logged, not fatal.
fn quarantine_corrupt_state_db(path: &Path) {
    let suffix = format!(
        "corrupt-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    let siblings = [
        path.to_path_buf(),
        path.with_extension("db-wal"),
        path.with_extension("db-shm"),
    ];
    for candidate in siblings {
        if !candidate.exists() {
            continue;
        }
        let name = candidate
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("state.db");
        let aside = candidate.with_file_name(format!("{name}.{suffix}"));
        match std::fs::rename(&candidate, &aside) {
            Ok(()) => eprintln!(
                "[state-db] quarantined corrupt {} -> {}",
                candidate.display(),
                aside.display()
            ),
            Err(err) => eprintln!(
                "[state-db] WARNING: failed to quarantine corrupt {}: {err}",
                candidate.display()
            ),
        }
    }
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

        CREATE TABLE IF NOT EXISTS state_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            document_hash TEXT NOT NULL,
            domain TEXT NOT NULL,
            fact_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            timestamp INTEGER NOT NULL
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
        "#,
    )?;
    ensure_dispatch_attempt_receipt_columns(conn)?;
    ensure_projection_diagnostic_columns(conn)?;
    ensure_queue_head_columns(conn)?;
    ensure_crash_recovery_marker_columns(conn)?;
    Ok(())
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

/// `#ctlrecycle`: is ANY dispatch in flight across every document/generation? The
/// controller's idle self-recycle (R1) uses this as its idle proof — it must never
/// exit while a turn is mid-dispatch for any session it coordinates. Same open-set
/// definition as [`has_open_in_flight_dispatch`] without the document/generation
/// filter.
pub fn has_any_open_in_flight_dispatch(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM dispatch_attempts
        WHERE failed_stage IS NULL
          AND COALESCE(result_status, '') IN ('accepted', 'queued', 'running')
          AND dispatch_start_proven = 0
        "#,
        [],
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
    fn open_state_db_quarantines_and_rebuilds_a_corrupt_non_sqlite_file() -> Result<()> {
        // #statedbgc: a state.db that is not a valid SQLite database (truncated, or
        // grown into a non-database blob — observed live at 4.3GB) must self-heal on
        // open — quarantine the corrupt image aside and rebuild a fresh DB — instead
        // of hard-erroring every caller (preflight actor-gc, closeout projection, ...).
        let dir = tempfile::TempDir::new()?;
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc"))?;
        let path = state_db_path(root);
        std::fs::write(&path, b"this is not a sqlite database at all\n")?;
        std::fs::write(path.with_extension("db-wal"), b"garbage-wal")?;

        // Open must succeed by quarantining the corrupt file and rebuilding fresh.
        let conn = open_state_db(root)?;
        let count: i64 = conn.query_row("SELECT count(*) FROM documents", [], |r| r.get(0))?;
        assert_eq!(
            count, 0,
            "rebuilt state.db should have an empty documents table"
        );

        // The corrupt image was moved aside (a *.corrupt-* sibling), not deleted.
        let quarantined = std::fs::read_dir(root.join(".agent-doc"))?
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(
            quarantined,
            "corrupt state.db must be quarantined aside for forensics"
        );
        assert!(
            path.exists(),
            "a fresh state.db must exist at the canonical path after rebuild"
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
}
