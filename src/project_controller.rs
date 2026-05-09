//! Project-local controller shell.
//!
//! The controller is the live authority for document session actor lookup,
//! generation changes, lifecycle reports, and routed dispatch acceptance.
//! `sessions.json` and tmux state remain projections and layout inputs.

use anyhow::{Context, Result};
use fs2::FileExt;
use interprocess::local_socket::{
    GenericFilePath, ListenerNonblockingMode, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOCKET_FILE: &str = "controller.sock";
const STATE_FILE: &str = "controller-state.json";
const STATE_DB_FILE: &str = "state.db";
const ACTOR_PROJECTION_FILE: &str = "session-actors.json";
const LOCK_FILE: &str = "controller-launch.lock";
const CONNECT_WAIT: Duration = Duration::from_secs(3);
const CONNECT_POLL: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_millis(250);
const CONTROLLER_IDLE_CLIENT_TIMEOUT: Duration = CONTROLLER_RPC_TIMEOUT;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Managed,
    Lazy,
}

impl LaunchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Lazy => "lazy",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "managed" => Ok(Self::Managed),
            "lazy" => Ok(Self::Lazy),
            other => anyhow::bail!("unknown controller launch mode: {other}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerBinaryIdentity {
    pub path: PathBuf,
    pub version: String,
    pub len: u64,
    pub modified_secs: u64,
    pub modified_nanos: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerBootstrap {
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
    pub launch_mode: LaunchMode,
    pub bootstrap_epoch: u64,
    pub pid: u32,
    #[serde(default)]
    pub controller_binary: Option<ControllerBinaryIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerStatus {
    pub active: bool,
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
    pub launch_mode: Option<LaunchMode>,
    pub bootstrap_epoch: Option<u64>,
    pub pid: Option<u32>,
    #[serde(default)]
    pub controller_binary: Option<ControllerBinaryIdentity>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StartSessionRequest {
    pub file: PathBuf,
    pub session_id: String,
    pub pane_id: String,
    pub window_id: String,
    pub generation: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttachPaneRequest {
    pub file: PathBuf,
    pub session_id: String,
    pub pane_id: String,
    pub window_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SupervisorRegistration {
    pub file: PathBuf,
    pub session_id: String,
    pub pane_id: String,
    pub generation: u64,
    pub supervisor_pid: u32,
    pub supervisor_socket: String,
    pub runtime_state: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LifecycleRequest {
    pub file: PathBuf,
    pub session_id: String,
    pub pane_id: String,
    pub generation: u64,
    pub state: crate::session_actor::ActorState,
    pub caller: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DispatchRequest {
    pub file: PathBuf,
    pub session_id: String,
    pub pane_id: String,
    pub generation: u64,
    pub command_kind: String,
    pub diagnostic_payload: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchAuthorization {
    pub record: crate::session_actor::ActorRecord,
    pub accepted_stage: String,
}

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
    pub record: Option<crate::session_actor::ActorRecord>,
    pub transitions: Vec<ActorTransitionStatus>,
    pub supervisor_lease: Option<SupervisorLeaseStatus>,
    pub dispatch_attempts: Vec<DispatchAttemptStatus>,
    pub projection_diagnostics: Vec<ProjectionDiagnosticStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ControllerRequest {
    command: String,
    file: Option<PathBuf>,
    session_id: Option<String>,
    pane_id: Option<String>,
    window_id: Option<String>,
    generation: Option<u64>,
    state: Option<String>,
    caller: Option<String>,
    reason: Option<String>,
    supervisor_pid: Option<u32>,
    supervisor_socket: Option<String>,
    command_kind: Option<String>,
    diagnostic_payload: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ControllerEnvelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

pub struct LaunchLock {
    _file: File,
}

impl LaunchLock {
    pub fn acquire(project_root: &Path) -> Result<Self> {
        let path = launch_lock_path(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        file.try_lock_exclusive().with_context(|| {
            format!("controller launch already in progress: {}", path.display())
        })?;
        Ok(Self { _file: file })
    }
}

impl Drop for LaunchLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

pub fn socket_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(SOCKET_FILE)
}

pub fn state_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(STATE_FILE)
}

pub fn state_db_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(STATE_DB_FILE)
}

fn actor_projection_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(ACTOR_PROJECTION_FILE)
}

pub fn launch_lock_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc/locks").join(LOCK_FILE)
}

pub fn read_bootstrap(project_root: &Path) -> Result<Option<ControllerBootstrap>> {
    let path = state_path(project_root);
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))
        .map(Some)
}

fn current_binary_identity() -> Result<ControllerBinaryIdentity> {
    let path = std::env::current_exe().context("failed to locate current agent-doc binary")?;
    let metadata = std::fs::metadata(&path)
        .with_context(|| format!("failed to stat current agent-doc binary {}", path.display()))?;
    let modified = metadata
        .modified()
        .with_context(|| format!("failed to read modified time for {}", path.display()))?
        .duration_since(UNIX_EPOCH)
        .with_context(|| format!("modified time before unix epoch for {}", path.display()))?;
    Ok(ControllerBinaryIdentity {
        path,
        version: env!("CARGO_PKG_VERSION").to_string(),
        len: metadata.len(),
        modified_secs: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
    })
}

fn write_bootstrap(project_root: &Path, launch_mode: LaunchMode) -> Result<ControllerBootstrap> {
    let bootstrap = ControllerBootstrap {
        project_root: project_root.to_path_buf(),
        socket_path: socket_path(project_root),
        launch_mode,
        bootstrap_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        pid: std::process::id(),
        controller_binary: Some(current_binary_identity()?),
    };
    let path = state_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&bootstrap)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(bootstrap)
}

fn open_state_db(project_root: &Path) -> Result<Connection> {
    let path = state_db_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn =
        Connection::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    initialize_state_db(&conn)?;
    Ok(conn)
}

fn initialize_state_db(conn: &Connection) -> Result<()> {
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
        "#,
    )?;
    Ok(())
}

fn sqlite_i64(value: u64, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} is too large for sqlite INTEGER"))
}

fn sqlite_u64(value: i64, name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{name} is negative in sqlite state"))
}

