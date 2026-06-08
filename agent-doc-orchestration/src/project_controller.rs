//! Project-local controller shell.
//!
//! The controller is the live authority for document session actor lookup,
//! generation changes, lifecycle reports, and routed dispatch acceptance.
//! `sessions.json` and tmux state remain projections and layout inputs.

use agent_doc_sqlite::state_store;
use anyhow::{Context, Result};
use fs2::FileExt;
use interprocess::local_socket::{
    GenericFilePath, ListenerNonblockingMode, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

// The SQLite state layer (the only `rusqlite::Connection` surface) lives in
// `agent-doc-sqlite::state_store`. The status types are re-exported here so the
// IPC/serde call sites that name `project_controller::SessionOperatorStatus`,
// etc. stay unchanged, and the helpers used by the SQL-glue functions below are
// imported by their original names.
pub use state_store::{
    ActorTransitionStatus, DispatchAttemptStatus, ProjectionDiagnosticStatus,
    SessionOperatorStatus, SupervisorLeaseStatus, state_db_path,
};
use state_store::{
    Connection, insert_projection_diagnostic, load_actor_record_from_db, load_actor_store_from_db,
    load_layout_state_from_db, load_session_operator_status_from_db, load_supervisor_lease_from_db,
    open_state_db, store_layout_state_in_db, timestamp_secs,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SOCKET_FILE: &str = "controller.sock";
const STATE_FILE: &str = "controller-state.json";
const ACTOR_PROJECTION_FILE: &str = "session-actors.json";
const LAYOUT_PROJECTION_FILE: &str = "last_layout.json";
const DEFAULT_LAYOUT_SCOPE: &str = "default";
const LOCK_FILE: &str = "controller-launch.lock";
const CONNECT_WAIT: Duration = Duration::from_secs(3);
const CONNECT_POLL: Duration = Duration::from_millis(50);
#[cfg(not(any(test, feature = "test-support")))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(any(test, feature = "test-support"))]
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
    #[serde(default = "default_controller_generation")]
    pub controller_generation: u64,
    #[serde(default)]
    pub handoff_state: ControllerHandoffState,
    #[serde(default)]
    pub handoff_started_at: Option<u64>,
    #[serde(default)]
    pub previous_controller_pid: Option<u32>,
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
    #[serde(default)]
    pub controller_generation: Option<u64>,
    #[serde(default)]
    pub handoff_state: Option<ControllerHandoffState>,
    #[serde(default)]
    pub handoff_started_at: Option<u64>,
    #[serde(default)]
    pub previous_controller_pid: Option<u32>,
    #[serde(default)]
    pub stale_duplicate_pids: Vec<u32>,
    #[serde(default = "default_control_plane_status")]
    pub control_plane: ControlPlaneStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPlaneStatus {
    pub process_model: String,
    pub external_boundary: String,
    pub state_authority: String,
    pub projection_authority: String,
    pub dispatch_actor: ControlPlaneActorStatus,
    pub store_actor: ControlPlaneActorStatus,
    pub session_actors: ControlPlaneActorStatus,
    pub supervisor_adapters: ControlPlaneActorStatus,
    pub projection_workers: ControlPlaneActorStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPlaneActorStatus {
    pub role: String,
    pub authority: String,
    pub state: String,
    pub owned_items: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerHandoffState {
    #[default]
    Stable,
    Preparing,
    Promoted,
    Retiring,
    Failed,
}

fn default_controller_generation() -> u64 {
    1
}

fn default_control_plane_status() -> ControlPlaneStatus {
    ControlPlaneStatus {
        process_model: "project_scoped_single_process".to_string(),
        external_boundary: "controller_ipc".to_string(),
        state_authority: ".agent-doc/state.db".to_string(),
        projection_authority: "compatibility_output".to_string(),
        dispatch_actor: ControlPlaneActorStatus {
            role: "dispatch_actor".to_string(),
            authority: "mutating_command_admission".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
        },
        store_actor: ControlPlaneActorStatus {
            role: "store_actor".to_string(),
            authority: "sqlite_write_serialization".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
        },
        session_actors: ControlPlaneActorStatus {
            role: "session_actor".to_string(),
            authority: "per_document_generation".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
        },
        supervisor_adapters: ControlPlaneActorStatus {
            role: "supervisor_adapter".to_string(),
            authority: "managed_harness_child".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
        },
        projection_workers: ControlPlaneActorStatus {
            role: "projection_worker".to_string(),
            authority: "compatibility_projection".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
        },
    }
}

fn sqlite_count(conn: &Connection, sql: &str, label: &str) -> Result<usize> {
    let count: i64 = conn
        .query_row(sql, [], |row| row.get(0))
        .with_context(|| format!("failed to count {label} in controller state"))?;
    usize::try_from(count).with_context(|| format!("{label} count is negative"))
}

fn control_plane_status(project_root: &Path, active: bool) -> Result<ControlPlaneStatus> {
    let conn = open_state_db(project_root)?;
    let document_rows = sqlite_count(&conn, "SELECT COUNT(*) FROM documents", "actor documents")?;
    let live_documents = sqlite_count(
        &conn,
        "SELECT COUNT(*) FROM documents WHERE actor_state != 'closed'",
        "live actor documents",
    )?;
    let supervisor_leases = sqlite_count(
        &conn,
        "SELECT COUNT(*) FROM supervisor_leases",
        "supervisor leases",
    )?;
    let dispatch_receipts = sqlite_count(
        &conn,
        "SELECT COUNT(*) FROM dispatch_attempts",
        "dispatch attempts",
    )?;
    let projection_diagnostics = sqlite_count(
        &conn,
        "SELECT COUNT(*) FROM projection_diagnostics",
        "projection diagnostics",
    )?;
    let layout_rows = sqlite_count(&conn, "SELECT COUNT(*) FROM layout_states", "layout states")?;
    let actor_state = if active { "ready" } else { "offline" };
    let store_state = if active { "ready" } else { "durable_offline" };

    Ok(ControlPlaneStatus {
        dispatch_actor: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: dispatch_receipts,
            ..default_control_plane_status().dispatch_actor
        },
        store_actor: ControlPlaneActorStatus {
            state: store_state.to_string(),
            owned_items: document_rows
                + supervisor_leases
                + dispatch_receipts
                + projection_diagnostics
                + layout_rows,
            ..default_control_plane_status().store_actor
        },
        session_actors: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: live_documents,
            ..default_control_plane_status().session_actors
        },
        supervisor_adapters: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: supervisor_leases,
            ..default_control_plane_status().supervisor_adapters
        },
        projection_workers: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: projection_diagnostics,
            ..default_control_plane_status().projection_workers
        },
        ..default_control_plane_status()
    })
}

fn controller_status_from_bootstrap(
    bootstrap: &ControllerBootstrap,
    active: bool,
) -> Result<ControllerStatus> {
    Ok(ControllerStatus {
        active,
        project_root: bootstrap.project_root.clone(),
        socket_path: bootstrap.socket_path.clone(),
        launch_mode: Some(bootstrap.launch_mode),
        bootstrap_epoch: Some(bootstrap.bootstrap_epoch),
        pid: Some(bootstrap.pid),
        controller_binary: bootstrap.controller_binary.clone(),
        controller_generation: Some(bootstrap.controller_generation),
        handoff_state: Some(bootstrap.handoff_state),
        handoff_started_at: bootstrap.handoff_started_at,
        previous_controller_pid: bootstrap.previous_controller_pid,
        stale_duplicate_pids: discover_stale_duplicate_pids(
            &bootstrap.project_root,
            Some(bootstrap.pid),
        ),
        control_plane: control_plane_status(&bootstrap.project_root, active)?,
    })
}

