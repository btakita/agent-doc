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
    ActorTransitionStatus, AdminOperationStatus, DispatchAttemptStatus, ProjectionDiagnosticStatus,
    QueueBackpressureStatus, QueueControlStatus, QueueHeadStatus, SessionOperatorStatus,
    SupervisorLeaseStatus, state_db_path,
};
use state_store::{
    Connection, ProjectionDiagnosticInsert, insert_projection_diagnostic,
    insert_projection_diagnostic_with_metadata, load_actor_record_from_db,
    load_actor_store_from_db, load_control_plane_store_counts, load_layout_state_from_db,
    load_session_operator_status_from_db, load_supervisor_lease_from_db, open_state_db,
    store_layout_state_in_db, timestamp_secs,
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
/// How long a contended launch waits for the current holder to finish before
/// giving up. Sized above `CONNECT_WAIT` so a waiter outlasts the holder's full
/// `launch_detached` + `wait_for_controller` window and can then adopt the
/// controller the holder published instead of failing the start (#suprecyclelock).
const LAUNCH_LOCK_WAIT: Duration = Duration::from_secs(8);
const LAUNCH_LOCK_POLL: Duration = Duration::from_millis(50);
#[cfg(not(any(test, feature = "test-support")))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(any(test, feature = "test-support"))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_millis(250);
const CONTROLLER_IDLE_CLIENT_TIMEOUT: Duration = CONTROLLER_RPC_TIMEOUT;

/// Default staleness threshold for the stuck-handoff reaper (#kqr6 / #sjwm /
/// #stuckhandoff). A controller handoff should reach `Promoted`/`Stable` within
/// seconds; a record still `Preparing`/`Promoted` past this age is treated as
/// wedged and its live process is terminated. Seconds, not the 1h projection GC
/// window — a wedged controller keeps re-corrupting the working tree every tick.
const DEFAULT_STALE_PREPARING_CONTROLLER_SECS: u64 = 45;
const STALE_PREPARING_CONTROLLER_SECS_ENV: &str = "AGENT_DOC_STALE_PREPARING_CONTROLLER_SECS";

/// `#ctlrecycle` — how long a process must continuously observe "wants-recycle AND
/// idle" before it self-recycles onto a freshly-installed binary. Debounce so a
/// short gap between queue items never triggers a recycle.
const DEFAULT_RECYCLE_IDLE_GRACE_SECS: u64 = 5;
const RECYCLE_IDLE_GRACE_SECS_ENV: &str = "AGENT_DOC_RECYCLE_IDLE_GRACE_SECS";
/// `#ctlrecycle` R3 — opt-in flag for the `start --route-owned` supervisor to
/// hot-reload onto a fresh binary when idle via an in-place `execve` that PRESERVES
/// the live harness child + tmux pane (`start.rs::supervisor_perform_reexec` +
/// `PtySession::adopt`). Default OFF: the in-place image swap of a live interactive
/// supervisor is high blast-radius and the two-process round-trip can only be proven
/// with a live editor + harness, so it stays a deliberate opt-in until that live
/// validation lands; when off the supervisor only logs `supervisor_binary_stale_detected`
/// and the operator restarts the session to pick up the new build.
const SUPERVISOR_AUTO_RECYCLE_ENV: &str = "AGENT_DOC_SUPERVISOR_AUTO_RECYCLE";

#[derive(Clone, Debug)]
pub struct SessionsProjectionHint {
    pub session_id: String,
    pub pane_id: String,
    pub file: String,
    pub pid: u32,
    pub window_id: String,
    pub cwd: String,
    pub supervisor_instance_id: String,
}

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
    #[serde(default)]
    pub categories: BTreeMap<String, usize>,
}

#[derive(Debug)]
struct ControllerMemoryState {
    actor_store: BTreeMap<String, crate::session_actor::ActorRecord>,
    map_backend: &'static str,
}

impl ControllerMemoryState {
    fn load(project_root: &Path) -> Result<Self> {
        Ok(Self {
            actor_store: load_actor_store(project_root)?,
            map_backend: "std_btree_map",
        })
    }
}

#[derive(Debug)]
pub(crate) struct ControllerRuntime {
    bootstrap: Mutex<ControllerBootstrap>,
    memory: Mutex<ControllerMemoryState>,
    /// `#ctlrecycle` R2 — set true by the `recycle` RPC (`agent-doc admin recycle`).
    /// The serve-loop idle poll honors it the same way it honors binary staleness:
    /// once no dispatch is in flight (debounced), the controller self-terminates and
    /// the next `connect_or_launch` relaunches the fresh binary.
    recycle_requested: AtomicBool,
}

impl ControllerRuntime {
    fn new(bootstrap: ControllerBootstrap) -> Result<Self> {
        if controller_restart_recovery_needed(&bootstrap) {
            recover_controller_after_restart(&bootstrap)?;
        }
        let memory = ControllerMemoryState::load(&bootstrap.project_root)?;
        Ok(Self {
            bootstrap: Mutex::new(bootstrap),
            memory: Mutex::new(memory),
            recycle_requested: AtomicBool::new(false),
        })
    }

    /// `#ctlrecycle` R2 — mark this controller to recycle at the next idle boundary.
    fn request_recycle(&self) {
        self.recycle_requested.store(true, Ordering::SeqCst);
    }

    fn recycle_requested(&self) -> bool {
        self.recycle_requested.load(Ordering::SeqCst)
    }

    fn bootstrap_snapshot(&self) -> Result<ControllerBootstrap> {
        self.bootstrap
            .lock()
            .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))
            .map(|guard| guard.clone())
    }

    fn actor_record(&self, document_id: &str) -> Result<Option<crate::session_actor::ActorRecord>> {
        self.memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))
            .map(|memory| memory.actor_store.get(document_id).cloned())
    }

    fn refresh_memory(&self) -> Result<()> {
        let project_root = self.bootstrap_snapshot()?.project_root;
        let next = ControllerMemoryState::load(&project_root)?;
        let mut memory = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))?;
        *memory = next;
        Ok(())
    }

    fn memory_categories(&self) -> Result<BTreeMap<String, usize>> {
        let memory = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))?;
        Ok(status_categories([
            ("actor_records", memory.actor_store.len()),
            (
                "map_backend_std_btree_map",
                usize::from(memory.map_backend == "std_btree_map"),
            ),
            ("write_through_sqlite", 1),
        ]))
    }
}

fn controller_restart_recovery_needed(bootstrap: &ControllerBootstrap) -> bool {
    bootstrap.controller_generation > 1 || bootstrap.previous_controller_pid.is_some()
}

#[derive(Debug, Default)]
struct CrashRecoveryStats {
    actor_records: usize,
    supervisor_reattached: usize,
    supervisor_stale: usize,
    dispatch_retryable: usize,
    dispatch_blocked: usize,
    open_cycles_preserved: usize,
    projections_emitted: usize,
}

fn recover_controller_after_restart(bootstrap: &ControllerBootstrap) -> Result<CrashRecoveryStats> {
    let project_root = &bootstrap.project_root;
    let conn = open_state_db(project_root)?;
    let store = load_actor_store_from_db(&conn)?;
    let mut stats = CrashRecoveryStats {
        actor_records: store.len(),
        ..CrashRecoveryStats::default()
    };

    reconcile_supervisor_leases_after_restart(&conn, &store, &mut stats)?;
    reconcile_open_dispatch_receipts_after_restart(&conn, &mut stats)?;
    preserve_open_closeout_cycles_after_restart(&conn, &mut stats)?;
    drop(conn);

    if !store.is_empty() {
        let actor_projection_hash = actor_projection_intended_hash(project_root).ok();
        match emit_actor_projection(project_root) {
            Ok(()) => stats.projections_emitted += 1,
            Err(err) => {
                let document_id = store
                    .keys()
                    .next()
                    .map(String::as_str)
                    .unwrap_or("__controller__");
                record_projection_diagnostic_with_metadata(
                    project_root,
                    ACTOR_PROJECTION_FILE,
                    document_id,
                    None,
                    actor_projection_hash.as_deref(),
                    "retry_pending",
                    &format!("failed to emit actor projection during controller recovery: {err}"),
                );
            }
        }
    }

    for record in store.values() {
        project_sessions_projection_for_actor(project_root, &record.document_id)?;
    }
    if !store.is_empty() {
        stats.projections_emitted += 1;
    }

    let conn = open_state_db(project_root)?;
    if state_store::layout_scope_exists(&conn, DEFAULT_LAYOUT_SCOPE)? {
        drop(conn);
        match emit_layout_projection(project_root) {
            Ok(()) => stats.projections_emitted += 1,
            Err(err) => record_projection_diagnostic_with_metadata(
                project_root,
                LAYOUT_PROJECTION_FILE,
                "__layout__",
                None,
                None,
                "retry_pending",
                &format!("failed to emit layout projection during controller recovery: {err}"),
            ),
        }
    }

    let conn = open_state_db(project_root)?;
    state_store::insert_crash_recovery_marker_in_db(
        &conn,
        "controller_restart_reconcile",
        None,
        None,
        "completed",
        Some(&format!(
            "actor_records={} supervisor_reattached={} supervisor_stale={} dispatch_retryable={} dispatch_blocked={} open_cycles_preserved={} projections_emitted={}",
            stats.actor_records,
            stats.supervisor_reattached,
            stats.supervisor_stale,
            stats.dispatch_retryable,
            stats.dispatch_blocked,
            stats.open_cycles_preserved,
            stats.projections_emitted
        )),
    )?;
    Ok(stats)
}

