//! Project-local controller shell.
//!
//! The controller is the live authority for document session actor lookup,
//! generation changes, lifecycle reports, and routed dispatch acceptance.
//! Tmux state remains a layout input; ownership state is stored in SQLite.

use crate::process::{is_same_project_controller_pid, process_is_alive};
use agent_doc_controller::dispatch::{
    ControllerDispatchProofScope, ControllerDispatchReceipt, ControllerDispatchResultStatus,
};
use agent_doc_controller::paths::{
    LAYOUT_PROJECTION_FILE, launch_lock_path, layout_projection_path, socket_path, state_path,
};
use agent_doc_controller::status::{
    self, ControlPlaneStoreCounts as ControllerControlPlaneStoreCounts, ControllerBinaryIdentity,
    ControllerBootstrapStatusFacts, ControllerFreshnessFacts, ControllerFreshnessStatus,
    ControllerHandoffState, ControllerStatus, CrashRecoveryStats, LaunchMode,
    controller_restart_recovery_needed, default_controller_generation,
    preparing_controller_is_stale, resolve_controller_identity_version,
    stale_preparing_controller_threshold_from_env_value,
};
use agent_doc_sqlite::state_store;
use agent_doc_turn_executor::binary::current_agent_doc_binary;
use anyhow::{Context, Result};
use fs2::FileExt;
use interprocess::local_socket::{
    GenericFilePath, ListenerNonblockingMode, ListenerOptions, ToFsName,
    traits::{Listener as _, Stream as _},
};
use lazily::{CellHandle, ThreadSafeContext, ThreadSafeSignalHandle};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

// The SQLite state layer (the only `rusqlite::Connection` surface) lives in
// `agent-doc-sqlite::state_store`. Keep its storage/status types private to
// this orchestration module; callers that need to name them should import the
// focused crate directly.
#[cfg(test)]
use state_store::state_db_path;
use state_store::{
    AdminOperationStatus, DispatchAttemptStatus, ProjectionDiagnosticStatus,
    QueueBackpressureStatus, QueueControlStatus, QueueHeadStatus, SessionOperatorStatus,
    SupervisorLeaseStatus,
};
use state_store::{
    Connection, ProjectionDiagnosticInsert, insert_projection_diagnostic,
    insert_projection_diagnostic_with_metadata, insert_state_event_in_db,
    load_actor_record_from_db, load_actor_store_from_db, load_control_plane_store_counts,
    load_layout_state_from_db, load_session_operator_status_from_db, load_state_events_from_db,
    load_supervisor_lease_from_db, open_state_db, store_layout_state_in_db, timestamp_secs,
};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Condvar, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_LAYOUT_SCOPE: &str = "default";
const CONNECT_WAIT: Duration = Duration::from_secs(3);
#[cfg(not(any(test, feature = "test-support")))]
const LAUNCH_CONNECT_WAIT: Duration = Duration::from_secs(45);
#[cfg(any(test, feature = "test-support"))]
const LAUNCH_CONNECT_WAIT: Duration = Duration::from_millis(500);
#[cfg(test)]
#[allow(dead_code)]
const HANDOFF_CONNECT_WAIT: Duration = Duration::from_secs(30);
const CONNECT_POLL: Duration = Duration::from_millis(50);
/// How long a contended launch waits for the current holder to finish before
/// giving up. Sized above `LAUNCH_CONNECT_WAIT` so a waiter outlasts the holder's
/// full `launch_detached` + `wait_for_controller_after_launch` window and can adopt the
/// controller the holder published instead of failing the start (#suprecyclelock).
#[cfg(not(any(test, feature = "test-support")))]
const LAUNCH_LOCK_WAIT: Duration = Duration::from_secs(50);
#[cfg(any(test, feature = "test-support"))]
const LAUNCH_LOCK_WAIT: Duration = Duration::from_secs(1);
const LAUNCH_LOCK_POLL: Duration = Duration::from_millis(50);
#[cfg(not(any(test, feature = "test-support")))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(any(test, feature = "test-support"))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_millis(250);
const CONTROLLER_IDLE_CLIENT_TIMEOUT: Duration = CONTROLLER_RPC_TIMEOUT;
const SUPERVISOR_RECYCLE_SETTLE_WAIT: Duration = Duration::from_secs(10);

const STALE_PREPARING_CONTROLLER_SECS_ENV: &str = "AGENT_DOC_STALE_PREPARING_CONTROLLER_SECS";

/// `#ctlrecycle` — how long a process must continuously observe "wants-recycle AND
/// idle" before it self-recycles onto a freshly-installed binary. Debounce so a
/// short gap between queue items never triggers a recycle.
const DEFAULT_RECYCLE_IDLE_GRACE_SECS: u64 = 5;
const RECYCLE_IDLE_GRACE_SECS_ENV: &str = "AGENT_DOC_RECYCLE_IDLE_GRACE_SECS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerQueueConsumptionOutcome {
    pub consumed_text: String,
    pub remaining: usize,
    pub drained: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerEditorRouteInvocation {
    pub file: PathBuf,
    pub relative_path: String,
    pub layout_args: Vec<String>,
    pub dispatch_only: bool,
    pub plain_trigger: bool,
    pub wait_for_ready_secs: Option<u64>,
    pub force_disk: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerEditorRouteRuntimeResult {
    pub exit_code: i32,
    pub output: String,
}

/// Outcome of an in-controller git commit (`commit_document`). The commit runs
/// inside the controller process, where its own converged relay canonical IS the
/// authority — so a document with a live editor commits authoritatively instead
/// of the CLI failing closed as a non-authoritative replica.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerCommitDocumentOutcome {
    pub did_commit: bool,
    #[serde(default)]
    pub vcs_refresh_signaled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTmuxLayoutSyncInvocation {
    pub columns: Vec<String>,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub no_autostart: bool,
    #[serde(default)]
    pub exact_visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTmuxLayoutSyncReceipt {
    pub applied: bool,
    pub reason: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
    pub no_autostart: bool,
    pub exact_visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTmuxLayoutSyncStateInvocation {
    pub columns: Vec<String>,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTmuxLayoutSyncStateReport {
    pub synced: bool,
    pub reason: String,
    pub expected_documents: Vec<String>,
    pub actual_documents: Vec<String>,
    pub panes: Vec<String>,
    #[serde(default)]
    pub session_name: Option<String>,
    #[serde(default)]
    pub window_id: Option<String>,
    #[serde(default)]
    pub window_name: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
}

pub trait ProjectControllerRuntimeEffects: Send + Sync + 'static {
    fn consume_queue_prompt_force_disk(
        &self,
        file: &Path,
    ) -> Result<Option<ControllerQueueConsumptionOutcome>>;

    fn route_auto_start(
        &self,
        tmux: &tmux_router::Tmux,
        file: &Path,
        session_id: &str,
        file_arg: &str,
        window: Option<&str>,
    ) -> Result<String>;

    fn run_editor_route(
        &self,
        invocation: ControllerEditorRouteInvocation,
    ) -> Result<ControllerEditorRouteRuntimeResult>;

    fn sync_tmux_layout(
        &self,
        project_root: &Path,
        invocation: ControllerTmuxLayoutSyncInvocation,
    ) -> Result<ControllerTmuxLayoutSyncReceipt>;

    /// Perform the git commit for `file` from inside the controller process, where
    /// the converged relay canonical IS the authority. `authoritative_compaction`
    /// selects the compaction-aware commit entry (guard stand-down). The binary
    /// wires this to `agent-doc-commit-io` (which depends on this crate, so the
    /// controller cannot call it directly — hence the effects port).
    fn commit_document(
        &self,
        file: &Path,
        authoritative_compaction: bool,
    ) -> Result<ControllerCommitDocumentOutcome>;
}

static RUNTIME_EFFECTS: OnceLock<&'static dyn ProjectControllerRuntimeEffects> = OnceLock::new();

pub fn install_runtime_effects(effects: &'static dyn ProjectControllerRuntimeEffects) {
    let _ = RUNTIME_EFFECTS.set(effects);
}

pub(crate) fn runtime_effects() -> Result<&'static dyn ProjectControllerRuntimeEffects> {
    if let Some(effects) = RUNTIME_EFFECTS.get().copied() {
        return Ok(effects);
    }
    #[cfg(test)]
    {
        Ok(&TEST_RUNTIME_EFFECTS)
    }
    #[cfg(not(test))]
    {
        Err(anyhow::anyhow!(
            "project controller runtime effects were not installed by the binary"
        ))
    }
}

#[cfg(test)]
struct TestProjectControllerRuntimeEffects;

#[cfg(test)]
impl ProjectControllerRuntimeEffects for TestProjectControllerRuntimeEffects {
    fn consume_queue_prompt_force_disk(
        &self,
        file: &Path,
    ) -> Result<Option<ControllerQueueConsumptionOutcome>> {
        let content = std::fs::read_to_string(file)
            .context("project controller test queue consume: failed to read document")?;
        let Some(consumed_text) = agent_doc_queue::queue_heads::active_queue_head_text(&content)?
        else {
            return Ok(None);
        };
        let node_keys = agent_doc_queue::queue_consume::queue_prompt_node_keys_for_texts(
            &content,
            std::slice::from_ref(&consumed_text),
            &[],
        )?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "project controller test queue consume: failed to derive active head node key"
            )
        })?;
        let mut new_content =
            agent_doc_queue::queue_consume::consume_queue_nodes_by_key(&content, &node_keys.keys)?;
        let remaining = agent_doc_queue::queue_heads::active_queue_heads(&new_content).len();
        if remaining == 0 {
            new_content =
                agent_doc_frontmatter::frontmatter::merge_queue_state(&new_content, false)?;
        }
        std::fs::write(file, &new_content)
            .context("project controller test queue consume: failed to write document")?;
        Ok(Some(ControllerQueueConsumptionOutcome {
            consumed_text,
            remaining,
            drained: remaining == 0,
        }))
    }

    fn route_auto_start(
        &self,
        _tmux: &tmux_router::Tmux,
        _file: &Path,
        _session_id: &str,
        _file_arg: &str,
        _window: Option<&str>,
    ) -> Result<String> {
        anyhow::bail!("project controller test runtime does not route auto-start")
    }

    fn run_editor_route(
        &self,
        invocation: ControllerEditorRouteInvocation,
    ) -> Result<ControllerEditorRouteRuntimeResult> {
        Ok(ControllerEditorRouteRuntimeResult {
            exit_code: 0,
            output: format!(
                "test editor route accepted for {}",
                invocation.relative_path
            ),
        })
    }

    fn commit_document(
        &self,
        _file: &Path,
        _authoritative_compaction: bool,
    ) -> Result<ControllerCommitDocumentOutcome> {
        anyhow::bail!("project controller test runtime does not commit documents")
    }

    fn sync_tmux_layout(
        &self,
        _project_root: &Path,
        invocation: ControllerTmuxLayoutSyncInvocation,
    ) -> Result<ControllerTmuxLayoutSyncReceipt> {
        Ok(ControllerTmuxLayoutSyncReceipt {
            applied: true,
            reason: "test_runtime".to_string(),
            columns: invocation.columns,
            window: invocation.window,
            focus: invocation.focus,
            no_autostart: invocation.no_autostart,
            exact_visible: invocation.exact_visible,
        })
    }
}

#[cfg(test)]
static TEST_RUNTIME_EFFECTS: TestProjectControllerRuntimeEffects =
    TestProjectControllerRuntimeEffects;

/// `#orchver` — stamp the binary crate's `CARGO_PKG_VERSION` into controller identity.
/// This keeps stale-binary warnings tied to the installed `agent-doc` executable even if
/// an internal crate version diverges in a future packaging layout. Library-only callers
/// and tests fall back to the orchestration crate version.
static BINARY_VERSION: OnceLock<String> = OnceLock::new();

/// Record the real top-level `agent-doc` binary version for identity reporting. Called once
/// from `main()`; the first writer wins and any later call is intentionally a no-op.
pub fn set_binary_version(version: &str) {
    if BINARY_VERSION.set(version.to_string()).is_err() {
        // Already initialized (e.g. a second call in tests) — keep the first version.
    }
}

