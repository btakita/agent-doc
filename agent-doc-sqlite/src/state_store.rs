//! Project-controller SQLite state store.
//!
//! This module owns every `rusqlite::Connection` interaction for the project
//! controller's durable actor/lease/dispatch/diagnostic/layout state, plus the
//! storage and status types those queries read and write. It deliberately
//! depends only on `rusqlite`, `serde_json`, and `std` so that
//! `agent-doc-orchestration` can call into it without pulling the bundled
//! SQLite C build into its own compile graph.
//!
//! Orchestration glue (ops-log, `sessions.json`/`session-actors.json`
//! projection, `read_bootstrap`, drift verification) stays in
//! `agent-doc-orchestration`; this module exposes the SQL primitives those
//! callers stitch together.

use anyhow::{Context, Result};
pub use rusqlite::Connection;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_DB_FILE: &str = "state.db";

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
    pub dispatch_receipts: usize,
    pub queue_heads: usize,
    pub document_cycles: usize,
    pub pending_mutations: usize,
    pub projection_diagnostics: usize,
    pub admin_operations: usize,
    pub crash_recovery_markers: usize,
    pub layout_states: usize,
}

impl ControlPlaneStoreCounts {
    pub fn total_authoritative_rows(&self) -> usize {
        self.actor_documents
            + self.actor_transitions
            + self.supervisor_leases
            + self.dispatch_receipts
            + self.queue_heads
            + self.document_cycles
            + self.pending_mutations
            + self.projection_diagnostics
            + self.admin_operations
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
    let conn =
        Connection::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    initialize_state_db(&conn)?;
    Ok(conn)
}

pub fn initialize_state_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA busy_timeout = 5000;

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
            head_id TEXT,
            prompt TEXT NOT NULL,
            state TEXT NOT NULL,
            selected_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (document_id, queue_name)
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
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let existing: String = row.get(1)?;
        if existing == column {
            return Ok(());
        }
    }
    conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {definition}"), [])?;
    Ok(())
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
        crash_recovery_markers: count_rows(
            conn,
            "SELECT COUNT(*) FROM crash_recovery_markers",
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
    let tx = conn.transaction()?;
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

/// True when the `documents` table holds no rows yet.
///
/// Orchestration uses this to gate the legacy `session-actors.json` read so the
/// JSON is only loaded when a migration could actually run.
pub fn actor_documents_empty(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
    Ok(count == 0)
}

/// Migrate a legacy `session-actors.json` store into the empty sqlite state.
///
/// The orchestration caller loads the legacy JSON and the bootstrap-derived
/// `launch_mode`/`controller_epoch` tendril, then hands them to this routine,
/// which only runs when the `documents` table is still empty.
pub fn migrate_actor_store_tx(
    conn: &mut Connection,
    store: &BTreeMap<String, ActorRecord>,
    launch_mode: Option<String>,
    controller_epoch: Option<i64>,
) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for record in store.values() {
        let transition_id = insert_actor_transition(&tx, None, record)?;
        upsert_actor_document(
            &tx,
            record,
            transition_id,
            launch_mode.clone(),
            controller_epoch,
        )?;
    }
    tx.commit()?;
    Ok(())
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
            "session-actors.json",
            &record.document_id,
            "projection lag",
        )?;
        insert_admin_operation_in_db(
            &conn,
            "projection_repair",
            Some(&record.document_id),
            "accepted",
            Some("store actor count test"),
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
        assert_eq!(counts.dispatch_receipts, 1);
        assert_eq!(counts.queue_heads, 1);
        assert_eq!(counts.document_cycles, 1);
        assert_eq!(counts.pending_mutations, 1);
        assert_eq!(counts.projection_diagnostics, 1);
        assert_eq!(counts.admin_operations, 1);
        assert_eq!(counts.crash_recovery_markers, 1);
        assert_eq!(counts.layout_states, 1);
        assert_eq!(counts.total_authoritative_rows(), 11);

        Ok(())
    }
}