fn timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn actor_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<crate::session_actor::ActorRecord> {
    let actor_state: String = row.get("actor_state")?;
    let state = crate::session_actor::ActorState::parse(&actor_state)
        .ok_or_else(|| rusqlite::Error::InvalidQuery)?;
    let generation: i64 = row.get("generation")?;
    let transition_prior: i64 = row.get("prior_generation")?;
    let transition_new: i64 = row.get("new_generation")?;
    let transition_timestamp: i64 = row.get("timestamp")?;
    Ok(crate::session_actor::ActorRecord {
        document_id: row.get("document_id")?,
        session_id: row.get("session_id")?,
        generation: sqlite_u64(generation, "generation")
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        pane_id: row.get("pane_id")?,
        window_id: row.get("window_id")?,
        harness: row.get("harness_id")?,
        state,
        last_transition: crate::session_actor::ActorLastTransition {
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

fn load_actor_record_from_db(
    conn: &Connection,
    document_id: &str,
) -> Result<Option<crate::session_actor::ActorRecord>> {
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

fn load_actor_transitions_from_db(
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

fn load_supervisor_lease_from_db(
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

fn load_dispatch_attempts_from_db(
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

fn load_projection_diagnostics_from_db(
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

fn load_session_operator_status_from_db(
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

fn legacy_actor_projection(
    project_root: &Path,
) -> Result<Option<BTreeMap<String, crate::session_actor::ActorRecord>>> {
    let path = actor_projection_path(project_root);
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    let store = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(store))
}

fn migrate_legacy_actor_projection(project_root: &Path, conn: &mut Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let Some(store) = legacy_actor_projection(project_root)? else {
        return Ok(());
    };
    let tx = conn.transaction()?;
    for record in store.values() {
        let transition_id = insert_actor_transition(&tx, None, record)?;
        upsert_actor_document(project_root, &tx, record, transition_id)?;
    }
    tx.commit()?;
    Ok(())
}

fn insert_actor_transition(
    conn: &Connection,
    previous: Option<&crate::session_actor::ActorRecord>,
    record: &crate::session_actor::ActorRecord,
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

fn upsert_actor_document(
    project_root: &Path,
    conn: &Connection,
    record: &crate::session_actor::ActorRecord,
    transition_id: i64,
) -> Result<()> {
    let bootstrap = read_bootstrap(project_root).ok().flatten();
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
            bootstrap
                .as_ref()
                .map(|state| state.launch_mode.as_str().to_string()),
            bootstrap
                .as_ref()
                .map(|state| sqlite_i64(state.bootstrap_epoch, "bootstrap_epoch"))
                .transpose()?,
            transition_id,
        ],
    )?;
    Ok(())
}

fn upsert_supervisor_lease(
    project_root: &Path,
    record: &crate::session_actor::ActorRecord,
    supervisor_pid: Option<u32>,
    supervisor_socket: Option<&str>,
    runtime_state: &str,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
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

fn insert_dispatch_attempt_record(
    project_root: &Path,
    document_id: &str,
    generation: u64,
    command_kind: &str,
    accepted_stage: Option<&str>,
    failed_stage: Option<&str>,
    diagnostic_payload: &str,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
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

pub fn load_actor_store(
    project_root: &Path,
) -> Result<BTreeMap<String, crate::session_actor::ActorRecord>> {
    let mut conn = open_state_db(project_root)?;
    migrate_legacy_actor_projection(project_root, &mut conn)?;
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

pub fn load_actor_record(
    project_root: &Path,
    document_id: &str,
) -> Result<Option<crate::session_actor::ActorRecord>> {
    let mut conn = open_state_db(project_root)?;
    migrate_legacy_actor_projection(project_root, &mut conn)?;
    load_actor_record_from_db(&conn, document_id)
}

pub fn store_actor_record(
    project_root: &Path,
    expected_prior_generation: Option<u64>,
    record: &crate::session_actor::ActorRecord,
) -> Result<crate::session_actor::ActorRecord> {
    let mut conn = open_state_db(project_root)?;
    migrate_legacy_actor_projection(project_root, &mut conn)?;
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
    upsert_actor_document(project_root, &tx, record, transition_id)?;
    tx.commit()?;

    if let Err(err) = emit_actor_projection(project_root) {
        record_projection_diagnostic(
            project_root,
            "session-actors.json",
            &record.document_id,
            &format!("failed to emit actor projection after sqlite commit: {err}"),
        );
    }
    if record.last_transition.caller == "start" && record.last_transition.reason == "session_start"
    {
        let _ = project_sessions_projection_for_actor(project_root, &record.document_id);
    }
    Ok(record.clone())
}

fn emit_actor_projection(project_root: &Path) -> Result<()> {
    let store = load_actor_store(project_root)?;
    let path = actor_projection_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&store)?;
    std::fs::write(&path, &content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    let written = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let projected: BTreeMap<String, crate::session_actor::ActorRecord> =
        serde_json::from_str(&written)
            .with_context(|| format!("failed to parse {}", path.display()))?;
    if projected != store {
        anyhow::bail!("session-actors.json projection drifted from sqlite state");
    }
    Ok(())
}

pub fn project_sessions_projection_for_actor(project_root: &Path, document_id: &str) -> Result<()> {
    let Some(record) = load_actor_record(project_root, document_id)? else {
        return Ok(());
    };
    let mut registry = match crate::sessions::load_in(project_root) {
        Ok(registry) => registry,
        Err(err) => {
            record_projection_diagnostic(
                project_root,
                "sessions.json",
                document_id,
                &format!("failed to load projection: {err}"),
            );
            return Ok(());
        }
    };
    let Some(entry) = registry.get_mut(document_id) else {
        record_projection_diagnostic(
            project_root,
            "sessions.json",
            document_id,
            "sessions projection has no registry entry for controller actor",
        );
        return Ok(());
    };
    entry.session_id = record.session_id.clone();
    entry.pane = record.pane_id.clone();
    entry.window = record.window_id.clone();
    if let Err(err) = crate::sessions::save_in(project_root, &registry) {
        record_projection_diagnostic(
            project_root,
            "sessions.json",
            document_id,
            &format!("failed to write projection: {err}"),
        );
        return Ok(());
    }
    let projected = match crate::sessions::load_in(project_root) {
        Ok(registry) => registry,
        Err(err) => {
            record_projection_diagnostic(
                project_root,
                "sessions.json",
                document_id,
                &format!("failed to reload projection: {err}"),
            );
            return Ok(());
        }
    };
    if projected.get(document_id).is_none_or(|entry| {
        entry.session_id != record.session_id
            || entry.pane != record.pane_id
            || entry.window != record.window_id
    }) {
        record_projection_diagnostic(
            project_root,
            "sessions.json",
            document_id,
            "sessions projection drifted from controller actor state",
        );
    }
    Ok(())
}

fn record_projection_diagnostic(
    project_root: &Path,
    projection: &str,
    document_id: &str,
    message: &str,
) {
    eprintln!(
        "[controller] projection drift projection={} document={} message={}",
        projection, document_id, message
    );
    if let Ok(conn) = open_state_db(project_root) {
        let _ = conn.execute(
            r#"
            INSERT INTO projection_diagnostics (projection, document_id, message, timestamp)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                projection,
                document_id,
                message,
                sqlite_i64(timestamp_secs(), "projection diagnostic timestamp").unwrap_or_default()
            ],
        );
    }
    crate::ops_log::log_op(
        Path::new(document_id),
        &format!(
            "projection_drift projection={} document={} message={}",
            projection, document_id, message
        ),
    );
}

fn connect(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    let name = socket_path(project_root).to_fs_name::<GenericFilePath>()?;
    interprocess::local_socket::ConnectOptions::new()
        .name(name)
        .connect_sync()
        .context("failed to connect to project controller")
}

fn request(project_root: &Path, command: &str) -> Result<String> {
    let stream = connect(project_root)?;
    stream
        .set_recv_timeout(Some(CONTROLLER_RPC_TIMEOUT))
        .context("failed to set project controller response timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    let mut request = serde_json::to_string(&serde_json::json!({ "command": command }))?;
    request.push('\n');
    writer_half.write_all(request.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut response = String::new();
    read_controller_response_line(&mut reader, &mut response)?;
    Ok(response.trim().to_string())
}

fn read_controller_response_line<R: BufRead>(reader: &mut R, response: &mut String) -> Result<()> {
    match reader.read_line(response) {
        Ok(0) => anyhow::bail!("project controller closed connection without a response"),
        Ok(_) => Ok(()),
        Err(err) if is_timeout_error(&err) => anyhow::bail!(
            "timed out after {:.1}s waiting for project controller response",
            CONTROLLER_RPC_TIMEOUT.as_secs_f32()
        ),
        Err(err) => Err(err).context("failed to read project controller response"),
    }
}

fn request_controller<T: DeserializeOwned>(
    project_root: &Path,
    request: ControllerRequest,
) -> Result<T> {
    let stream = connect_or_launch(project_root, LaunchMode::Lazy)?;
    stream
        .set_recv_timeout(Some(CONTROLLER_RPC_TIMEOUT))
        .context("failed to set project controller response timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    let mut raw = serde_json::to_string(&request)?;
    raw.push('\n');
    writer_half.write_all(raw.as_bytes())?;
    writer_half.flush()?;

    let mut reader = BufReader::new(reader_half);
    let mut response = String::new();
    read_controller_response_line(&mut reader, &mut response)?;
    let envelope: ControllerEnvelope<T> = serde_json::from_str(response.trim())
        .context("failed to parse project controller response envelope")?;
    if envelope.ok {
        envelope
            .data
            .context("project controller returned ok response without data")
    } else {
        anyhow::bail!(
            "{}",
            envelope
                .error
                .unwrap_or_else(|| "project controller request failed".to_string())
        )
    }
}

pub fn start_session(
    project_root: &Path,
    request: StartSessionRequest,
) -> Result<crate::session_actor::ActorRecord> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "start_session".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: Some(request.window_id),
            generation: Some(request.generation),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn register_supervisor(
    project_root: &Path,
    registration: SupervisorRegistration,
) -> Result<crate::session_actor::ActorRecord> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "register_supervisor".to_string(),
            file: Some(registration.file),
            session_id: Some(registration.session_id),
            pane_id: Some(registration.pane_id),
            window_id: None,
            generation: Some(registration.generation),
            state: Some(registration.runtime_state),
            caller: None,
            reason: None,
            supervisor_pid: Some(registration.supervisor_pid),
            supervisor_socket: Some(registration.supervisor_socket),
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn mark_lifecycle(
    project_root: &Path,
    request: LifecycleRequest,
) -> Result<crate::session_actor::ActorRecord> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: None,
            generation: Some(request.generation),
            state: Some(request.state.as_str().to_string()),
            caller: Some(request.caller),
            reason: Some(request.reason),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn authoritative_actor_binding(
    project_root: &Path,
    file: &Path,
) -> Result<Option<crate::session_actor::ActorRecord>> {
    #[cfg(test)]
    {
        let document_id =
            crate::session_actor::canonical_document_id_in(project_root, &file.to_string_lossy());
        return load_actor_record(project_root, &document_id);
    }

    #[cfg(not(test))]
    {
        request_controller(
            project_root,
            ControllerRequest {
                command: "actor_binding".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }
}

pub fn authorize_dispatch(
    project_root: &Path,
    request: DispatchRequest,
) -> Result<DispatchAuthorization> {
    #[cfg(test)]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
        };
        return handle_dispatch(
            &bootstrap,
            ControllerRequest {
                command: "dispatch".to_string(),
                file: Some(request.file),
                session_id: Some(request.session_id),
                pane_id: Some(request.pane_id),
                window_id: None,
                generation: Some(request.generation),
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some(request.command_kind),
                diagnostic_payload: Some(request.diagnostic_payload),
            },
        );
    }

    #[cfg(not(test))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: None,
            generation: Some(request.generation),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(request.command_kind),
            diagnostic_payload: Some(request.diagnostic_payload),
        },
    )
}

pub fn session_operator_status(project_root: &Path, file: &Path) -> Result<SessionOperatorStatus> {
    #[cfg(test)]
    {
        let document_id =
            crate::session_actor::canonical_document_id_in(project_root, &file.to_string_lossy());
        let mut conn = open_state_db(project_root)?;
        migrate_legacy_actor_projection(project_root, &mut conn)?;
        return load_session_operator_status_from_db(&conn, &document_id);
    }

    #[cfg(not(test))]
    {
        request_controller(
            project_root,
            ControllerRequest {
                command: "session_status".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: None,
                diagnostic_payload: None,
            },
        )
    }
}

pub fn attach_pane(
    project_root: &Path,
    request: AttachPaneRequest,
) -> Result<crate::session_actor::ActorRecord> {
    request_controller(
        project_root,
        ControllerRequest {
            command: "attach_pane".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: Some(request.window_id),
            generation: None,
            state: None,
            caller: Some("session".to_string()),
            reason: Some("manual_attach".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn authorize_operator_command(
    project_root: &Path,
    file: &Path,
    command_kind: &str,
) -> Result<DispatchAuthorization> {
    #[cfg(test)]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
        };
        return handle_operator_command(
            &bootstrap,
            ControllerRequest {
                command: "operator_command".to_string(),
                file: Some(file.to_path_buf()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: None,
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some(command_kind.to_string()),
                diagnostic_payload: Some("session operator command".to_string()),
            },
        );
    }

    #[cfg(not(test))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "operator_command".to_string(),
            file: Some(file.to_path_buf()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some(command_kind.to_string()),
            diagnostic_payload: Some("session operator command".to_string()),
        },
    )
}

pub fn status(project_root: &Path) -> Result<ControllerStatus> {
    match request(project_root, "status") {
        Ok(response) => {
            let mut status: ControllerStatus = serde_json::from_str(&response)
                .context("failed to parse project controller status response")?;
            status.active = true;
            Ok(status)
        }
        Err(_) => {
            let bootstrap = read_bootstrap(project_root)?;
            Ok(ControllerStatus {
                active: false,
                project_root: project_root.to_path_buf(),
                socket_path: socket_path(project_root),
                launch_mode: bootstrap.as_ref().map(|state| state.launch_mode),
                bootstrap_epoch: bootstrap.as_ref().map(|state| state.bootstrap_epoch),
                pid: bootstrap.as_ref().map(|state| state.pid),
                controller_binary: bootstrap
                    .as_ref()
                    .and_then(|state| state.controller_binary.clone()),
            })
        }
    }
}

fn active_controller_matches_current_binary(project_root: &Path) -> Result<bool> {
    let response = request(project_root, "status")?;
    let status: ControllerStatus =
        serde_json::from_str(&response).context("failed to parse project controller status")?;
    controller_status_matches_current_binary(&status)
}

fn controller_status_matches_current_binary(status: &ControllerStatus) -> Result<bool> {
    Ok(status.controller_binary.as_ref() == Some(&current_binary_identity()?))
}

fn shutdown_stale_controller(project_root: &Path) {
    let _ = request(project_root, "shutdown");
    let start = Instant::now();
    while start.elapsed() < CONNECT_WAIT {
        if connect(project_root).is_err() {
            return;
        }
        std::thread::sleep(CONNECT_POLL);
    }
}

pub fn connect_or_launch(
    project_root: &Path,
    launch_mode: LaunchMode,
) -> Result<interprocess::local_socket::Stream> {
    if active_controller_matches_current_binary(project_root).unwrap_or(false) {
        return connect(project_root);
    }

    let _lock = LaunchLock::acquire(project_root)?;
    if active_controller_matches_current_binary(project_root).unwrap_or(false) {
        return connect(project_root);
    }
    if connect(project_root).is_ok() {
        shutdown_stale_controller(project_root);
    }

    launch_detached(project_root, launch_mode)?;
    wait_for_controller(project_root)
}

pub fn ensure_controller_running(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    let stream = connect_or_launch(project_root, launch_mode)?;
    drop(stream);
    Ok(())
}

fn launch_detached(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    let exe = std::env::current_exe().context("failed to locate current agent-doc binary")?;
    Command::new(exe)
        .arg("controller")
        .arg("serve")
        .arg("--project-root")
        .arg(project_root)
        .arg("--launch-mode")
        .arg(launch_mode.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to launch project controller")?;
    Ok(())
}

fn wait_for_controller(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    let start = Instant::now();
    loop {
        if let Ok(stream) = connect(project_root) {
            return Ok(stream);
        }
        if start.elapsed() >= CONNECT_WAIT {
            anyhow::bail!(
                "timed out waiting for project controller at {}",
                socket_path(project_root).display()
            );
        }
        std::thread::sleep(CONNECT_POLL);
    }
}

pub fn serve(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    let sock = socket_path(project_root);
    if sock.exists() {
        let _ = std::fs::remove_file(&sock);
    }
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bootstrap = write_bootstrap(project_root, launch_mode)?;
    let name = sock.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .with_context(|| format!("failed to listen on {}", sock.display()))?;
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .context("failed to set project controller listener nonblocking")?;

    let should_stop = Arc::new(AtomicBool::new(false));
    while !should_stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => {
                let bootstrap = bootstrap.clone();
                let should_stop = Arc::clone(&should_stop);
                let sock = sock.clone();
                std::thread::spawn(move || {
                    if let Err(err) = serve_client(stream, &bootstrap, &should_stop, &sock) {
                        eprintln!("[controller] client error: {err}");
                    }
                });
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(CONNECT_POLL);
            }
            Err(err) => return Err(err).context("failed to accept project controller client"),
        }
    }
    let _ = std::fs::remove_file(&sock);
    Ok(())
}

fn serve_client(
    stream: interprocess::local_socket::Stream,
    bootstrap: &ControllerBootstrap,
    should_stop: &AtomicBool,
    sock: &Path,
) -> Result<()> {
    stream
        .set_recv_timeout(Some(CONTROLLER_IDLE_CLIENT_TIMEOUT))
        .context("failed to set project controller client read timeout")?;
    let (reader_half, mut writer_half) = stream.split();
    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();
    loop {
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()),
            Ok(_) => {
                let mut request_should_stop = false;
                let response = handle_request(&line, bootstrap, &mut request_should_stop)?;
                writer_half.write_all(response.as_bytes())?;
                writer_half.write_all(b"\n")?;
                writer_half.flush()?;
                line.clear();
                if request_should_stop {
                    should_stop.store(true, Ordering::SeqCst);
                    let _ = std::fs::remove_file(sock);
                    return Ok(());
                }
            }
            Err(err) if is_timeout_error(&err) => {
                eprintln!(
                    "[controller] closing idle client after {:.1}s without a complete request",
                    CONTROLLER_IDLE_CLIENT_TIMEOUT.as_secs_f32()
                );
                return Ok(());
            }
            Err(err) => return Err(err).context("failed to read project controller request"),
        }
    }
}

fn is_timeout_error(err: &std::io::Error) -> bool {
    matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
}

fn handle_request(
    line: &str,
    bootstrap: &ControllerBootstrap,
    should_stop: &mut bool,
) -> Result<String> {
    let request: ControllerRequest = serde_json::from_str(line.trim())?;
    match request.command.as_str() {
        "status" => Ok(serde_json::to_string(&ControllerStatus {
            active: true,
            project_root: bootstrap.project_root.clone(),
            socket_path: bootstrap.socket_path.clone(),
            launch_mode: Some(bootstrap.launch_mode),
            bootstrap_epoch: Some(bootstrap.bootstrap_epoch),
            pid: Some(bootstrap.pid),
            controller_binary: bootstrap.controller_binary.clone(),
        })?),
        "shutdown" => {
            *should_stop = true;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "start_session" => controller_envelope(handle_start_session(bootstrap, request)),
        "register_supervisor" => {
            controller_envelope(handle_register_supervisor(bootstrap, request))
        }
        "mark_lifecycle" => controller_envelope(handle_mark_lifecycle(bootstrap, request)),
        "actor_binding" => controller_envelope(handle_actor_binding(bootstrap, request)),
        "dispatch" => controller_envelope(handle_dispatch(bootstrap, request)),
        "session_status" => controller_envelope(handle_session_status(bootstrap, request)),
        "attach_pane" => controller_envelope(handle_attach_pane(bootstrap, request)),
        "operator_command" => controller_envelope(handle_operator_command(bootstrap, request)),
        other => Ok(serde_json::to_string(&serde_json::json!({
            "ok": false,
            "error": format!("unknown controller command: {other}")
        }))?),
    }
}

fn controller_envelope<T: Serialize>(result: Result<T>) -> Result<String> {
    match result {
        Ok(data) => Ok(serde_json::to_string(&serde_json::json!({
            "ok": true,
            "data": data
        }))?),
        Err(err) => Ok(serde_json::to_string(&serde_json::json!({
            "ok": false,
            "error": err.to_string()
        }))?),
    }
}

fn request_file(request: &ControllerRequest) -> Result<PathBuf> {
    request
        .file
        .clone()
        .context("controller request missing file")
}

fn request_string(value: &Option<String>, name: &str) -> Result<String> {
    value
        .clone()
        .with_context(|| format!("controller request missing {name}"))
}

fn request_u64(value: Option<u64>, name: &str) -> Result<u64> {
    value.with_context(|| format!("controller request missing {name}"))
}

fn handle_start_session(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<crate::session_actor::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let window_id = request_string(&request.window_id, "window_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let record = crate::session_actor::record_session_start_direct(
        &file,
        &session_id,
        &pane_id,
        &window_id,
        generation,
    )
    .with_context(|| {
        format!(
            "controller failed to start session actor for {}",
            file.display()
        )
    })?;
    let _ = project_sessions_projection_for_actor(&bootstrap.project_root, &record.document_id);
    crate::ops_log::log_op(
        &file,
        &format!(
            "controller_session_start session={} pane={} generation={} state={}",
            session_id,
            pane_id,
            record.generation,
            record.state.as_str()
        ),
    );
    Ok(record)
}

fn handle_register_supervisor(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<crate::session_actor::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let runtime_state = request
        .state
        .as_deref()
        .unwrap_or(crate::session_actor::ActorState::Starting.as_str());
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let record = load_actor_record(&bootstrap.project_root, &document_id)?
        .with_context(|| format!("missing actor record for supervisor {}", file.display()))?;
    if record.session_id != session_id
        || record.pane_id != pane_id
        || record.generation != generation
    {
        anyhow::bail!(
            "stale supervisor registration for {}: requested session={} pane={} generation={}, current session={} pane={} generation={}",
            file.display(),
            session_id,
            pane_id,
            generation,
            record.session_id,
            record.pane_id,
            record.generation
        );
    }
    upsert_supervisor_lease(
        &bootstrap.project_root,
        &record,
        request.supervisor_pid,
        request.supervisor_socket.as_deref(),
        runtime_state,
    )?;
    crate::ops_log::log_op(
        &file,
        &format!(
            "controller_supervisor_registered session={} pane={} generation={} state={}",
            session_id, pane_id, generation, runtime_state
        ),
    );
    Ok(record)
}

fn handle_mark_lifecycle(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<crate::session_actor::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let state_raw = request_string(&request.state, "state")?;
    let state = crate::session_actor::ActorState::parse(&state_raw)
        .with_context(|| format!("unknown lifecycle state: {state_raw}"))?;
    let caller = request_string(&request.caller, "caller")?;
    let reason = request_string(&request.reason, "reason")?;
    let record = crate::session_actor::transition_state_direct(
        &file,
        &session_id,
        &pane_id,
        Some(generation),
        state,
        &caller,
        &reason,
    )?;
    upsert_supervisor_lease(
        &bootstrap.project_root,
        &record,
        request.supervisor_pid,
        request.supervisor_socket.as_deref(),
        state.as_str(),
    )?;
    crate::ops_log::log_op(
        &file,
        &format!(
            "controller_lifecycle session={} pane={} generation={} state={} caller={} reason={}",
            session_id,
            pane_id,
            generation,
            state.as_str(),
            caller,
            reason
        ),
    );
    Ok(record)
}

fn handle_actor_binding(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<Option<crate::session_actor::ActorRecord>> {
    let file = request_file(&request)?;
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    load_actor_record(&bootstrap.project_root, &document_id)
}

fn handle_dispatch(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<DispatchAuthorization> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let generation = request_u64(request.generation, "generation")?;
    let command_kind = request_string(&request.command_kind, "command_kind")?;
    let diagnostic_payload = request
        .diagnostic_payload
        .as_deref()
        .unwrap_or_default()
        .to_string();
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let record = load_actor_record(&bootstrap.project_root, &document_id)?
        .with_context(|| format!("missing actor record for dispatch {}", file.display()))?;
    let mut failed_stage = None;
    let mut failure = None;
    if record.session_id != session_id {
        failed_stage = Some("stale_session");
        failure = Some(format!(
            "dispatch rejected for {}: requested session {}, current session {}",
            file.display(),
            session_id,
            record.session_id
        ));
    } else if record.pane_id != pane_id {
        failed_stage = Some("stale_pane");
        failure = Some(format!(
            "dispatch rejected for {}: requested pane {}, current pane {}",
            file.display(),
            pane_id,
            record.pane_id
        ));
    } else if record.generation != generation {
        failed_stage = Some("stale_generation");
        failure = Some(format!(
            "dispatch rejected for {}: requested generation {}, current generation {}",
            file.display(),
            generation,
            record.generation
        ));
    } else if matches!(
        record.state,
        crate::session_actor::ActorState::Blocked | crate::session_actor::ActorState::Closed
    ) {
        failed_stage = Some(record.state.as_str());
        failure = Some(format!(
            "dispatch rejected for {}: authoritative actor generation {} is {}",
            file.display(),
            generation,
            record.state.as_str()
        ));
    }

    if let Some(message) = failure {
        let stage = failed_stage.unwrap_or("rejected");
        let _ = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            &document_id,
            generation,
            &command_kind,
            None,
            Some(stage),
            &diagnostic_payload,
        );
        anyhow::bail!(message);
    }

    let accepted_stage = match record.state {
        crate::session_actor::ActorState::Ready => "ready",
        crate::session_actor::ActorState::Starting => "starting_queue",
        crate::session_actor::ActorState::Busy => "busy_queue",
        crate::session_actor::ActorState::WaitingInput => "waiting_input_recovery",
        crate::session_actor::ActorState::Blocked | crate::session_actor::ActorState::Closed => {
            unreachable!("blocked/closed dispatch rejected above")
        }
    };
    insert_dispatch_attempt_record(
        &bootstrap.project_root,
        &document_id,
        record.generation,
        &command_kind,
        Some(accepted_stage),
        None,
        &diagnostic_payload,
    )?;
    crate::ops_log::log_op(
        &file,
        &format!(
            "controller_dispatch_accepted session={} pane={} generation={} state={} kind={} stage={}",
            session_id,
            pane_id,
            generation,
            record.state.as_str(),
            command_kind,
            accepted_stage
        ),
    );
    Ok(DispatchAuthorization {
        record,
        accepted_stage: accepted_stage.to_string(),
    })
}

fn handle_session_status(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<SessionOperatorStatus> {
    let file = request_file(&request)?;
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let mut conn = open_state_db(&bootstrap.project_root)?;
    migrate_legacy_actor_projection(&bootstrap.project_root, &mut conn)?;
    load_session_operator_status_from_db(&conn, &document_id)
}

fn handle_attach_pane(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<crate::session_actor::ActorRecord> {
    let file = request_file(&request)?;
    let session_id = request_string(&request.session_id, "session_id")?;
    let pane_id = request_string(&request.pane_id, "pane_id")?;
    let window_id = request_string(&request.window_id, "window_id")?;
    crate::session_actor::project_binding_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
        &session_id,
        &pane_id,
        &window_id,
        request.caller.as_deref().unwrap_or("session"),
        request.reason.as_deref().unwrap_or("manual_attach"),
    )?;
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let record = load_actor_record(&bootstrap.project_root, &document_id)?
        .with_context(|| format!("missing actor record after attach for {}", file.display()))?;
    let _ = project_sessions_projection_for_actor(&bootstrap.project_root, &record.document_id);
    crate::ops_log::log_op(
        &file,
        &format!(
            "controller_attach_pane session={} pane={} generation={} state={}",
            session_id,
            pane_id,
            record.generation,
            record.state.as_str()
        ),
    );
    Ok(record)
}

fn handle_operator_command(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<DispatchAuthorization> {
    let file = request_file(&request)?;
    let command_kind = request_string(&request.command_kind, "command_kind")?;
    let diagnostic_payload = request
        .diagnostic_payload
        .as_deref()
        .unwrap_or_default()
        .to_string();
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let Some(record) = load_actor_record(&bootstrap.project_root, &document_id)? else {
        let _ = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            &document_id,
            0,
            &command_kind,
            None,
            Some("missing_actor"),
            &diagnostic_payload,
        );
        anyhow::bail!(
            "operator command `{}` rejected for {}: stage=missing_actor",
            command_kind,
            file.display()
        );
    };
    if matches!(
        record.state,
        crate::session_actor::ActorState::Blocked | crate::session_actor::ActorState::Closed
    ) {
        let failed_stage = record.state.as_str();
        let _ = insert_dispatch_attempt_record(
            &bootstrap.project_root,
            &document_id,
            record.generation,
            &command_kind,
            None,
            Some(failed_stage),
            &diagnostic_payload,
        );
        anyhow::bail!(
            "operator command `{}` rejected for {}: generation {} is {}",
            command_kind,
            file.display(),
            record.generation,
            failed_stage
        );
    }
    let accepted_stage = format!("operator_{}", record.state.as_str());
    insert_dispatch_attempt_record(
        &bootstrap.project_root,
        &document_id,
        record.generation,
        &command_kind,
        Some(&accepted_stage),
        None,
        &diagnostic_payload,
    )?;
    crate::ops_log::log_op(
        &file,
        &format!(
            "controller_operator_command_accepted kind={} session={} pane={} generation={} stage={}",
            command_kind, record.session_id, record.pane_id, record.generation, accepted_stage
        ),
    );
    Ok(DispatchAuthorization {
        record,
        accepted_stage,
    })
}

fn project_root_from_arg(root: Option<&Path>) -> Result<PathBuf> {
    let cwd;
    let start = match root {
        Some(path) => path,
        None => {
            cwd = std::env::current_dir()?;
            &cwd
        }
    };
    crate::snapshot::find_project_root(start)
        .or_else(|| {
            if start.join(".git").exists() || start.join(".agent-doc").exists() {
                Some(start.to_path_buf())
            } else {
                None
            }
        })
        .with_context(|| format!("no project root found from {}", start.display()))
}

pub fn run_status(root: Option<&Path>, ensure: bool) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    if ensure {
        ensure_controller_running(&project_root, LaunchMode::Lazy)?;
    }
    println!("{}", serde_json::to_string_pretty(&status(&project_root)?)?);
    Ok(())
}

pub fn run_serve(root: Option<&Path>, launch_mode: &str) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    serve(&project_root, LaunchMode::parse(launch_mode)?)
}

pub fn run_shutdown(root: Option<&Path>) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    println!("{}", request(&project_root, "shutdown")?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn controller_paths_are_project_local() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            socket_path(dir.path()),
            dir.path().join(".agent-doc/controller.sock")
        );
        assert_eq!(
            launch_lock_path(dir.path()),
            dir.path().join(".agent-doc/locks/controller-launch.lock")
        );
        assert_eq!(
            state_path(dir.path()),
            dir.path().join(".agent-doc/controller-state.json")
        );
    }

    fn actor_record(
        document_id: &str,
        pane: &str,
        window: &str,
    ) -> crate::session_actor::ActorRecord {
        crate::session_actor::ActorRecord {
            document_id: document_id.to_string(),
            session_id: "session-1".to_string(),
            generation: 1,
            pane_id: pane.to_string(),
            window_id: window.to_string(),
            harness: "codex".to_string(),
            state: crate::session_actor::ActorState::Starting,
            last_transition: crate::session_actor::ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 10,
                prior_generation: 0,
                new_generation: 1,
            },
        }
    }

    #[test]
    fn actor_store_writes_sqlite_before_actor_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%41", "@1");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT pane_id FROM documents WHERE document_id = ?1",
                params![document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "%41");

        let projection: BTreeMap<String, crate::session_actor::ActorRecord> = serde_json::from_str(
            &std::fs::read_to_string(actor_projection_path(dir.path())).unwrap(),
        )
        .unwrap();
        assert_eq!(projection.get(&record.document_id).unwrap(), &record);
    }

    #[test]
    fn sessions_projection_reconciles_existing_registry_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            document_id.clone(),
            crate::sessions::SessionEntry {
                pane: "%old".to_string(),
                pid: 123,
                cwd: dir.path().to_string_lossy().to_string(),
                started: "1".to_string(),
                session_id: "old-session".to_string(),
                file: document_id.clone(),
                window: "@old".to_string(),
                supervisor_instance_id: "supervisor-1".to_string(),
            },
        );
        crate::sessions::save_in(dir.path(), &registry).unwrap();

        let record = actor_record(&document_id, "%51", "@2");
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let projected = crate::sessions::load_in(dir.path()).unwrap();
        let entry = projected.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%51");
        assert_eq!(entry.window, "@2");
        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.pid, 123);
        assert_eq!(entry.supervisor_instance_id, "supervisor-1");
    }

    #[test]
    fn missing_sessions_projection_records_drift_diagnostic() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%61", "@3");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let message: String = conn
            .query_row(
                "SELECT message FROM projection_diagnostics WHERE projection = 'sessions.json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(message.contains("no registry entry"));
    }

    #[test]
    fn singleton_launch_lock_rejects_concurrent_holder() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = LaunchLock::acquire(dir.path()).unwrap();
        let second = LaunchLock::acquire(dir.path());
        assert!(second.is_err());
        drop(first);
        assert!(LaunchLock::acquire(dir.path()).is_ok());
    }

    #[test]
    fn bootstrap_state_round_trips_launch_mode_and_epoch() {
        let dir = tempfile::TempDir::new().unwrap();
        let written = write_bootstrap(dir.path(), LaunchMode::Lazy).unwrap();
        let read = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(read.launch_mode, LaunchMode::Lazy);
        assert_eq!(read.bootstrap_epoch, written.bootstrap_epoch);
        assert_eq!(read.controller_binary, written.controller_binary);
        assert_eq!(
            read.controller_binary,
            Some(current_binary_identity().unwrap())
        );
        assert_eq!(
            read.socket_path,
            dir.path().join(".agent-doc/controller.sock")
        );
    }

    #[test]
    fn controller_status_reports_startup_binary_identity() {
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let response = handle_request(
            &(serde_json::json!({ "command": "status" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();

        assert!(status.active);
        assert_eq!(status.controller_binary, bootstrap.controller_binary);
        assert!(controller_status_matches_current_binary(&status).unwrap());
    }

    #[test]
    fn controller_client_response_read_times_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let sock = socket_path(dir.path());
        std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
        let name = sock.clone().to_fs_name::<GenericFilePath>().unwrap();
        let listener = ListenerOptions::new().name(name).create_sync().unwrap();
        let handle = std::thread::spawn(move || {
            let _stream = listener.accept().unwrap();
            std::thread::sleep(CONTROLLER_RPC_TIMEOUT * 2);
        });

        let started = Instant::now();
        let err = request(dir.path(), "status").unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "controller request should fail within the bounded timeout"
        );
        assert!(
            err.to_string().contains("timed out") || format!("{err:#}").contains("timed out"),
            "{err:#}"
        );
        handle.join().unwrap();
    }

    #[test]
    fn idle_controller_client_does_not_block_later_status_request() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let idle_stream = connect(&project_root).unwrap();
        let response = request(&project_root, "status").unwrap();
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();
        assert!(status.active);
        assert_eq!(status.project_root, project_root);

        drop(idle_stream);
        let shutdown = request(&project_root, "shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    #[test]
    fn run_status_ensure_does_not_hold_idle_controller_stream() {
        let dir = tempfile::TempDir::new().unwrap();
        let project_root = dir.path().to_path_buf();
        let server_root = project_root.clone();
        let handle = std::thread::spawn(move || serve(&server_root, LaunchMode::Lazy).unwrap());
        wait_for_test_controller(&project_root);

        let idle_stream = connect(&project_root).unwrap();
        let started = Instant::now();
        run_status(Some(&project_root), true).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "controller status --ensure should complete without holding an idle stream"
        );

        drop(idle_stream);
        let shutdown = request(&project_root, "shutdown").unwrap();
        assert!(shutdown.contains("\"ok\":true"), "{shutdown}");
        handle.join().unwrap();
    }

    fn wait_for_test_controller(project_root: &Path) {
        let started = Instant::now();
        loop {
            if connect(project_root).is_ok() {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "test controller did not start"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn missing_or_changed_controller_binary_identity_is_stale() {
        let current = current_binary_identity().unwrap();
        let missing = ControllerStatus {
            active: true,
            project_root: PathBuf::from("/tmp/project"),
            socket_path: PathBuf::from("/tmp/project/.agent-doc/controller.sock"),
            launch_mode: Some(LaunchMode::Lazy),
            bootstrap_epoch: Some(1),
            pid: Some(2),
            controller_binary: None,
        };
        assert!(!controller_status_matches_current_binary(&missing).unwrap());

        let mut changed = current.clone();
        changed.modified_nanos = changed.modified_nanos.wrapping_add(1);
        let stale = ControllerStatus {
            controller_binary: Some(changed),
            ..missing
        };
        assert!(!controller_status_matches_current_binary(&stale).unwrap());

        let fresh = ControllerStatus {
            controller_binary: Some(current),
            ..stale
        };
        assert!(controller_status_matches_current_binary(&fresh).unwrap());
    }

    fn test_bootstrap(dir: &tempfile::TempDir) -> ControllerBootstrap {
        ControllerBootstrap {
            project_root: dir.path().to_path_buf(),
            socket_path: socket_path(dir.path()),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 123,
            pid: 456,
            controller_binary: Some(current_binary_identity().unwrap()),
        }
    }

    #[test]
    fn controller_start_register_and_lifecycle_update_actor_and_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-controller\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-controller".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: Some("@1".to_string()),
            generation: Some(1),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&start).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(record.state, crate::session_actor::ActorState::Starting);

        let register = ControllerRequest {
            command: "register_supervisor".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-controller".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("starting".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: Some(999),
            supervisor_socket: Some("/tmp/agent-doc-test.sock".to_string()),
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&register).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);

        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-controller".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("ready".to_string()),
            caller: Some("supervisor".to_string()),
            reason: Some("prompt_ready".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&lifecycle).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(record.state, crate::session_actor::ActorState::Ready);
        assert_eq!(record.last_transition.reason, "prompt_ready");

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let (pid, socket, runtime_state): (i64, String, String) = conn
            .query_row(
                "SELECT supervisor_pid, supervisor_socket, runtime_state FROM supervisor_leases WHERE document_id = ?1 AND generation = 1",
                params![doc.to_string_lossy().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(pid, 999);
        assert_eq!(socket, "/tmp/agent-doc-test.sock");
        assert_eq!(runtime_state, "ready");
    }

    #[test]
    fn controller_lifecycle_rejects_stale_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stale.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-stale\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-stale", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &doc.to_string_lossy(),
            "session-stale",
            "%42",
            "@2",
            "sync",
            "recover_owner",
        )
        .unwrap();

        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-stale".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("ready".to_string()),
            caller: Some("supervisor".to_string()),
            reason: Some("prompt_ready".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&lifecycle).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        assert!(envelope.error.unwrap().contains("no longer current"));
    }

    #[test]
    fn controller_actor_binding_and_dispatch_use_authoritative_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/route.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-route\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-route", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-route",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let binding = ControllerRequest {
            command: "actor_binding".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&binding).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<Option<crate::session_actor::ActorRecord>> =
            serde_json::from_str(&response).unwrap();
        assert_eq!(envelope.data.unwrap().unwrap().pane_id, "%41");

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-route".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("test dispatch".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&dispatch).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        assert_eq!(envelope.data.unwrap().accepted_stage, "ready");

        let stale = ControllerRequest {
            generation: Some(0),
            ..dispatch
        };
        let response = handle_request(
            &(serde_json::to_string(&stale).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        assert!(envelope.error.unwrap().contains("requested generation 0"));

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let accepted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE accepted_stage = 'ready'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let failed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'stale_generation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 1);
        assert_eq!(failed, 1);
    }

    #[test]
    fn controller_session_operator_status_reports_history_and_command_stages() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/operator.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-operator\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-operator", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-operator",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let operator_command = ControllerRequest {
            command: "operator_command".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("session_clear".to_string()),
            diagnostic_payload: Some("test operator command".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&operator_command).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        assert_eq!(envelope.data.unwrap().accepted_stage, "operator_ready");

        let status = ControllerRequest {
            command: "session_status".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&status).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<SessionOperatorStatus> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let status = envelope.data.unwrap();
        assert_eq!(
            status.record.unwrap().state,
            crate::session_actor::ActorState::Ready
        );
        assert_eq!(status.transitions.len(), 2);
        assert_eq!(
            status
                .dispatch_attempts
                .last()
                .unwrap()
                .accepted_stage
                .as_deref(),
            Some("operator_ready")
        );
    }

    #[test]
    fn controller_attach_pane_creates_manual_attach_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/attach.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-attach\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-attach", "%41", "@1", 1)
            .unwrap();

        let attach = ControllerRequest {
            command: "attach_pane".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-attach".to_string()),
            pane_id: Some("%42".to_string()),
            window_id: Some("@2".to_string()),
            generation: None,
            state: None,
            caller: Some("session".to_string()),
            reason: Some("manual_attach".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&attach).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<crate::session_actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(record.pane_id, "%42");
        assert_eq!(record.window_id, "@2");
        assert_eq!(record.generation, 2);
        assert_eq!(record.last_transition.caller, "session");
        assert_eq!(record.last_transition.reason, "manual_attach");
    }
}