/// Resolve the version stamped into [`ControllerBinaryIdentity`]. Prefers the binary-injected
/// value; falls back to the orchestration crate version only when unset.
fn identity_version() -> String {
    resolve_controller_identity_version(
        BINARY_VERSION.get().map(String::as_str),
        env!("CARGO_PKG_VERSION"),
    )
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

#[derive(Debug)]
struct ControllerMemoryState {
    actor_store: BTreeMap<String, agent_doc_sqlite::state_store::ActorRecord>,
    state_ledger: agent_doc_state_backbone::EventLedger,
    state_projection: agent_doc_state_backbone::StateBackboneProjection,
    map_backend: &'static str,
}

impl ControllerMemoryState {
    fn load(project_root: &Path) -> Result<Self> {
        let state_ledger = load_state_event_ledger(project_root)?;
        let state_projection = state_ledger.project();
        Ok(Self {
            actor_store: load_actor_store(project_root)?,
            state_ledger,
            state_projection,
            map_backend: "std_btree_map",
        })
    }
}

pub(crate) struct ControllerRuntime {
    bootstrap: Mutex<ControllerBootstrap>,
    memory: Mutex<ControllerMemoryState>,
    supervisor_recycle_graph: ControllerSupervisorRecycleGraph,
    supervisor_recycle_waiters: Condvar,
    /// `#ctlrecycle` R2 — set true by the `recycle` RPC (`agent-doc admin recycle`).
    /// The serve-loop idle poll honors it the same way it honors binary staleness:
    /// once no dispatch is in flight (debounced), the controller self-terminates and
    /// the next `connect_or_launch` relaunches the fresh binary.
    recycle_requested: AtomicBool,
    /// `#recycleforce` — set true by the `recycle_force` RPC (`agent-doc admin
    /// recycle --force`). An explicit operator override: the serve-loop idle poll
    /// recycles WITHOUT waiting on the in-flight-dispatch idle gate, so a forced
    /// recycle takes effect at the next tick even mid-turn. Implies
    /// `recycle_requested`.
    recycle_forced: AtomicBool,
}

impl ControllerRuntime {
    fn new(bootstrap: ControllerBootstrap) -> Result<Self> {
        if controller_restart_recovery_needed(
            bootstrap.controller_generation,
            bootstrap.previous_controller_pid,
        ) {
            recover_controller_after_restart(&bootstrap)?;
        }
        let memory = ControllerMemoryState::load(&bootstrap.project_root)?;
        let supervisor_recycle_graph = ControllerSupervisorRecycleGraph::new(
            memory.state_projection.project_supervisor_recycle(),
        );
        Ok(Self {
            bootstrap: Mutex::new(bootstrap),
            memory: Mutex::new(memory),
            supervisor_recycle_graph,
            supervisor_recycle_waiters: Condvar::new(),
            recycle_requested: AtomicBool::new(false),
            recycle_forced: AtomicBool::new(false),
        })
    }

    /// `#ctlrecycle` R2 — mark this controller to recycle at the next idle boundary.
    fn request_recycle(&self) {
        self.recycle_requested.store(true, Ordering::SeqCst);
    }

    fn recycle_requested(&self) -> bool {
        self.recycle_requested.load(Ordering::SeqCst)
    }

    /// `#recycleforce` — mark this controller to recycle promptly, overriding the
    /// in-flight-dispatch idle gate (`agent-doc admin recycle --force`). Also sets
    /// `recycle_requested` so the existing want-recycle predicate fires.
    fn request_recycle_force(&self) {
        self.recycle_forced.store(true, Ordering::SeqCst);
        self.recycle_requested.store(true, Ordering::SeqCst);
    }

    fn recycle_forced(&self) -> bool {
        self.recycle_forced.load(Ordering::SeqCst)
    }

    fn bootstrap_snapshot(&self) -> Result<ControllerBootstrap> {
        self.bootstrap
            .lock()
            .map_err(|_| anyhow::anyhow!("controller bootstrap lock poisoned"))
            .map(|guard| guard.clone())
    }

    fn actor_record(
        &self,
        document_id: &str,
    ) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
        self.memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))
            .map(|memory| memory.actor_store.get(document_id).cloned())
    }

    fn refresh_memory(&self) -> Result<()> {
        let project_root = self.bootstrap_snapshot()?.project_root;
        let next = ControllerMemoryState::load(&project_root)?;
        let recycle = next.state_projection.project_supervisor_recycle();
        let mut memory = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))?;
        *memory = next;
        drop(memory);
        self.supervisor_recycle_graph.set(recycle);
        self.supervisor_recycle_waiters.notify_all();
        Ok(())
    }

    fn apply_state_event(&self, event: &agent_doc_state_backbone::StateEvent) -> Result<()> {
        let recycle = {
            let mut memory = self
                .memory
                .lock()
                .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))?;
            memory.state_ledger.append(event.clone());
            memory.state_projection.apply(event);
            memory.state_projection.project_supervisor_recycle()
        };
        self.supervisor_recycle_graph.set(recycle);
        self.supervisor_recycle_waiters.notify_all();
        Ok(())
    }

    fn state_subscribe(
        &self,
        document_hash: &str,
        last_epoch: u64,
    ) -> Result<agent_doc_state_wire::WireSubscribe> {
        self.memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))
            .map(|memory| {
                agent_doc_state_wire::subscribe(&memory.state_ledger, document_hash, last_epoch)
            })
    }

    fn supervisor_recycle_projection(
        &self,
    ) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
        Ok(self.supervisor_recycle_graph.projection())
    }

    fn document_state_projection(
        &self,
        document_hash: &str,
    ) -> Result<Option<agent_doc_state_backbone::DocumentStateProjection>> {
        self.memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))
            .map(|memory| memory.state_projection.document(document_hash).cloned())
    }

    fn wait_for_supervisor_recycle_settle(
        &self,
        timeout: Duration,
    ) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
        let started = Instant::now();
        let mut memory = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))?;
        loop {
            let projection = memory.state_projection.project_supervisor_recycle();
            self.supervisor_recycle_graph.set(projection.clone());
            if !self.supervisor_recycle_graph.in_flight() {
                return Ok(projection);
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                anyhow::bail!(
                    "supervisor recycle still in flight after {}ms reason={} recycle_epoch={}",
                    elapsed.as_millis(),
                    projection.reason.as_deref().unwrap_or("unknown"),
                    projection.recycle_epoch
                );
            }
            let remaining = timeout.saturating_sub(elapsed);
            let (next_memory, _wait) = self
                .supervisor_recycle_waiters
                .wait_timeout(memory, remaining)
                .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))?;
            memory = next_memory;
        }
    }

    fn memory_categories(&self) -> Result<BTreeMap<String, usize>> {
        let memory = self
            .memory
            .lock()
            .map_err(|_| anyhow::anyhow!("controller memory lock poisoned"))?;
        Ok(agent_doc_controller::status::status_categories([
            ("actor_records", memory.actor_store.len()),
            (
                "state_backbone_documents",
                memory.state_projection.documents.len(),
            ),
            (
                "map_backend_std_btree_map",
                usize::from(memory.map_backend == "std_btree_map"),
            ),
            ("write_through_sqlite", 1),
        ]))
    }
}

struct ControllerSupervisorRecycleGraph {
    ctx: ThreadSafeContext,
    projection: CellHandle<agent_doc_state_backbone::SupervisorRecycleProjection>,
    in_flight: ThreadSafeSignalHandle<bool>,
}

impl ControllerSupervisorRecycleGraph {
    fn new(initial: agent_doc_state_backbone::SupervisorRecycleProjection) -> Self {
        let ctx = ThreadSafeContext::new();
        let projection = ctx.cell(initial);
        let in_flight = ctx.signal(move |ctx| {
            matches!(
                ctx.get_cell(&projection).phase,
                agent_doc_state_backbone::SupervisorRecyclePhase::InFlight
            )
        });
        Self {
            ctx,
            projection,
            in_flight,
        }
    }

    fn set(&self, projection: agent_doc_state_backbone::SupervisorRecycleProjection) {
        self.ctx.set_cell(&self.projection, projection);
    }

    fn projection(&self) -> agent_doc_state_backbone::SupervisorRecycleProjection {
        self.ctx.get_cell(&self.projection)
    }

    fn in_flight(&self) -> bool {
        self.ctx.get_signal(&self.in_flight)
    }
}