fn reconcile_supervisor_leases_after_restart(
    conn: &Connection,
    store: &BTreeMap<String, crate::session_actor::ActorRecord>,
    stats: &mut CrashRecoveryStats,
) -> Result<()> {
    let now = timestamp_secs();
    for record in store.values() {
        if record.state == crate::session_actor::ActorState::Closed {
            continue;
        }
        let Some(lease) =
            load_supervisor_lease_from_db(conn, &record.document_id, record.generation)?
        else {
            continue;
        };
        let fresh = supervisor_lease_is_fresh_or_alive(&lease, now, Duration::from_secs(60));
        let status = if fresh {
            stats.supervisor_reattached += 1;
            "reattached"
        } else {
            stats.supervisor_stale += 1;
            "stale"
        };
        state_store::insert_crash_recovery_marker_in_db(
            conn,
            "supervisor_lease_reconcile",
            Some(&record.document_id),
            Some(record.generation),
            status,
            Some(&format!(
                "session={} pane={} runtime_state={} heartbeat={}",
                record.session_id,
                record.pane_id,
                lease.runtime_state.as_deref().unwrap_or("unknown"),
                lease
                    .last_heartbeat
                    .map(|timestamp| timestamp.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )),
        )?;
    }
    Ok(())
}

fn reconcile_open_dispatch_receipts_after_restart(
    conn: &Connection,
    stats: &mut CrashRecoveryStats,
) -> Result<()> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, document_id, generation, command_kind, result_status, proof_scope, dispatch_start_proven
        FROM dispatch_attempts
        WHERE failed_stage IS NULL
          AND COALESCE(result_status, '') IN ('accepted', 'queued', 'running')
          AND (COALESCE(proof_scope, '') = 'accepted_only' OR dispatch_start_proven = 0)
        "#,
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let receipt_id: i64 = row.get("id")?;
        let document_id: String = row.get("document_id")?;
        let generation: i64 = row.get("generation")?;
        let command_kind: String = row.get("command_kind")?;
        let result_status: Option<String> = row.get("result_status")?;
        let proof_scope: Option<String> = row.get("proof_scope")?;
        let dispatch_start_proven: i64 = row.get("dispatch_start_proven")?;
        let status = if dispatch_start_proven == 0 {
            stats.dispatch_retryable += 1;
            "retryable"
        } else {
            stats.dispatch_blocked += 1;
            "blocked"
        };
        state_store::insert_crash_recovery_marker_in_db(
            conn,
            "dispatch_receipt_reconcile",
            Some(&document_id),
            Some(state_store::sqlite_u64(generation, "dispatch generation")?),
            status,
            Some(&format!(
                "receipt_id={} command_kind={} result_status={} proof_scope={} dispatch_start_proven={}",
                state_store::sqlite_u64(receipt_id, "dispatch receipt id")?,
                command_kind,
                result_status.as_deref().unwrap_or("unknown"),
                proof_scope.as_deref().unwrap_or("unknown"),
                dispatch_start_proven != 0
            )),
        )?;
    }
    Ok(())
}

fn preserve_open_closeout_cycles_after_restart(
    conn: &Connection,
    stats: &mut CrashRecoveryStats,
) -> Result<()> {
    let mut stmt = conn.prepare(
        r#"
        SELECT document_id, cycle_id, state, queue_head_id
        FROM document_cycles
        WHERE state NOT IN ('committed', 'abandoned')
        "#,
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let document_id: String = row.get("document_id")?;
        let cycle_id: String = row.get("cycle_id")?;
        let state: String = row.get("state")?;
        let queue_head_id: Option<String> = row.get("queue_head_id")?;
        stats.open_cycles_preserved += 1;
        state_store::insert_crash_recovery_marker_in_db(
            conn,
            "open_closeout_preserved",
            Some(&document_id),
            None,
            "preserved",
            Some(&format!(
                "cycle_id={} state={} queue_head_id={}",
                cycle_id,
                state,
                queue_head_id.as_deref().unwrap_or("none")
            )),
        )?;
    }
    Ok(())
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
            categories: BTreeMap::new(),
        },
        store_actor: ControlPlaneActorStatus {
            role: "store_actor".to_string(),
            authority: "sqlite_write_serialization".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
            categories: BTreeMap::new(),
        },
        session_actors: ControlPlaneActorStatus {
            role: "session_actor".to_string(),
            authority: "in_memory_actor_map".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
            categories: BTreeMap::new(),
        },
        supervisor_adapters: ControlPlaneActorStatus {
            role: "supervisor_adapter".to_string(),
            authority: "managed_harness_child".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
            categories: BTreeMap::new(),
        },
        projection_workers: ControlPlaneActorStatus {
            role: "projection_worker".to_string(),
            authority: "compatibility_projection".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
            categories: BTreeMap::new(),
        },
    }
}

fn status_categories<const N: usize>(pairs: [(&str, usize); N]) -> BTreeMap<String, usize> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn control_plane_status(
    project_root: &Path,
    active: bool,
    memory_categories: Option<BTreeMap<String, usize>>,
) -> Result<ControlPlaneStatus> {
    let conn = open_state_db(project_root)?;
    let counts = load_control_plane_store_counts(&conn)?;
    let actor_state = if active { "ready" } else { "offline" };
    let store_state = if active { "ready" } else { "durable_offline" };
    let mut session_categories = memory_categories
        .unwrap_or_else(|| status_categories([("actor_records", counts.live_actor_documents)]));
    session_categories.insert("queue_heads".to_string(), counts.queue_heads);
    session_categories.insert("queue_controls".to_string(), counts.queue_controls);
    session_categories.insert("queue_backpressure".to_string(), counts.queue_backpressure);
    session_categories.insert("document_cycles".to_string(), counts.document_cycles);
    session_categories.insert("pending_mutations".to_string(), counts.pending_mutations);
    let session_owned_items = session_categories
        .get("actor_records")
        .copied()
        .unwrap_or(counts.live_actor_documents)
        + counts.queue_heads
        + counts.queue_controls
        + counts.queue_backpressure
        + counts.document_cycles
        + counts.pending_mutations;

    Ok(ControlPlaneStatus {
        dispatch_actor: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: counts.dispatch_receipts,
            categories: status_categories([("dispatch_receipts", counts.dispatch_receipts)]),
            ..default_control_plane_status().dispatch_actor
        },
        store_actor: ControlPlaneActorStatus {
            state: store_state.to_string(),
            owned_items: counts.total_authoritative_rows(),
            categories: status_categories([
                ("actor_documents", counts.actor_documents),
                ("actor_transitions", counts.actor_transitions),
                ("supervisor_leases", counts.supervisor_leases),
                ("dispatch_receipts", counts.dispatch_receipts),
                ("queue_heads", counts.queue_heads),
                ("queue_controls", counts.queue_controls),
                ("queue_backpressure", counts.queue_backpressure),
                ("document_cycles", counts.document_cycles),
                ("pending_mutations", counts.pending_mutations),
                ("projection_diagnostics", counts.projection_diagnostics),
                ("admin_operations", counts.admin_operations),
                ("crash_recovery_markers", counts.crash_recovery_markers),
                ("layout_states", counts.layout_states),
            ]),
            ..default_control_plane_status().store_actor
        },
        session_actors: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: session_owned_items,
            categories: session_categories,
            ..default_control_plane_status().session_actors
        },
        supervisor_adapters: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: counts.supervisor_leases,
            categories: status_categories([("supervisor_leases", counts.supervisor_leases)]),
            ..default_control_plane_status().supervisor_adapters
        },
        projection_workers: ControlPlaneActorStatus {
            state: actor_state.to_string(),
            owned_items: counts.projection_diagnostics,
            categories: status_categories([(
                "projection_diagnostics",
                counts.projection_diagnostics,
            )]),
            ..default_control_plane_status().projection_workers
        },
        ..default_control_plane_status()
    })
}

