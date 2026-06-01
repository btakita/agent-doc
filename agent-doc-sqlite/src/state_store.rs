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
    pub generation: u64,
    pub command_kind: String,
    pub accepted_stage: Option<String>,
    pub failed_stage: Option<String>,
    pub diagnostic_payload: Option<String>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionDiagnosticStatus {
    pub projection: String,
    pub message: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOperatorStatus {
    pub record: Option<ActorRecord>,
    pub transitions: Vec<ActorTransitionStatus>,
    pub supervisor_lease: Option<SupervisorLeaseStatus>,
    pub dispatch_attempts: Vec<DispatchAttemptStatus>,
    pub projection_diagnostics: Vec<ProjectionDiagnosticStatus>,
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
            timestamp INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS projection_diagnostics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            projection TEXT NOT NULL,
            document_id TEXT NOT NULL,
            message TEXT NOT NULL,
            timestamp INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS layout_states (
            scope TEXT PRIMARY KEY,
            columns_json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        "#,
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
            generation,
            command_kind,
            accepted_stage,
            failed_stage,
            diagnostic_payload,
            timestamp
        FROM dispatch_attempts
        WHERE document_id = ?1
        ORDER BY id DESC
        LIMIT 10
        "#,
    )?;
    let mut attempts = Vec::new();
    for row in stmt.query_map(params![document_id], |row| {
        let generation: i64 = row.get("generation")?;
        let timestamp: i64 = row.get("timestamp")?;
        Ok(DispatchAttemptStatus {
            generation: sqlite_u64(generation, "dispatch generation")
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            command_kind: row.get("command_kind")?,
            accepted_stage: row.get("accepted_stage")?,
            failed_stage: row.get("failed_stage")?,
            diagnostic_payload: row.get("diagnostic_payload")?,
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
        SELECT projection, message, timestamp
        FROM projection_diagnostics
        WHERE document_id = ?1
        ORDER BY id DESC
        LIMIT 10
        "#,
    )?;
    let mut diagnostics = Vec::new();
    for row in stmt.query_map(params![document_id], |row| {
        let timestamp: i64 = row.get("timestamp")?;
        Ok(ProjectionDiagnosticStatus {
            projection: row.get("projection")?,
            message: row.get("message")?,
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
    document_id: &str,
    generation: u64,
    command_kind: &str,
    accepted_stage: Option<&str>,
    failed_stage: Option<&str>,
    diagnostic_payload: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO dispatch_attempts (
            document_id,
            generation,
            command_kind,
            accepted_stage,
            failed_stage,
            diagnostic_payload,
            timestamp
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            document_id,
            sqlite_i64(generation, "generation")?,
            command_kind,
            accepted_stage,
            failed_stage,
            diagnostic_payload,
            sqlite_i64(timestamp_secs(), "dispatch attempt timestamp")?,
        ],
    )?;
    Ok(())
}

pub fn insert_projection_diagnostic(
    conn: &Connection,
    projection: &str,
    document_id: &str,
    message: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO projection_diagnostics (projection, document_id, message, timestamp)
        VALUES (?1, ?2, ?3, ?4)
        "#,
        params![
            projection,
            document_id,
            message,
            sqlite_i64(timestamp_secs(), "projection diagnostic timestamp")?
        ],
    )?;
    Ok(())
}

/// Run the compare-and-swap actor write transaction.
///
/// Mirrors the prior `project_controller::store_actor_record` transaction body:
/// load the previous record, enforce the optional CAS expectation, reject
/// generation regressions, insert the transition, then upsert the document row
/// using the lifted `launch_mode`/`controller_epoch` bootstrap tendril.
pub fn store_actor_record_tx(
    conn: &mut Connection,
    expected_prior_generation: Option<u64>,
    record: &ActorRecord,
    launch_mode: Option<String>,
    controller_epoch: Option<i64>,
) -> Result<()> {
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
    let transition_id = insert_actor_transition(&tx, previous.as_ref(), record)?;
    upsert_actor_document(&tx, record, transition_id, launch_mode, controller_epoch)?;
    tx.commit()?;
    Ok(())
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