fn inactive_controller_status(
    project_root: &Path,
    bootstrap: Option<ControllerBootstrap>,
) -> Result<ControllerStatus> {
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
        controller_generation: bootstrap.as_ref().map(|state| state.controller_generation),
        handoff_state: bootstrap.as_ref().map(|state| state.handoff_state),
        handoff_started_at: bootstrap
            .as_ref()
            .and_then(|state| state.handoff_started_at),
        previous_controller_pid: bootstrap
            .as_ref()
            .and_then(|state| state.previous_controller_pid),
        stale_duplicate_pids: discover_stale_duplicate_pids(project_root, None),
        control_plane: control_plane_status(project_root, false)?,
    })
}

fn parse_handoff_state(raw: &str) -> Result<ControllerHandoffState> {
    match raw {
        "stable" => Ok(ControllerHandoffState::Stable),
        "preparing" => Ok(ControllerHandoffState::Preparing),
        "promoted" => Ok(ControllerHandoffState::Promoted),
        "retiring" => Ok(ControllerHandoffState::Retiring),
        "failed" => Ok(ControllerHandoffState::Failed),
        other => anyhow::bail!("unknown controller handoff state: {other}"),
    }
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
pub struct SupervisorHeartbeatRequest {
    pub file: PathBuf,
    pub session_id: String,
    pub pane_id: String,
    pub generation: u64,
    pub supervisor_pid: Option<u32>,
    pub supervisor_socket: Option<String>,
    pub runtime_state: String,
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

// `ActorTransitionStatus`, `SupervisorLeaseStatus`, `DispatchAttemptStatus`,
// `ProjectionDiagnosticStatus`, and `SessionOperatorStatus` now live in
// `agent-doc-sqlite::state_store` (they depend on the storage types stored
// there) and are re-exported at the top of this module so the IPC/serde call
// sites stay unchanged.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorBindingStatus {
    Bound,
    NotFound,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorBindingResponse {
    pub status: ActorBindingStatus,
    #[serde(default)]
    pub record: Option<crate::session_actor::ActorRecord>,
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

fn actor_projection_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(ACTOR_PROJECTION_FILE)
}

fn layout_projection_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc").join(LAYOUT_PROJECTION_FILE)
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
    let path = current_agent_doc_binary()?;
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

fn current_agent_doc_binary() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    resolve_agent_doc_binary_from_env(
        std::env::current_exe().ok(),
        std::env::args_os().next(),
        std::env::var_os("PATH"),
        &cwd,
    )
}

fn resolve_agent_doc_binary_from_env(
    current_exe: Option<PathBuf>,
    argv0: Option<OsString>,
    path_env: Option<OsString>,
    cwd: &Path,
) -> Result<PathBuf> {
    let stale_current_exe = match current_exe {
        Some(path) if launchable_file(&path) => return Ok(path),
        other => other,
    };

    let mut path_search_names = Vec::new();
    if let Some(raw_argv0) = argv0.as_deref() {
        let argv0_path = Path::new(raw_argv0);
        if has_path_separator(argv0_path) {
            let candidate = if argv0_path.is_absolute() {
                argv0_path.to_path_buf()
            } else {
                cwd.join(argv0_path)
            };
            if launchable_file(&candidate) {
                return Ok(candidate);
            }
        } else if !raw_argv0.is_empty() {
            path_search_names.push(raw_argv0.to_os_string());
        }
    }
    if !path_search_names
        .iter()
        .any(|name| name == OsStr::new("agent-doc"))
    {
        path_search_names.push(OsString::from("agent-doc"));
    }

    if let Some(path_env) = path_env {
        for dir in std::env::split_paths(&path_env) {
            for name in &path_search_names {
                let candidate = dir.join(name);
                if launchable_file(&candidate) {
                    return Ok(candidate);
                }
            }
        }
    }

    let stale = stale_current_exe
        .as_ref()
        .map(|path| format!("; skipped missing current_exe {}", path.display()))
        .unwrap_or_default();
    anyhow::bail!("failed to locate launchable agent-doc binary{stale}");
}

fn launchable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn has_path_separator(path: &Path) -> bool {
    path.is_absolute() || path.components().count() > 1
}

fn write_bootstrap(project_root: &Path, launch_mode: LaunchMode) -> Result<ControllerBootstrap> {
    let prior = read_bootstrap(project_root)?.map(|state| state.controller_generation);
    write_bootstrap_with_options(
        project_root,
        socket_path(project_root),
        launch_mode,
        prior.unwrap_or(0).saturating_add(1).max(1),
        ControllerHandoffState::Stable,
        None,
    )
}

fn write_bootstrap_with_options(
    project_root: &Path,
    advertised_socket_path: PathBuf,
    launch_mode: LaunchMode,
    controller_generation: u64,
    handoff_state: ControllerHandoffState,
    previous_controller_pid: Option<u32>,
) -> Result<ControllerBootstrap> {
    let bootstrap = ControllerBootstrap {
        project_root: project_root.to_path_buf(),
        socket_path: advertised_socket_path,
        launch_mode,
        bootstrap_epoch: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        pid: std::process::id(),
        controller_binary: Some(current_binary_identity()?),
        controller_generation,
        handoff_state,
        handoff_started_at: matches!(
            handoff_state,
            ControllerHandoffState::Preparing | ControllerHandoffState::Promoted
        )
        .then(timestamp_secs),
        previous_controller_pid,
    };
    write_bootstrap_state(&bootstrap)?;
    Ok(bootstrap)
}

fn write_bootstrap_state(bootstrap: &ControllerBootstrap) -> Result<()> {
    let path = state_path(&bootstrap.project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&bootstrap)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

// The SQLite connection layer (`open_state_db`, `initialize_state_db`,
// `sqlite_i64`/`sqlite_u64`, `timestamp_secs`, `actor_record_from_row`, the
// `load_*_from_db` readers, `insert_actor_transition`, `upsert_actor_document`,
// `insert_projection_diagnostic`, and the layout-state helpers) now lives in
// `agent-doc-sqlite::state_store` and is imported at the top of this module.
// The functions below remain in orchestration because they stitch the SQL
// primitives together with ops-log, projection, and bootstrap glue.

/// Compute the lifted bootstrap tendril for `upsert_actor_document`:
/// `(launch_mode_short_string, controller_epoch)` derived from `read_bootstrap`.
fn actor_document_bootstrap_columns(project_root: &Path) -> Result<(Option<String>, Option<i64>)> {
    let bootstrap = read_bootstrap(project_root).ok().flatten();
    let launch_mode = bootstrap
        .as_ref()
        .map(|state| state.launch_mode.as_str().to_string());
    let controller_epoch = bootstrap
        .as_ref()
        .map(|state| state_store::sqlite_i64(state.bootstrap_epoch, "bootstrap_epoch"))
        .transpose()?;
    Ok((launch_mode, controller_epoch))
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

/// Migrate a legacy `session-actors.json` projection into empty sqlite state.
///
/// Glue: the count gate and JSON load stay in orchestration (the `.json`
/// read goes through `crate::fs_util`); the actual transition+document
/// transaction lives in `state_store`, fed the lifted `read_bootstrap`
/// `launch_mode`/`controller_epoch` tendril.
fn migrate_legacy_actor_projection(project_root: &Path, conn: &mut Connection) -> Result<()> {
    if !state_store::actor_documents_empty(conn)? {
        return Ok(());
    }
    let Some(store) = legacy_actor_projection(project_root)? else {
        return Ok(());
    };
    let (launch_mode, controller_epoch) = actor_document_bootstrap_columns(project_root)?;
    state_store::migrate_actor_store_tx(conn, &store, launch_mode, controller_epoch)
}

fn upsert_supervisor_lease(
    project_root: &Path,
    record: &crate::session_actor::ActorRecord,
    supervisor_pid: Option<u32>,
    supervisor_socket: Option<&str>,
    runtime_state: &str,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
    state_store::upsert_supervisor_lease_in_db(
        &conn,
        record,
        supervisor_pid,
        supervisor_socket,
        runtime_state,
    )
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
    state_store::insert_dispatch_attempt_in_db(
        &conn,
        document_id,
        generation,
        command_kind,
        accepted_stage,
        failed_stage,
        diagnostic_payload,
    )
}

pub fn load_actor_store(
    project_root: &Path,
) -> Result<BTreeMap<String, crate::session_actor::ActorRecord>> {
    let mut conn = open_state_db(project_root)?;
    migrate_legacy_actor_projection(project_root, &mut conn)?;
    load_actor_store_from_db(&conn)
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
    let (launch_mode, controller_epoch) = actor_document_bootstrap_columns(project_root)?;
    let evicted_document_ids = state_store::store_actor_record_tx(
        &mut conn,
        expected_prior_generation,
        record,
        launch_mode,
        controller_epoch,
    )?;

    if let Err(err) = emit_actor_projection(project_root) {
        record_projection_diagnostic(
            project_root,
            "session-actors.json",
            &record.document_id,
            &format!("failed to emit actor projection after sqlite commit: {err}"),
        );
    }
    for evicted_document_id in evicted_document_ids {
        let _ = project_sessions_projection_for_actor(project_root, &evicted_document_id);
    }
    if record.last_transition.caller == "start" && record.last_transition.reason == "session_start"
    {
        let _ = project_sessions_projection_for_actor(project_root, &record.document_id);
    }
    Ok(record.clone())
}

fn supervisor_lease_is_fresh_or_alive(
    lease: &SupervisorLeaseStatus,
    now: u64,
    stale_after: Duration,
) -> bool {
    let fresh_heartbeat = lease
        .last_heartbeat
        .map(|timestamp| now.saturating_sub(timestamp) <= stale_after.as_secs())
        .unwrap_or(false);
    if !fresh_heartbeat {
        return false;
    }
    lease.supervisor_pid.map(process_is_alive).unwrap_or(false)
}

pub fn close_stale_starting_actors_for_caller(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let now = timestamp_secs();
    let store = load_actor_store(project_root)?;
    let conn = open_state_db(project_root)?;
    let mut closed = 0;
    let mut kept = 0;

    for record in store.values() {
        if record.state != crate::session_actor::ActorState::Starting {
            continue;
        }
        let age = now.saturating_sub(record.last_transition.timestamp);
        if age <= stale_after.as_secs() {
            kept += 1;
            continue;
        }

        let lease = load_supervisor_lease_from_db(&conn, &record.document_id, record.generation)?;
        if lease
            .as_ref()
            .is_some_and(|lease| supervisor_lease_is_fresh_or_alive(lease, now, stale_after))
        {
            kept += 1;
            continue;
        }

        if dry_run {
            eprintln!(
                "[{}] would close stale starting actor: {} session={} pane={} generation={} age_secs={}",
                caller,
                record.document_id,
                record.session_id,
                record.pane_id,
                record.generation,
                age
            );
            closed += 1;
            continue;
        }

        let mut next = record.clone();
        next.state = crate::session_actor::ActorState::Closed;
        next.last_transition = crate::session_actor::ActorLastTransition {
            caller: caller.to_string(),
            reason: "stale_starting_actor".to_string(),
            timestamp: now,
            prior_generation: record.generation,
            new_generation: record.generation,
        };
        store_actor_record(project_root, Some(record.generation), &next)?;
        crate::ops_log::log_op(
            Path::new(&record.document_id),
            &format!(
                "{}_closed_stale_starting_actor file={} session={} pane={} generation={} age_secs={}",
                caller,
                record.document_id,
                record.session_id,
                record.pane_id,
                record.generation,
                age
            ),
        );
        closed += 1;
    }

    Ok((closed, kept))
}

pub fn close_stale_starting_actors(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
) -> Result<(usize, usize)> {
    close_stale_starting_actors_for_caller(project_root, stale_after, dry_run, "gc")
}

/// #monsterrod-pane-cross-doc-contamination / "1 pane = 1 document": repair-only
/// cleanup for stale actor aliases. Normal actor binding paths must refuse a
/// non-closed cross-document pane alias before storing the new owner; they must
/// not call this helper to commandeer an existing document pane. Best-effort per
/// record: a CAS failure (a concurrent writer raced this eviction) logs and skips
/// that record rather than failing the caller. Returns the number of evicted
/// bindings.
pub fn evict_cross_document_pane_bindings(
    project_root: &Path,
    owner_document_id: &str,
    pane_id: &str,
    caller: &str,
) -> Result<usize> {
    if pane_id.is_empty() {
        return Ok(0);
    }
    let now = timestamp_secs();
    let store = load_actor_store(project_root)?;
    let mut evicted = 0;
    for record in store.values() {
        if record.document_id == owner_document_id || record.pane_id != pane_id {
            continue;
        }
        let mut next = record.clone();
        next.state = crate::session_actor::ActorState::Closed;
        next.pane_id.clear();
        next.window_id.clear();
        next.last_transition = crate::session_actor::ActorLastTransition {
            caller: caller.to_string(),
            reason: format!("evicted_cross_document_pane owner={owner_document_id} pane={pane_id}"),
            timestamp: now,
            prior_generation: record.generation,
            new_generation: record.generation,
        };
        match store_actor_record(project_root, Some(record.generation), &next) {
            Ok(_) => {
                crate::ops_log::log_op(
                    Path::new(&record.document_id),
                    &format!(
                        "{}_evicted_cross_document_pane_binding stale_document={} owner_document={} pane={} generation={} prior_state={}",
                        caller,
                        record.document_id,
                        owner_document_id,
                        pane_id,
                        record.generation,
                        record.state.as_str()
                    ),
                );
                evicted += 1;
            }
            Err(err) => {
                crate::ops_log::log_op(
                    Path::new(&record.document_id),
                    &format!(
                        "{}_evict_cross_document_pane_binding_skipped stale_document={} pane={} generation={} error={}",
                        caller, record.document_id, pane_id, record.generation, err
                    ),
                );
            }
        }
    }
    Ok(evicted)
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

fn migrate_legacy_layout_projection(project_root: &Path, conn: &Connection) -> Result<()> {
    if state_store::layout_scope_exists(conn, DEFAULT_LAYOUT_SCOPE)? {
        return Ok(());
    }

    let path = layout_projection_path(project_root);
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(());
    };
    match serde_json::from_str::<Vec<String>>(&content) {
        Ok(columns) => store_layout_state_in_db(conn, DEFAULT_LAYOUT_SCOPE, &columns),
        Err(err) => {
            let _ = insert_projection_diagnostic(
                conn,
                LAYOUT_PROJECTION_FILE,
                "__layout__",
                &format!("failed to migrate legacy layout projection: {err}"),
            );
            Ok(())
        }
    }
}

fn emit_layout_projection(project_root: &Path) -> Result<()> {
    let conn = open_state_db(project_root)?;
    let layout_state = load_layout_state_from_db(&conn, DEFAULT_LAYOUT_SCOPE)?;
    let path = layout_projection_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string(&layout_state)?;
    std::fs::write(&path, &content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    let written = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let projected: Vec<String> = serde_json::from_str(&written)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if projected != layout_state {
        anyhow::bail!("last_layout.json projection drifted from sqlite state");
    }
    Ok(())
}

pub fn load_layout_state(project_root: &Path) -> Result<Vec<String>> {
    let conn = open_state_db(project_root)?;
    migrate_legacy_layout_projection(project_root, &conn)?;
    load_layout_state_from_db(&conn, DEFAULT_LAYOUT_SCOPE)
}

pub fn store_layout_state(project_root: &Path, columns: &[String]) -> Result<()> {
    let conn = open_state_db(project_root)?;
    store_layout_state_in_db(&conn, DEFAULT_LAYOUT_SCOPE, columns)?;
    if let Err(err) = emit_layout_projection(project_root) {
        record_projection_diagnostic(
            project_root,
            LAYOUT_PROJECTION_FILE,
            "__layout__",
            &format!("failed to emit layout projection after sqlite commit: {err}"),
        );
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
    if record.state == crate::session_actor::ActorState::Closed || record.pane_id.is_empty() {
        if registry.remove(document_id).is_none() {
            return Ok(());
        }
        if let Err(err) = crate::sessions::save_in(project_root, &registry) {
            record_projection_diagnostic(
                project_root,
                "sessions.json",
                document_id,
                &format!("failed to remove closed actor projection: {err}"),
            );
            return Ok(());
        }
        return Ok(());
    }
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
        let _ = insert_projection_diagnostic(&conn, projection, document_id, message);
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
    connect_path(&socket_path(project_root))
}

fn connect_path(path: &Path) -> Result<interprocess::local_socket::Stream> {
    let name = path.to_fs_name::<GenericFilePath>()?;
    interprocess::local_socket::ConnectOptions::new()
        .name(name)
        .connect_sync()
        .context("failed to connect to project controller")
}

fn request(project_root: &Path, command: &str) -> Result<String> {
    request_path(&socket_path(project_root), command)
}

fn request_path(path: &Path, command: &str) -> Result<String> {
    let stream = connect_path(path)?;
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
    decode_controller_response(project_root, &request, response.trim())
}

fn decode_controller_response<T: DeserializeOwned>(
    project_root: &Path,
    request: &ControllerRequest,
    raw_response: &str,
) -> Result<T> {
    let envelope: ControllerEnvelope<T> =
        serde_json::from_str(raw_response).with_context(|| {
            format!(
                "failed to parse project controller response envelope for command `{}`: raw={}",
                request.command, raw_response
            )
        })?;
    if envelope.ok {
        match envelope.data {
            Some(data) => Ok(data),
            None => {
                if let Some(file) = request.file.as_ref() {
                    let log_file = if file.is_absolute() {
                        file.clone()
                    } else {
                        project_root.join(file)
                    };
                    crate::ops_log::log_op(
                        &log_file,
                        &format!(
                            "controller_response_missing_data command={} raw={}",
                            request.command, raw_response
                        ),
                    );
                }
                anyhow::bail!(
                    "project controller command `{}` returned ok response without data: raw={}",
                    request.command,
                    raw_response
                )
            }
        }
    } else {
        anyhow::bail!(
            "project controller command `{}` failed: {}",
            request.command,
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
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        return handle_mark_lifecycle(
            &bootstrap,
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
        );
    }

    #[cfg(not(any(test, feature = "test-support")))]
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

pub fn refresh_supervisor_lease(
    project_root: &Path,
    request: SupervisorHeartbeatRequest,
) -> Result<SupervisorLeaseStatus> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        return handle_supervisor_heartbeat(
            &bootstrap,
            ControllerRequest {
                command: "supervisor_heartbeat".to_string(),
                file: Some(request.file),
                session_id: Some(request.session_id),
                pane_id: Some(request.pane_id),
                window_id: None,
                generation: Some(request.generation),
                state: Some(request.runtime_state),
                caller: None,
                reason: None,
                supervisor_pid: request.supervisor_pid,
                supervisor_socket: request.supervisor_socket,
                command_kind: None,
                diagnostic_payload: None,
            },
        );
    }

    #[cfg(not(any(test, feature = "test-support")))]
    request_controller(
        project_root,
        ControllerRequest {
            command: "supervisor_heartbeat".to_string(),
            file: Some(request.file),
            session_id: Some(request.session_id),
            pane_id: Some(request.pane_id),
            window_id: None,
            generation: Some(request.generation),
            state: Some(request.runtime_state),
            caller: None,
            reason: None,
            supervisor_pid: request.supervisor_pid,
            supervisor_socket: request.supervisor_socket,
            command_kind: None,
            diagnostic_payload: None,
        },
    )
}

pub fn authoritative_actor_binding(
    project_root: &Path,
    file: &Path,
) -> Result<Option<crate::session_actor::ActorRecord>> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let document_id =
            crate::session_actor::canonical_document_id_in(project_root, &file.to_string_lossy());
        return load_actor_record(project_root, &document_id);
    }

    #[cfg(not(any(test, feature = "test-support")))]
    {
        let response: ActorBindingResponse = request_controller(
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
        )?;
        match response.status {
            ActorBindingStatus::Bound => response.record.map(Some).with_context(|| {
                format!(
                    "project controller command `actor_binding` returned bound status without record for {}",
                    file.display()
                )
            }),
            ActorBindingStatus::NotFound => Ok(None),
        }
    }
}

pub fn authorize_dispatch(
    project_root: &Path,
    request: DispatchRequest,
) -> Result<DispatchAuthorization> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
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

    #[cfg(not(any(test, feature = "test-support")))]
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
    #[cfg(any(test, feature = "test-support"))]
    {
        let document_id =
            crate::session_actor::canonical_document_id_in(project_root, &file.to_string_lossy());
        let mut conn = open_state_db(project_root)?;
        migrate_legacy_actor_projection(project_root, &mut conn)?;
        return load_session_operator_status_from_db(&conn, &document_id);
    }

    #[cfg(not(any(test, feature = "test-support")))]
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
    #[cfg(any(test, feature = "test-support"))]
    {
        let bootstrap = ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity()?),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
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

    #[cfg(not(any(test, feature = "test-support")))]
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
            status.stale_duplicate_pids = discover_stale_duplicate_pids(project_root, status.pid);
            Ok(status)
        }
        Err(_) => {
            let bootstrap = read_bootstrap(project_root)?;
            inactive_controller_status(project_root, bootstrap)
        }
    }
}