fn controller_status_from_bootstrap(
    bootstrap: &ControllerBootstrap,
    active: bool,
    memory_categories: Option<BTreeMap<String, usize>>,
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
        control_plane: control_plane_status(&bootstrap.project_root, active, memory_categories)?,
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
        control_plane: control_plane_status(project_root, false, None)?,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerDispatchResultStatus {
    Rejected,
    Accepted,
    Queued,
    Running,
    Completed,
    Blocked,
}

impl ControllerDispatchResultStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Accepted => "accepted",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerDispatchProofScope {
    AcceptedOnly,
    DispatchStart,
}

impl ControllerDispatchProofScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::AcceptedOnly => "accepted_only",
            Self::DispatchStart => "dispatch_start",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerDispatchReceipt {
    pub receipt_id: u64,
    pub command_kind: String,
    pub status: ControllerDispatchResultStatus,
    pub stage: String,
    #[serde(default)]
    pub accepted_stage: Option<String>,
    #[serde(default)]
    pub failed_stage: Option<String>,
    pub proof_scope: ControllerDispatchProofScope,
    pub dispatch_start_proven: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchAuthorization {
    pub record: crate::session_actor::ActorRecord,
    pub accepted_stage: String,
    pub receipt: ControllerDispatchReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerAdminReceipt {
    pub receipt_id: u64,
    pub operation_kind: String,
    #[serde(default)]
    pub document_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub diagnostic_payload: Option<String>,
    #[serde(default)]
    pub failed_stage: Option<String>,
    #[serde(default)]
    pub unblock_hint: Option<String>,
    #[serde(default)]
    pub observed_generation: Option<u64>,
    #[serde(default)]
    pub current_generation: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerActorInspection {
    pub target: String,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub record: Option<crate::session_actor::ActorRecord>,
    #[serde(default)]
    pub supervisor_lease: Option<SupervisorLeaseStatus>,
    #[serde(default)]
    pub queue_head: Option<QueueHeadStatus>,
    #[serde(default)]
    pub queue_control: Option<QueueControlStatus>,
    #[serde(default)]
    pub queue_backpressure: Vec<QueueBackpressureStatus>,
    pub projection_lag: bool,
    pub dispatch_attempts: Vec<DispatchAttemptStatus>,
    pub admin_operations: Vec<AdminOperationStatus>,
    pub projection_diagnostics: Vec<ProjectionDiagnosticStatus>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ControllerRequest {
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
    /// Non-blocking acquire: fails immediately if another launch holds the lock.
    pub fn acquire(project_root: &Path) -> Result<Self> {
        Self::acquire_inner(project_root, None)
    }

    /// Bounded blocking acquire. Launch-lock contention is **not** a hard error:
    /// it means another agent-doc process (a concurrent `start`, a sibling
    /// document's controller launch on the same project-root lock, or a
    /// freshly-`execve`'d self-recycle racing its predecessor) is mid-launch.
    /// Failing fast turned that benign race into a `start` failure that surfaced
    /// `controller launch already in progress ... (os error 11)` on the pane —
    /// observed when a `next_queue_item` supervisor hot-reload re-ran `start`
    /// while another launcher still held the lock (#suprecyclelock). Wait up to
    /// `timeout` for the holder to finish so the caller's double-checked
    /// `status` + `connect` can adopt the controller the holder published; only a
    /// genuinely wedged holder (timeout exceeded) returns the error.
    pub fn acquire_blocking(project_root: &Path, timeout: Duration) -> Result<Self> {
        Self::acquire_inner(project_root, Some(timeout))
    }

    fn acquire_inner(project_root: &Path, timeout: Option<Duration>) -> Result<Self> {
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
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(err) => {
                    let contended = err.kind() == std::io::ErrorKind::WouldBlock;
                    match deadline {
                        Some(deadline) if contended && Instant::now() < deadline => {
                            std::thread::sleep(LAUNCH_LOCK_POLL);
                            continue;
                        }
                        _ => {
                            return Err(err).with_context(|| {
                                format!(
                                    "controller launch already in progress: {}",
                                    path.display()
                                )
                            });
                        }
                    }
                }
            }
        }
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

pub(crate) fn current_binary_identity() -> Result<ControllerBinaryIdentity> {
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

/// Resolve the freshly-installed launchable agent-doc binary path. Prefers a
/// launchable `current_exe`, but explicitly skips a missing/`(deleted)` mapped
/// inode (the post-`cargo install` state) and falls back to argv0 + `PATH` so the
/// returned path is the build on disk. Also used by `#suprecycleexe` so the
/// supervisor self-`execve` targets the fresh binary instead of `/proc/self/exe`,
/// which is marked `(deleted)` exactly when the recycle fires.
pub(crate) fn current_agent_doc_binary() -> Result<PathBuf> {
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

struct ControllerDispatchReceiptInsert<'a> {
    document_id: &'a str,
    generation: u64,
    command_kind: &'a str,
    accepted_stage: Option<&'a str>,
    failed_stage: Option<&'a str>,
    diagnostic_payload: &'a str,
    result_status: ControllerDispatchResultStatus,
    proof_scope: ControllerDispatchProofScope,
    dispatch_start_proven: bool,
}

fn insert_dispatch_attempt_record(
    project_root: &Path,
    attempt: ControllerDispatchReceiptInsert<'_>,
) -> Result<ControllerDispatchReceipt> {
    let conn = open_state_db(project_root)?;
    let receipt_id = state_store::insert_dispatch_attempt_in_db(
        &conn,
        &state_store::DispatchAttemptInsert {
            document_id: attempt.document_id,
            generation: attempt.generation,
            command_kind: attempt.command_kind,
            accepted_stage: attempt.accepted_stage,
            failed_stage: attempt.failed_stage,
            diagnostic_payload: attempt.diagnostic_payload,
            result_status: attempt.result_status.as_str(),
            proof_scope: attempt.proof_scope.as_str(),
            dispatch_start_proven: attempt.dispatch_start_proven,
        },
    )?;
    let stage = attempt
        .accepted_stage
        .or(attempt.failed_stage)
        .unwrap_or_else(|| attempt.result_status.as_str())
        .to_string();
    Ok(ControllerDispatchReceipt {
        receipt_id,
        command_kind: attempt.command_kind.to_string(),
        status: attempt.result_status,
        stage,
        accepted_stage: attempt.accepted_stage.map(ToOwned::to_owned),
        failed_stage: attempt.failed_stage.map(ToOwned::to_owned),
        proof_scope: attempt.proof_scope,
        dispatch_start_proven: attempt.dispatch_start_proven,
    })
}

fn insert_admin_operation_record(
    project_root: &Path,
    operation_kind: &str,
    document_id: Option<&str>,
    status: &str,
    diagnostic_payload: Option<&str>,
) -> Result<ControllerAdminReceipt> {
    let conn = open_state_db(project_root)?;
    let receipt_id = state_store::insert_admin_operation_in_db(
        &conn,
        operation_kind,
        document_id,
        status,
        diagnostic_payload,
    )?;
    Ok(ControllerAdminReceipt {
        receipt_id,
        operation_kind: operation_kind.to_string(),
        document_id: document_id.map(ToOwned::to_owned),
        status: status.to_string(),
        diagnostic_payload: diagnostic_payload.map(ToOwned::to_owned),
        failed_stage: None,
        unblock_hint: None,
        observed_generation: None,
        current_generation: None,
    })
}

pub fn persist_session_actor_closeout(file: &Path) -> Result<bool> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(false);
    };
    let Some(project_root) = crate::snapshot::find_project_root(file) else {
        return Ok(false);
    };
    let document_id =
        crate::session_actor::canonical_document_id_in(&project_root, &file.to_string_lossy());
    let queue_head_prompt = state
        .active_queue_heads
        .first()
        .or_else(|| state.active_free_text_queue_heads.first())
        .map(String::as_str);
    let queue_head_id = queue_head_prompt.and_then(queue_head_id_from_prompt);
    let response_commit = state
        .file_hash
        .as_deref()
        .or(state.normalized_file_hash.as_deref())
        .or(state.capture_id.as_deref());
    let mutations = session_actor_closeout_mutations(&state);

    let mut conn = open_state_db(&project_root)?;
    state_store::commit_session_actor_closeout_in_db(
        &mut conn,
        &state_store::SessionActorCloseoutCommit {
            document_id: &document_id,
            cycle_id: &state.cycle_id,
            cycle_state: cycle_phase_store_label(state.phase),
            queue_name: "agent:queue",
            queue_head_id: queue_head_id.as_deref(),
            queue_head_prompt,
            queue_head_state: "consumed",
            response_commit,
            mutations,
        },
    )?;
    Ok(true)
}

fn cycle_phase_store_label(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
}

fn queue_head_id_from_prompt(prompt: &str) -> Option<String> {
    crate::session_check::do_directive_target_ids(&[prompt.to_string()])
        .into_iter()
        .next()
}

fn session_actor_closeout_mutations(
    state: &crate::cycle_state::CycleState,
) -> Vec<state_store::SessionActorCloseoutMutation<'_>> {
    let mut mutations = Vec::new();
    append_closeout_mutations(&mut mutations, &state.pending_done_ids, "done");
    append_closeout_mutations(&mut mutations, &state.pending_gated_ids, "gated");
    append_closeout_mutations(&mut mutations, &state.pending_kept_open_ids, "kept_open");
    append_closeout_mutations(&mut mutations, &state.reaped_pending_ids, "reaped");
    mutations
}

fn append_closeout_mutations<'a>(
    mutations: &mut Vec<state_store::SessionActorCloseoutMutation<'a>>,
    ids: &'a [String],
    status: &'static str,
) {
    for item_id in ids.iter().map(String::as_str).filter(|id| !id.is_empty()) {
        mutations.push(state_store::SessionActorCloseoutMutation {
            item_id,
            mutation_kind: "backlog_completion",
            status,
        });
    }
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

    let actor_projection_hash = actor_projection_intended_hash(project_root).ok();
    if let Err(err) = emit_actor_projection(project_root) {
        record_projection_diagnostic_with_metadata(
            project_root,
            "session-actors.json",
            &record.document_id,
            Some(record.generation),
            actor_projection_hash.as_deref(),
            "retry_pending",
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

/// Resolve the stuck-`Preparing` controller staleness threshold, honoring the
/// `AGENT_DOC_STALE_PREPARING_CONTROLLER_SECS` env override.
pub fn stale_preparing_controller_threshold() -> Duration {
    let secs = std::env::var(STALE_PREPARING_CONTROLLER_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_STALE_PREPARING_CONTROLLER_SECS);
    Duration::from_secs(secs.max(1))
}

/// Pure staleness predicate for the stuck-handoff reaper (#kqr6 / #sjwm). A
/// controller stuck in `Preparing` (or `Promoted`-but-never-finalized) past
/// `stale_after` is wedged. `Stable`/`Retiring`/`Failed` and records with no
/// `handoff_started_at` are never stale. Side-effect free for deterministic
/// unit tests.
fn preparing_controller_is_stale(
    handoff_state: ControllerHandoffState,
    handoff_started_at: Option<u64>,
    now: u64,
    stale_after: Duration,
) -> bool {
    if !matches!(
        handoff_state,
        ControllerHandoffState::Preparing | ControllerHandoffState::Promoted
    ) {
        return false;
    }
    let Some(started) = handoff_started_at else {
        return false;
    };
    now.saturating_sub(started) > stale_after.as_secs()
}

/// Terminate a controller wedged in `Preparing`/`Promoted` past `stale_after`
/// (#kqr6 / #sjwm / #stuckhandoff). Unlike `close_stale_starting_actors_for_caller`
/// — which closes a projection *record* and so cannot stop a live process — this
/// kills the live wedged controller *process* (via `reap_verified_controller_pid`,
/// which only SIGTERM/KILLs a verified same-project `controller serve` pid that is
/// not us) so it stops racing the IDE listener on `ipc.sock`, then transitions the
/// bootstrap to `Failed` so the next invoke promotes a clean controller. Returns
/// `(reaped, kept)`.
///
/// The single controller bootstrap carries no `document_id`, so the per-document
/// supervisor-lease gate the starting-actor reaper uses does not transfer here.
/// The seconds-scale staleness threshold (a healthy handoff completes well under
/// it) plus the `/proc`-cmdline + not-self verification inside
/// `reap_verified_controller_pid` are the safety against killing a healthy
/// mid-handoff.
pub fn terminate_stale_preparing_controllers_for_caller(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let Some(bootstrap) = read_bootstrap(project_root)? else {
        return Ok((0, 0));
    };
    let now = timestamp_secs();
    if !preparing_controller_is_stale(
        bootstrap.handoff_state,
        bootstrap.handoff_started_at,
        now,
        stale_after,
    ) {
        return Ok((0, 1));
    }

    let pid = bootstrap.pid;
    let generation = bootstrap.controller_generation;
    let age = now.saturating_sub(bootstrap.handoff_started_at.unwrap_or(now));

    // Only reap a verified same-project controller process, and never ourselves.
    if pid == std::process::id() || !is_same_project_controller_pid(project_root, pid) {
        crate::ops_log::log_op(
            project_root,
            &format!(
                "stale_preparing_controller_reaped_skipped reason=not_same_project_controller pid={pid} generation={generation} age_secs={age} caller={caller}"
            ),
        );
        return Ok((0, 1));
    }

    if dry_run {
        eprintln!(
            "[{caller}] would terminate stale preparing controller: pid={pid} generation={generation} age_secs={age}"
        );
        return Ok((1, 0));
    }

    reap_verified_controller_pid(project_root, pid, generation);

    // Supersede the wedged record with `Failed` so the next bind promotes fresh
    // instead of re-adopting the stuck generation.
    let mut next = bootstrap.clone();
    next.handoff_state = ControllerHandoffState::Failed;
    if let Err(err) = write_bootstrap_state(&next) {
        eprintln!(
            "[{caller}] warning: failed to mark stale preparing controller failed pid={pid} generation={generation}: {err}"
        );
    }

    crate::ops_log::log_op(
        project_root,
        &format!(
            "stale_preparing_controller_reaped pid={pid} generation={generation} age_secs={age} caller={caller}"
        ),
    );
    Ok((1, 0))
}

/// gc/self-heal entry point for the stuck-handoff reaper. See
/// [`terminate_stale_preparing_controllers_for_caller`].
pub fn terminate_stale_preparing_controllers(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
) -> Result<(usize, usize)> {
    terminate_stale_preparing_controllers_for_caller(project_root, stale_after, dry_run, "gc")
}

/// Process start age (seconds) for a live pid, derived from the `/proc/<pid>`
/// directory mtime (the kernel stamps it at process start). Returns `None` when the
/// process is gone or `/proc` is unavailable.
fn process_start_age_secs(pid: u32) -> Option<u64> {
    let modified = std::fs::metadata(format!("/proc/{pid}"))
        .ok()?
        .modified()
        .ok()?;
    SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

/// True when `/proc/<pid>/cmdline` carries `--handoff-state preparing` — i.e. the
/// controller process was *launched* as a replacement mid-handoff. The wedged
/// replacement (client died before `promote_handoff`) keeps this arg for its whole
/// life, which is exactly what makes it reapable by cmdline scan even after a newer
/// controller overwrote the single per-project bootstrap record.
fn cmdline_has_preparing_handoff(pid: u32) -> bool {
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let args: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).to_string())
        .collect();
    args.windows(2)
        .any(|window| window[0] == "--handoff-state" && window[1] == "preparing")
}

/// M3 (#stuckhandoff2) — process-scan reaper for *orphaned* preparing controllers.
///
/// The record-scoped [`terminate_stale_preparing_controllers_for_caller`] only knows
/// the single `bootstrap.pid`; once a newer clean controller overwrites that record,
/// an old replacement still wedged in `--handoff-state preparing` becomes invisible to
/// it (the operator's `pkill -f 'controller serve ... --handoff-state preparing'`
/// case). This walks `/proc` for same-project `controller serve` processes that still
/// carry `--handoff-state preparing` and whose start age exceeds `stale_after`, then
/// reaps each via the verified-pid path (cmdline + not-self gated). Returns
/// `(reaped, kept)`.
pub fn reap_orphaned_preparing_controllers_for_caller(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let self_pid = std::process::id();
    let generation = read_bootstrap(project_root)?
        .map(|bootstrap| bootstrap.controller_generation)
        .unwrap_or(0);
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Ok((0, 0));
    };
    let mut reaped = 0;
    let mut kept = 0;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        if !is_same_project_controller_pid(project_root, pid) {
            continue;
        }
        if !cmdline_has_preparing_handoff(pid) {
            continue;
        }
        let age = process_start_age_secs(pid).unwrap_or(0);
        if age <= stale_after.as_secs() {
            // Freshly-launched replacement still inside a healthy handoff window.
            kept += 1;
            continue;
        }
        if dry_run {
            eprintln!(
                "[{caller}] would reap orphaned preparing controller: pid={pid} age_secs={age}"
            );
            reaped += 1;
            continue;
        }
        reap_verified_controller_pid(project_root, pid, generation);
        crate::ops_log::log_op(
            project_root,
            &format!(
                "orphaned_preparing_controller_reaped pid={pid} age_secs={age} threshold_secs={} caller={caller}",
                stale_after.as_secs()
            ),
        );
        reaped += 1;
    }
    Ok((reaped, kept))
}

/// gc/self-heal entry point for the orphaned-preparing process-scan reaper. See
/// [`reap_orphaned_preparing_controllers_for_caller`].
pub fn reap_orphaned_preparing_controllers(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
) -> Result<(usize, usize)> {
    reap_orphaned_preparing_controllers_for_caller(project_root, stale_after, dry_run, "gc")
}

/// Parse a `/proc/<pid>/cmdline` arg vector and, if it is an `agent-doc ...
/// controller serve --project-root <root> ...` process, return that `<root>` —
/// regardless of which project it belongs to. The project-scoped
/// [`args_match_same_project_controller`] only answers "is this MY project's
/// controller?"; M5 needs "is this ANY project's controller, and which one?".
fn controller_serve_project_root_from_args(args: &[String]) -> Option<PathBuf> {
    if !args.iter().any(|arg| arg.ends_with("agent-doc")) {
        return None;
    }
    if !args
        .windows(2)
        .any(|window| window[0] == "controller" && window[1] == "serve")
    {
        return None;
    }
    args.windows(2)
        .find_map(|window| (window[0] == "--project-root").then(|| PathBuf::from(&window[1])))
}

fn controller_serve_project_root(pid: u32) -> Option<PathBuf> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).to_string())
        .collect();
    controller_serve_project_root_from_args(&args)
}