fn recover_controller_after_restart(bootstrap: &ControllerBootstrap) -> Result<CrashRecoveryStats> {
    let project_root = &bootstrap.project_root;
    let conn = open_state_db(project_root)?;
    let store = load_actor_store_from_db(&conn)?;
    let mut stats = CrashRecoveryStats::new(store.len());

    reconcile_supervisor_leases_after_restart(&conn, &store, &mut stats)?;
    reconcile_open_dispatch_receipts_after_restart(&conn, &mut stats)?;
    preserve_open_closeout_cycles_after_restart(&conn, &mut stats)?;
    drop(conn);

    let conn = open_state_db(project_root)?;
    if state_store::layout_scope_exists(&conn, DEFAULT_LAYOUT_SCOPE)? {
        drop(conn);
        match emit_layout_projection(project_root) {
            Ok(()) => stats.record_projection_emitted(),
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
    state_store::upsert_crash_recovery_marker_in_db(
        &conn,
        "controller_restart_reconcile",
        "controller_restart_reconcile:project",
        None,
        None,
        "completed",
        Some(&stats.completion_payload()),
    )?;
    Ok(stats)
}

fn reconcile_supervisor_leases_after_restart(
    conn: &Connection,
    store: &BTreeMap<String, agent_doc_sqlite::state_store::ActorRecord>,
    stats: &mut CrashRecoveryStats,
) -> Result<()> {
    let now = timestamp_secs();
    for record in store.values() {
        if record.state == agent_doc_sqlite::state_store::ActorState::Closed {
            continue;
        }
        let Some(lease) =
            load_supervisor_lease_from_db(conn, &record.document_id, record.generation)?
        else {
            continue;
        };
        let fresh = status::supervisor_lease_is_fresh_and_alive(
            lease.last_heartbeat,
            lease.supervisor_pid.is_some_and(process_is_alive),
            now,
            Duration::from_secs(60),
        );
        let marker_status = stats.record_supervisor_lease_reconcile(fresh);
        let marker_payload = status::supervisor_lease_reconcile_payload(
            &record.session_id,
            &record.pane_id,
            lease.runtime_state.as_deref(),
            lease.last_heartbeat,
        );
        let dedupe_key = format!(
            "supervisor_lease_reconcile:{}:{}",
            record.document_id, record.generation
        );
        state_store::upsert_crash_recovery_marker_in_db(
            conn,
            "supervisor_lease_reconcile",
            &dedupe_key,
            Some(&record.document_id),
            Some(record.generation),
            marker_status,
            Some(&marker_payload),
        )?;
    }
    Ok(())
}

fn reconcile_open_dispatch_receipts_after_restart(
    conn: &Connection,
    stats: &mut CrashRecoveryStats,
) -> Result<()> {
    let receipts = {
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
        let mut receipts = Vec::new();
        while let Some(row) = rows.next()? {
            receipts.push((
                row.get::<_, i64>("id")?,
                row.get::<_, String>("document_id")?,
                row.get::<_, i64>("generation")?,
                row.get::<_, String>("command_kind")?,
                row.get::<_, Option<String>>("result_status")?,
                row.get::<_, Option<String>>("proof_scope")?,
                row.get::<_, i64>("dispatch_start_proven")?,
            ));
        }
        receipts
    };
    for (
        receipt_id,
        document_id,
        generation,
        command_kind,
        result_status,
        proof_scope,
        dispatch_start_proven,
    ) in &receipts
    {
        let dispatch_start_proven = *dispatch_start_proven != 0;
        let marker_status = stats.record_dispatch_receipt_reconcile(dispatch_start_proven);
        let receipt_id = state_store::sqlite_u64(*receipt_id, "dispatch receipt id")?;
        let generation = state_store::sqlite_u64(*generation, "dispatch generation")?;
        let marker_payload = status::dispatch_receipt_reconcile_payload(
            receipt_id,
            command_kind.as_str(),
            result_status.as_deref(),
            proof_scope.as_deref(),
            dispatch_start_proven,
        );
        let dedupe_key = format!("dispatch_receipt_reconcile:receipt:{receipt_id}");
        state_store::upsert_crash_recovery_marker_in_db(
            conn,
            "dispatch_receipt_reconcile",
            &dedupe_key,
            Some(document_id.as_str()),
            Some(generation),
            marker_status,
            Some(&marker_payload),
        )?;
    }
    Ok(())
}

fn preserve_open_closeout_cycles_after_restart(
    conn: &Connection,
    stats: &mut CrashRecoveryStats,
) -> Result<()> {
    let cycles = {
        let mut stmt = conn.prepare(
            r#"
            SELECT document_id, cycle_id, state, queue_head_id
            FROM document_cycles
            WHERE state NOT IN ('committed', 'abandoned')
            "#,
        )?;
        let mut rows = stmt.query([])?;
        let mut cycles = Vec::new();
        while let Some(row) = rows.next()? {
            cycles.push((
                row.get::<_, String>("document_id")?,
                row.get::<_, String>("cycle_id")?,
                row.get::<_, String>("state")?,
                row.get::<_, Option<String>>("queue_head_id")?,
            ));
        }
        cycles
    };
    for (document_id, cycle_id, state, queue_head_id) in &cycles {
        let marker_status = stats.record_open_closeout_preserved();
        let marker_payload = status::open_closeout_preserved_payload(
            cycle_id.as_str(),
            state.as_str(),
            queue_head_id.as_deref(),
        );
        let dedupe_key = format!("open_closeout_preserved:{document_id}:{cycle_id}");
        state_store::upsert_crash_recovery_marker_in_db(
            conn,
            "open_closeout_preserved",
            &dedupe_key,
            Some(document_id.as_str()),
            None,
            marker_status,
            Some(&marker_payload),
        )?;
    }
    Ok(())
}

pub(crate) fn controller_bootstrap_status_facts(
    bootstrap: &ControllerBootstrap,
) -> ControllerBootstrapStatusFacts {
    ControllerBootstrapStatusFacts {
        project_root: bootstrap.project_root.clone(),
        socket_path: bootstrap.socket_path.clone(),
        launch_mode: bootstrap.launch_mode,
        bootstrap_epoch: bootstrap.bootstrap_epoch,
        pid: bootstrap.pid,
        controller_binary: bootstrap.controller_binary.clone(),
        controller_generation: bootstrap.controller_generation,
        handoff_state: bootstrap.handoff_state,
        handoff_started_at: bootstrap.handoff_started_at,
        previous_controller_pid: bootstrap.previous_controller_pid,
    }
}

pub(crate) fn control_plane_store_counts(
    project_root: &Path,
) -> Result<ControllerControlPlaneStoreCounts> {
    let conn = open_state_db(project_root)?;
    let counts = load_control_plane_store_counts(&conn)?;
    Ok(ControllerControlPlaneStoreCounts {
        actor_documents: counts.actor_documents,
        live_actor_documents: counts.live_actor_documents,
        actor_transitions: counts.actor_transitions,
        supervisor_leases: counts.supervisor_leases,
        state_events: counts.state_events,
        dispatch_receipts: counts.dispatch_receipts,
        queue_heads: counts.queue_heads,
        document_cycles: counts.document_cycles,
        pending_mutations: counts.pending_mutations,
        projection_diagnostics: counts.projection_diagnostics,
        admin_operations: counts.admin_operations,
        queue_controls: counts.queue_controls,
        queue_backpressure: counts.queue_backpressure,
        crash_recovery_markers: counts.crash_recovery_markers,
        layout_states: counts.layout_states,
    })
}

pub(crate) fn controller_freshness_facts(
    controller_pid: Option<u32>,
    route_owned_supervisor_pid: Option<u32>,
) -> ControllerFreshnessFacts {
    let installed_binary = current_binary_identity().ok();
    let installed_inode = installed_binary
        .as_ref()
        .and_then(|identity| agent_doc_fs::inode_of_path(&identity.path));
    ControllerFreshnessFacts {
        installed_binary,
        installed_inode,
        controller_pid,
        controller_running_inode: controller_pid.and_then(agent_doc_fs::running_exe_inode_for_pid),
        route_owned_supervisor_pid,
        route_owned_supervisor_running_inode: route_owned_supervisor_pid
            .and_then(agent_doc_fs::running_exe_inode_for_pid),
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
    pub state: agent_doc_sqlite::state_store::ActorState,
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
    pub record: agent_doc_sqlite::state_store::ActorRecord,
    pub accepted_stage: String,
    pub receipt: ControllerDispatchReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorReplacementRequest {
    pub file: PathBuf,
    pub mode: String,
    pub force: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorReplacementReceipt {
    pub record: agent_doc_sqlite::state_store::ActorRecord,
    pub accepted_stage: String,
    pub operator_receipt: ControllerDispatchReceipt,
    pub background_started: bool,
    pub mode: String,
    pub force: bool,
    pub session_id: String,
    pub pane_id: String,
    pub generation: u64,
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
    pub record: Option<agent_doc_sqlite::state_store::ActorRecord>,
    #[serde(default)]
    pub supervisor_lease: Option<SupervisorLeaseStatus>,
    #[serde(default)]
    pub freshness: Option<ControllerFreshnessStatus>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTmuxFocusState {
    pub active: bool,
    pub reason: String,
    #[serde(default)]
    pub session_name: Option<String>,
    #[serde(default)]
    pub window_id: Option<String>,
    #[serde(default)]
    pub window_name: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub record: Option<agent_doc_sqlite::state_store::ActorRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTmuxFocusReceipt {
    pub focused: bool,
    pub reason: String,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub session_name: Option<String>,
    #[serde(default)]
    pub window_id: Option<String>,
    #[serde(default)]
    pub window_name: Option<String>,
}

// Controller status records now live in `agent-doc-sqlite::state_store`; this
// module imports them privately while callers that need to name them import the
// focused crate directly.

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
    pub record: Option<agent_doc_sqlite::state_store::ActorRecord>,
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
    waited: bool,
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

    pub fn waited(&self) -> bool {
        self.waited
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
        let mut waited = false;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => {
                    return Ok(Self {
                        _file: file,
                        waited,
                    });
                }
                Err(err) => {
                    let contended = err.kind() == std::io::ErrorKind::WouldBlock;
                    match deadline {
                        Some(deadline) if contended && Instant::now() < deadline => {
                            waited = true;
                            std::thread::sleep(LAUNCH_LOCK_POLL);
                            continue;
                        }
                        _ => {
                            return Err(err).with_context(|| {
                                format!("controller launch already in progress: {}", path.display())
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

pub fn read_bootstrap(project_root: &Path) -> Result<Option<ControllerBootstrap>> {
    let path = state_path(project_root);
    // A truncated/0-byte or otherwise unparseable controller-state.json (e.g. an
    // interrupted auto_install_reexec recycle killed mid-write before atomic
    // writes landed) must not hard-error and wedge every future launch. Quarantine
    // the corrupt file aside and re-bootstrap cleanly (#corrupt-state-quarantine):
    // this automates the manual "move controller-state.json aside" recovery so a
    // lingering bad file cannot keep confusing later reads/forensics. The quarantine
    // helper warns (never silently swallows) so the recovery is visible.
    agent_doc_fs::read_valid_or_quarantine(&path, |content| {
        serde_json::from_str::<ControllerBootstrap>(content).ok()
    })
}

pub fn current_binary_identity() -> Result<ControllerBinaryIdentity> {
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
        version: identity_version(),
        len: metadata.len(),
        modified_secs: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
    })
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
    let json = serde_json::to_string_pretty(&bootstrap)?;
    // Atomic write (temp + rename) so an interrupted recycle/execve cannot leave
    // a truncated 0-byte controller-state.json that wedges every future launch.
    agent_doc_fs::write_atomic(&path, json.as_bytes())
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

fn upsert_supervisor_lease(
    project_root: &Path,
    record: &agent_doc_sqlite::state_store::ActorRecord,
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
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(false);
    };
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(false);
    };
    let document_id = agent_doc_session_actor_io::canonical_document_id_in(
        &project_root,
        &file.to_string_lossy(),
    );
    let queue_head_prompt = state
        .active_queue_heads
        .first()
        .or_else(|| state.active_free_text_queue_heads.first())
        .map(String::as_str);
    let queue_head_id =
        queue_head_prompt.and_then(agent_doc_queue::queue_directive::first_directive_target_id);
    let response_commit = state
        .file_hash
        .as_deref()
        .or(state.normalized_file_hash.as_deref())
        .or(state.capture_id.as_deref());
    let mutations = state_store::session_actor_closeout_mutations(
        &state.pending_done_ids,
        &state.pending_gated_ids,
        &state.pending_kept_open_ids,
        &state.reaped_pending_ids,
    );

    let mut conn = open_state_db(&project_root)?;
    state_store::commit_session_actor_closeout_in_db(
        &mut conn,
        &state_store::SessionActorCloseoutCommit {
            document_id: &document_id,
            cycle_id: &state.cycle_id,
            cycle_state: state.phase.as_str(),
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

pub fn append_state_event(
    project_root: &Path,
    event: &agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    let conn = open_state_db(project_root)?;
    let payload_json = serde_json::to_string(event).context("serialize state backbone event")?;
    insert_state_event_in_db(
        &conn,
        &state_store::StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    )
}

pub fn load_state_event_ledger(
    project_root: &Path,
) -> Result<agent_doc_state_backbone::EventLedger> {
    let conn = open_state_db(project_root)?;
    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    for row in load_state_events_from_db(&conn, None)? {
        let event: agent_doc_state_backbone::StateEvent = serde_json::from_str(&row.payload_json)
            .with_context(|| {
            format!(
                "parse state backbone event {} from controller state",
                row.event_id
            )
        })?;
        ledger.append(event);
    }
    Ok(ledger)
}

pub fn load_state_backbone_projection(
    project_root: &Path,
) -> Result<agent_doc_state_backbone::StateBackboneProjection> {
    Ok(load_state_event_ledger(project_root)?.project())
}

pub fn load_actor_store(
    project_root: &Path,
) -> Result<BTreeMap<String, agent_doc_sqlite::state_store::ActorRecord>> {
    let conn = open_state_db(project_root)?;
    load_actor_store_from_db(&conn)
}

pub fn load_actor_record(
    project_root: &Path,
    document_id: &str,
) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    let conn = open_state_db(project_root)?;
    load_actor_record_from_db(&conn, document_id)
}

pub fn store_actor_record(
    project_root: &Path,
    expected_prior_generation: Option<u64>,
    record: &agent_doc_sqlite::state_store::ActorRecord,
) -> Result<agent_doc_sqlite::state_store::ActorRecord> {
    let mut conn = open_state_db(project_root)?;
    let (launch_mode, controller_epoch) = actor_document_bootstrap_columns(project_root)?;
    let _evicted_document_ids = state_store::store_actor_record_tx(
        &mut conn,
        expected_prior_generation,
        record,
        launch_mode,
        controller_epoch,
    )?;

    Ok(record.clone())
}

/// Default staleness window for the cross-document supervisor-lease guard
/// (`#xdocsuper0`). Matches the 60s heartbeat freshness window used by
/// `reconcile_supervisor_leases_after_restart` and the GC's stale-actor sweep.
pub const SUPERVISOR_LEASE_GUARD_STALE_AFTER: Duration = Duration::from_secs(60);

/// `#xdocsuper0`: does a FRESH lease held by a LIVE *foreign* supervisor still
/// own this document?
///
/// The claim binding is pane/session-keyed, so
/// `agent_doc_controller::claim::cross_session_decision` auto-forces
/// (`AcceptStale`) whenever the prior/configured tmux session is dead — without
/// ever checking whether another live supervisor still holds a fresh lease on
/// the document. That window lets two supervisors (an old one and a relaunched
/// one) both believe they own one document, which produces
/// stale-CRDT replay, `live_prompt_drift_after_preflight`, and post-commit
/// worktree corruption.
///
/// This predicate consults the document's supervisor lease so claim can refuse
/// to auto-commandeer a document a live supervisor still owns. It returns `true`
/// only when ALL of the following hold for the document's current actor
/// generation:
///   - a supervisor lease row exists,
///   - the lease heartbeat is fresh (within `stale_after`) AND its
///     `supervisor_pid` is a live process
///     (`status::supervisor_lease_is_fresh_and_alive`),
///   - the lease's `supervisor_pid` is *foreign* — i.e. not `self_pid` (the
///     short-lived claim CLI's own process), so we never count our own process
///     as a competing supervisor.
///
/// Any error loading state (missing db, no actor record, no lease) is treated as
/// "no fresh foreign lease" so the guard can never turn a normal stale-session
/// reclaim into a hard failure on absent state — it only fires on positive proof
/// of a live competing supervisor.
pub fn fresh_foreign_supervisor_lease_holds_document(
    project_root: &Path,
    document_id: &str,
    self_pid: u32,
    stale_after: Duration,
) -> bool {
    let now = timestamp_secs();
    let Ok(Some(record)) = load_actor_record(project_root, document_id) else {
        return false;
    };
    let conn = match open_state_db(project_root) {
        Ok(conn) => conn,
        Err(_) => return false,
    };
    let Ok(Some(lease)) = load_supervisor_lease_from_db(&conn, document_id, record.generation)
    else {
        return false;
    };
    if !status::supervisor_lease_pid_is_foreign(lease.supervisor_pid, self_pid) {
        return false;
    }
    status::supervisor_lease_is_fresh_and_alive(
        lease.last_heartbeat,
        lease.supervisor_pid.is_some_and(process_is_alive),
        now,
        stale_after,
    )
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
        if record.state != agent_doc_sqlite::state_store::ActorState::Starting {
            continue;
        }
        let age = now.saturating_sub(record.last_transition.timestamp);
        if age <= stale_after.as_secs() {
            kept += 1;
            continue;
        }

        let lease = load_supervisor_lease_from_db(&conn, &record.document_id, record.generation)?;
        if lease.as_ref().is_some_and(|lease| {
            status::supervisor_lease_is_fresh_and_alive(
                lease.last_heartbeat,
                lease.supervisor_pid.is_some_and(process_is_alive),
                now,
                stale_after,
            )
        }) {
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
        next.state = agent_doc_sqlite::state_store::ActorState::Closed;
        next.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
            caller: caller.to_string(),
            reason: "stale_starting_actor".to_string(),
            timestamp: now,
            prior_generation: record.generation,
            new_generation: record.generation,
        };
        store_actor_record(project_root, Some(record.generation), &next)?;
        agent_doc_ops_log_io::log_op(
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

/// Close non-`Closed` actor records whose tmux pane is no longer alive.
///
/// `admin detect` reports this shape as `stale_dead_pane`: the actor store still
/// believes a document owns a pane, but tmux can no longer address that pane.
/// Route/sync/start should not keep routing work through that dead binding, so
/// this helper transitions the record to `Closed` and clears the pane/window
/// projection through the same actor-store CAS used by manual admin reaps.
///
/// The injected predicate reports whether the pane still *owns a live agent*
/// (`#adsessreap1`), not merely whether tmux can address it: a pane that dropped
/// `claude → zsh` is alive but ownerless, and its record is reaped like a dead
/// pane. The closure is injected so tests can stay deterministic and callers can
/// use the same liveness backend they already trust for actor diagnostics.
pub fn close_stale_dead_pane_actors_for_caller<F>(
    project_root: &Path,
    mut pane_alive: F,
    dry_run: bool,
    caller: &str,
    reason: &str,
) -> Result<(usize, usize)>
where
    F: FnMut(&str) -> bool,
{
    let now = timestamp_secs();
    let store = load_actor_store(project_root)?;
    let mut closed = 0;
    let mut kept = 0;

    for record in store.values() {
        if record.state == agent_doc_sqlite::state_store::ActorState::Closed
            || record.pane_id.is_empty()
        {
            continue;
        }
        if pane_alive(&record.pane_id) {
            kept += 1;
            continue;
        }

        if dry_run {
            eprintln!(
                "[{}] would close stale dead-pane actor: {} session={} pane={} generation={} state={} reason={}",
                caller,
                record.document_id,
                record.session_id,
                record.pane_id,
                record.generation,
                record.state.as_str(),
                reason
            );
            closed += 1;
            continue;
        }

        let mut next = record.clone();
        next.state = agent_doc_sqlite::state_store::ActorState::Closed;
        next.pane_id.clear();
        next.window_id.clear();
        next.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
            caller: caller.to_string(),
            reason: reason.to_string(),
            timestamp: now,
            prior_generation: record.generation,
            new_generation: record.generation,
        };
        match store_actor_record(project_root, Some(record.generation), &next) {
            Ok(_) => {
                agent_doc_ops_log_io::log_op(
                    Path::new(&record.document_id),
                    &format!(
                        "{}_closed_stale_dead_pane_actor file={} session={} pane={} generation={} prior_state={} reason={}",
                        caller,
                        record.document_id,
                        record.session_id,
                        record.pane_id,
                        record.generation,
                        record.state.as_str(),
                        reason
                    ),
                );
                closed += 1;
            }
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    Path::new(&record.document_id),
                    &format!(
                        "{}_close_stale_dead_pane_actor_skipped file={} pane={} generation={} error={}",
                        caller, record.document_id, record.pane_id, record.generation, err
                    ),
                );
                kept += 1;
            }
        }
    }

    Ok((closed, kept))
}

pub fn close_stale_dead_pane_actors_with_tmux_for_caller(
    project_root: &Path,
    dry_run: bool,
    caller: &str,
    reason: &str,
) -> Result<(usize, usize)> {
    let tmux = tmux_router::Tmux::default_server();
    if let Err(err) = agent_doc_tmux_io::list_panes(&tmux, None, "#{pane_id}") {
        agent_doc_ops_log_io::log_op(
            project_root,
            &format!(
                "{caller}_stale_dead_pane_actor_gc_skipped reason=tmux_unavailable error={err}"
            ),
        );
        return Ok((0, 0));
    }
    close_stale_dead_pane_actors_for_caller(
        project_root,
        // `#adsessreap1`: reap a record whose pane degraded `claude → zsh`. A
        // bare-shell pane is alive but no longer owns the agent, so it must
        // transition the actor to `Closed`, not be kept as false-alive.
        |pane| agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(&tmux, pane),
        dry_run,
        caller,
        reason,
    )
}

/// Default age threshold for pruning dead `Closed` actor records (`#actorprune`).
/// A record `Closed` longer than this with no fresh/alive supervisor lease is
/// genuinely dead — its session is gone — so its `documents`/transitions/lease
/// rows are removed to bound `admin list` growth. Matches the 1-hour window the
/// stale-`Starting` sweep uses.
pub const DEAD_ACTOR_PRUNE_AFTER: Duration = Duration::from_secs(3600);

/// `#actorprune`: hard-remove long-dead `Closed` actor records.
///
/// `close_stale_starting_actors` only transitions `Starting`→`Closed`; nothing
/// removes records already `Closed`, so the actor store accumulates dead
/// `session-clear` rows forever (the operator observed 251 in one project). This
/// prunes a record when ALL hold:
///   - state is `Closed`,
///   - `last_transition.timestamp` is older than `dead_after`,
///   - no fresh/alive supervisor lease owns its document/generation
///     (`status::supervisor_lease_is_fresh_and_alive`, the same guard the
///     stale-`Starting` sweep uses) — so a live actor is never pruned.
///
/// `dry_run` logs the prune candidates without deleting. Every prune is logged
/// (`<caller>_pruned_dead_actor ... reason=dead_closed_record`) — never silent.
/// Returns `(pruned, kept)`.
pub fn prune_dead_actors_for_caller(
    project_root: &Path,
    dead_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let now = timestamp_secs();
    let store = load_actor_store(project_root)?;
    let mut conn = open_state_db(project_root)?;
    let mut pruned = 0;
    let mut kept = 0;

    for record in store.values() {
        if record.state != agent_doc_sqlite::state_store::ActorState::Closed {
            kept += 1;
            continue;
        }
        let age = now.saturating_sub(record.last_transition.timestamp);
        if age <= dead_after.as_secs() {
            kept += 1;
            continue;
        }
        // Never prune a record a live supervisor still owns (same guard as the
        // stale-Starting sweep). The pid-alive check inside means a dead
        // supervisor's lingering lease never blocks the prune.
        let lease = load_supervisor_lease_from_db(&conn, &record.document_id, record.generation)?;
        if lease.as_ref().is_some_and(|lease| {
            status::supervisor_lease_is_fresh_and_alive(
                lease.last_heartbeat,
                lease.supervisor_pid.is_some_and(process_is_alive),
                now,
                dead_after,
            )
        }) {
            kept += 1;
            continue;
        }

        if dry_run {
            eprintln!(
                "[{}] would prune dead actor record: {} session={} generation={} state={} age_secs={}",
                caller,
                record.document_id,
                record.session_id,
                record.generation,
                record.state.as_str(),
                age
            );
            agent_doc_ops_log_io::log_op(
                Path::new(&record.document_id),
                &format!(
                    "{}_would_prune_dead_actor document_id={} generation={} state={} age_secs={} reason=dead_closed_record",
                    caller,
                    record.document_id,
                    record.generation,
                    record.state.as_str(),
                    age
                ),
            );
            pruned += 1;
            continue;
        }

        let removed = state_store::delete_actor_document_tx(&mut conn, &record.document_id)?;
        if removed > 0 {
            agent_doc_ops_log_io::log_op(
                Path::new(&record.document_id),
                &format!(
                    "{}_pruned_dead_actor document_id={} generation={} state={} age_secs={} reason=dead_closed_record",
                    caller,
                    record.document_id,
                    record.generation,
                    record.state.as_str(),
                    age
                ),
            );
            pruned += 1;
        } else {
            kept += 1;
        }
    }

    Ok((pruned, kept))
}

pub fn prune_dead_actors(
    project_root: &Path,
    dead_after: Duration,
    dry_run: bool,
) -> Result<(usize, usize)> {
    prune_dead_actors_for_caller(project_root, dead_after, dry_run, "gc")
}

/// Resolve the stuck-`Preparing` controller staleness threshold, honoring the
/// `AGENT_DOC_STALE_PREPARING_CONTROLLER_SECS` env override.
pub fn stale_preparing_controller_threshold() -> Duration {
    let raw = std::env::var(STALE_PREPARING_CONTROLLER_SECS_ENV).ok();
    stale_preparing_controller_threshold_from_env_value(raw.as_deref())
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
        if pid != std::process::id() && !process_is_alive(pid) {
            if dry_run {
                eprintln!(
                    "[{caller}] would mark stale preparing controller failed: pid={pid} generation={generation} age_secs={age} reason=dead_pid"
                );
                return Ok((1, 0));
            }

            let mut next = bootstrap.clone();
            next.handoff_state = ControllerHandoffState::Failed;
            if let Err(err) = write_bootstrap_state(&next) {
                eprintln!(
                    "[{caller}] warning: failed to mark dead stale preparing controller failed pid={pid} generation={generation}: {err}"
                );
            }

            agent_doc_ops_log_io::log_op(
                project_root,
                &format!(
                    "stale_preparing_controller_record_marked_failed reason=dead_pid pid={pid} generation={generation} age_secs={age} caller={caller}"
                ),
            );
            return Ok((1, 0));
        }
        agent_doc_ops_log_io::log_op(
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

    agent_doc_ops_log_io::log_op(
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

/// M3 (#stuckhandoff2) — process-scan reaper for *orphaned* preparing controllers.
///
/// The record-scoped [`terminate_stale_preparing_controllers_for_caller`] only knows
/// the single `bootstrap.pid`; once a newer clean controller overwrites that record,
/// an old replacement still wedged in `--handoff-state preparing` becomes invisible to
/// it (the operator's `pkill -f 'controller serve ... --handoff-state preparing'`
/// case). This walks `/proc` for same-project, non-bootstrap-owned `controller serve`
/// processes that still carry `--handoff-state preparing` and whose start age
/// exceeds `stale_after`, then reaps each via the verified-pid path (cmdline +
/// not-self gated). Returns `(reaped, kept)`.
pub fn reap_orphaned_preparing_controllers_for_caller(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let generation = read_bootstrap(project_root)?
        .map(|bootstrap| bootstrap.controller_generation)
        .unwrap_or(0);
    let mut reaped = 0;
    let mut kept = 0;
    for pid in crate::process::project_controller_pids(project_root) {
        if pid == std::process::id() {
            continue;
        }
        if !crate::process::cmdline_has_preparing_handoff(pid) {
            continue;
        }
        if bootstrap_owns_controller_pid(project_root, pid)? {
            continue;
        }
        let age = crate::process::process_start_age_secs(pid).unwrap_or(0);
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
        agent_doc_ops_log_io::log_op(
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

fn bootstrap_owns_controller_pid(project_root: &Path, pid: u32) -> Result<bool> {
    let Some(bootstrap) = read_bootstrap(project_root)? else {
        return Ok(false);
    };
    if bootstrap.pid != pid {
        return Ok(false);
    }
    if !is_same_project_controller_pid(project_root, pid) {
        return Ok(false);
    }
    Ok(true)
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

/// Reap controller processes whose recorded `--project-root` has been removed.
///
/// This covers detached controllers launched for temporary test/dev projects. Once
/// the root disappears, no future in-root GC tick can reach that controller, and
/// older binaries did not self-exit. Verification still goes through the process
/// cmdline (`agent-doc controller serve --project-root <root>`) and never targets
/// the current process.
pub fn reap_removed_project_root_controllers_for_caller(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    if project_root.exists() {
        return Ok((0, 0));
    }
    let mut reaped = 0;
    let mut kept = 0;
    for pid in crate::process::project_controller_pids(project_root) {
        if pid == std::process::id() {
            continue;
        }
        let age = crate::process::process_start_age_secs(pid).unwrap_or(0);
        if age < stale_after.as_secs() {
            kept += 1;
            continue;
        }
        if dry_run {
            eprintln!(
                "[{caller}] would reap controller for removed project root: pid={pid} root={} age_secs={age}",
                project_root.display()
            );
            reaped += 1;
            continue;
        }
        reap_verified_controller_pid(project_root, pid, 0);
        eprintln!(
            "[{caller}] reaped controller for removed project root: pid={pid} root={} age_secs={age}",
            project_root.display()
        );
        reaped += 1;
    }
    Ok((reaped, kept))
}

pub fn reap_removed_project_root_controllers(
    project_root: &Path,
    stale_after: Duration,
    dry_run: bool,
) -> Result<(usize, usize)> {
    reap_removed_project_root_controllers_for_caller(project_root, stale_after, dry_run, "gc")
}

/// Cross-project sweep for stable controllers whose temp project roots vanished.
pub fn reap_removed_project_root_controllers_all_projects(
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let mut reaped = 0;
    let mut kept = 0;
    for root in crate::process::controller_project_roots(std::process::id()) {
        if root.exists() {
            continue;
        }
        let (root_reaped, root_kept) =
            reap_removed_project_root_controllers_for_caller(&root, stale_after, dry_run, caller)?;
        reaped += root_reaped;
        kept += root_kept;
    }
    Ok((reaped, kept))
}

/// M5 (#stuckhandoff2) — cross-project process-scan sweep for wedged `Preparing`
/// controllers.
///
/// The per-project reaper [`reap_orphaned_preparing_controllers_for_caller`] only
/// reaps controllers whose `--project-root` matches the caller's, and gc runs only
/// for the triggering project. A controller wedged in ANOTHER project root (a
/// `sample-app` handoff that died while the operator is working in `agent-loop`)
/// stays invisible until agent-doc is next invoked there. M1's self-watchdog
/// already covers this without any external tick, but this sweep is the
/// belt-and-suspenders breadth rung: it walks `/proc` for any non-bootstrap-owned
/// `agent-doc ... controller serve --handoff-state preparing` process (across all
/// project roots) whose start age exceeds `stale_after` and reaps each through the
/// verified-pid path keyed to that process's OWN `--project-root`. This is the
/// cross-project equivalent of the operator's `pkill -f 'controller serve ...
/// --handoff-state preparing'`, and it needs no global registry - `/proc` is the
/// index. Returns `(reaped, kept)`.
pub fn reap_orphaned_preparing_controllers_all_projects(
    stale_after: Duration,
    dry_run: bool,
    caller: &str,
) -> Result<(usize, usize)> {
    let mut reaped = 0;
    let mut kept = 0;
    for pid in crate::process::process_pids() {
        if pid == std::process::id() {
            continue;
        }
        let Some(root) = crate::process::controller_serve_project_root(pid) else {
            continue;
        };
        if !crate::process::cmdline_has_preparing_handoff(pid) {
            continue;
        }
        if bootstrap_owns_controller_pid(&root, pid)? {
            continue;
        }
        let age = crate::process::process_start_age_secs(pid).unwrap_or(0);
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
        agent_doc_ops_log_io::log_op(
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
        next.state = agent_doc_sqlite::state_store::ActorState::Closed;
        next.pane_id.clear();
        next.window_id.clear();
        next.last_transition = agent_doc_sqlite::state_store::ActorLastTransition {
            caller: caller.to_string(),
            reason: format!("evicted_cross_document_pane owner={owner_document_id} pane={pane_id}"),
            timestamp: now,
            prior_generation: record.generation,
            new_generation: record.generation,
        };
        match store_actor_record(project_root, Some(record.generation), &next) {
            Ok(_) => {
                agent_doc_ops_log_io::log_op(
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
                agent_doc_ops_log_io::log_op(
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

fn migrate_legacy_layout_projection(project_root: &Path, conn: &Connection) -> Result<()> {
    if state_store::layout_scope_exists(conn, DEFAULT_LAYOUT_SCOPE)? {
        return Ok(());
    }

    let path = layout_projection_path(project_root);
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
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
        .map(|content| agent_doc_hash::content_hash(&content));
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
    agent_doc_ops_log_io::log_op(
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

/// `#agent-doc-command-plane` — agent-doc control-plane ops on lazily command-plane-v1.
pub mod command_plane;

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
            && crate::process::cmdline_has_preparing_handoff(pid))
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    child
}

#[cfg(test)]
pub(crate) fn wait_for_test_child_exit(
    mut child: std::process::Child,
    timeout: Duration,
    failure: &str,
) -> std::process::ExitStatus {
    let start = Instant::now();
    while start.elapsed() < timeout {
        match child.try_wait().expect("poll test child") {
            Some(status) => return status,
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    let pid = child.id();
    let _ = child.kill();
    let status = child.wait().expect("wait for timed-out test child");
    panic!("{failure}; child pid {pid} was still running, cleaned up with status {status:?}");
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
            started.elapsed() < Duration::from_secs(5),
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

    struct TestPipelineFrontmatterEffects;

    impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
        for TestPipelineFrontmatterEffects
    {
        fn read_current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current_content: &str,
            target_content: &str,
            _reason: &str,
        ) -> Result<()> {
            std::fs::write(file, target_content)?;
            Ok(())
        }

        fn log_op(&self, _file: &Path, _message: &str) {}
    }

    const TEST_PIPELINE_FRONTMATTER_EFFECTS: TestPipelineFrontmatterEffects =
        TestPipelineFrontmatterEffects;

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

    #[test]
    fn read_bootstrap_treats_empty_state_as_absent() {
        // Regression: an interrupted auto_install_reexec recycle left a truncated
        // 0-byte controller-state.json; read_bootstrap must self-heal to Ok(None)
        // (re-bootstrap) instead of hard-erroring and wedging every future launch.
        let dir = tempfile::TempDir::new().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"").unwrap();
        assert!(read_bootstrap(dir.path()).unwrap().is_none());
        // #corrupt-state-quarantine: the bad file is moved aside, not left to
        // linger and confuse later reads/forensics.
        assert!(!path.exists(), "0-byte state must be quarantined");
        assert!(
            std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().contains(".corrupt-")),
            "a quarantine sibling must exist"
        );
    }

    #[test]
    fn read_bootstrap_treats_corrupt_state_as_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ partial-write not valid json").unwrap();
        assert!(read_bootstrap(dir.path()).unwrap().is_none());
        assert!(!path.exists(), "corrupt state must be quarantined aside");
    }

    #[test]
    fn write_then_read_bootstrap_roundtrips() {
        // The atomic write path must still produce a fully-readable state file.
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        write_bootstrap_state(&bootstrap).unwrap();
        let read = read_bootstrap(dir.path())
            .unwrap()
            .expect("bootstrap present after write");
        assert_eq!(read.controller_generation, bootstrap.controller_generation);
        assert_eq!(read.project_root, bootstrap.project_root);
    }

    #[test]
    fn crdt_checkpoint_skips_detached_actor_without_supervisor_route() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/detached.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let mut record = actor_record(&document_id, "%41", "@1");
        record.state = agent_doc_sqlite::state_store::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let summary =
            checkpoint_route_owned_documents_for_project(dir.path(), "test_recycle").unwrap();
        assert_eq!(summary.detached, 1);
        assert_eq!(summary.failed, 0);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_crdt_checkpoint_skipped"));
        assert!(ops_log.contains("reason=detached_authority"));
    }

    #[test]
    fn crdt_checkpoint_defers_editor_attached_actor_without_supervisor_route() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/editor.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        assert!(agent_doc_plugin_owner::try_acquire_plugin_owner(
            &document_id,
            "jetbrains-test",
            std::process::id()
        ));
        let mut record = actor_record(&document_id, "%41", "@1");
        record.state = agent_doc_sqlite::state_store::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let summary =
            checkpoint_route_owned_documents_for_project(dir.path(), "test_recycle").unwrap();
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.detached, 0);
        assert_eq!(summary.skipped, 1);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_crdt_checkpoint"));
        assert!(ops_log.contains("status=deferred"));
        assert!(ops_log.contains("recovery=background_yrs_repair"));
        assert!(!ops_log.contains("supervisor_crdt_checkpoint"));
    }

    #[test]
    fn recycle_controller_continues_when_checkpoint_lacks_editor_model() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/recycle-editor.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        assert!(agent_doc_plugin_owner::try_acquire_plugin_owner(
            &document_id,
            "jetbrains-test",
            std::process::id()
        ));
        let mut record = actor_record(&document_id, "%41", "@1");
        record.state = agent_doc_sqlite::state_store::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let recycled = recycle_controller_force(dir.path(), false).unwrap();

        assert!(!recycled, "no live controller was present to recycle");
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_crdt_checkpoint"));
        assert!(ops_log.contains("status=deferred"));
        assert!(ops_log.contains("recovery=background_yrs_repair"));
        assert!(!ops_log.contains("supervisor_crdt_checkpoint"));
    }

    #[test]
    fn crdt_checkpoint_uses_controller_document_model_directly() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/editor-current.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        assert!(agent_doc_plugin_owner::try_acquire_plugin_owner(
            &document_id,
            "jetbrains-test",
            std::process::id()
        ));
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, "intellij:test")
            .unwrap()
            .expect("editor-attached register should allocate model");
        let mut record = actor_record(&document_id, "%41", "@1");
        record.state = agent_doc_sqlite::state_store::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let summary =
            checkpoint_route_owned_documents_for_project(dir.path(), "test_recycle").unwrap();
        assert_eq!(summary.checkpointed, 1);
        assert_eq!(summary.failed, 0);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_crdt_checkpoint"));
        assert!(ops_log.contains("status=checkpointed"));
        assert!(!ops_log.contains("supervisor_crdt_checkpoint"));
    }

    fn actor_record(
        document_id: &str,
        pane: &str,
        window: &str,
    ) -> agent_doc_sqlite::state_store::ActorRecord {
        agent_doc_sqlite::state_store::ActorRecord {
            document_id: document_id.to_string(),
            session_id: "session-1".to_string(),
            generation: 1,
            pane_id: pane.to_string(),
            window_id: window.to_string(),
            harness: "codex".to_string(),
            state: agent_doc_sqlite::state_store::ActorState::Starting,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 10,
                prior_generation: 0,
                new_generation: 1,
            },
        }
    }
    fn closed_actor_record(
        document_id: &str,
        timestamp: u64,
    ) -> agent_doc_sqlite::state_store::ActorRecord {
        agent_doc_sqlite::state_store::ActorRecord {
            document_id: document_id.to_string(),
            session_id: "session-clear".to_string(),
            generation: 1,
            pane_id: String::new(),
            window_id: String::new(),
            harness: "codex".to_string(),
            state: agent_doc_sqlite::state_store::ActorState::Closed,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "session".to_string(),
                reason: "session-clear".to_string(),
                timestamp,
                prior_generation: 0,
                new_generation: 1,
            },
        }
    }

    #[test]
    fn prune_dead_actors_removes_old_closed_records() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/dead.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let old_ts = timestamp_secs().saturating_sub(7200); // 2h ago
        store_actor_record(
            dir.path(),
            Some(0),
            &closed_actor_record(&document_id, old_ts),
        )
        .unwrap();

        let (pruned, _kept) =
            prune_dead_actors(dir.path(), std::time::Duration::from_secs(3600), false).unwrap();
        assert_eq!(
            pruned, 1,
            "old closed record with no lease should be pruned"
        );
        assert!(
            load_actor_record(dir.path(), &document_id)
                .unwrap()
                .is_none(),
            "pruned record should be gone from the store"
        );
    }

    #[test]
    fn prune_dead_actors_keeps_recent_closed() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/recent.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let recent_ts = timestamp_secs().saturating_sub(60); // 1min ago, inside window
        store_actor_record(
            dir.path(),
            Some(0),
            &closed_actor_record(&document_id, recent_ts),
        )
        .unwrap();

        let (pruned, kept) =
            prune_dead_actors(dir.path(), std::time::Duration::from_secs(3600), false).unwrap();
        assert_eq!(
            pruned, 0,
            "recently-closed record within the window is kept"
        );
        assert_eq!(kept, 1);
        assert!(
            load_actor_record(dir.path(), &document_id)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn prune_dead_actors_keeps_live_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/live.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let old_ts = timestamp_secs().saturating_sub(7200);
        let record = closed_actor_record(&document_id, old_ts);
        store_actor_record(dir.path(), Some(0), &record).unwrap();
        // A fresh lease held by THIS (live) process must keep the record even
        // though it looks old + closed.
        upsert_supervisor_lease(
            dir.path(),
            &record,
            Some(std::process::id()),
            None,
            "running",
        )
        .unwrap();

        let (pruned, kept) =
            prune_dead_actors(dir.path(), std::time::Duration::from_secs(3600), false).unwrap();
        assert_eq!(pruned, 0, "a fresh/alive lease must protect the record");
        assert_eq!(kept, 1);
        assert!(
            load_actor_record(dir.path(), &document_id)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn prune_dead_actors_dry_run_preserves() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/dry.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let old_ts = timestamp_secs().saturating_sub(7200);
        store_actor_record(
            dir.path(),
            Some(0),
            &closed_actor_record(&document_id, old_ts),
        )
        .unwrap();

        let (pruned, _kept) =
            prune_dead_actors(dir.path(), std::time::Duration::from_secs(3600), true).unwrap();
        assert_eq!(pruned, 1, "dry-run reports the prune candidate");
        assert!(
            load_actor_record(dir.path(), &document_id)
                .unwrap()
                .is_some(),
            "dry-run must NOT delete the record"
        );
    }

    #[test]
    fn close_stale_dead_pane_actors_closes_and_clears_dead_binding() {
        let dir = tempfile::TempDir::new().unwrap();
        let dead_doc = dir.path().join("tasks/dead-pane.md");
        let live_doc = dir.path().join("tasks/live-pane.md");
        std::fs::create_dir_all(dead_doc.parent().unwrap()).unwrap();
        std::fs::write(&dead_doc, "body").unwrap();
        std::fs::write(&live_doc, "body").unwrap();
        let dead_id = dead_doc.to_string_lossy().to_string();
        let live_id = live_doc.to_string_lossy().to_string();
        let mut dead = actor_record(&dead_id, "%dead", "@1");
        dead.state = agent_doc_sqlite::state_store::ActorState::Ready;
        let mut live = actor_record(&live_id, "%live", "@1");
        live.state = agent_doc_sqlite::state_store::ActorState::Busy;
        store_actor_record(dir.path(), Some(0), &dead).unwrap();
        store_actor_record(dir.path(), Some(0), &live).unwrap();

        let (closed, kept) = close_stale_dead_pane_actors_for_caller(
            dir.path(),
            |pane| pane == "%live",
            false,
            "test",
            "stale_dead_pane_actor",
        )
        .unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 1);

        let closed_record = load_actor_record(dir.path(), &dead_id).unwrap().unwrap();
        assert_eq!(
            closed_record.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
        assert_eq!(closed_record.pane_id, "");
        assert_eq!(closed_record.window_id, "");
        assert_eq!(closed_record.last_transition.caller, "test");
        assert_eq!(
            closed_record.last_transition.reason,
            "stale_dead_pane_actor"
        );

        let live_record = load_actor_record(dir.path(), &live_id).unwrap().unwrap();
        assert_eq!(
            live_record.state,
            agent_doc_sqlite::state_store::ActorState::Busy
        );
        assert_eq!(live_record.pane_id, "%live");
    }

    #[test]
    fn close_stale_dead_pane_actors_dry_run_preserves_binding() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/dry-dead-pane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let mut record = actor_record(&document_id, "%dead", "@1");
        record.state = agent_doc_sqlite::state_store::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let (closed, kept) = close_stale_dead_pane_actors_for_caller(
            dir.path(),
            |_| false,
            true,
            "test",
            "stale_dead_pane_actor",
        )
        .unwrap();
        assert_eq!(closed, 1);
        assert_eq!(kept, 0);

        let current = load_actor_record(dir.path(), &document_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            current.state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );
        assert_eq!(current.pane_id, "%dead");
    }

    #[test]
    fn actor_store_writes_sqlite_authority() {
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

        assert!(
            state_db_path(dir.path()).exists(),
            "actor store must persist to controller state.db"
        );
    }
    #[test]
    fn durable_registry_reconciles_existing_registry_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            document_id.clone(),
            tmux_router::RegistryEntry {
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
        agent_doc_session_registry_io::save_in(dir.path(), &registry).unwrap();

        let record = actor_record(&document_id, "%51", "@2");
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let durable_registry = agent_doc_session_registry_io::load_in(dir.path()).unwrap();
        let entry = durable_registry.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%51");
        assert_eq!(entry.window, "@2");
        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.pid, 123);
        assert_eq!(entry.supervisor_instance_id, "supervisor-1");
    }
    #[test]
    fn durable_registry_removes_displaced_cross_document_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc_a = dir.path().join("tasks/a.md");
        let doc_b = dir.path().join("tasks/b.md");
        std::fs::create_dir_all(doc_a.parent().unwrap()).unwrap();
        std::fs::write(&doc_a, "a").unwrap();
        std::fs::write(&doc_b, "b").unwrap();
        let document_a = doc_a.to_string_lossy().to_string();
        let document_b = doc_b.to_string_lossy().to_string();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            document_a.clone(),
            tmux_router::RegistryEntry {
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
            tmux_router::RegistryEntry {
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
        agent_doc_session_registry_io::save_in(dir.path(), &registry).unwrap();

        let mut record_a = actor_record(&document_a, "%70", "@7");
        record_a.session_id = "session-a".to_string();
        store_actor_record(dir.path(), Some(0), &record_a).unwrap();
        let mut record_b = actor_record(&document_b, "%70", "@7");
        record_b.session_id = "session-b".to_string();
        store_actor_record(dir.path(), Some(0), &record_b).unwrap();

        let durable_registry = agent_doc_session_registry_io::load_in(dir.path()).unwrap();
        assert!(
            !durable_registry.contains_key(&document_a),
            "displaced document must not remain in the durable registry"
        );
        let entry_b = durable_registry.get(&document_b).unwrap();
        assert_eq!(entry_b.pane, "%70");
        assert_eq!(entry_b.window, "@7");
        assert_eq!(entry_b.session_id, "session-b");
    }
    #[test]
    fn durable_registry_exposes_controller_state_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("tasks/test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        let record = actor_record(&document_id, "%61", "@3");

        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let durable_registry = agent_doc_session_registry_io::load_in(dir.path()).unwrap();
        let entry = durable_registry.get(&document_id).unwrap();
        assert_eq!(entry.pane, "%61");
        assert_eq!(entry.window, "@3");
        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.file, document_id);
        assert_eq!(entry.cwd, doc.parent().unwrap().to_string_lossy());

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let diagnostics: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'session_registry'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(diagnostics, 0);
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
        assert!(
            acquired.is_ok(),
            "blocking acquire should wait out the holder"
        );
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
    fn prepare_handoff_is_reentrant_and_abort_reconciles_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut bootstrap = write_bootstrap_with_options(
            dir.path(),
            socket_path(dir.path()),
            LaunchMode::Lazy,
            4,
            ControllerHandoffState::Preparing,
            None,
        )
        .unwrap();
        let original_started = timestamp_secs().saturating_sub(600);
        bootstrap.handoff_started_at = Some(original_started);
        write_bootstrap_state(&bootstrap).unwrap();
        let mut should_stop = false;

        let prepare = handle_request(
            &(serde_json::json!({ "command": "prepare_handoff" }).to_string() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        assert!(prepare.contains("\"ok\":true"), "{prepare}");
        let prepared = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(prepared.handoff_state, ControllerHandoffState::Preparing);
        assert_eq!(
            prepared.handoff_started_at,
            Some(original_started),
            "reentrant prepare must not refresh the watchdog deadline"
        );

        let abort = handle_request(
            &(serde_json::json!({ "command": "abort_handoff" }).to_string() + "\n"),
            &prepared,
            &mut should_stop,
        )
        .unwrap();
        assert!(abort.contains("\"ok\":true"), "{abort}");
        let aborted = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(aborted.handoff_state, ControllerHandoffState::Stable);
        assert_eq!(aborted.handoff_started_at, None);
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
            freshness: None,
            control_plane: agent_doc_controller::status::default_control_plane_status(),
        };
        assert!(
            !agent_doc_controller::status::controller_binary_identity_matches(
                missing.controller_binary.as_ref(),
                Some(&current)
            )
        );

        let mut changed = current.clone();
        changed.modified_nanos = changed.modified_nanos.wrapping_add(1);
        let stale = ControllerStatus {
            controller_binary: Some(changed),
            ..missing
        };
        assert!(
            !agent_doc_controller::status::controller_binary_identity_matches(
                stale.controller_binary.as_ref(),
                Some(&current)
            )
        );

        let fresh = ControllerStatus {
            controller_binary: Some(current.clone()),
            ..stale
        };
        assert!(
            agent_doc_controller::status::controller_binary_identity_matches(
                fresh.controller_binary.as_ref(),
                Some(&current)
            )
        );
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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );
        agent_doc_supervisor_io::startup_miss::record_startup_miss(
            &doc,
            "%41",
            "session-controller",
            "codex",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();

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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );
        assert_eq!(record.last_transition.reason, "prompt_ready");
        assert!(
            agent_doc_supervisor_io::startup_miss::load_startup_miss(&doc)
                .unwrap()
                .is_none(),
            "prompt-ready lifecycle transition must clear stale startup-miss markers"
        );

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

        agent_doc_supervisor_io::startup_miss::record_startup_miss(
            &doc,
            "%41",
            "session-controller",
            "codex",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();
        let closed_lifecycle = ControllerRequest {
            command: "mark_lifecycle".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-controller".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: Some("closed".to_string()),
            caller: Some("sync".to_string()),
            reason: Some("stale_dead_pane_actor".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&closed_lifecycle).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
        assert!(
            agent_doc_supervisor_io::startup_miss::load_startup_miss(&doc)
                .unwrap()
                .is_none(),
            "closed lifecycle transition must clear stale startup-miss markers"
        );
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

        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-heartbeat",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let record = agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-heartbeat",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
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
    fn controller_supervisor_heartbeat_replaces_closed_same_supervisor_session() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/heartbeat-replace.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-old\nagent: opencode\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);

        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-old",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let ready = agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-old",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        upsert_supervisor_lease(
            dir.path(),
            &ready,
            Some(999),
            Some("/tmp/same-supervisor.sock"),
            "ready",
        )
        .unwrap();
        let conn = open_state_db(dir.path()).unwrap();
        state_store::upsert_queue_control_in_db(
            &conn,
            &state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &doc.to_string_lossy(),
                state: "paused",
                reason: Some(
                    "stale route-owned supervisor (pid 999) replaying already-answered queue item",
                ),
                operation_receipt_id: None,
            },
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-old",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Closed,
            "supervisor",
            "user_quit_clean_exit",
        )
        .unwrap();

        let heartbeat = ControllerRequest {
            command: "supervisor_heartbeat".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-new".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(2),
            state: Some("ready".to_string()),
            caller: None,
            reason: None,
            supervisor_pid: Some(999),
            supervisor_socket: Some("/tmp/same-supervisor.sock".to_string()),
            command_kind: None,
            diagnostic_payload: None,
        };
        let lease = handle_supervisor_heartbeat(&bootstrap, None, heartbeat).unwrap();
        assert_eq!(lease.generation, 2);
        assert_eq!(lease.supervisor_pid, Some(999));

        let record = load_actor_record(dir.path(), &doc.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(record.session_id, "session-new");
        assert_eq!(record.generation, 2);
        assert_eq!(record.pane_id, "%41");
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );

        let conn = open_state_db(dir.path()).unwrap();
        let effective = state_store::load_effective_queue_control_from_db(
            &conn,
            &doc.to_string_lossy(),
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        assert!(
            effective.is_none(),
            "replacement heartbeat should clear stale-supervisor queue pause"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_supervisor_replaced_closed_session"));
        assert!(ops_log.contains("stale_supervisor_pause_superseded"));
    }

    #[test]
    fn dispatch_clears_stale_supervisor_pause_that_predates_current_actor() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/dispatch-replace.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-old\nagent: opencode\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);

        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-old",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-old",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Closed,
            "supervisor",
            "user_quit_clean_exit",
        )
        .unwrap();
        let conn = open_state_db(dir.path()).unwrap();
        state_store::upsert_queue_control_in_db(
            &conn,
            &state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &doc.to_string_lossy(),
                state: "paused",
                reason: Some(
                    "stale route-owned supervisor (pid 999) replaying already-answered queue item",
                ),
                operation_receipt_id: None,
            },
        )
        .unwrap();

        let start = ControllerRequest {
            command: "start_session".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-new".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: Some("@1".to_string()),
            generation: Some(2),
            state: None,
            caller: Some("start".to_string()),
            reason: Some("session_start".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: None,
            diagnostic_payload: None,
        };
        let record = handle_start_session(&bootstrap, None, start).unwrap();
        assert_eq!(record.generation, 2);

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-new".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(2),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("replacement dispatch".to_string()),
        };
        let auth = handle_dispatch(&bootstrap, None, dispatch).unwrap();
        assert_eq!(auth.record.session_id, "session-new");
        assert_eq!(auth.record.generation, 2);

        let conn = open_state_db(dir.path()).unwrap();
        let effective = state_store::load_effective_queue_control_from_db(
            &conn,
            &doc.to_string_lossy(),
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        assert!(
            effective.is_none(),
            "dispatch should clear stale-supervisor pause from superseded actor"
        );
        let blocked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'queue_paused'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(blocked, 0, "dispatch must not stay queue_paused");
    }

    #[test]
    fn dispatch_clears_stale_supervisor_pause_when_named_pid_is_dead_after_reboot() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/dispatch-reboot.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-reboot\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let dead_pid = u32::MAX;
        assert!(
            !process_is_alive(dead_pid),
            "sentinel PID must not exist for the reboot-stale regression"
        );

        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-reboot",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let record = agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-reboot",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        assert_eq!(record.generation, 1);

        let conn = open_state_db(dir.path()).unwrap();
        let boot_timestamp = crate::process::system_boot_timestamp_secs(timestamp_secs())
            .expect("/proc/uptime should be available in tests");
        let old_transition_timestamp = boot_timestamp.saturating_sub(2);
        let preboot_pause_timestamp = boot_timestamp.saturating_sub(1);
        conn.execute(
            "UPDATE actor_transitions SET timestamp = ?1 WHERE new_generation = 1",
            params![sqlite_i64(old_transition_timestamp, "old transition timestamp").unwrap()],
        )
        .unwrap();
        state_store::upsert_queue_control_in_db(
            &conn,
            &state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &doc.to_string_lossy(),
                state: "paused",
                reason: Some(&format!(
                    "stale route-owned supervisor (pid {dead_pid}) replaying already-answered queue item after reboot"
                )),
                operation_receipt_id: None,
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE queue_controls SET updated_at = ?1 WHERE scope_kind = 'document' AND scope_id = ?2",
            params![
                sqlite_i64(preboot_pause_timestamp, "queue control timestamp")
                    .unwrap(),
                doc.to_string_lossy().to_string()
            ],
        )
        .unwrap();

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-reboot".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("idle_queue_continuation".to_string()),
            diagnostic_payload: Some("post-reboot auto dispatch".to_string()),
        };
        let auth = handle_dispatch(&bootstrap, None, dispatch).unwrap();
        assert_eq!(auth.record.session_id, "session-reboot");
        assert_eq!(auth.record.generation, 1);

        let effective = state_store::load_effective_queue_control_from_db(
            &conn,
            &doc.to_string_lossy(),
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        assert!(
            effective.is_none(),
            "dispatch should clear stale-supervisor pause when the named old PID is dead"
        );
        let blocked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dispatch_attempts WHERE failed_stage = 'queue_paused'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(blocked, 0, "dispatch must not stay queue_paused");
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_supervisor_pause_superseded"));
        assert!(ops_log.contains("stale_pid_dead=true"));
        assert!(ops_log.contains("pause_predates_boot=true"));
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
        let record = agent_doc_session_actor_io::record_session_start_direct(
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
        assert_eq!(
            updated.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
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
        let record = agent_doc_session_actor_io::record_session_start_direct(
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
        assert_eq!(
            updated.state,
            agent_doc_sqlite::state_store::ActorState::Starting
        );
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
        let record = agent_doc_session_actor_io::record_session_start_direct(
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
        assert_eq!(
            updated.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
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
        let record = agent_doc_session_actor_io::record_session_start_direct(
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
        assert_eq!(
            updated.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
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
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-stale",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
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
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-same",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-same",
            "%41",
            "@1",
            2,
        )
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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "same-pane stale generation should succeed: {:?}",
            envelope.error
        );
        assert_eq!(
            envelope.data.unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Ready
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
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-route",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-route",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
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
            concat!(
                "---\n",
                "agent_doc_session: session-queue\n",
                "agent: codex\n",
                "queue: start\n",
                "---\n\n",
                "<!-- agent:queue -->\n",
                "- do [#jbrunlogproof]\n",
                "<!-- /agent:queue -->\n\n",
                "Body\n"
            ),
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-queue",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-queue",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
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

        // `#qpauserun`: a pause blocks UNATTENDED auto-dispatch (idle-watch /
        // `/loop` continuation). Use an auto command_kind here so this asserts the
        // pause-block; an explicit operator reopen (`managed_reopen`) is admitted
        // past the pause and is covered by
        // `dispatch_operator_reopen_bypasses_paused_queue`.
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
            command_kind: Some("idle_queue_continuation".to_string()),
            diagnostic_payload: Some("paused dispatch test harness=codex".to_string()),
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
        let error = envelope.error.as_deref().unwrap_or_default().to_string();
        let expected_head = "do [#jbrunlogproof]";
        let expected_trigger =
            agent_doc_harness::HarnessConfig::codex().trigger_command(&doc.to_string_lossy());
        assert!(
            error.contains("failed_stage=queue_paused"),
            "dispatch error must include queue paused stage: {error}"
        );
        assert!(
            error.contains("ui_outcome=blocked_with_exact_unblocker"),
            "generic queue pause must carry a typed UI outcome: {error}"
        );
        assert!(
            error.contains("unblocker=resume_or_clear_queue_control"),
            "generic queue pause must carry an exact unblocker: {error}"
        );
        assert!(error.contains(&format!("blocked_head_bytes={}", expected_head.len())));
        assert!(error.contains(&format!(
            "blocked_head_sha256={}",
            agent_doc_hash::content_hash(expected_head)
        )));
        assert!(error.contains(&format!("trigger_bytes={}", expected_trigger.len())));
        assert!(error.contains(&format!(
            "trigger_sha256={}",
            agent_doc_hash::content_hash(&expected_trigger)
        )));

        let conn = open_state_db(dir.path()).unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
        let (failed_stage, diagnostic_payload): (String, Option<String>) = conn
            .query_row(
                "SELECT failed_stage, diagnostic_payload FROM dispatch_attempts WHERE document_id = ?1 ORDER BY id DESC LIMIT 1",
                params![&document_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(failed_stage, "queue_paused");
        let diagnostic_payload = diagnostic_payload.unwrap_or_default();
        assert!(diagnostic_payload.contains("ui_outcome=blocked_with_exact_unblocker"));
        assert!(diagnostic_payload.contains("unblocker=resume_or_clear_queue_control"));
        assert!(
            diagnostic_payload.contains(&format!("blocked_head_bytes={}", expected_head.len()))
        );
        assert!(diagnostic_payload.contains(&format!(
            "blocked_head_sha256={}",
            agent_doc_hash::content_hash(expected_head)
        )));
        assert!(diagnostic_payload.contains(&format!(
            "trigger_sha256={}",
            agent_doc_hash::content_hash(&expected_trigger)
        )));
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
        let freshness = inspection
            .freshness
            .as_ref()
            .expect("inspect_actor should expose controller/supervisor freshness proof");
        assert_eq!(freshness.controller.pid, Some(bootstrap.pid));
        assert!(freshness.installed_binary.is_some());
        assert_eq!(
            inspection
                .queue_control
                .as_ref()
                .map(|control| control.state.as_str()),
            Some("paused")
        );
        let pressure = inspection.queue_backpressure.last().unwrap();
        assert_eq!(pressure.capacity_class, "queue_paused");
        assert_eq!(pressure.command_kind, "idle_queue_continuation");
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
    fn dispatch_operator_reopen_bypasses_paused_queue() {
        // `#qpauserun`: an explicit operator reopen (JB `Run Agent Doc` →
        // `managed_reopen` / `dispatch_only_reopen`) must START even when the
        // queue is controller-paused — the pause governs unattended auto-draining,
        // not whether the operator can run a cycle. One-shot: the pause row stays.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/operator-reopen.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            concat!(
                "---\n",
                "agent_doc_session: session-reopen\n",
                "agent: codex\n",
                "queue: start\n",
                "---\n\n",
                "<!-- agent:queue -->\n",
                "- do [#reopenhead]\n",
                "<!-- /agent:queue -->\n\n",
                "Body\n"
            ),
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-reopen",
            "%42",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-reopen",
            "%42",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let pause = ControllerRequest {
            command: "queue_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(1),
            state: Some("pause".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("operator pause".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("pause".to_string()),
            diagnostic_payload: None,
        };
        let response = handle_request(
            &(serde_json::to_string(&pause).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<ControllerAdminReceipt> =
            serde_json::from_str(&response).unwrap();
        assert_eq!(envelope.data.unwrap().status, "accepted");

        // Auto-dispatch stays blocked by the pause.
        let auto = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-reopen".to_string()),
            pane_id: Some("%42".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("idle_queue_continuation".to_string()),
            diagnostic_payload: Some("auto dispatch harness=codex".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&auto).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            !envelope.ok,
            "auto-dispatch must stay blocked by a paused queue"
        );
        assert!(
            envelope
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("failed_stage=queue_paused")
        );

        // Explicit operator reopen is ADMITTED past the pause.
        let reopen = ControllerRequest {
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("operator reopen harness=codex".to_string()),
            ..auto
        };
        let response = handle_request(
            &(serde_json::to_string(&reopen).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "explicit operator reopen must be admitted past a paused queue: {:?}",
            envelope.error
        );

        // The pause row stays (one-shot bypass — auto callers remain blocked).
        let conn = open_state_db(dir.path()).unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
        let state: String = conn
            .query_row(
                "SELECT state FROM queue_controls WHERE scope_kind = 'document' AND scope_id = ?1",
                params![&document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            state, "paused",
            "operator reopen bypass is one-shot — the pause must remain"
        );
    }

    #[test]
    fn dispatch_repairs_spent_preset_pause_when_head_is_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/spent-preset-absent.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: session-preset\n",
            "agent: codex\n",
            "queue_active: true\n",
            "prompt_presets:\n",
            "  '#advance-review': Go through review items.\n",
            "---\n\n",
            "<!-- agent:queue priority go -->\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-preset",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-preset",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let pause = ControllerRequest {
            command: "queue_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(1),
            state: Some("pause".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line.".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("pause".to_string()),
            diagnostic_payload: None,
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

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-preset".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("spent preset absent repair".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&dispatch).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "dispatch should not stay queue_paused: {response}"
        );

        let conn = open_state_db(dir.path()).unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
        let effective = state_store::load_effective_queue_control_from_db(
            &conn,
            &document_id,
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        assert!(
            effective.is_none(),
            "spent-preset pause with absent head must be cleared"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("spent_preset_pause_repaired"));
        assert!(ops_log.contains("action=resume_absent_head"));
    }

    #[test]
    fn dispatch_repairs_spent_preset_pause_by_consuming_present_preset_head() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/spent-preset-present.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: session-preset\n",
            "agent: codex\n",
            "queue_active: true\n",
            "prompt_presets:\n",
            "  '#advance-review': Go through review items.\n",
            "---\n\n",
            "<!-- agent:queue priority go -->\n",
            "- #advance-review\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-preset",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-preset",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let pause = ControllerRequest {
            command: "queue_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(1),
            state: Some("pause".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("advance-review preset head is spent (backlog added + both features shipped); pausing so the go-queue does not re-trigger advance-review. Operator can clear the '- #advance-review' line.".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("pause".to_string()),
            diagnostic_payload: None,
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

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-preset".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("managed_reopen".to_string()),
            diagnostic_payload: Some("spent preset present repair".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&dispatch).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "dispatch should not stay queue_paused: {response}"
        );

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("- #advance-review"),
            "registered preset head should be consumed:\n{updated}"
        );
        assert!(
            updated.contains("queue: stop") && !updated.contains("queue_active: true"),
            "drained preset queue must deactivate:\n{updated}"
        );
        let conn = open_state_db(dir.path()).unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
        let effective = state_store::load_effective_queue_control_from_db(
            &conn,
            &document_id,
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        assert!(
            effective.is_none(),
            "spent-preset pause must be cleared after consuming the head"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("spent_preset_pause_repaired"));
        assert!(ops_log.contains("action=consume_head"));
    }

    #[test]
    fn dispatch_repairs_unserviceable_preset_token_pause_by_consuming_present_head() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/unserviceable-preset-token.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: session-preset\n",
            "agent: opencode\n",
            "queue_active: true\n",
            "prompt_presets:\n",
            "  '#bugs-observed': Record observed bugs.\n",
            "---\n\n",
            "<!-- agent:queue priority go -->\n",
            "- #bugs-observed\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-preset",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-preset",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        let pause = ControllerRequest {
            command: "queue_control".to_string(),
            file: Some(doc.clone()),
            session_id: None,
            pane_id: None,
            window_id: None,
            generation: Some(1),
            state: Some("pause".to_string()),
            caller: Some("admin".to_string()),
            reason: Some("preset-token queue heads (#bugs-observed) un-drainable: consume rejects as id-backed, --done fails (no such backlog id); work committed to HEAD 9f363f77d. Halting go-mode flood pending operator clear / agent-doc fix.".to_string()),
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("pause".to_string()),
            diagnostic_payload: None,
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

        let dispatch = ControllerRequest {
            command: "dispatch".to_string(),
            file: Some(doc.clone()),
            session_id: Some("session-preset".to_string()),
            pane_id: Some("%41".to_string()),
            window_id: None,
            generation: Some(1),
            state: None,
            caller: None,
            reason: None,
            supervisor_pid: None,
            supervisor_socket: None,
            command_kind: Some("dispatch_only_reopen".to_string()),
            diagnostic_payload: Some("preset-token unserviceable repair".to_string()),
        };
        let response = handle_request(
            &(serde_json::to_string(&dispatch).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "dispatch should not stay queue_paused: {response}"
        );

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("- #bugs-observed"),
            "registered preset-token head should be consumed:\n{updated}"
        );
        let conn = open_state_db(dir.path()).unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
        let effective = state_store::load_effective_queue_control_from_db(
            &conn,
            &document_id,
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        assert!(
            effective.is_none(),
            "preset-token pause must be cleared after consuming the head"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("spent_preset_pause_repaired"));
        assert!(ops_log.contains("preset=#bugs-observed"));
        assert!(ops_log.contains("action=consume_head"));
    }

    /// `#jbrestale`: a `queue_paused` dispatch bail whose pause reason is a stale-supervisor
    /// churn-stop must carry the `supervisor_restart_redirect` marker + the named stale pid
    /// (so the route path restarts + re-dispatches once), while a deliberate operator pause
    /// must stay terminal (no marker → fail closed).
    #[test]
    fn dispatch_queue_paused_stale_supervisor_emits_restart_redirect_marker() {
        fn paused_dispatch_error(pause_reason: &str) -> (String, String) {
            let dir = tempfile::TempDir::new().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            let doc = dir.path().join("tasks/jbrestale.md");
            std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
            std::fs::write(
                &doc,
                "---\nagent_doc_session: session-jbr\nagent: codex\n---\nBody\n",
            )
            .unwrap();
            let bootstrap = test_bootstrap(&dir);
            let mut should_stop = false;
            agent_doc_session_actor_io::record_session_start_direct(
                &doc,
                "session-jbr",
                "%52",
                "@1",
                1,
            )
            .unwrap();
            agent_doc_session_actor_io::transition_state_direct(
                &doc,
                "session-jbr",
                "%52",
                Some(1),
                agent_doc_sqlite::state_store::ActorState::Ready,
                "supervisor",
                "prompt_ready",
            )
            .unwrap();
            let pause = ControllerRequest {
                command: "queue_control".to_string(),
                file: Some(doc.clone()),
                session_id: None,
                pane_id: None,
                window_id: None,
                generation: Some(1),
                state: Some("pause".to_string()),
                caller: Some("admin".to_string()),
                reason: Some(pause_reason.to_string()),
                supervisor_pid: None,
                supervisor_socket: None,
                command_kind: Some("pause".to_string()),
                diagnostic_payload: None,
            };
            let response = handle_request(
                &(serde_json::to_string(&pause).unwrap() + "\n"),
                &bootstrap,
                &mut should_stop,
            )
            .unwrap();
            let envelope: ControllerEnvelope<ControllerAdminReceipt> =
                serde_json::from_str(&response).unwrap();
            assert_eq!(envelope.data.unwrap().status, "accepted");
            let dispatch = ControllerRequest {
                command: "dispatch".to_string(),
                file: Some(doc.clone()),
                session_id: Some("session-jbr".to_string()),
                pane_id: Some("%52".to_string()),
                window_id: None,
                generation: Some(1),
                state: None,
                caller: None,
                reason: None,
                supervisor_pid: None,
                supervisor_socket: None,
                // `#qpauserun`: use an auto command_kind so BOTH a stale-supervisor
                // churn-stop pause and a deliberate operator pause block here — this
                // test asserts the restart-redirect *marker* distinction on the
                // queue_paused bail, not the operator-reopen bypass (covered by
                // `dispatch_operator_reopen_bypasses_paused_queue`).
                command_kind: Some("idle_queue_continuation".to_string()),
                diagnostic_payload: Some("jbrestale paused dispatch".to_string()),
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
            let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
                .unwrap_or_default();
            (envelope.error.unwrap_or_default(), ops_log)
        }

        // Stale-supervisor churn-stop → recoverable: marker + pid present so the route
        // path restarts the supervisor and re-dispatches once.
        let (recoverable, recoverable_ops_log) = paused_dispatch_error(
            "churn-stop: do[#c2b6] operator-verify head re-injected by stale supervisor pid1368698 (pre-0.34.0); needs operator recycle, not agent drain",
        );
        assert!(recoverable.contains("failed_stage=queue_paused"));
        assert!(
            recoverable.contains(
                agent_doc_controller::dispatch::DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER
            ),
            "stale-supervisor pause must carry the restart-redirect marker: {recoverable}"
        );
        assert_eq!(
            agent_doc_controller::dispatch::stale_queue_pause_pid_from_dispatch_error(&recoverable),
            Some(1368698)
        );
        assert!(
            recoverable_ops_log.contains("binary_outcome=recoverable"),
            "recoverable stale queue pause must emit a typed outcome proof: {recoverable_ops_log}"
        );
        assert!(
            recoverable.contains("ui_outcome=recovered_and_retried"),
            "stale-supervisor pause bail must carry the user-facing recovery outcome: {recoverable}"
        );
        assert!(
            recoverable_ops_log.contains("ui_outcome=recovered_and_retried"),
            "recoverable stale queue pause must log the user-facing recovery outcome: {recoverable_ops_log}"
        );
        assert!(recoverable_ops_log.contains("invariant=stale_queue_pause"));
        assert!(recoverable_ops_log.contains("proof_marker=supervisor_restart_redirect"));
        assert!(recoverable_ops_log.contains("next_action=restart_supervisor_once_and_retry"));

        // Deliberate operator pause → terminal: no marker, stays fail-closed.
        let (terminal, terminal_ops_log) =
            paused_dispatch_error("operator paused this queue for manual review");
        assert!(terminal.contains("failed_stage=queue_paused"));
        assert!(
            !terminal.contains(
                agent_doc_controller::dispatch::DISPATCH_SUPERVISOR_RESTART_REDIRECT_MARKER
            ),
            "deliberate operator pause must NOT carry the restart-redirect marker: {terminal}"
        );
        assert_eq!(
            agent_doc_controller::dispatch::stale_queue_pause_pid_from_dispatch_error(&terminal),
            None
        );
        assert!(
            !terminal_ops_log.contains("binary_outcome=recoverable"),
            "operator pauses must not emit stale-pause recovery outcomes: {terminal_ops_log}"
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
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-admin-control",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-admin-control",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
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

        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
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
        assert_eq!(
            record.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
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
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_active_queue_heads(
            &doc,
            &["do [#ctrlplane-sessionactor]".to_string()],
        )
        .unwrap();
        agent_doc_cycle_state_io::record_pending_done_ids(
            &doc,
            &["ctrlplane-sessionactor".to_string()],
        )
        .unwrap();
        agent_doc_cycle_state_io::record_pending_gated_ids(&doc, &["held-item".to_string()])
            .unwrap();
        agent_doc_cycle_state_io::record_pending_kept_open_ids(&doc, &["later-item".to_string()])
            .unwrap();
        agent_doc_cycle_state_io::record_reaped_pending_ids(&doc, &["stale-item".to_string()])
            .unwrap();
        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc).unwrap().unwrap();
        let stale_open_sidecar = std::fs::read(&sidecar_path).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        std::fs::write(&sidecar_path, stale_open_sidecar).unwrap();
        assert!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .is_open(),
            "fixture should leave compatibility sidecar stale and open"
        );

        assert!(persist_session_actor_closeout(&doc).unwrap());

        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
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

        let _ = std::fs::remove_file(layout_projection_path(dir.path()));

        let mut bootstrap = test_bootstrap(&dir);
        bootstrap.controller_generation = 2;
        let runtime = ControllerRuntime::new(bootstrap).unwrap();

        let memory_record = runtime.actor_record(&document_id).unwrap().unwrap();
        assert_eq!(memory_record.pane_id, "%88");
        assert_eq!(memory_record.session_id, "session-restart");

        let durable_registry = agent_doc_session_registry_io::load_in(dir.path()).unwrap();
        let entry = durable_registry.get(&document_id).unwrap();
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
    fn controller_restart_recovery_upserts_open_dispatch_markers() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/restart-flood.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-restart-flood\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let document_id = agent_doc_session_actor_io::canonical_document_id_in(
            dir.path(),
            &doc.to_string_lossy(),
        );
        let mut conn = open_state_db(dir.path()).unwrap();
        let record = agent_doc_sqlite::state_store::ActorRecord {
            document_id: document_id.clone(),
            session_id: "session-restart-flood".to_string(),
            generation: 1,
            pane_id: "%88".to_string(),
            window_id: "@8".to_string(),
            harness: "codex".to_string(),
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "test".to_string(),
                reason: "seed".to_string(),
                timestamp: 1,
                prior_generation: 0,
                new_generation: 1,
            },
        };
        store_actor_record(dir.path(), None, &record).unwrap();
        let receipt_count = 5_i64;
        for index in 0..receipt_count {
            state_store::insert_dispatch_attempt_in_db(
                &conn,
                &state_store::DispatchAttemptInsert {
                    document_id: &document_id,
                    generation: 1,
                    command_kind: "session_restart",
                    accepted_stage: Some("operator_starting"),
                    failed_stage: None,
                    diagnostic_payload: "",
                    result_status: "accepted",
                    proof_scope: "accepted_only",
                    dispatch_start_proven: false,
                },
            )
            .unwrap_or_else(|err| panic!("insert open dispatch attempt {index}: {err}"));
        }
        drop(conn);

        let mut bootstrap = test_bootstrap(&dir);
        bootstrap.controller_generation = 2;
        let runtime = ControllerRuntime::new(bootstrap).unwrap();
        let memory_record = runtime.actor_record(&document_id).unwrap().unwrap();
        assert_eq!(memory_record.pane_id, "%88");

        conn = open_state_db(dir.path()).unwrap();
        let marker_count = |conn: &Connection| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM crash_recovery_markers WHERE marker_kind = 'dispatch_receipt_reconcile'",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(marker_count(&conn), receipt_count);
        let dedupe_key_count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT dedupe_key) FROM crash_recovery_markers WHERE marker_kind = 'dispatch_receipt_reconcile'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dedupe_key_count, receipt_count);
        drop(conn);

        let mut bootstrap = test_bootstrap(&dir);
        bootstrap.controller_generation = 3;
        let runtime = ControllerRuntime::new(bootstrap).unwrap();
        assert!(runtime.actor_record(&document_id).unwrap().is_some());

        conn = open_state_db(dir.path()).unwrap();
        assert_eq!(marker_count(&conn), receipt_count);
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
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-closed-clear",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-closed-clear",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Closed,
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
    fn controller_session_recovery_commands_accept_blocked_actor_generation() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/blocked-clear.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-blocked-clear\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        let bootstrap = test_bootstrap(&dir);
        let mut should_stop = false;
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-blocked-clear",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-blocked-clear",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Blocked,
            "route",
            "starting_actor_timeout",
        )
        .unwrap();

        // #clear-blocked-actor: a `Blocked` actor (starting-timeout) is a stuck
        // state that recovery commands must be able to fix, not a wall that
        // rejects them. `session_clear` must accept it just like a `Closed`
        // actor.
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
            diagnostic_payload: Some("test clear blocked actor".to_string()),
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
            "session_clear should accept blocked actors: {:?}",
            envelope.error
        );
        assert_eq!(envelope.data.unwrap().accepted_stage, "operator_blocked");

        let interrupt_clear = ControllerRequest {
            command_kind: Some("session_interrupt_clear".to_string()),
            ..clear.clone()
        };
        let response = handle_request(
            &(serde_json::to_string(&interrupt_clear).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "session_interrupt_clear should accept blocked actors: {:?}",
            envelope.error
        );
        assert_eq!(envelope.data.unwrap().accepted_stage, "operator_blocked");

        let restart = ControllerRequest {
            command_kind: Some("session_restart".to_string()),
            ..clear.clone()
        };
        let response = handle_request(
            &(serde_json::to_string(&restart).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "session_restart should supersede a blocked actor (blue/green #supkill-bg): {:?}",
            envelope.error
        );
        assert_eq!(envelope.data.unwrap().accepted_stage, "operator_blocked");

        let ops_log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("supersede_blocked_actor"),
            "restart on a blocked actor must record the supersede redirect:\n{ops_log}"
        );

        // A non-recovery command must still be rejected on a Blocked actor.
        let non_recovery = ControllerRequest {
            command_kind: Some("dispatch".to_string()),
            ..clear
        };
        let response = handle_request(
            &(serde_json::to_string(&non_recovery).unwrap() + "\n"),
            &bootstrap,
            &mut should_stop,
        )
        .unwrap();
        let envelope: ControllerEnvelope<DispatchAuthorization> =
            serde_json::from_str(&response).unwrap();
        assert!(
            !envelope.ok,
            "non-recovery dispatch must still be rejected on a blocked actor"
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
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-attach",
            "%41",
            "@1",
            1,
        )
        .unwrap();
        let conn = Connection::open(state_db_path(dir.path())).unwrap();
        let diagnostics_before_attach: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'session_registry'",
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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
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
                "SELECT COUNT(*) FROM projection_diagnostics WHERE projection = 'session_registry'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            diagnostics_after_attach, diagnostics_before_attach,
            "controller attach should not add a legacy registry diagnostic"
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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
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
        let envelope: ControllerEnvelope<agent_doc_sqlite::state_store::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok, "mark_lifecycle with relative path failed");
        assert_eq!(
            envelope.data.unwrap().state,
            agent_doc_sqlite::state_store::ActorState::Ready
        );
    }
    // ── Stuck-`Preparing` controller reaper (#kqr6 / #sjwm / #stuckhandoff) ──
    #[test]
    fn reaper_keeps_fresh_preparing_bootstrap() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // Fresh handoff_started_at (just now) ⇒ healthy mid-handoff, keep.
        write_preparing_bootstrap(dir.path(), std::process::id(), Some(timestamp_secs()));
        let (reaped, kept) =
            terminate_stale_preparing_controllers(dir.path(), Duration::from_secs(45), false)
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
        let (reaped, kept) =
            terminate_stale_preparing_controllers(dir.path(), Duration::from_secs(45), false)
                .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(
            after.handoff_state,
            ControllerHandoffState::Preparing,
            "a non-controller pid must never be killed or marked Failed"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_reaped_skipped"));
        assert!(ops_log.contains("reason=not_same_project_controller"));
    }

    #[test]
    fn reaper_marks_dead_preparing_bootstrap_failed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let dead_pid = u32::MAX - 1;
        assert!(!process_is_alive(dead_pid), "test requires a dead pid");
        let old = timestamp_secs() - 600;
        write_preparing_bootstrap(dir.path(), dead_pid, Some(old));

        let (reaped, kept) =
            terminate_stale_preparing_controllers(dir.path(), Duration::from_secs(45), false)
                .unwrap();
        assert_eq!((reaped, kept), (1, 0));
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Failed);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_record_marked_failed"));
        assert!(ops_log.contains("reason=dead_pid"));
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
        let state_projection = agent_doc_state_backbone::StateBackboneProjection::default();
        let state_ledger = agent_doc_state_backbone::EventLedger::default();
        let supervisor_recycle_graph =
            ControllerSupervisorRecycleGraph::new(state_projection.project_supervisor_recycle());
        ControllerRuntime {
            bootstrap: Mutex::new(bootstrap),
            memory: Mutex::new(ControllerMemoryState {
                actor_store: BTreeMap::new(),
                state_ledger,
                state_projection,
                map_backend: "std_btree_map",
            }),
            supervisor_recycle_graph,
            supervisor_recycle_waiters: Condvar::new(),
            recycle_requested: AtomicBool::new(false),
            recycle_forced: AtomicBool::new(false),
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
            !controller_self_watchdog_should_suicide(
                &runtime,
                None,
                Duration::ZERO,
                Duration::from_secs(45),
            ),
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
            !controller_self_watchdog_should_suicide(
                &runtime,
                None,
                Duration::ZERO,
                Duration::from_secs(0),
            ),
            "a Stable controller must never self-terminate, even at a zero threshold"
        );
    }
    #[test]
    fn self_watchdog_suicides_and_marks_failed_on_stale_preparing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let stale = timestamp_secs().saturating_sub(600);
        let bootstrap =
            preparing_runtime_bootstrap(dir.path(), ControllerHandoffState::Preparing, Some(stale));
        write_bootstrap_state(&bootstrap).unwrap();
        let runtime = runtime_for_bootstrap(bootstrap);

        assert!(
            controller_self_watchdog_should_suicide(
                &runtime,
                None,
                Duration::ZERO,
                Duration::from_secs(45),
            ),
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
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_self_reaped pid="));
        assert!(ops_log.contains("caller=self_watchdog"));
    }
    // ---- M1b (#stuckhandoff2 reopen): stranded post-promote replacement ----
    #[test]
    fn self_watchdog_suicides_when_replacement_temp_socket_persists_past_threshold() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let temp = dir.path().join("controller-handoff-1234-9.sock");
        std::fs::write(&temp, b"").unwrap();
        // Promotion's rename never removed the temp socket and the threshold elapsed:
        // a stranded replacement, even though its in-memory state may read `Stable`.
        let runtime = runtime_for_bootstrap(preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Stable,
            None,
        ));
        assert!(controller_self_watchdog_should_suicide(
            &runtime,
            Some(temp.as_path()),
            Duration::from_secs(600),
            Duration::from_secs(45),
        ));
    }
    #[test]
    fn self_watchdog_keeps_replacement_after_promote_rename_removes_temp_socket() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // A completed handoff renamed temp -> public, so the temp path is gone.
        let temp = dir.path().join("controller-handoff-1234-9.sock");
        assert!(!temp.exists());
        let runtime = runtime_for_bootstrap(preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Stable,
            None,
        ));
        assert!(
            !controller_self_watchdog_should_suicide(
                &runtime,
                Some(temp.as_path()),
                Duration::from_secs(600),
                Duration::from_secs(45),
            ),
            "a promoted+renamed replacement (temp socket gone) is authoritative, never stranded"
        );
    }
    #[test]
    fn self_watchdog_suicide_marks_failed_for_stranded_stable_replacement() {
        // The M1b structural watchdog hands a `Stable`-in-memory stranded replacement
        // to the same suicide path; it must still mark the bootstrap Failed + log.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let bootstrap =
            preparing_runtime_bootstrap(dir.path(), ControllerHandoffState::Stable, None);
        write_bootstrap_state(&bootstrap).unwrap();
        let runtime = runtime_for_bootstrap(bootstrap);

        controller_self_watchdog_suicide(&runtime, Duration::from_secs(45));

        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Failed);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_self_reaped pid="));
    }
    #[test]
    fn self_watchdog_suicide_preserves_superseded_generation_record() {
        // A stranded replacement that a newer clean controller already superseded on
        // disk must NOT clobber that newer record to Failed when it self-reaps.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut newer =
            preparing_runtime_bootstrap(dir.path(), ControllerHandoffState::Stable, None);
        newer.controller_generation = 99;
        write_bootstrap_state(&newer).unwrap();
        // The wedged replacement is the older generation 7.
        let stranded =
            preparing_runtime_bootstrap(dir.path(), ControllerHandoffState::Stable, None);
        let runtime = runtime_for_bootstrap(stranded);

        controller_self_watchdog_suicide(&runtime, Duration::from_secs(45));

        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(
            after.controller_generation, 99,
            "the newer controller's record must survive the older replacement's self-reap"
        );
        assert_eq!(
            after.handoff_state,
            ControllerHandoffState::Stable,
            "self-reap must not flip a superseding controller's record to Failed"
        );
        // In-memory still flips Failed so this process exits.
        assert_eq!(
            runtime.bootstrap_snapshot().unwrap().handoff_state,
            ControllerHandoffState::Failed
        );
    }
    // ---- M3 (#stuckhandoff2): orphaned-preparing process-scan reaper ----
    #[test]
    fn process_start_age_secs_reports_for_self() {
        assert!(
            crate::process::process_start_age_secs(std::process::id()).is_some(),
            "process start age must resolve from /proc for a live pid"
        );
    }
    #[test]
    fn orphan_reaper_keeps_fresh_preparing_sentinel() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(crate::process::cmdline_has_preparing_handoff(pid));

        // Just-launched (age ~0s) ⇒ inside a healthy handoff window ⇒ keep.
        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(45), false)
                .unwrap();
        assert_eq!((reaped, kept), (0, 1));
        assert!(
            process_is_alive(pid),
            "a fresh preparing sentinel must be kept"
        );

        let _ = sentinel.kill();
        let _ = sentinel.wait();
    }
    #[test]
    fn orphan_reaper_ignores_non_preparing_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(!crate::process::cmdline_has_preparing_handoff(pid));

        // No `--handoff-state preparing` ⇒ not an orphaned handoff ⇒ never scanned.
        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(0), false).unwrap();
        assert_eq!((reaped, kept), (0, 0));
        assert!(
            process_is_alive(pid),
            "a plain controller must never be reaped here"
        );

        let _ = sentinel.kill();
        let _ = sentinel.wait();
    }

    #[test]
    fn orphan_reaper_skips_self_promoted_stable_bootstrap_with_preparing_argv() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let mut sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(crate::process::cmdline_has_preparing_handoff(pid));

        let mut bootstrap =
            preparing_runtime_bootstrap(dir.path(), ControllerHandoffState::Stable, None);
        bootstrap.pid = pid;
        write_bootstrap_state(&bootstrap).unwrap();

        let (reaped, kept) =
            reap_orphaned_preparing_controllers(dir.path(), Duration::from_secs(0), false).unwrap();
        assert_eq!(
            (reaped, kept),
            (0, 0),
            "a self-promoted stable bootstrap must not be treated as an orphan even if argv still says preparing"
        );
        assert!(
            process_is_alive(pid),
            "the stable bootstrap-owned controller must survive the orphan scan"
        );

        let _ = sentinel.kill();
        let _ = sentinel.wait();
    }

    #[test]
    fn removed_project_root_reaper_reaps_stable_temp_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let project_root = dir.path().to_path_buf();
        let sentinel = spawn_controller_sentinel(&project_root);
        let pid = sentinel.id();
        assert!(is_same_project_controller_pid(&project_root, pid));

        drop(dir);

        let (reaped, kept) = reap_removed_project_root_controllers_for_caller(
            &project_root,
            Duration::from_secs(0),
            false,
            "test",
        )
        .unwrap();
        assert_eq!((reaped, kept), (1, 0));

        let status = wait_for_test_child_exit(
            sentinel,
            Duration::from_secs(2),
            "stable temp-root controller must be reaped after its project root disappears",
        );
        assert!(
            !status.success(),
            "removed-root controller must be signal-terminated: {status:?}"
        );
    }

    #[test]
    fn recycle_reaps_aged_orphaned_preparing_controller() {
        // #stuckhandoff2: `admin recycle` must process-scan-reap a wedged `Preparing`
        // orphan in the root, not merely re-exec the authoritative controller (which
        // an orphan — invisible to the project socket — would survive). With no live
        // controller listening, the recycle RPC no-ops but the project-scoped orphan
        // reap must still fire (`caller=recycle`), so a recycle no longer leaves the
        // zombie behind for M1's later self-watchdog tick to clear.
        let _env = agent_doc_harness::prompt_source::TEST_ENV_LOCK
            .lock()
            .unwrap();
        unsafe { std::env::set_var(STALE_PREPARING_CONTROLLER_SECS_ENV, "1") };

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(crate::process::cmdline_has_preparing_handoff(pid));

        // Age strictly past the 1s threshold. Start age is whole seconds (/proc dir
        // mtime), and the reaper keeps when `age <= threshold`, so a ~1.1s-old
        // process floors to age=1 and is kept; sleep past 2s so age=2 > 1 ⇒ reaped.
        std::thread::sleep(Duration::from_millis(2200));

        // No authoritative controller listens on the project socket ⇒ recycle RPC
        // no-ops (Ok(false)); the orphan reap still runs.
        let recycled = recycle_controller(dir.path()).unwrap();
        assert!(
            !recycled,
            "no authoritative controller answered the recycle"
        );

        // The aged orphan is our child; poll try_wait for its termination.
        let status = wait_for_test_child_exit(
            sentinel,
            Duration::from_secs(2),
            "aged preparing orphan must be reaped by recycle",
        );
        assert!(
            !status.success(),
            "orphan must be signal-terminated: {status:?}"
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("orphaned_preparing_controller_reaped pid="));
        assert!(ops_log.contains("caller=recycle"));

        unsafe { std::env::remove_var(STALE_PREPARING_CONTROLLER_SECS_ENV) };
    }

    #[test]
    fn recycle_reaps_stale_bootstrap_owned_preparing_controller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(crate::process::cmdline_has_preparing_handoff(pid));

        let mut bootstrap = preparing_runtime_bootstrap(
            dir.path(),
            ControllerHandoffState::Preparing,
            Some(timestamp_secs() - 600),
        );
        bootstrap.pid = pid;
        write_bootstrap_state(&bootstrap).unwrap();

        let recycled = recycle_controller(dir.path()).unwrap();
        assert!(
            !recycled,
            "no authoritative controller answered the recycle"
        );

        let status = wait_for_test_child_exit(
            sentinel,
            Duration::from_secs(2),
            "stale bootstrap-owned preparing controller must be reaped by recycle",
        );
        assert!(
            !status.success(),
            "bootstrap-owned preparing controller must be signal-terminated: {status:?}"
        );
        let after = read_bootstrap(dir.path()).unwrap().unwrap();
        assert_eq!(after.handoff_state, ControllerHandoffState::Failed);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("stale_preparing_controller_reaped pid="));
        assert!(ops_log.contains("caller=recycle"));
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
        agent_doc_session_actor_io::record_session_start_direct(&doc, "session-m2", "%41", "@1", 1)
            .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-m2",
            "%41",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
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
        assert_eq!(
            refused, 1,
            "non-Stable dispatch refusal must record a receipt"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
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
        agent_doc_session_actor_io::record_session_start_direct(
            &doc,
            "session-idle",
            "%61",
            "@1",
            1,
        )
        .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-idle",
            "%61",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
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
        agent_doc_session_actor_io::record_session_start_direct(&doc, "session-sb", "%51", "@1", 1)
            .unwrap();
        agent_doc_session_actor_io::transition_state_direct(
            &doc,
            "session-sb",
            "%51",
            Some(1),
            agent_doc_sqlite::state_store::ActorState::Ready,
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
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
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
        use agent_doc_controller::command_line::controller_serve_project_root_from_args;

        // The cmdline shape a sentinel/real controller presents in `/proc`, for a
        // project root that is NOT the caller's — the breadth M5 adds over the
        // per-project reaper.
        let args = vec![
            "/some/bin/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/sample-app".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&args),
            Some(PathBuf::from("/home/me/work/sample-app"))
        );

        let shell_sentinel = vec![
            "sh".to_string(),
            "-c".to_string(),
            "sleep 30; :".to_string(),
            "/home/me/work/sample-app/agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/sample-app".to_string(),
            "--handoff-state".to_string(),
            "preparing".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&shell_sentinel),
            Some(PathBuf::from("/home/me/work/sample-app"))
        );

        let tmux_launcher = vec![
            "tmux".to_string(),
            "new-session".to_string(),
            "agent-doc".to_string(),
            "controller".to_string(),
            "serve".to_string(),
            "--project-root".to_string(),
            "/home/me/work/sample-app".to_string(),
        ];
        assert_eq!(
            controller_serve_project_root_from_args(&tmux_launcher),
            None
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
        let sentinel = spawn_preparing_controller_sentinel(dir.path());
        let pid = sentinel.id();
        assert!(crate::process::cmdline_has_preparing_handoff(pid));
        assert_eq!(
            crate::process::controller_serve_project_root(pid).as_deref(),
            Some(dir.path()),
            "the sweep must recover the sentinel's own --project-root from /proc"
        );
        // Age past a zero threshold (start age = /proc dir mtime).
        std::thread::sleep(Duration::from_millis(1100));

        let (reaped, _kept) =
            reap_orphaned_preparing_controllers_all_projects(Duration::from_secs(0), false, "test")
                .unwrap();
        assert!(
            reaped >= 1,
            "cross-project sweep must reap the aged preparing sentinel"
        );

        // The live orphan must actually be terminated. The sentinel is our child, so
        // a killed process lingers as a zombie until `wait()` — poll `try_wait`.
        let status = wait_for_test_child_exit(
            sentinel,
            Duration::from_secs(2),
            "aged cross-project preparing orphan must be reaped",
        );
        assert!(
            !status.success(),
            "orphan must be signal-terminated: {status:?}"
        );

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("orphaned_preparing_controller_reaped_cross_project pid="));
        assert!(ops_log.contains("caller=test"));
    }
}