fn controller_status_matches_current_binary(status: &ControllerStatus) -> Result<bool> {
    Ok(status.controller_binary.as_ref() == Some(&current_binary_identity()?))
}

fn discover_stale_duplicate_pids(project_root: &Path, authoritative_pid: Option<u32>) -> Vec<u32> {
    let mut pids = BTreeSet::new();
    if let Ok(Some(state)) = read_bootstrap(project_root) {
        if Some(state.pid) != authoritative_pid {
            pids.insert(state.pid);
        }
        if let Some(pid) = state.previous_controller_pid
            && Some(pid) != authoritative_pid
        {
            pids.insert(pid);
        }
    }

    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            if Some(pid) == authoritative_pid || pid == std::process::id() {
                continue;
            }
            if is_same_project_controller_pid(project_root, pid) {
                pids.insert(pid);
            }
        }
    }

    pids.retain(|pid| {
        Some(*pid) != authoritative_pid && *pid != std::process::id() && process_is_alive(*pid)
    });
    pids.into_iter().collect()
}

fn is_same_project_controller_pid(project_root: &Path, pid: u32) -> bool {
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).to_string())
        .collect();
    args_match_same_project_controller(&args, project_root)
}

fn args_match_same_project_controller(args: &[String], project_root: &Path) -> bool {
    if !args.iter().any(|arg| arg.ends_with("agent-doc")) {
        return false;
    }
    if !args
        .windows(2)
        .any(|window| window[0] == "controller" && window[1] == "serve")
    {
        return false;
    }
    let Some(raw_root) = args
        .windows(2)
        .find_map(|window| (window[0] == "--project-root").then(|| PathBuf::from(&window[1])))
    else {
        return false;
    };
    canonical_path_for_compare(&raw_root) == canonical_path_for_compare(project_root)
}