/// M5 (#stuckhandoff2) — cross-project process-scan sweep for wedged `Preparing`
/// controllers.
///
/// The per-project reaper [`reap_orphaned_preparing_controllers_for_caller`] only
/// reaps controllers whose `--project-root` matches the caller's, and gc runs only
/// for the triggering project. A controller wedged in ANOTHER project root (a
/// `boost-client` handoff that died while the operator is working in `agent-loop`)
/// stays invisible until agent-doc is next invoked there. M1's self-watchdog
/// already covers this without any external tick, but this sweep is the
/// belt-and-suspenders breadth rung: it walks `/proc` for ANY `agent-doc ...
/// controller serve --handoff-state preparing` process (across all project roots)
/// whose start age exceeds `stale_after` and reaps each through the verified-pid
/// path keyed to that process's OWN `--project-root`. This is the cross-project
/// equivalent of the operator's `pkill -f 'controller serve ... --handoff-state
/// preparing'`, and it needs no global registry — `/proc` is the index. Returns
/// `(reaped, kept)`.
pub fn reap_orphaned_preparing_controllers_all_projects(
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let self_pid = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Ok((0, 0));
    };
    let mut reaped = 0;
    let mut kept = 0;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        let Some(root) = controller_serve_project_root(pid) else {
            continue;
        };
        if !cmdline_has_preparing_handoff(pid) {
            continue;
        }
        let age = process_start_age_secs(pid).unwrap_or(0);
        if age <= stale_after.as_secs() {
            // Freshly-launched replacement still inside a healthy handoff window.
            kept += 1;
            continue;
        }
        if dry_run {
            eprintln!(
                "[{caller}] would reap cross-project preparing controller: pid={pid} root={} age_secs={age}",
                root.display()
            );
            reaped += 1;
            continue;
        }
        let generation = read_bootstrap(&root)?
            .map(|bootstrap| bootstrap.controller_generation)
            .unwrap_or(0);
        reap_verified_controller_pid(&root, pid, generation);
        crate::ops_log::log_op(
            &root,
            &format!(
                "orphaned_preparing_controller_reaped_cross_project pid={pid} root={} age_secs={age} threshold_secs={} caller={caller}",
                root.display(),
                stale_after.as_secs()
            ),
        );
        reaped += 1;
    }
    Ok((reaped, kept))
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

fn actor_projection_intended_hash(project_root: &Path) -> Result<String> {
    let store = load_actor_store(project_root)?;
    Ok(crate::ops_log::content_hash(&serde_json::to_string_pretty(
        &store,
    )?))
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
    let intended_hash = serde_json::to_string(columns)
        .ok()
        .map(|content| crate::ops_log::content_hash(&content));
    if let Err(err) = emit_layout_projection(project_root) {
        record_projection_diagnostic_with_metadata(
            project_root,
            LAYOUT_PROJECTION_FILE,
            "__layout__",
            None,
            intended_hash.as_deref(),
            "retry_pending",
            &format!("failed to emit layout projection after sqlite commit: {err}"),
        );
    }
    Ok(())
}

pub fn project_sessions_projection_for_actor(project_root: &Path, document_id: &str) -> Result<()> {
    project_sessions_projection_for_actor_with_hint(project_root, document_id, None)
}

pub fn project_sessions_projection_for_actor_with_hint(
    project_root: &Path,
    document_id: &str,
    hint: Option<&SessionsProjectionHint>,
) -> Result<()> {
    let Some(record) = load_actor_record(project_root, document_id)? else {
        return Ok(());
    };
    emit_sessions_projection(project_root, &record, hint)
}