fn canonical_path_for_compare(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn reap_verified_controller_pid(project_root: &Path, pid: u32, generation: u64) {
    if pid == std::process::id() || !is_same_project_controller_pid(project_root, pid) {
        return;
    }
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(750) {
        if !process_is_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if is_same_project_controller_pid(project_root, pid) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
        eprintln!(
            "[controller] reaped stale same-project controller pid={pid} generation={generation}"
        );
    }
}

fn reap_stale_duplicate_controllers(
    project_root: &Path,
    authoritative_pid: Option<u32>,
    generation: u64,
) {
    for pid in discover_stale_duplicate_pids(project_root, authoritative_pid) {
        reap_verified_controller_pid(project_root, pid, generation);
    }
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

fn handoff_stale_controller(
    project_root: &Path,
    launch_mode: LaunchMode,
    old_status: ControllerStatus,
) -> Result<interprocess::local_socket::Stream> {
    let public_sock = socket_path(project_root);
    let old_pid = old_status.pid;
    let old_generation = old_status.controller_generation.unwrap_or(1);
    let new_generation = old_generation.saturating_add(1).max(1);
    let temp_sock = project_root.join(".agent-doc").join(format!(
        "controller-handoff-{}-{}.sock",
        std::process::id(),
        new_generation
    ));
    let _ = std::fs::remove_file(&temp_sock);
    let _ = request(project_root, "prepare_handoff");

    launch_detached_at(
        project_root,
        launch_mode,
        Some(&temp_sock),
        Some(new_generation),
        old_pid,
        ControllerHandoffState::Preparing,
    )?;
    let _temp_stream = wait_for_controller_path(&temp_sock)?;
    let replacement_status: ControllerStatus = serde_json::from_str(
        &request_path(&temp_sock, "status").context("failed to read replacement status")?,
    )
    .context("failed to parse replacement controller status")?;
    let promoted_response = request_path(&temp_sock, "promote_handoff")?;
    if !promoted_response.contains("\"ok\":true") {
        anyhow::bail!("replacement controller refused promotion: {promoted_response}");
    }

    if public_sock.exists() {
        let _ = std::fs::remove_file(&public_sock);
    }
    std::fs::rename(&temp_sock, &public_sock).with_context(|| {
        format!(
            "failed to promote controller socket {} to {}",
            temp_sock.display(),
            public_sock.display()
        )
    })?;

    reap_stale_duplicate_controllers(project_root, replacement_status.pid, new_generation);

    wait_for_controller(project_root)
}

pub fn connect_or_launch(
    project_root: &Path,
    launch_mode: LaunchMode,
) -> Result<interprocess::local_socket::Stream> {
    if let Ok(active_status) = status(project_root)
        && active_status.active
        && controller_status_matches_current_binary(&active_status).unwrap_or(false)
    {
        reap_stale_duplicate_controllers(
            project_root,
            active_status.pid,
            active_status.controller_generation.unwrap_or(1),
        );
        return connect(project_root);
    }

    let _lock = LaunchLock::acquire(project_root)?;
    if let Ok(active_status) = status(project_root)
        && active_status.active
        && controller_status_matches_current_binary(&active_status).unwrap_or(false)
    {
        reap_stale_duplicate_controllers(
            project_root,
            active_status.pid,
            active_status.controller_generation.unwrap_or(1),
        );
        return connect(project_root);
    }
    if connect(project_root).is_ok() {
        if let Ok(old_status) = status(project_root)
            && old_status.active
        {
            return handoff_stale_controller(project_root, launch_mode, old_status);
        }
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
    launch_detached_at(
        project_root,
        launch_mode,
        None,
        None,
        None,
        ControllerHandoffState::Stable,
    )
}

fn launch_detached_at(
    project_root: &Path,
    launch_mode: LaunchMode,
    listen_socket: Option<&Path>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: ControllerHandoffState,
) -> Result<()> {
    let exe = current_agent_doc_binary()?;
    let mut command = Command::new(exe);
    command
        .arg("controller")
        .arg("serve")
        .arg("--project-root")
        .arg(project_root)
        .arg("--launch-mode")
        .arg(launch_mode.as_str());
    if let Some(path) = listen_socket {
        command.arg("--listen-socket").arg(path);
    }
    if let Some(generation) = controller_generation {
        command
            .arg("--controller-generation")
            .arg(generation.to_string());
    }
    if let Some(pid) = previous_controller_pid {
        command
            .arg("--previous-controller-pid")
            .arg(pid.to_string());
    }
    if handoff_state != ControllerHandoffState::Stable {
        command.arg("--handoff-state").arg(match handoff_state {
            ControllerHandoffState::Stable => "stable",
            ControllerHandoffState::Preparing => "preparing",
            ControllerHandoffState::Promoted => "promoted",
            ControllerHandoffState::Retiring => "retiring",
            ControllerHandoffState::Failed => "failed",
        });
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to launch project controller")?;
    Ok(())
}

fn wait_for_controller(project_root: &Path) -> Result<interprocess::local_socket::Stream> {
    wait_for_controller_path(&socket_path(project_root))
}

fn wait_for_controller_path(path: &Path) -> Result<interprocess::local_socket::Stream> {
    let start = Instant::now();
    loop {
        if let Ok(stream) = connect_path(path) {
            return Ok(stream);
        }
        if start.elapsed() >= CONNECT_WAIT {
            anyhow::bail!(
                "timed out waiting for project controller at {}",
                path.display()
            );
        }
        std::thread::sleep(CONNECT_POLL);
    }
}

#[allow(dead_code)]
pub fn serve(project_root: &Path, launch_mode: LaunchMode) -> Result<()> {
    serve_with_options(
        project_root,
        launch_mode,
        None,
        None,
        None,
        ControllerHandoffState::Stable,
    )
}

fn serve_with_options(
    project_root: &Path,
    launch_mode: LaunchMode,
    listen_socket: Option<PathBuf>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: ControllerHandoffState,
) -> Result<()> {
    let sock = listen_socket.unwrap_or_else(|| socket_path(project_root));
    if sock.exists() {
        let _ = std::fs::remove_file(&sock);
    }
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bootstrap = if let Some(generation) = controller_generation {
        write_bootstrap_with_options(
            project_root,
            sock.clone(),
            launch_mode,
            generation,
            handoff_state,
            previous_controller_pid,
        )?
    } else {
        write_bootstrap(project_root, launch_mode)?
    };
    let name = sock.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .with_context(|| format!("failed to listen on {}", sock.display()))?;
    listener
        .set_nonblocking(ListenerNonblockingMode::Accept)
        .context("failed to set project controller listener nonblocking")?;

    let bootstrap = Arc::new(Mutex::new(bootstrap));
    let should_stop = Arc::new(AtomicBool::new(false));
    while !should_stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok(stream) => {
                let bootstrap = Arc::clone(&bootstrap);
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
    bootstrap: &Arc<Mutex<ControllerBootstrap>>,
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
                let response = handle_request_locked(&line, bootstrap, &mut request_should_stop)?;
                writer_half.write_all(response.as_bytes())?;
                writer_half.write_all(b"\n")?;
                writer_half.flush()?;
                line.clear();
                if request_should_stop {
                    should_stop.store(true, Ordering::SeqCst);
                    let _ = std::fs::remove_file(sock);
                    if let Ok(bootstrap) = bootstrap.lock()
                        && bootstrap.socket_path != sock
                    {
                        let _ = std::fs::remove_file(&bootstrap.socket_path);
                    }
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

#[cfg(any(test, feature = "test-support"))]
fn handle_request(
    line: &str,
    bootstrap: &ControllerBootstrap,
    should_stop: &mut bool,
) -> Result<String> {
    handle_request_locked(line, &Arc::new(Mutex::new(bootstrap.clone())), should_stop)
}

fn handle_request_locked(
    line: &str,
    bootstrap: &Arc<Mutex<ControllerBootstrap>>,
    should_stop: &mut bool,
) -> Result<String> {
    let request: ControllerRequest = serde_json::from_str(line.trim())?;
    let bootstrap_snapshot = bootstrap
        .lock()
        .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))?
        .clone();
    match request.command.as_str() {
        "status" => Ok(serde_json::to_string(&controller_status_from_bootstrap(
            &bootstrap_snapshot,
            true,
        )?)?),
        "prepare_handoff" => {
            let mut state = bootstrap
                .lock()
                .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))?;
            state.handoff_state = ControllerHandoffState::Preparing;
            state.handoff_started_at = Some(timestamp_secs());
            write_bootstrap_state(&state)?;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "promote_handoff" => {
            let mut state = bootstrap
                .lock()
                .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))?;
            state.socket_path = socket_path(&state.project_root);
            state.handoff_state = ControllerHandoffState::Stable;
            state.handoff_started_at = None;
            write_bootstrap_state(&state)?;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "retire_after_handoff" => {
            {
                let mut state = bootstrap
                    .lock()
                    .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))?;
                state.handoff_state = ControllerHandoffState::Retiring;
                write_bootstrap_state(&state)?;
            }
            *should_stop = true;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "shutdown" => {
            *should_stop = true;
            Ok(serde_json::to_string(&serde_json::json!({ "ok": true }))?)
        }
        "start_session" => controller_envelope(handle_start_session(&bootstrap_snapshot, request)),
        "register_supervisor" => {
            controller_envelope(handle_register_supervisor(&bootstrap_snapshot, request))
        }
        "mark_lifecycle" => {
            controller_envelope(handle_mark_lifecycle(&bootstrap_snapshot, request))
        }
        "supervisor_heartbeat" => {
            controller_envelope(handle_supervisor_heartbeat(&bootstrap_snapshot, request))
        }
        "actor_binding" => controller_envelope(handle_actor_binding(&bootstrap_snapshot, request)),
        "dispatch" => controller_envelope(handle_dispatch(&bootstrap_snapshot, request)),
        "session_status" => {
            controller_envelope(handle_session_status(&bootstrap_snapshot, request))
        }
        "attach_pane" => controller_envelope(handle_attach_pane(&bootstrap_snapshot, request)),
        "operator_command" => {
            controller_envelope(handle_operator_command(&bootstrap_snapshot, request))
        }
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
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let record = crate::session_actor::transition_state_in(
        &bootstrap.project_root,
        &document_id,
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

fn handle_supervisor_heartbeat(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<SupervisorLeaseStatus> {
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
            "stale supervisor heartbeat for {}: requested session={} pane={} generation={}, current session={} pane={} generation={}",
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
            "controller_supervisor_heartbeat session={} pane={} generation={} state={}",
            session_id, pane_id, generation, runtime_state
        ),
    );
    load_supervisor_lease_from_db(
        &open_state_db(&bootstrap.project_root)?,
        &record.document_id,
        record.generation,
    )?
    .context("missing supervisor lease after heartbeat")
}

fn handle_actor_binding(
    bootstrap: &ControllerBootstrap,
    request: ControllerRequest,
) -> Result<ActorBindingResponse> {
    let file = request_file(&request)?;
    let document_id = crate::session_actor::canonical_document_id_in(
        &bootstrap.project_root,
        &file.to_string_lossy(),
    );
    let record = load_actor_record(&bootstrap.project_root, &document_id)?;
    Ok(match record {
        Some(record) => ActorBindingResponse {
            status: ActorBindingStatus::Bound,
            record: Some(record),
        },
        None => ActorBindingResponse {
            status: ActorBindingStatus::NotFound,
            record: None,
        },
    })
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
    let clear_closed_actor = matches!(
        command_kind.as_str(),
        "session_clear" | "session_interrupt_clear"
    ) && record.state == crate::session_actor::ActorState::Closed;
    if matches!(record.state, crate::session_actor::ActorState::Blocked)
        || (record.state == crate::session_actor::ActorState::Closed && !clear_closed_actor)
    {
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

pub fn run_serve(
    root: Option<&Path>,
    launch_mode: &str,
    listen_socket: Option<&Path>,
    controller_generation: Option<u64>,
    previous_controller_pid: Option<u32>,
    handoff_state: &str,
) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    serve_with_options(
        &project_root,
        LaunchMode::parse(launch_mode)?,
        listen_socket.map(Path::to_path_buf),
        controller_generation,
        previous_controller_pid,
        parse_handoff_state(handoff_state)?,
    )
}

pub fn run_shutdown(root: Option<&Path>) -> Result<()> {
    let project_root = project_root_from_arg(root)?;
    println!("{}", request(&project_root, "shutdown")?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // `rusqlite` is a dev-dependency: these tests open the controller state DB
    // directly to assert the schema/rows the seam writes. `Connection` is the
    // `state_store` re-export already in scope via `super::*`.
    use agent_doc_sqlite::state_store::{load_actor_transitions_from_db, sqlite_i64};
    use rusqlite::params;
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
    fn sessions_projection_removes_displaced_cross_document_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc_a = dir.path().join("tasks/a.md");
        let doc_b = dir.path().join("tasks/b.md");
        std::fs::create_dir_all(doc_a.parent().unwrap()).unwrap();
        std::fs::write(&doc_a, "a").unwrap();
        std::fs::write(&doc_b, "b").unwrap();
        let document_a = doc_a.to_string_lossy().to_string();
        let document_b = doc_b.to_string_lossy().to_string();
        let mut registry = crate::sessions::SessionRegistry::new();
        registry.insert(
            document_a.clone(),
            crate::sessions::SessionEntry {
                pane: "%70".to_string(),
                pid: 100,
                cwd: dir.path().to_string_lossy().to_string(),
                started: "1".to_string(),
                session_id: "session-a".to_string(),
                file: document_a.clone(),
                window: "@7".to_string(),
                supervisor_instance_id: "supervisor-a".to_string(),
            },
        );
        registry.insert(
            document_b.clone(),
            crate::sessions::SessionEntry {
                pane: "%old".to_string(),
                pid: 200,
                cwd: dir.path().to_string_lossy().to_string(),
                started: "2".to_string(),
                session_id: "session-b".to_string(),
                file: document_b.clone(),
                window: "@old".to_string(),
                supervisor_instance_id: "supervisor-b".to_string(),
            },
        );
        crate::sessions::save_in(dir.path(), &registry).unwrap();

        let mut record_a = actor_record(&document_a, "%70", "@7");
        record_a.session_id = "session-a".to_string();
        store_actor_record(dir.path(), Some(0), &record_a).unwrap();
        let mut record_b = actor_record(&document_b, "%70", "@7");
        record_b.session_id = "session-b".to_string();
        store_actor_record(dir.path(), Some(0), &record_b).unwrap();

        let projected = crate::sessions::load_in(dir.path()).unwrap();
        assert!(
            !projected.contains_key(&document_a),
            "displaced document must not remain in sessions.json"
        );
        let entry_b = projected.get(&document_b).unwrap();
        assert_eq!(entry_b.pane, "%70");
        assert_eq!(entry_b.window, "@7");
        assert_eq!(entry_b.session_id, "session-b");
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
    fn layout_state_migrates_legacy_projection_to_sqlite() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            layout_projection_path(dir.path()),
            serde_json::to_string(&vec!["tasks/a.md".to_string(), "tasks/b.md".to_string()])
                .unwrap(),
        )
        .unwrap();

        let loaded = load_layout_state(dir.path()).unwrap();

        assert_eq!(loaded, vec!["tasks/a.md", "tasks/b.md"]);
        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT columns_json FROM layout_states WHERE scope = ?1",
                params![DEFAULT_LAYOUT_SCOPE],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&stored).unwrap(),
            loaded
        );
    }

    #[test]
    fn layout_state_prefers_sqlite_over_drifted_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        store_layout_state(dir.path(), &["tasks/current.md".to_string()]).unwrap();
        std::fs::write(
            layout_projection_path(dir.path()),
            serde_json::to_string(&vec!["tasks/stale.md".to_string()]).unwrap(),
        )
        .unwrap();

        let loaded = load_layout_state(dir.path()).unwrap();

        assert_eq!(loaded, vec!["tasks/current.md"]);
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
        assert_eq!(read.controller_generation, written.controller_generation);
        assert_eq!(read.handoff_state, ControllerHandoffState::Stable);
        assert_eq!(
            read.socket_path,
            dir.path().join(".agent-doc/controller.sock")
        );
    }

    #[test]
    fn handoff_bootstrap_persists_generation_previous_pid_and_temp_socket() {
        let dir = tempfile::TempDir::new().unwrap();
        let temp_sock = dir.path().join(".agent-doc/controller-handoff-test.sock");
        let written = write_bootstrap_with_options(
            dir.path(),
            temp_sock.clone(),
            LaunchMode::Lazy,
            7,
            ControllerHandoffState::Preparing,
            Some(1234),
        )
        .unwrap();
        let read = read_bootstrap(dir.path()).unwrap().unwrap();

        assert_eq!(written.controller_generation, 7);
        assert_eq!(read.controller_generation, 7);
        assert_eq!(read.socket_path, temp_sock);
        assert_eq!(read.handoff_state, ControllerHandoffState::Preparing);
        assert_eq!(read.previous_controller_pid, Some(1234));
        assert!(read.handoff_started_at.is_some());
    }

    #[test]
    fn prepare_and_promote_handoff_update_controller_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = write_bootstrap_with_options(
            dir.path(),
            dir.path().join(".agent-doc/controller-handoff-test.sock"),
            LaunchMode::Lazy,
            2,
            ControllerHandoffState::Preparing,
            Some(111),
        )
        .unwrap();
        let mut should_stop = false;

        let prepare = handle_request(
            &(serde_json::json!({ "command": "prepare_handoff" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        assert!(prepare.contains("\"ok\":true"), "{prepare}");
        let preparing = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(preparing.handoff_state, ControllerHandoffState::Preparing);

        let promote = handle_request(
            &(serde_json::json!({ "command": "promote_handoff" }).to_string() + "\n"),
            &preparing,
            &mut should_stop,
        )
        .unwrap();
        assert!(promote.contains("\"ok\":true"), "{promote}");
        let promoted = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(promoted.socket_path, socket_path(dir.path()));
        assert_eq!(promoted.handoff_state, ControllerHandoffState::Stable);
        assert_eq!(promoted.handoff_started_at, None);
    }

    #[test]
    fn duplicate_scan_only_matches_same_project_controller_args() {
        let dir = tempfile::TempDir::new().unwrap();
        let args = vec![
            "/home/user/.cargo/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            dir.path().display().to_string(),
        ];
        assert!(args_match_same_project_controller(&args, dir.path()));

        let other_dir = tempfile::TempDir::new().unwrap();
        assert!(!args_match_same_project_controller(&args, other_dir.path()));

        let non_controller = vec![
            "agent-doc".to_string(),
            "preflight".to_string(),
            dir.path().join("task.md").display().to_string(),
        ];
        assert!(!args_match_same_project_controller(
            &non_controller,
            dir.path()
        ));
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
            controller_generation: Some(1),
            handoff_state: Some(ControllerHandoffState::Stable),
            handoff_started_at: None,
            previous_controller_pid: None,
            stale_duplicate_pids: Vec::new(),
            control_plane: default_control_plane_status(),
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

    #[test]
    fn controller_binary_resolution_prefers_existing_current_exe() {
        let dir = tempfile::TempDir::new().unwrap();
        let current = dir.path().join("current-agent-doc");
        let path_bin_dir = dir.path().join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&current, "current").unwrap();
        std::fs::write(&path_bin, "path").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            Some(current.clone()),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            dir.path(),
        )
        .unwrap();

        assert_eq!(resolved, current);
    }

    #[test]
    fn controller_binary_resolution_falls_back_to_path_when_current_exe_is_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path_bin_dir = dir.path().join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&path_bin, "path").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            Some(dir.path().join("deleted-agent-doc")),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            dir.path(),
        )
        .unwrap();

        assert_eq!(resolved, path_bin);
    }

    fn test_bootstrap(dir: &tempfile::TempDir) -> ControllerBootstrap {
        ControllerBootstrap {
            project_root: dir.path().to_path_buf(),
            socket_path: socket_path(dir.path()),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 123,
            pid: 456,
            controller_binary: Some(current_binary_identity().unwrap()),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
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
    fn controller_supervisor_heartbeat_refreshes_stale_lease_without_actor_transition() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/heartbeat.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-heartbeat\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);

        crate::session_actor::record_session_start_direct(
            &doc,
            "session-heartbeat",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let record = crate::session_actor::transition_state_direct(
            &doc,
            "session-heartbeat",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        upsert_supervisor_lease(
            dir.path(),
            &record,
            Some(999),
            Some("/tmp/old.sock"),
            "starting",
        )
        .unwrap();

        let heartbeat = ControllerRequest {
            command: "supervisor_heartbeat".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-heartbeat".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("ready".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: Some(1001),
            supervisor_socket: Some("/tmp/new.sock".to_string()),
            command_kind: None,
            diagnostic_payload: None,
        };
        let lease = handle_supervisor_heartbeat(&bootstrap, heartbeat).unwrap();
        assert_eq!(lease.runtime_state.as_deref(), Some("ready"));
        assert_eq!(lease.supervisor_pid, Some(1001));
        assert_eq!(lease.supervisor_socket.as_deref(), Some("/tmp/new.sock"));

        let transitions = load_actor_transitions_from_db(
            &Connection::open(state_db_path(dir.path())).unwrap(),
            &doc.to_string_lossy(),
        )
        .unwrap();
        assert_eq!(
            transitions.len(),
            2,
            "heartbeat must not create an actor transition"
        );
    }

    #[test]
    fn gc_closes_stale_starting_actor_without_fresh_supervisor_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stale-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-stale-starting\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-stale-starting",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        Connection::open(state_db_path(dir.path()))
            .unwrap()
            .execute(
                "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
                params![sqlite_i64(old, "old timestamp").unwrap()],
            )
            .unwrap();

        let (closed, kept) =
            close_stale_starting_actors(dir.path(), Duration::from_secs(3600), false).unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 0);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
        assert_eq!(updated.last_transition.caller, "gc");
        assert_eq!(updated.last_transition.reason, "stale_starting_actor");
    }

    #[test]
    fn gc_keeps_stale_starting_actor_with_fresh_supervisor_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/live-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-live-starting\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-live-starting",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        upsert_supervisor_lease(
            dir.path(),
            &record,
            Some(std::process::id()),
            Some("/tmp/live-starting.sock"),
            "starting",
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        Connection::open(state_db_path(dir.path()))
            .unwrap()
            .execute(
                "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
                params![sqlite_i64(old, "old timestamp").unwrap()],
            )
            .unwrap();

        let (closed, kept) =
            close_stale_starting_actors(dir.path(), Duration::from_secs(3600), false).unwrap();
        assert_eq!(closed, 0);
        assert_eq!(kept, 1);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Starting);
    }

    #[test]
    fn gc_closes_stale_starting_actor_with_stale_heartbeat_even_when_pid_is_alive() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stuck-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-stuck-starting\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-stuck-starting",
            "%42",
            "@1",
            1,
        )
        .unwrap();
        upsert_supervisor_lease(
            dir.path(),
            &record,
            Some(std::process::id()),
            Some("/tmp/stuck-starting.sock"),
            "starting",
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        conn.execute(
            "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
            params![sqlite_i64(old, "old transition timestamp").unwrap()],
        )
        .unwrap();
        conn.execute(
            "UPDATE supervisor_leases SET last_heartbeat = ?1 WHERE document_id = ?2 AND generation = 1",
            params![
                sqlite_i64(old, "old heartbeat timestamp").unwrap(),
                record.document_id
            ],
        )
        .unwrap();

        let (closed, kept) =
            close_stale_starting_actors(dir.path(), Duration::from_secs(3600), false).unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 0);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
        assert_eq!(updated.last_transition.reason, "stale_starting_actor");
    }

    #[test]
    fn normal_path_actor_cleanup_records_calling_surface() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/preflight-stale-starting.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-preflight-stale\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let record = crate::session_actor::record_session_start_direct(
            &doc,
            "session-preflight-stale",
            "%51",
            "@1",
            1,
        )
        .unwrap();
        let old = timestamp_secs() - 7200;
        Connection::open(state_db_path(dir.path()))
            .unwrap()
            .execute(
                "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
                params![sqlite_i64(old, "old timestamp").unwrap()],
            )
            .unwrap();

        let (closed, kept) = close_stale_starting_actors_for_caller(
            dir.path(),
            Duration::from_secs(3600),
            false,
            "preflight",
        )
        .unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 0);

        let updated = load_actor_record(dir.path(), &record.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
        assert_eq!(updated.last_transition.caller, "preflight");
        assert_eq!(updated.last_transition.reason, "stale_starting_actor");
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
    fn controller_lifecycle_allows_same_pane_stale_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/same-pane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-same\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-same", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-same", "%41", "@1", 2)
            .unwrap();

        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-same".to_string()),
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
        assert!(
            envelope.ok,
            "same-pane stale generation should succeed: {:?}",
            envelope.error
        );
        assert_eq!(
            envelope.data.unwrap().state,
            crate::session_actor::ActorState::Ready
        );
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
        let envelope: ControllerEnvelope<ActorBindingResponse> =
            serde_json::from_str(&response).unwrap();
        let binding = envelope.data.unwrap();
        assert_eq!(binding.status, ActorBindingStatus::Bound);
        assert_eq!(binding.record.unwrap().pane_id, "%41");

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
    fn controller_actor_binding_absent_is_typed_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/no-binding.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-route\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

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
        let envelope: ControllerEnvelope<ActorBindingResponse> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "actor_binding not_found should not be an error"
        );
        let binding = envelope.data.unwrap();
        assert_eq!(binding.status, ActorBindingStatus::NotFound);
        assert!(binding.record.is_none());
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
    fn controller_status_reports_single_process_control_plane_runtime() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/control-plane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-control-plane\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-control-plane".to_string()),
            pane_id: Some("%77".to_string()),
            window_id: Some("@7".to_string()),
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

        let register = ControllerRequest {
            command: "register_supervisor".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-control-plane".to_string()),
            pane_id: Some("%77".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("starting".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: Some(4242),
            supervisor_socket: Some("supervisor.sock".to_string()),
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

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-control-plane".to_string()),
            pane_id: Some("%77".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("control-plane status test".to_string()),
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

        record_projection_diagnostic(
            dir.path(),
            "session-actors.json",
            &doc.to_string_lossy(),
            "test projection lag",
        );

        let status = ControllerRequest {
            command: "status".to_string(),
            file: None,
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
        let status: ControllerStatus = serde_json::from_str(&response).unwrap();

        assert!(status.active);
        assert_eq!(
            status.control_plane.process_model,
            "project_scoped_single_process"
        );
        assert_eq!(status.control_plane.external_boundary, "controller_ipc");
        assert_eq!(status.control_plane.state_authority, ".agent-doc/state.db");
        assert_eq!(
            status.control_plane.projection_authority,
            "compatibility_output"
        );
        assert_eq!(status.control_plane.dispatch_actor.owned_items, 1);
        assert_eq!(status.control_plane.session_actors.owned_items, 1);
        assert_eq!(status.control_plane.supervisor_adapters.owned_items, 1);
        assert!(status.control_plane.projection_workers.owned_items >= 1);
        assert!(status.control_plane.store_actor.owned_items >= 4);
    }

    #[test]
    fn controller_session_clear_accepts_closed_actor_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/closed-clear.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-closed-clear\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(
            &doc,
            "session-closed-clear",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-closed-clear",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Closed,
            "supervisor",
            "cycle_committed",
        )
        .unwrap();

        let clear = ControllerRequest {
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
            diagnostic_payload: Some("test clear closed actor".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&clear).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "session_clear should accept closed actors: {:?}",
            envelope.error
        );
        assert_eq!(envelope.data.unwrap().accepted_stage, "operator_closed");

        let restart = ControllerRequest {
            command_kind: Some("session_restart".to_string()),
            ..clear
        };
        let response = handle_request(
            &(serde_json::to_string(&restart).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        assert!(envelope.error.unwrap().contains("generation 1 is closed"));
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
        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let diagnostics_before_attach: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'sessions.json'",
                [],
                |row| row.get(0),
            )
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
        let diagnostics_after_attach: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'sessions.json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            diagnostics_after_attach, diagnostics_before_attach,
            "controller attach should not add a projection diagnostic before the caller updates sessions.json"
        );
    }

    #[test]
    fn controller_mark_lifecycle_resolves_relative_path_via_project_root() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/relative.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-relative\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-relative".to_string()),
            pane_id: Some("%51".to_string()),
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

        let relative = std::path::PathBuf::from("tasks/relative.md");
        let lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(relative),
            session_id: Some("session-relative".to_string()),
            pane_id: Some("%51".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("ready".to_string()),
            caller: Some("route".to_string()),
            reason: Some("dispatch_ready_prompt".to_string()),
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
        assert!(envelope.ok, "mark_lifecycle with relative path failed");
        assert_eq!(
            envelope.data.unwrap().state,
            crate::session_actor::ActorState::Ready
        );
    }

    #[test]
    fn typed_controller_decode_reports_missing_data_with_command_and_raw_envelope() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/missing-data.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let request = ControllerRequest {
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

        let err = decode_controller_response::<SessionOperatorStatus>(
            dir.path(),
            &request,
            r#"{"ok":true}"#,
        )
        .expect_err("typed controller response without data must fail");

        let message = err.to_string();
        assert!(message.contains("command `session_status`"));
        assert!(message.contains(r#"{"ok":true}"#));
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_response_missing_data command=session_status"));
    }
}