fn emit_sessions_projection(
    project_root: &Path,
    focused_record: &crate::session_actor::ActorRecord,
    hint: Option<&SessionsProjectionHint>,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
    let store = load_actor_store_from_db(&conn)?;
    let mut registry = match crate::sessions::load_in(project_root) {
        Ok(registry) => registry,
        Err(err) => {
            record_projection_diagnostic_with_metadata(
                project_root,
                "sessions.json",
                &focused_record.document_id,
                Some(focused_record.generation),
                None,
                "retry_pending",
                &format!("failed to load projection: {err}"),
            );
            crate::sessions::SessionRegistry::new()
        }
    };
    let live_actor_panes: BTreeSet<String> = store
        .values()
        .filter(|record| {
            record.state != crate::session_actor::ActorState::Closed && !record.pane_id.is_empty()
        })
        .map(|record| record.pane_id.clone())
        .collect();
    registry
        .retain(|key, entry| store.contains_key(key) || !live_actor_panes.contains(&entry.pane));

    for record in store.values() {
        if record.state == crate::session_actor::ActorState::Closed || record.pane_id.is_empty() {
            registry.remove(&record.document_id);
            continue;
        }
        let projected_hint = hint.filter(|hint| {
            crate::sessions::canonical_registry_key_in(project_root, &hint.file)
                == record.document_id
        });
        let prior = registry.get(&record.document_id);
        let lease = load_supervisor_lease_from_db(&conn, &record.document_id, record.generation)?;
        let entry = sessions_projection_entry(project_root, record, prior, projected_hint, lease);
        registry.insert(record.document_id.clone(), entry);
    }

    let intended_hash = serde_json::to_string_pretty(&registry)
        .ok()
        .map(|content| crate::ops_log::content_hash(&content));
    if let Err(err) = crate::sessions::save_in(project_root, &registry) {
        record_projection_diagnostic_with_metadata(
            project_root,
            "sessions.json",
            &focused_record.document_id,
            Some(focused_record.generation),
            intended_hash.as_deref(),
            "retry_pending",
            &format!("failed to write projection: {err}"),
        );
        return Ok(());
    }
    let projected = match crate::sessions::load_in(project_root) {
        Ok(registry) => registry,
        Err(err) => {
            record_projection_diagnostic_with_metadata(
                project_root,
                "sessions.json",
                &focused_record.document_id,
                Some(focused_record.generation),
                intended_hash.as_deref(),
                "retry_pending",
                &format!("failed to reload projection: {err}"),
            );
            return Ok(());
        }
    };
    if focused_record.state == crate::session_actor::ActorState::Closed
        || focused_record.pane_id.is_empty()
    {
        if projected.contains_key(&focused_record.document_id) {
            record_projection_diagnostic_with_metadata(
                project_root,
                "sessions.json",
                &focused_record.document_id,
                Some(focused_record.generation),
                intended_hash.as_deref(),
                "retry_pending",
                "sessions projection kept a closed controller actor binding",
            );
        }
    } else if projected
        .get(&focused_record.document_id)
        .is_none_or(|entry| {
            entry.session_id != focused_record.session_id
                || entry.pane != focused_record.pane_id
                || entry.window != focused_record.window_id
        })
    {
        record_projection_diagnostic_with_metadata(
            project_root,
            "sessions.json",
            &focused_record.document_id,
            Some(focused_record.generation),
            intended_hash.as_deref(),
            "retry_pending",
            "sessions projection drifted from controller actor state",
        );
    }
    Ok(())
}

fn sessions_projection_entry(
    project_root: &Path,
    record: &crate::session_actor::ActorRecord,
    prior: Option<&crate::sessions::SessionEntry>,
    hint: Option<&SessionsProjectionHint>,
    lease: Option<SupervisorLeaseStatus>,
) -> crate::sessions::SessionEntry {
    let pid = prior
        .map(|entry| entry.pid)
        .filter(|pid| *pid != 0)
        .or_else(|| hint.map(|hint| hint.pid).filter(|pid| *pid != 0))
        .or_else(|| lease.and_then(|lease| lease.supervisor_pid))
        .unwrap_or(0);
    let cwd = prior
        .map(|entry| entry.cwd.clone())
        .filter(|cwd| !cwd.is_empty())
        .or_else(|| {
            hint.map(|hint| hint.cwd.clone())
                .filter(|cwd| !cwd.is_empty())
        })
        .unwrap_or_else(|| project_root.to_string_lossy().to_string());
    let started = prior
        .map(|entry| entry.started.as_str())
        .filter(|started| !started.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| timestamp_secs().to_string());
    let file = prior
        .map(|entry| entry.file.as_str())
        .filter(|file| !file.is_empty())
        .or_else(|| {
            hint.map(|hint| hint.file.as_str())
                .filter(|file| !file.is_empty())
        })
        .unwrap_or(record.document_id.as_str())
        .to_string();
    let supervisor_instance_id = prior
        .map(|entry| entry.supervisor_instance_id.as_str())
        .filter(|id| !id.is_empty())
        .or_else(|| {
            hint.map(|hint| hint.supervisor_instance_id.as_str())
                .filter(|id| !id.is_empty())
        })
        .unwrap_or("")
        .to_string();

    crate::sessions::SessionEntry {
        pane: record.pane_id.clone(),
        pid,
        cwd,
        started,
        session_id: record.session_id.clone(),
        file,
        window: record.window_id.clone(),
        supervisor_instance_id,
    }
}

#[cfg(test)]
fn record_projection_diagnostic(
    project_root: &Path,
    projection: &str,
    document_id: &str,
    message: &str,
) {
    record_projection_diagnostic_with_metadata(
        project_root,
        projection,
        document_id,
        None,
        None,
        "retry_pending",
        message,
    );
}

fn record_projection_diagnostic_with_metadata(
    project_root: &Path,
    projection: &str,
    document_id: &str,
    source_generation: Option<u64>,
    intended_hash: Option<&str>,
    retry_status: &str,
    message: &str,
) {
    eprintln!(
        "[controller] projection drift projection={} document={} message={}",
        projection, document_id, message
    );
    if let Ok(conn) = open_state_db(project_root) {
        let _ = insert_projection_diagnostic_with_metadata(
            &conn,
            &ProjectionDiagnosticInsert {
                projection,
                document_id,
                message,
                source_generation,
                intended_hash,
                retry_status,
            },
        );
    }
    crate::ops_log::log_op(
        Path::new(document_id),
        &format!(
            "projection_drift projection={} document={} source_generation={} intended_hash={} retry_status={} message={}",
            projection,
            document_id,
            source_generation
                .map(|generation| generation.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            intended_hash.unwrap_or("unknown"),
            retry_status,
            message
        ),
    );
}

mod rpc;
pub use rpc::*;



#[cfg(test)]
/// Spawn a long-sleep sentinel whose `/proc/<pid>/cmdline` matches the
/// `agent-doc controller serve --project-root <root>` shape
/// `is_same_project_controller_pid` checks, without exec-collapsing the
/// shell (the `; :` keeps `sh` resident). The trailing positional params
/// after the `-c` script name become `$0..$N` and are ignored by `sleep`.
pub(crate) fn spawn_controller_sentinel(project_root: &Path) -> std::process::Child {
    let argv0 = project_root.join("agent-doc");
    let child = Command::new("sh")
        .arg("-c")
        .arg("sleep 30; :")
        .arg(argv0.to_string_lossy().to_string())
        .arg("controller")
        .arg("serve")
        .arg("--project-root")
        .arg(project_root.to_string_lossy().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn controller sentinel");
    // `/proc/<pid>/cmdline` is not populated the instant `spawn` returns; wait
    // until the sentinel presents the matching controller cmdline so the
    // reaper's `is_same_project_controller_pid` gate sees it deterministically.
    let pid = child.id();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2)
        && !is_same_project_controller_pid(project_root, pid)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    child
}

#[cfg(test)]
/// Like [`spawn_controller_sentinel`] but the cmdline also carries
/// `--handoff-state preparing`, mirroring a replacement controller launched
/// mid-handoff that wedged because the client never sent `promote_handoff`.
pub(crate) fn spawn_preparing_controller_sentinel(project_root: &Path) -> std::process::Child {
    let argv0 = project_root.join("agent-doc");
    let child = Command::new("sh")
        .arg("-c")
        .arg("sleep 30; :")
        .arg(argv0.to_string_lossy().to_string())
        .arg("controller")
        .arg("serve")
        .arg("--project-root")
        .arg(project_root.to_string_lossy().to_string())
        .arg("--handoff-state")
        .arg("preparing")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn preparing controller sentinel");
    let pid = child.id();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2)
        && !(is_same_project_controller_pid(project_root, pid)
            && cmdline_has_preparing_handoff(pid))
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    child
}

#[cfg(test)]
pub(crate) fn test_bootstrap(dir: &tempfile::TempDir) -> ControllerBootstrap {
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

#[cfg(test)]
pub(crate) fn wait_for_test_controller(project_root: &Path) {
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

#[cfg(test)]
pub(crate) fn write_preparing_bootstrap(
    project_root: &Path,
    pid: u32,
    handoff_started_at: Option<u64>,
) -> ControllerBootstrap {
    let bootstrap = ControllerBootstrap {
        project_root: project_root.to_path_buf(),
        socket_path: socket_path(project_root),
        launch_mode: LaunchMode::Lazy,
        bootstrap_epoch: 0,
        pid,
        controller_binary: None,
        controller_generation: 1004,
        handoff_state: ControllerHandoffState::Preparing,
        handoff_started_at,
        previous_controller_pid: Some(1002),
    };
    write_bootstrap_state(&bootstrap).unwrap();
    bootstrap
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
    fn sessions_projection_creates_missing_registry_entry_from_controller_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%61", "@3");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let projected = crate::sessions::load_in(dir.path()).unwrap();
        let entry = projected.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%61");
        assert_eq!(entry.window, "@3");
        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.file, document_id);
        assert_eq!(entry.cwd, dir.path().to_string_lossy());

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let diagnostics: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'sessions.json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(diagnostics, 0);
    }
    #[test]
    fn sessions_projection_failure_records_generation_hash_retry_metadata() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/sessions.json")).unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%61", "@3");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let (source_generation, intended_hash, retry_status, message): (
            i64,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT source_generation, intended_hash, retry_status, message \
                 FROM projection_diagnostics \
                 WHERE projection = 'sessions.json' \
                 ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(source_generation, 1);
        assert!(!intended_hash.is_empty());
        assert_eq!(retry_status, "retry_pending");
        assert!(message.contains("failed to write projection"));
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
    fn blocking_launch_lock_waits_for_holder_then_acquires() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = LaunchLock::acquire(dir.path()).unwrap();
        let root = dir.path().to_path_buf();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(first);
        });
        // Times out far enough above the holder's release that contention resolves
        // into a successful acquire rather than an error.
        let acquired = LaunchLock::acquire_blocking(dir.path(), Duration::from_secs(2));
        assert!(acquired.is_ok(), "blocking acquire should wait out the holder");
        releaser.join().unwrap();
        let _ = root;
    }
    #[test]
    fn blocking_launch_lock_times_out_when_holder_never_releases() {
        let dir = tempfile::TempDir::new().unwrap();
        let _held = LaunchLock::acquire(dir.path()).unwrap();
        let acquired = LaunchLock::acquire_blocking(dir.path(), Duration::from_millis(100));
        assert!(acquired.is_err(), "a wedged holder must time out");
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
    #[test]
    fn controller_binary_resolution_skips_deleted_proc_self_exe_suffix() {
        // `#suprecycleexe` — the supervisor self-`execve` recycle fires PRECISELY
        // when a fresh binary replaced the running one, so on Linux `current_exe()`
        // (read from `/proc/self/exe`) returns the old inode's path with a literal
        // ` (deleted)` suffix. That path is not launchable; resolution must skip it
        // and fall back to the fresh `PATH` binary instead of returning the deleted
        // path (which `exec` rejects with ENOENT / "os error 2").
        let dir = tempfile::TempDir::new().unwrap();
        let path_bin_dir = dir.path().join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&path_bin, "fresh").unwrap();

        let deleted_exe = dir.path().join("agent-doc (deleted)");
        assert!(!deleted_exe.exists());

        let resolved = resolve_agent_doc_binary_from_env(
            Some(deleted_exe),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            dir.path(),
        )
        .unwrap();

        assert_eq!(resolved, path_bin);
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
        let lease = handle_supervisor_heartbeat(&bootstrap, None, heartbeat).unwrap();
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
        let authorization = envelope.data.unwrap();
        assert_eq!(authorization.accepted_stage, "ready");
        assert!(authorization.receipt.receipt_id > 0);
        assert_eq!(
            authorization.receipt.status,
            ControllerDispatchResultStatus::Accepted
        );
        assert_eq!(
            authorization.receipt.proof_scope,
            ControllerDispatchProofScope::AcceptedOnly
        );
        assert!(!authorization.receipt.dispatch_start_proven);

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
        let error = envelope.error.unwrap();
        assert!(error.contains("requested generation 0"));
        assert!(error.contains("receipt_id="));

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
        let typed_accepted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE result_status = 'accepted' AND proof_scope = 'accepted_only' AND dispatch_start_proven = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let typed_rejected: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE result_status = 'rejected' AND proof_scope = 'accepted_only' AND dispatch_start_proven = 0 AND failed_stage = 'stale_generation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 1);
        assert_eq!(failed, 1);
        assert_eq!(typed_accepted, 1);
        assert_eq!(typed_rejected, 1);
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
    fn controller_admin_operation_returns_durable_receipt() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-admin\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;

        let admin = ControllerRequest {
            command: "admin_operation".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: None,
            state: Some("accepted".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("preflight".to_string()),
            diagnostic_payload: Some("admin receipt test".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&admin).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert!(receipt.receipt_id > 0);
        assert_eq!(receipt.operation_kind, "preflight");
        assert_eq!(receipt.status, "accepted");
        let document_id = doc.to_string_lossy().to_string();
        assert_eq!(receipt.document_id.as_deref(), Some(document_id.as_str()));

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let stored: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM admin_operations WHERE id = ?1 AND operation_kind = 'preflight' AND status = 'accepted'",
                params![sqlite_i64(receipt.receipt_id, "admin receipt id").unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, 1);
    }
    #[test]
    fn controller_queue_control_rejects_stale_generation_and_blocks_dispatch_when_paused() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/queue-control.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-queue\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(&doc, "session-queue", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-queue",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let stale_pause = ControllerRequest {
            command: "queue_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(0),
            state: Some("pause".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("test stale generation".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("pause".to_string()),
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&stale_pause).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert_eq!(receipt.status, "rejected");
        assert_eq!(receipt.failed_stage.as_deref(), Some("stale_generation"));
        assert_eq!(receipt.observed_generation, Some(0));
        assert_eq!(receipt.current_generation, Some(1));

        let conn = open_state_db(dir.path()).unwrap();
        let controls: i64 = conn
            .query_row("SELECT COUNT(*) FROM queue_controls", [], |row| row.get(0))
            .unwrap();
        assert_eq!(controls, 0, "stale queue control must not mutate state");

        let pause = ControllerRequest {
            generation: Some(1),
            reason: Some("operator pause".to_string()),
            ..stale_pause
        };
        let response = handle_request(
            &(serde_json::to_string(&pause).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert_eq!(receipt.status, "accepted");

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-queue".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("paused dispatch test".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&dispatch).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(!envelope.ok);
        assert!(
            envelope
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("failed_stage=queue_paused")
        );

        let conn = open_state_db(dir.path()).unwrap();
        let document_id =
            crate::session_actor::canonical_document_id_in(dir.path(), &doc.to_string_lossy());
        let failed_stage: String = conn
            .query_row(
                "SELECT failed_stage FROM dispatch_attempts WHERE document_id = ?1 ORDER BY id DESC LIMIT 1",
                params![&document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_stage, "queue_paused");
        let backpressure: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM queue_backpressure WHERE document_id = ?1 AND capacity_class = 'queue_paused'",
                params![&document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(backpressure, 1);

        let inspect = ControllerRequest {
            command: "inspect_actor".to_string(),
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
            &(serde_json::to_string(&inspect).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerActorInspection> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let inspection = envelope.data.unwrap();
        assert_eq!(
            inspection
                .queue_control
                .as_ref()
                .map(|control| control.state.as_str()),
            Some("paused")
        );
        let pressure = inspection.queue_backpressure.last().unwrap();
        assert_eq!(pressure.capacity_class, "queue_paused");
        assert_eq!(pressure.command_kind, "managed_reopen");
        assert_eq!(pressure.generation, Some(1));
        assert!(pressure.dispatch_receipt_id.is_some());
        let pressure_json = serde_json::to_value(pressure).unwrap();
        assert_eq!(pressure_json["capacity_class"], "queue_paused");
        assert_eq!(pressure_json["generation"].as_u64(), pressure.generation);
        assert!(
            inspection
                .admin_operations
                .iter()
                .any(|operation| operation.operation_kind == "queue_paused"
                    && operation.status == "accepted")
        );
    }
    #[test]
    fn controller_admin_handoff_and_reap_require_observed_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-control.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-admin-control\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        crate::session_actor::record_session_start_direct(
            &doc,
            "session-admin-control",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-admin-control",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let stale_handoff = ControllerRequest {
            command: "admin_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: Some("%42".to_string()),
            window_id: None,
            generation: Some(0),
            state: Some("handoff".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("test stale handoff".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("handoff".to_string()),
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&stale_handoff).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let receipt = envelope.data.unwrap();
        assert_eq!(receipt.status, "rejected");
        assert_eq!(receipt.failed_stage.as_deref(), Some("stale_generation"));

        let handoff = ControllerRequest {
            generation: Some(1),
            reason: Some("test accepted handoff".to_string()),
            ..stale_handoff
        };
        let response = handle_request(
            &(serde_json::to_string(&handoff).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        assert_eq!(envelope.data.unwrap().status, "accepted");

        let document_id =
            crate::session_actor::canonical_document_id_in(dir.path(), &doc.to_string_lossy());
        let record = load_actor_record(dir.path(), &document_id)
            .unwrap()
            .unwrap();
        assert_eq!(record.pane_id, "%42");
        assert_eq!(record.generation, 2);

        let reap = ControllerRequest {
            command: "admin_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(2),
            state: Some("reap".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("test reap".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("reap".to_string()),
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&reap).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        assert_eq!(envelope.data.unwrap().status, "accepted");

        let record = load_actor_record(dir.path(), &document_id)
            .unwrap()
            .unwrap();
        assert_eq!(record.state, crate::session_actor::ActorState::Closed);
        assert!(record.pane_id.is_empty());
    }
    #[test]
    fn session_actor_closeout_persists_queue_head_cycle_and_pending_mutations() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/session-closeout.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\nagent_doc_session: session-closeout\nagent: codex\n---\n\
agent:queue\n\
- do [#ctrlplane-sessionactor]\n";
        std::fs::write(&doc, content).unwrap();

        let state =
            crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::record_active_queue_heads(
            &doc,
            &["do [#ctrlplane-sessionactor]".to_string()],
        )
        .unwrap();
        crate::cycle_state::record_pending_done_ids(&doc, &["ctrlplane-sessionactor".to_string()])
            .unwrap();
        crate::cycle_state::record_pending_gated_ids(&doc, &["held-item".to_string()]).unwrap();
        crate::cycle_state::record_pending_kept_open_ids(&doc, &["later-item".to_string()])
            .unwrap();
        crate::cycle_state::record_reaped_pending_ids(&doc, &["stale-item".to_string()]).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(content), Some(content))
            .unwrap();

        assert!(persist_session_actor_closeout(&doc).unwrap());

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let document_id =
            crate::session_actor::canonical_document_id_in(dir.path(), &doc.to_string_lossy());
        let cycle: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT state, queue_head_id, response_commit FROM document_cycles WHERE document_id = ?1 AND cycle_id = ?2",
                params![&document_id, &state.cycle_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(cycle.0, "committed");
        assert_eq!(cycle.1.as_deref(), Some("ctrlplane-sessionactor"));
        assert!(cycle.2.is_some());

        let queue: (Option<String>, String, String) = conn
            .query_row(
                "SELECT head_id, prompt, state FROM queue_heads WHERE document_id = ?1 AND queue_name = 'agent:queue'",
                params![&document_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(queue.0.as_deref(), Some("ctrlplane-sessionactor"));
        assert_eq!(queue.1, "do [#ctrlplane-sessionactor]");
        assert_eq!(queue.2, "consumed");

        let mutations: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT item_id, status FROM pending_mutations WHERE document_id = ?1 AND cycle_id = ?2 ORDER BY item_id",
                )
                .unwrap();
            stmt.query_map(params![&document_id, &state.cycle_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
        };
        assert_eq!(
            mutations,
            vec![
                ("ctrlplane-sessionactor".to_string(), "done".to_string()),
                ("held-item".to_string(), "gated".to_string()),
                ("later-item".to_string(), "kept_open".to_string()),
                ("stale-item".to_string(), "reaped".to_string()),
            ]
        );
    }
    #[test]
    fn controller_restart_recovery_rebuilds_memory_and_repairs_projections() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/restart.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-restart\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let mut record = actor_record(&document_id, "%88", "@8");
        record.session_id = "session-restart".to_string();
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let conn = open_state_db(dir.path()).unwrap();
        state_store::upsert_supervisor_lease_in_db(
            &conn,
            &record,
            Some(std::process::id()),
            Some("/tmp/supervisor.sock"),
            "ready",
        )
        .unwrap();
        state_store::insert_dispatch_attempt_in_db(
            &conn,
            &state_store::DispatchAttemptInsert {
                document_id: &document_id,
                generation: record.generation,
                command_kind: "managed_reopen",
                accepted_stage: Some("ready"),
                failed_stage: None,
                diagnostic_payload: "restart recovery test",
                result_status: "accepted",
                proof_scope: "accepted_only",
                dispatch_start_proven: false,
            },
        )
        .unwrap();
        state_store::upsert_document_cycle_state_in_db(
            &conn,
            &document_id,
            "cycle-restart",
            "preflight_started",
            Some("ctrlplane-crashrecover"),
            None,
        )
        .unwrap();
        state_store::store_layout_state_in_db(
            &conn,
            DEFAULT_LAYOUT_SCOPE,
            &["tasks/restart.md".to_string()],
        )
        .unwrap();
        drop(conn);

        std::fs::write(actor_projection_path(dir.path()), "{}").unwrap();
        let _ = std::fs::remove_file(crate::sessions::registry_path_in(dir.path()));
        let _ = std::fs::remove_file(layout_projection_path(dir.path()));

        let mut bootstrap = test_bootstrap(&dir);
        bootstrap.controller_generation = 2;
        let runtime = ControllerRuntime::new(bootstrap).unwrap();

        let memory_record = runtime.actor_record(&document_id).unwrap().unwrap();
        assert_eq!(memory_record.pane_id, "%88");
        assert_eq!(memory_record.session_id, "session-restart");

        let actor_projection: BTreeMap<String, crate::session_actor::ActorRecord> =
            serde_json::from_str(
                &std::fs::read_to_string(actor_projection_path(dir.path())).unwrap(),
            )
            .unwrap();
        assert_eq!(actor_projection.get(&document_id).unwrap(), &record);

        let sessions_projection = crate::sessions::load_in(dir.path()).unwrap();
        let entry = sessions_projection.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%88");
        assert_eq!(entry.window, "@8");
        assert_eq!(entry.session_id, "session-restart");

        let layout_projection: Vec<String> = serde_json::from_str(
            &std::fs::read_to_string(layout_projection_path(dir.path())).unwrap(),
        )
        .unwrap();
        assert_eq!(layout_projection, vec!["tasks/restart.md"]);

        let conn = open_state_db(dir.path()).unwrap();
        let marker_count = |kind: &str, status: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM crash_recovery_markers WHERE marker_kind = ?1 AND status = ?2",
                params![kind, status],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(marker_count("supervisor_lease_reconcile", "reattached"), 1);
        assert_eq!(marker_count("dispatch_receipt_reconcile", "retryable"), 1);
        assert_eq!(marker_count("open_closeout_preserved", "preserved"), 1);
        assert_eq!(marker_count("controller_restart_reconcile", "completed"), 1);
        let cycle_state: String = conn
            .query_row(
                "SELECT state FROM document_cycles WHERE document_id = ?1 AND cycle_id = 'cycle-restart'",
                params![document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cycle_state, "preflight_started");
    }
    #[test]
    fn controller_session_recovery_commands_accept_closed_actor_generation() {
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
        // #supkill-bg blue/green: a `session_restart` against a `Closed` actor now
        // SUPERSEDES it instead of failing closed with `generation 1 is closed`.
        // Restart's purpose is to replace the dead generation, so a closed actor is
        // exactly when it must be accepted (this is the operator's
        // `session restart-supervisor` "generation N is closed" wall).
        assert!(
            envelope.ok,
            "session_restart should supersede a closed actor (blue/green #supkill-bg): {:?}",
            envelope.error
        );
        assert_eq!(envelope.data.unwrap().accepted_stage, "operator_closed");
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
    // ── Stuck-`Preparing` controller reaper (#kqr6 / #sjwm / #stuckhandoff) ──
    #[test]
    fn preparing_controller_staleness_truth_table() {
        let stale_after = Duration::from_secs(45);
        let now = 10_000u64;
        // Preparing + old handoff_started_at + no fresh lease ⇒ reap.
        assert!(preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 100),
            now,
            stale_after,
        ));
        // Promoted-but-never-finalized + old ⇒ reap.
        assert!(preparing_controller_is_stale(
            ControllerHandoffState::Promoted,
            Some(now - 100),
            now,
            stale_after,
        ));
        // Within threshold ⇒ keep (healthy mid-handoff).
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 5),
            now,
            stale_after,
        ));
        // Exactly at threshold ⇒ keep (strictly greater-than is stale).
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 45),
            now,
            stale_after,
        ));
        // Stable / Retiring / Failed ⇒ never stale even when old.
        for state in [
            ControllerHandoffState::Stable,
            ControllerHandoffState::Retiring,
            ControllerHandoffState::Failed,
        ] {
            assert!(!preparing_controller_is_stale(
                state,
                Some(now - 100),
                now,
                stale_after,
            ));
        }
        // No handoff_started_at ⇒ never stale.
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            None,
            now,
            stale_after,
        ));
    }
    #[test]
    fn reaper_keeps_fresh_preparing_bootstrap() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Fresh handoff_started_at (just now) ⇒ healthy mid-handoff, keep.
        write_preparing_bootstrap(dir.path(), std::process::id(), Some(timestamp_secs()));
        let (reaped, kept) = terminate_stale_preparing_controllers(
            dir.path(),
            Duration::from_secs(45),
            false,
        )
        .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Preparing);
    }
    #[test]
    fn reaper_skips_pid_that_is_not_a_same_project_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Our own test pid is alive but is NOT an `agent-doc controller serve`
        // process, so the cmdline gate must refuse to kill it and keep the record.
        let old = timestamp_secs() - 600;
        write_preparing_bootstrap(dir.path(), std::process::id(), Some(old));
        let (reaped, kept) = terminate_stale_preparing_controllers(
            dir.path(),
            Duration::from_secs(45),
            false,
        )
        .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(
            after.handoff_state,
            ControllerHandoffState::Preparing,
            "a non-controller pid must never be killed or marked Failed"
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_reaped_skipped"));
        assert!(ops_log.contains("reason=not_same_project_controller"));
    }
    #[test]
    fn reaper_dry_run_reports_without_killing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_controller_sentinel(dir.path());
        let pid = sentinel.id();
        let old = timestamp_secs() - 600;
        write_preparing_bootstrap(dir.path(), pid, Some(old));

        let (reaped, kept) = terminate_stale_preparing_controllers(
            dir.path(),
            Duration::from_secs(45),
            true, // dry-run
        )
        .unwrap();
        assert_eq!((reaped, kept), (1, 0));
        assert!(process_is_alive(pid), "dry-run must not kill the sentinel");
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Preparing);

        let _ = sentinel.kill();
        let _ = sentinel.wait();
    }
    // ---- M1 (#stuckhandoff2): controller self-watchdog ----
    fn runtime_for_bootstrap(bootstrap: ControllerBootstrap) -> ControllerRuntime {
        // Construct directly (bypassing `ControllerRuntime::new`'s restart-recovery /
        // state-DB load) so the self-watchdog predicate is exercised in isolation.
        ControllerRuntime {
            bootstrap: Mutex::new(bootstrap),
            memory: Mutex::new(ControllerMemoryState {
                actor_store: BTreeMap::new(),
                map_backend: "std_btree_map",
            }),
            recycle_requested: AtomicBool::new(false),
        }
    }
    fn preparing_runtime_bootstrap(
        project_root: &Path,
        handoff_state: ControllerHandoffState,
        handoff_started_at: Option<u64>,
    ) -> ControllerBootstrap {
        ControllerBootstrap {
            project_root: project_root.to_path_buf(),
            socket_path: socket_path(project_root),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: None,
            controller_generation: 7,
            handoff_state,
            handoff_started_at,
            previous_controller_pid: None,
        }
    }
    #[test]
    fn self_watchdog_keeps_fresh_preparing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let runtime = runtime_for_bootstrap(preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Preparing,
            Some(timestamp_secs()),
        ));
        assert!(
            !controller_self_watchdog_should_suicide(&runtime, Duration::from_secs(45)),
            "a controller mid-handoff (fresh handoff_started_at) must not self-terminate"
        );
    }
    #[test]
    fn self_watchdog_keeps_stable() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let runtime = runtime_for_bootstrap(preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Stable,
            None,
        ));
        assert!(
            !controller_self_watchdog_should_suicide(&runtime, Duration::from_secs(0)),
            "a Stable controller must never self-terminate, even at a zero threshold"
        );
    }
    #[test]
    fn self_watchdog_suicides_and_marks_failed_on_stale_preparing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let stale = timestamp_secs().saturating_sub(600);
        let bootstrap = preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Preparing,
            Some(stale),
        );
        write_bootstrap_state(&bootstrap).unwrap();
        let runtime = runtime_for_bootstrap(bootstrap);

        assert!(
            controller_self_watchdog_should_suicide(&runtime, Duration::from_secs(45)),
            "a controller wedged in Preparing past the threshold must self-terminate"
        );
        controller_self_watchdog_suicide(&runtime, Duration::from_secs(45));

        // On-disk bootstrap superseded with Failed so the next bind promotes fresh.
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Failed);
        assert_eq!(after.handoff_started_at, None);
        // In-memory bootstrap mirrors the transition.
        assert_eq!(
            runtime.bootstrap_snapshot().unwrap().handoff_state,
            ControllerHandoffState::Failed
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_self_reaped pid="));
        assert!(ops_log.contains("caller=self_watchdog"));
    }
    // ---- M3 (#stuckhandoff2): orphaned-preparing process-scan reaper ----
    #[test]
    fn process_start_age_secs_reports_for_self() {
        assert!(
            process_start_age_secs(std::process::id()).is_some(),
            "process start age must resolve from /proc for a live pid"
        );
    }
    #[test]
    fn orphan_reaper_keeps_fresh_preparing_sentinel() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(cmdline_has_preparing_handoff(pid));

        // Just-launched (age ~0s) ⇒ inside a healthy handoff window ⇒ keep.
        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(45), false)
                .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        assert!(process_is_alive(pid), "a fresh preparing sentinel must be kept");

        let _ = sentinel.kill();
        let _ = sentinel.wait();
    }
    #[test]
    fn orphan_reaper_ignores_non_preparing_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(!cmdline_has_preparing_handoff(pid));

        // No `--handoff-state preparing` ⇒ not an orphaned handoff ⇒ never scanned.
        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(0), false)
                .unwrap();
        assert_eq!((reaped, kept), (0, 0));
        assert!(process_is_alive(pid), "a plain controller must never be reaped here");

        let _ = sentinel.kill();
        let _ = sentinel.wait();
    }
    #[test]
    fn recycle_reaps_aged_orphaned_preparing_controller() {
        // #stuckhandoff2: `admin recycle` must process-scan-reap a wedged `Preparing`
        // orphan in the root, not merely re-exec the authoritative controller (which
        // an orphan — invisible to the project socket — would survive). With no live
        // controller listening, the recycle RPC no-ops but the project-scoped orphan
        // reap must still fire (`caller=recycle`), so a recycle no longer leaves the
        // zombie behind for M1's later self-watchdog tick to clear.
        let _env = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var(STALE_PREPARING_CONTROLLER_SECS_ENV, "1") };

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(cmdline_has_preparing_handoff(pid));

        // Age strictly past the 1s threshold. Start age is whole seconds (/proc dir
        // mtime), and the reaper keeps when `age <= threshold`, so a ~1.1s-old
        // process floors to age=1 and is kept; sleep past 2s so age=2 > 1 ⇒ reaped.
        std::thread::sleep(Duration::from_millis(2200));

        // No authoritative controller listens on the project socket ⇒ recycle RPC
        // no-ops (Ok(false)); the orphan reap still runs.
        let recycled = recycle_controller(dir.path()).unwrap();
        assert!(!recycled, "no authoritative controller answered the recycle");

        // The aged orphan is our child; poll try_wait for its termination.
        let start = Instant::now();
        let mut exit = None;
        while start.elapsed() < Duration::from_secs(2) {
            match sentinel.try_wait().unwrap() {
                Some(status) => {
                    exit = Some(status);
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
        let status = exit.expect("aged preparing orphan must be reaped by recycle");
        assert!(!status.success(), "orphan must be signal-terminated: {status:?}");

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("orphaned_preparing_controller_reaped pid="));
        assert!(ops_log.contains("caller=recycle"));

        unsafe { std::env::remove_var(STALE_PREPARING_CONTROLLER_SECS_ENV) };
    }
    // ---- #qflood: in-flight dispatch coalescing decision ----
    // ---- M2 (#stuckhandoff2): non-Stable controller refuses dispatch ----
    #[test]
    fn dispatch_refused_when_controller_not_stable() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/m2-gate.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-m2\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-m2", "%41", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-m2",
            "%41",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let dispatch_request = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-m2".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("m2 gate test".to_string()),
        };

        // A controller wedged in Preparing (client died before promote_handoff) is
        // non-authoritative: it must refuse to admit the dispatch.
        let preparing = ControllerBootstrap {
            handoff_state: ControllerHandoffState::Preparing,
            handoff_started_at: Some(timestamp_secs()),
            ..test_bootstrap(&dir)
        };
        let err = handle_dispatch(&preparing, None, dispatch_request()).unwrap_err();
        assert!(
            format!("{err:#}").contains("controller not authoritative"),
            "a Preparing controller must refuse dispatch admission: {err:#}"
        );

        // The refusal is recorded as a rejection receipt + ops-log line for forensics.
        let conn = open_state_db(dir.path()).unwrap();
        let refused: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'controller_not_authoritative'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(refused, 1, "non-Stable dispatch refusal must record a receipt");
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("dispatch_refused_non_stable_controller"));

        // The identical dispatch on a Stable controller passes the authority gate —
        // it may proceed to admit (or fail for an unrelated reason), but never for
        // non-authority.
        let stable = test_bootstrap(&dir); // handoff_state: Stable
        if let Err(err) = handle_dispatch(&stable, None, dispatch_request()) {
            assert!(
                !format!("{err:#}").contains("controller not authoritative"),
                "a Stable controller must not be refused for authority: {err:#}"
            );
        }
    }
    #[test]
    fn autonomous_idle_queue_continuation_refused_when_controller_not_stable() {
        // M2 worktree-write gate (#stuckhandoff2 / #fcc0e): the supervisor's
        // self-driving queue drain is the autonomous worktree-write driver a wedged
        // `Preparing` controller would otherwise use to corrupt the tree between
        // wedge and M1 self-reap — it issues a `dispatch` with
        // `command_kind=idle_queue_continuation` (no external client), so it is the
        // exact path the dispatch-admission gate must refuse on a non-Stable
        // controller. This proves that gate covers the AUTONOMOUS driver, not just
        // operator/route dispatches — the worktree-write protection M2 promises.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/m2-idle.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-idle\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-idle", "%61", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-idle",
            "%61",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let idle_continuation = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-idle".to_string()),
            pane_id: Some("%61".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("idle_queue_continuation".to_string()),
            diagnostic_payload: Some("autonomous queue drain".to_string()),
        };

        let preparing = ControllerBootstrap {
            handoff_state: ControllerHandoffState::Preparing,
            handoff_started_at: Some(timestamp_secs()),
            ..test_bootstrap(&dir)
        };
        let err = handle_dispatch(&preparing, None, idle_continuation()).unwrap_err();
        assert!(
            format!("{err:#}").contains("controller not authoritative"),
            "a Preparing controller must refuse the autonomous idle-queue worktree-write driver: {err:#}"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("dispatch_refused_non_stable_controller"),
            "the autonomous-driver refusal must be logged for forensics:\n{ops_log}"
        );

        // A Stable controller is never refused for authority on the same driver.
        let stable = test_bootstrap(&dir);
        if let Err(err) = handle_dispatch(&stable, None, idle_continuation()) {
            assert!(
                !format!("{err:#}").contains("controller not authoritative"),
                "a Stable controller must not be refused for authority: {err:#}"
            );
        }
    }
    #[test]
    fn dispatch_refused_when_controller_binary_stale() {
        // `#ctlstalebin` (#stuckhandoff2 follow-up): a Stable controller whose own
        // recorded binary no longer matches the installed agent-doc must refuse
        // dispatch admission, so a stale (old-binary) controller cannot keep driving
        // session writes between a `cargo install` and the next handoff — the
        // operator's observed "old binary churns until manual restart" failure. The
        // refusal records a `controller_binary_stale` receipt + ops-log line so the
        // recycle backstop is provable from the logs.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/stale-bin.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-sb\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        crate::session_actor::record_session_start_direct(&doc, "session-sb", "%51", "@1", 1)
            .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "session-sb",
            "%51",
            Some(1),
            crate::session_actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let dispatch_request = || ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-sb".to_string()),
            pane_id: Some("%51".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("idle_queue_continuation".to_string()),
            diagnostic_payload: Some("stale binary test".to_string()),
        };

        // Stable handoff state, but the recorded binary is an old/different build.
        let stale = ControllerBootstrap {
            controller_binary: Some(ControllerBinaryIdentity {
                path: PathBuf::from("/nonexistent/old-agent-doc"),
                version: "0.0.0-stale".to_string(),
                len: 1,
                modified_secs: 1,
                modified_nanos: 0,
            }),
            ..test_bootstrap(&dir)
        };
        let err = handle_dispatch(&stale, None, dispatch_request()).unwrap_err();
        assert!(
            format!("{err:#}").contains("controller_binary_stale"),
            "a stale-binary controller must refuse dispatch admission: {err:#}"
        );

        let conn = open_state_db(dir.path()).unwrap();
        let refused: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'controller_binary_stale'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            refused, 1,
            "stale-binary dispatch refusal must record a receipt"
        );
        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("dispatch_refused_stale_binary"));

        // A current-binary Stable controller is never refused for staleness (it may
        // admit, or fail for an unrelated reason, but never `controller_binary_stale`).
        let current = test_bootstrap(&dir);
        if let Err(err) = handle_dispatch(&current, None, dispatch_request()) {
            assert!(
                !format!("{err:#}").contains("controller_binary_stale"),
                "a current-binary controller must not be refused for staleness: {err:#}"
            );
        }
    }
    // ---- M4 (#stuckhandoff2): client handoff drop-guard ----
    // ---- M5 (#stuckhandoff2): cross-project orphaned-preparing sweep ----
    #[test]
    fn controller_serve_project_root_from_args_extracts_root_for_any_project() {
        // The cmdline shape a sentinel/real controller presents in `/proc`, for a
        // project root that is NOT the caller's — the breadth M5 adds over the
        // per-project reaper.
        let args = vec![
            "/some/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/boost-client".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&args),
            Some(PathBuf::from("/home/me/work/boost-client"))
        );
    }
    #[test]
    #[ignore = "global /proc preparing-controller sweep: would reap the per-project M3 \
                sentinel tests' processes under nextest concurrency. Runs in the \
                `make check` --ignored leg, where it is the only such sweeper."]
    fn all_projects_reaper_reaps_aged_cross_project_preparing_sentinel() {
        // The all-projects API takes no project_root: it must discover this wedged
        // Preparing controller purely from `/proc` and reap it keyed to its OWN root.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(cmdline_has_preparing_handoff(pid));
        assert_eq!(
            controller_serve_project_root(pid).as_deref(),
            Some(dir.path()),
            "the sweep must recover the sentinel's own --project-root from /proc"
        );
        // Age past a zero threshold (start age = /proc dir mtime).
        std::thread::sleep(Duration::from_millis(1100));

        let (reaped, _kept) = reap_orphaned_preparing_controllers_all_projects(
            Duration::from_secs(0),
            false,
            "test",
        )
        .unwrap();
        assert!(
            reaped >= 1,
            "cross-project sweep must reap the aged preparing sentinel"
        );

        // The live orphan must actually be terminated. The sentinel is our child, so
        // a killed process lingers as a zombie until `wait()` — poll `try_wait`.
        let start = Instant::now();
        let mut exit = None;
        while start.elapsed() < Duration::from_secs(2) {
            match sentinel.try_wait().unwrap() {
                Some(status) => {
                    exit = Some(status);
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        }
        let status = exit.expect("aged cross-project preparing orphan must be reaped");
        assert!(!status.success(), "orphan must be signal-terminated: {status:?}");

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("orphaned_preparing_controller_reaped_cross_project pid="));
        assert!(ops_log.contains("caller=test"));
    }
}
