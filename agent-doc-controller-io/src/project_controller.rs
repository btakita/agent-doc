//! Project-local controller shell.
//!
//! The controller is the live authority for document session actor lookup,
//! generation changes, lifecycle reports, and routed dispatch acceptance.
//! Tmux state remains a layout input. Actor ownership is a controller-lifetime
//! Lazily projection; SQLite hydrates and durably records that model.

use crate::process::{is_same_project_controller_pid, process_is_alive};
use agent_doc_controller::dispatch::{
    ControllerDispatchProofScope, ControllerDispatchReceipt, ControllerDispatchResultStatus,
};
use agent_doc_controller::paths::socket_path;
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
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerNonblockingMode, ListenerOptions, ToFsName,
    ToNsName,
    traits::{Listener as _, Stream as _},
};
use lazily::{Computed, Source, ThreadSafeContext};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

// The SQLite state layer (the only `rusqlite::Connection` surface) lives in
// `agent-doc-sqlite::state_store`. Keep persistence types private to this
// orchestration module. Domain/controller vocabulary belongs to the model
// crates even when SQLite serializes it.
use parking_lot::{Condvar, Mutex};
#[cfg(test)]
use state_store::state_db_path;
use state_store::{
    AdminOperationStatus, DispatchAttemptStatus, ProjectionDiagnosticStatus,
    QueueBackpressureStatus, QueueControlStatus, QueueHeadStatus, SessionOperatorStatus,
    SupervisorLeaseStatus,
};
use state_store::{
    Connection, insert_state_event_in_db, load_actor_record_from_db, load_actor_store_from_db,
    load_control_plane_store_counts, load_layout_state_from_db,
    load_session_operator_status_from_db, load_state_events_from_db, load_supervisor_lease_from_db,
    open_state_db, store_layout_state_in_db, timestamp_secs,
};
#[cfg(test)]
use state_store::{ProjectionDiagnosticInsert, insert_projection_diagnostic_with_metadata};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_LAYOUT_SCOPE: &str = "default";
const CONTROLLER_BOOTSTRAP_SCOPE: &str = "project";
const CONNECT_WAIT: Duration = Duration::from_secs(3);
#[cfg(not(any(test, feature = "test-support")))]
const LAUNCH_CONNECT_WAIT: Duration = Duration::from_secs(45);
#[cfg(any(test, feature = "test-support"))]
const LAUNCH_CONNECT_WAIT: Duration = Duration::from_millis(500);
#[cfg(test)]
#[allow(dead_code)]
const HANDOFF_CONNECT_WAIT: Duration = Duration::from_secs(30);
const CONNECT_POLL: Duration = Duration::from_millis(50);
/// How long a contended launch waits for the current bootstrap claimant to
/// finish before giving up. Sized above `LAUNCH_CONNECT_WAIT` so a waiter outlasts the claimant's
/// full `launch_detached` + `wait_for_controller_after_launch` window and can adopt the
/// controller the holder published instead of failing the start (#suprecyclelock).
#[cfg(not(any(test, feature = "test-support")))]
const LAUNCH_CLAIM_WAIT: Duration = Duration::from_secs(50);
#[cfg(any(test, feature = "test-support"))]
const LAUNCH_CLAIM_WAIT: Duration = Duration::from_secs(1);
const LAUNCH_CLAIM_POLL: Duration = Duration::from_millis(50);
#[cfg(not(any(test, feature = "test-support")))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_secs(5);
/// The test-mode deadline exists so a *genuinely absent* controller fails fast
/// instead of stalling the suite for 5s per call. It is not a latency assertion —
/// no test checks that a response arrives within it.
///
/// 250ms made it one: on a loaded CI runner the fake in-process listener is not
/// always scheduled inside a quarter second, so `compact_commit_preserves_only_
/// unresolved_prompt_in_live_editor` failed with `transport failed at epoch 1 …
/// timed out after 0.2s` while asserting nothing about timing. A deadline that
/// fails when the machine is busy is measuring the runner, not the code. Two
/// seconds keeps the fast-fail property (a missing controller still cannot cost
/// 5s) with enough headroom that scheduling jitter cannot decide the outcome; a
/// real hang is still caught by the harness's own per-test timeout.
#[cfg(any(test, feature = "test-support"))]
const CONTROLLER_RPC_TIMEOUT: Duration = Duration::from_secs(2);
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
    /// Known authoritative pane for controller-owned recovery work. Editor
    /// requests leave this unset and use the normal route resolver.
    pub pane: Option<String>,
    pub layout_args: Vec<String>,
    pub dispatch_only: bool,
    pub plain_trigger: bool,
    pub wait_for_ready_secs: Option<u64>,
    pub force_disk: bool,
    /// Whether route may perform the fleet-wide stale-registry prune before
    /// lookup. Targeted controller recovery already owns a proven pane and
    /// must not mutate unrelated sessions.
    pub prune_before_lookup: bool,
    /// Background recovery is restricted to the already-proven pane and must
    /// preserve the operator's current tmux client/window/pane focus. It may not
    /// rescue a stash pane, select an alternate pane, or auto-start a new pane.
    pub background_existing_pane_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerEditorRouteRuntimeResult {
    pub exit_code: i32,
    pub output: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerTurnSteeringInvocation {
    pub file: PathBuf,
    pub steering_id: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerTurnSteeringOutcome {
    Delivered,
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerTurnSteeringReceipt {
    pub kind: String,
    pub steering_id: String,
    pub outcome: ControllerTurnSteeringOutcome,
    pub accepted_bytes: usize,
    pub actor_session_id: String,
    pub actor_pane_id: String,
    pub actor_generation: u64,
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

/// Complete Compact Exchange invocation executed inside the CP process. The
/// editor/CLI is only a command submitter; all reads, CRDT mutation, archive,
/// commit, and delivery acknowledgement remain under controller ownership.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerCompactDocumentInvocation {
    pub keep: Option<usize>,
    pub component_name: Option<String>,
    pub message: Option<String>,
    pub tag: Option<String>,
    pub commit: bool,
    pub force_disk: bool,
}

pub const COMPACT_COMMIT_SCOPE_NOTE: &str = "[compact] note: --commit persists only the compacted document state now in HEAD; any later console explanation still needs its own `agent-doc finalize` or `agent-doc write --commit` cycle to land in `exchange`";

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
    #[serde(default)]
    pub caller_kind: String,
    /// Controller-local actor bindings joined reactively with `columns`.
    ///
    /// This is never accepted from or serialized to an RPC caller. The
    /// controller's pane-layout effect fills it from the process-scoped actor
    /// projection immediately before crossing the tmux boundary.
    #[serde(skip)]
    pub actor_bindings: Vec<ControllerTmuxActorBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerTmuxActorBinding {
    pub document_path: String,
    pub session_id: String,
    pub pane_id: String,
    pub generation: u64,
}

impl ControllerTmuxLayoutSyncInvocation {
    pub fn routes_created_panes(&self) -> bool {
        !self.no_autostart && matches!(self.caller_kind.as_str(), "manual" | "projection")
    }
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
    pub routes_created_panes: bool,
    /// Exact file-to-pane assignments observed by the tmux sync effect.
    ///
    /// The controller retains these as generation-scoped reactive evidence so
    /// observation never has to infer nested-project pane identity from a
    /// partial actor store.
    #[serde(default)]
    pub file_panes: Vec<(String, String)>,
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

/// First cross-process state-plane channel: the editor's desired pane layout.
///
/// The payload node uses `agent-doc.pane-layout.desired.v1`; the channel name is
/// intentionally transport/domain neutral so other Lazily peers can subscribe
/// without knowing which editor published it.
pub const PANE_LAYOUT_DESIRED_STATE_CHANNEL: &str = "agent-doc/pane-layout/desired/v1";
/// Controller-published desired/observed/effect projection for pane layout.
pub const PANE_LAYOUT_STATUS_STATE_CHANNEL: &str = "agent-doc/pane-layout/status/v1";
const STATE_PLANE_MAX_CHANNELS: usize = 256;
// Four full editor warm sets can overlap during focus replacement or while
// multiple editor clients observe the same controller. Each editor admits at
// most 256 documents and retires low-priority streams reactively.
const DOCUMENT_AUTHORITY_MAX_CHANNELS: usize = 1_024;
const DOCUMENT_AUTHORITY_STATE_CHANNEL_PREFIX: &str = "agent-doc/document-turn-authority/v1/";
const STATE_PLANE_MAX_RETAINED_FRAMES_PER_CHANNEL: usize = 1_024;
const STATE_PLANE_MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
const STATE_PLANE_VERSION_NAMESPACE_BITS: u32 = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerStatePlanePublishInvocation {
    pub channel: String,
    pub producer_id: String,
    /// Canonical Lazily `IpcMessage` JSON (`Snapshot` or `Delta`).
    pub message_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerStatePlaneFrame {
    pub channel: String,
    pub producer_id: String,
    pub epoch: u64,
    #[serde(default)]
    pub base_epoch: Option<u64>,
    pub plane_version: u64,
    pub message_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerStatePlanePublishReceipt {
    pub accepted: bool,
    pub reason: String,
    pub channel: String,
    pub producer_id: String,
    pub epoch: u64,
    pub plane_version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerStatePlaneSubscribeInvocation {
    pub channel: String,
    /// The controller incarnation that owns `after_version`. A changed
    /// generation makes the version cursor cold immediately.
    #[serde(default)]
    pub after_controller_generation: Option<u64>,
    #[serde(default)]
    pub after_version: u64,
    #[serde(default)]
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerStatePlaneSubscription {
    pub channel: String,
    /// Cursor namespace. `latest_version` is meaningful only within this
    /// controller generation.
    #[serde(default)]
    pub controller_generation: u64,
    pub latest_version: u64,
    pub timed_out: bool,
    /// A covering Snapshot followed by any causally-applicable Deltas, or only
    /// frames newer than `after_version` when that cursor is still retained.
    pub frames: Vec<ControllerStatePlaneFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneLayoutDesired {
    pub generation: u64,
    pub source_plane_version: Option<u64>,
    pub invocation: ControllerTmuxLayoutSyncInvocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneLayoutObservation {
    pub generation: u64,
    pub actor_bindings: Vec<ControllerTmuxActorBinding>,
    pub report: ControllerTmuxLayoutSyncStateReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PaneLayoutStructure {
    columns: Vec<String>,
    window: Option<String>,
    no_autostart: bool,
    exact_visible: bool,
}

impl From<&ControllerTmuxLayoutSyncInvocation> for PaneLayoutStructure {
    fn from(invocation: &ControllerTmuxLayoutSyncInvocation) -> Self {
        Self {
            columns: invocation.columns.clone(),
            window: invocation.window.clone(),
            no_autostart: invocation.no_autostart,
            exact_visible: invocation.exact_visible,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneLayoutStructuralReceipt {
    structure: PaneLayoutStructure,
    actor_bindings: Vec<ControllerTmuxActorBinding>,
    pub report: Option<ControllerTmuxLayoutSyncStateReport>,
    pub file_panes: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
pub(crate) enum PaneLayoutEffectPhase {
    Idle,
    InFlight,
    RetryPending,
    Converged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneLayoutEffectReceipt {
    pub generation: u64,
    pub actor_bindings: Vec<ControllerTmuxActorBinding>,
    pub attempt: u64,
    pub phase: PaneLayoutEffectPhase,
    pub reason: String,
    pub file_panes: Vec<(String, String)>,
    /// Whether this generation still required a pane-selection consequence
    /// after desktop-focus policy was applied.
    pub focus_required: bool,
    /// Receipt from the final, generation-fenced `select-pane` consequence.
    pub focus_applied: bool,
}

impl Default for PaneLayoutEffectReceipt {
    fn default() -> Self {
        Self {
            generation: 0,
            actor_bindings: Vec::new(),
            attempt: 0,
            phase: PaneLayoutEffectPhase::Idle,
            reason: "idle".to_string(),
            file_panes: Vec::new(),
            focus_required: false,
            focus_applied: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PaneLayoutProjection {
    Absent,
    NeedsEffect(PaneLayoutDesired),
    Applying(PaneLayoutDesired),
    RetryPending(PaneLayoutDesired),
    Converged(PaneLayoutDesired),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerPaneLayoutPhase {
    NeedsEffect,
    Applying,
    RetryPending,
    Converged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerPaneLayoutReasonCode {
    Unobserved,
    EffectInFlight,
    ObservedConvergence,
    PaneCountMismatch,
    PaneOrderMismatch,
    TmuxUnavailable,
    EffectFailed,
    ObservationFailed,
    RetryScheduled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerPaneLayoutStateProjection {
    pub generation: u64,
    #[serde(default)]
    pub source_plane_version: Option<u64>,
    pub phase: ControllerPaneLayoutPhase,
    pub reason_code: ControllerPaneLayoutReasonCode,
    #[serde(default)]
    pub reason_detail: Option<String>,
    pub columns: Vec<String>,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub observation: Option<ControllerTmuxLayoutSyncStateReport>,
    pub attempt: u64,
}

fn derive_pane_layout_projection(
    desired: Option<PaneLayoutDesired>,
    actor_bindings: Vec<ControllerTmuxActorBinding>,
    observed: Option<PaneLayoutObservation>,
    receipt: PaneLayoutEffectReceipt,
) -> PaneLayoutProjection {
    let Some(desired) = desired else {
        return PaneLayoutProjection::Absent;
    };
    let receipt_is_current =
        receipt.generation == desired.generation && receipt.actor_bindings == actor_bindings;
    let observation_is_current = observed.as_ref().is_some_and(|observed| {
        observed.generation == desired.generation && observed.actor_bindings == actor_bindings
    });
    let focus_converged = desired.invocation.focus.is_none()
        || (receipt_is_current && (!receipt.focus_required || receipt.focus_applied));
    if focus_converged
        && observation_is_current
        && observed
            .as_ref()
            .is_some_and(|observed| observed.report.synced)
    {
        return PaneLayoutProjection::Converged(desired);
    }
    if receipt_is_current {
        match receipt.phase {
            PaneLayoutEffectPhase::InFlight => {
                return PaneLayoutProjection::Applying(desired);
            }
            PaneLayoutEffectPhase::RetryPending => {
                return PaneLayoutProjection::RetryPending(desired);
            }
            PaneLayoutEffectPhase::Idle | PaneLayoutEffectPhase::Converged => {}
        }
    }
    PaneLayoutProjection::NeedsEffect(desired)
}

pub(crate) trait PaneLayoutProjectionSink: Send + Sync + 'static {
    fn reconcile(&self, desired: PaneLayoutDesired);
}

type ControllerActorStore = BTreeMap<String, agent_doc_controller::actor::ActorRecord>;

/// Process-scoped reactive actor authority.
///
/// SQLite hydrates `records` at controller start and receives transition
/// effects for durability, but hot-path readers observe `live_bindings`.
/// Filtering closed/empty bindings once in this `Computed` replaces the
/// repeated imperative actor checks that previously re-entered controller RPC.
struct ControllerActorGraph {
    ctx: ThreadSafeContext,
    records: Source<ControllerActorStore>,
    live_bindings: Computed<ControllerActorStore>,
    /// Per-document actor state used by document-authority projections.
    ///
    /// `records` remains the process-wide durable mirror for consumers that
    /// genuinely need the whole actor store. Authority needs only one
    /// document's model state, so a keyed Source prevents an unrelated actor
    /// heartbeat from invalidating every warm document.
    document_model_states:
        lazily::ThreadSafeSourceMap<String, Option<agent_doc_controller::actor::ActorState>>,
}

impl ControllerActorGraph {
    fn new_in(scope: &agent_doc_state_scope::ProcessScope, initial: ControllerActorStore) -> Self {
        let ctx = scope.ctx().clone();
        let document_model_states = lazily::ThreadSafeSourceMap::new(&ctx);
        for (document_id, record) in &initial {
            document_model_states.set(&ctx, document_id.clone(), Some(record.state));
        }
        let records = ctx.source(initial);
        let live_bindings = ctx.computed(move |ctx| {
            ctx.get(&records)
                .into_iter()
                .filter(|(_, record)| {
                    record.state != agent_doc_controller::actor::ActorState::Closed
                        && !record.pane_id.trim().is_empty()
                })
                .collect()
        });
        Self {
            ctx,
            records,
            live_bindings,
            document_model_states,
        }
    }

    fn set(&self, records: ControllerActorStore) {
        let document_ids = self
            .document_model_states
            .present_keys()
            .into_iter()
            .chain(records.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        for document_id in document_ids {
            self.document_model_states.set(
                &self.ctx,
                document_id.clone(),
                records.get(&document_id).map(|record| record.state),
            );
        }
        self.ctx.set(&self.records, records);
    }

    fn apply_store_write(&self, write: &agent_doc_controller::actor::ActorStoreWrite) {
        let mut records = self.ctx.get(&self.records);
        let mut evicted_document_ids = write
            .evicted_document_ids
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        if write.record.state != agent_doc_controller::actor::ActorState::Closed
            && !write.record.pane_id.trim().is_empty()
        {
            evicted_document_ids.extend(records.values().filter_map(|record| {
                (record.document_id != write.record.document_id
                    && record.pane_id == write.record.pane_id)
                    .then_some(record.document_id.clone())
            }));
        }
        let mut changed_document_ids = evicted_document_ids.clone();
        for document_id in evicted_document_ids {
            let Some(record) = records.get_mut(&document_id) else {
                continue;
            };
            record.state = agent_doc_controller::actor::ActorState::Closed;
            record.pane_id.clear();
            record.window_id.clear();
            record.last_transition = agent_doc_controller::actor::ActorLastTransition {
                caller: write.record.last_transition.caller.clone(),
                reason: format!(
                    "evicted_cross_document_pane owner={} pane={}",
                    write.record.document_id, write.record.pane_id
                ),
                timestamp: write.record.last_transition.timestamp,
                prior_generation: record.generation,
                new_generation: record.generation,
            };
        }
        changed_document_ids.insert(write.record.document_id.clone());
        records.insert(write.record.document_id.clone(), write.record.clone());
        for document_id in changed_document_ids {
            self.document_model_states.set(
                &self.ctx,
                document_id.clone(),
                records.get(&document_id).map(|record| record.state),
            );
        }
        self.ctx.set(&self.records, records);
    }

    fn record(&self, document_id: &str) -> Option<agent_doc_controller::actor::ActorRecord> {
        self.ctx.get(&self.records).get(document_id).cloned()
    }

    fn records(&self) -> ControllerActorStore {
        self.ctx.get(&self.records)
    }

    fn live_bindings_handle(&self) -> Computed<ControllerActorStore> {
        self.live_bindings
    }

    fn document_model_states_handle(
        &self,
    ) -> lazily::ThreadSafeSourceMap<String, Option<agent_doc_controller::actor::ActorState>> {
        self.document_model_states.clone()
    }
}

fn derive_layout_actor_bindings(
    desired: Option<&PaneLayoutDesired>,
    actors: &ControllerActorStore,
) -> Vec<ControllerTmuxActorBinding> {
    let Some(desired) = desired else {
        return Vec::new();
    };
    desired
        .invocation
        .columns
        .iter()
        .flat_map(|column| column.split(','))
        .map(str::trim)
        .filter(|document| !document.is_empty())
        .filter_map(|document| {
            // The ingress adapter canonicalizes desired document IDs before
            // publishing this Source. Keep this Computed a pure join over
            // already-published values: no filesystem canonicalization, RPC,
            // SQLite reads, or tmux probes belong here.
            let record = actors.get(document)?;
            Some(ControllerTmuxActorBinding {
                document_path: document.to_string(),
                session_id: record.session_id.clone(),
                pane_id: record.pane_id.clone(),
                generation: record.generation,
            })
        })
        .collect()
}

/// Controller-lifetime Lazily graph for the pane layout projection.
///
/// IDE observations set `desired`; tmux observations set `observed`; the
/// `projection` Computed derives whether an effect is needed; and the retained
/// Lazily Effect invokes the single tmux adapter sink. SQLite persists only the
/// desired columns so a controller restart can rebuild this graph without
/// treating tmux or a sidecar as authority.
#[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
struct ControllerPaneLayoutGraph {
    ctx: ThreadSafeContext,
    desired: Source<Option<PaneLayoutDesired>>,
    actor_bindings: Computed<Vec<ControllerTmuxActorBinding>>,
    observed: Source<Option<PaneLayoutObservation>>,
    receipt: Source<PaneLayoutEffectReceipt>,
    structural_receipt: Source<Option<PaneLayoutStructuralReceipt>>,
    applicable_receipt: Computed<Option<PaneLayoutEffectReceipt>>,
    projection: Computed<PaneLayoutProjection>,
    effect: Mutex<Option<lazily::Effect>>,
    sink: Arc<OnceLock<Arc<dyn PaneLayoutProjectionSink>>>,
    next_generation: AtomicU64,
    waiters: Condvar,
    wait_lock: Mutex<()>,
}

impl ControllerPaneLayoutGraph {
    fn new_in(
        scope: &agent_doc_state_scope::ProcessScope,
        persisted_columns: Vec<String>,
        live_actor_bindings: Computed<ControllerActorStore>,
    ) -> Self {
        let ctx = scope.ctx().clone();
        let initial_desired = (!persisted_columns.is_empty()).then(|| PaneLayoutDesired {
            generation: 1,
            source_plane_version: None,
            invocation: ControllerTmuxLayoutSyncInvocation {
                columns: persisted_columns,
                window: None,
                focus: None,
                no_autostart: false,
                exact_visible: true,
                caller_kind: "projection".to_string(),
                actor_bindings: Vec::new(),
            },
        });
        let desired = ctx.source(initial_desired);
        let desired_for_actor_bindings = desired;
        let actor_bindings = ctx.computed(move |ctx| {
            let desired = ctx.get(&desired_for_actor_bindings);
            let actors = ctx.get(&live_actor_bindings);
            derive_layout_actor_bindings(desired.as_ref(), &actors)
        });
        let observed = ctx.source(None);
        let receipt = ctx.source(PaneLayoutEffectReceipt::default());
        let structural_receipt = ctx.source(None);
        let applicable_receipt = ctx.computed(move |ctx| {
            let desired = ctx.get(&desired)?;
            let actor_bindings = ctx.get(&actor_bindings);
            let receipt = ctx.get(&receipt);
            (receipt.generation == desired.generation && receipt.actor_bindings == actor_bindings)
                .then_some(receipt)
        });
        let desired_for_projection = desired;
        let actor_bindings_for_projection = actor_bindings;
        let observed_for_projection = observed;
        let receipt_for_projection = receipt;
        let projection = ctx.computed(move |ctx| {
            derive_pane_layout_projection(
                ctx.get(&desired_for_projection),
                ctx.get(&actor_bindings_for_projection),
                ctx.get(&observed_for_projection),
                ctx.get(&receipt_for_projection),
            )
        });
        let sink: Arc<OnceLock<Arc<dyn PaneLayoutProjectionSink>>> = Arc::new(OnceLock::new());
        let projection_for_effect = projection;
        let actor_bindings_for_effect = actor_bindings;
        let sink_for_effect = Arc::clone(&sink);
        let effect = ctx.effect(move |ctx| {
            let PaneLayoutProjection::NeedsEffect(desired) = ctx.get(&projection_for_effect) else {
                return;
            };
            // Subscribe the effect to actor authority as well as layout
            // applicability. A late actor event wakes the same retained effect;
            // no caller-owned retry or actor RPC is needed.
            let _ = ctx.get(&actor_bindings_for_effect);
            if let Some(sink) = sink_for_effect.get() {
                sink.reconcile(desired);
            }
        });
        Self {
            ctx,
            desired,
            actor_bindings,
            observed,
            receipt,
            structural_receipt,
            applicable_receipt,
            projection,
            effect: Mutex::new(Some(effect)),
            sink,
            next_generation: AtomicU64::new(2),
            waiters: Condvar::new(),
            wait_lock: Mutex::new(()),
        }
    }

    #[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
    fn install_sink(&self, sink: Arc<dyn PaneLayoutProjectionSink>) {
        if self.sink.set(sink).is_ok()
            && let Some(desired) = self.ctx.get(&self.desired)
        {
            // The effect ran once before its late-bound sink existed. Publishing
            // the reconstructed desired fact again invalidates the Computed and
            // lets the retained Effect drive restart recovery.
            self.ctx.set(&self.desired, Some(desired));
        }
    }

    fn set_desired(
        &self,
        mut invocation: ControllerTmuxLayoutSyncInvocation,
        source_plane_version: Option<u64>,
    ) -> PaneLayoutDesired {
        if invocation.caller_kind.is_empty() {
            invocation.caller_kind = "projection".to_string();
        }
        if let Some(mut current) = self.ctx.get(&self.desired)
            && current.invocation == invocation
        {
            if source_plane_version > current.source_plane_version {
                current.source_plane_version = source_plane_version;
            }
            // Republish the identical value without manufacturing a generation.
            // A converged Computed stays inert; a retained NeedsEffect projection
            // can restart its worker if an earlier adapter thread exited.
            self.ctx.set(&self.desired, Some(current.clone()));
            self.waiters.notify_all();
            return current;
        }
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst);
        let desired = PaneLayoutDesired {
            generation,
            source_plane_version,
            invocation,
        };
        self.ctx.batch(|ctx| {
            ctx.set(&self.observed, None);
            ctx.set(
                &self.receipt,
                PaneLayoutEffectReceipt {
                    generation,
                    ..PaneLayoutEffectReceipt::default()
                },
            );
            ctx.set(&self.desired, Some(desired.clone()));
        });
        self.waiters.notify_all();
        desired
    }

    fn actor_bindings(&self) -> Vec<ControllerTmuxActorBinding> {
        self.ctx.get(&self.actor_bindings)
    }

    fn desired(&self) -> Option<PaneLayoutDesired> {
        self.ctx.get(&self.desired)
    }

    fn projection(&self) -> PaneLayoutProjection {
        self.ctx.get(&self.projection)
    }

    fn state_projection(&self) -> Option<ControllerPaneLayoutStateProjection> {
        let desired = self.desired()?;
        let actor_bindings = self.actor_bindings();
        let phase = match self.projection() {
            PaneLayoutProjection::Absent => return None,
            PaneLayoutProjection::NeedsEffect(_) => ControllerPaneLayoutPhase::NeedsEffect,
            PaneLayoutProjection::Applying(_) => ControllerPaneLayoutPhase::Applying,
            PaneLayoutProjection::RetryPending(_) => ControllerPaneLayoutPhase::RetryPending,
            PaneLayoutProjection::Converged(_) => ControllerPaneLayoutPhase::Converged,
        };
        let observation = self
            .ctx
            .get(&self.observed)
            .filter(|observation| {
                observation.generation == desired.generation
                    && observation.actor_bindings == actor_bindings
            })
            .map(|observation| observation.report);
        let receipt = self.ctx.get(&self.receipt);
        let (attempt, reason_detail) = if receipt.generation == desired.generation
            && receipt.actor_bindings == actor_bindings
        {
            (receipt.attempt, Some(receipt.reason))
        } else {
            (0, None)
        };
        let reason_code = match phase {
            ControllerPaneLayoutPhase::Converged => {
                ControllerPaneLayoutReasonCode::ObservedConvergence
            }
            ControllerPaneLayoutPhase::Applying => ControllerPaneLayoutReasonCode::EffectInFlight,
            ControllerPaneLayoutPhase::NeedsEffect => ControllerPaneLayoutReasonCode::Unobserved,
            ControllerPaneLayoutPhase::RetryPending => {
                let observation_reason = observation
                    .as_ref()
                    .map(|report| report.reason.as_str())
                    .unwrap_or_default();
                if observation_reason == "pane_count_mismatch" {
                    ControllerPaneLayoutReasonCode::PaneCountMismatch
                } else if observation_reason == "pane_order_mismatch" {
                    ControllerPaneLayoutReasonCode::PaneOrderMismatch
                } else if matches!(
                    observation_reason,
                    "missing_tmux_session" | "tmux_session_not_alive" | "missing_agent_doc_window"
                ) {
                    ControllerPaneLayoutReasonCode::TmuxUnavailable
                } else if reason_detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains("tmux_effect_failed"))
                {
                    ControllerPaneLayoutReasonCode::EffectFailed
                } else if observation_reason.contains("observation_failed") {
                    ControllerPaneLayoutReasonCode::ObservationFailed
                } else {
                    ControllerPaneLayoutReasonCode::RetryScheduled
                }
            }
        };
        Some(ControllerPaneLayoutStateProjection {
            generation: desired.generation,
            source_plane_version: desired.source_plane_version,
            phase,
            reason_code,
            reason_detail,
            columns: desired.invocation.columns,
            window: desired.invocation.window,
            focus: desired.invocation.focus,
            observation,
            attempt,
        })
    }

    fn record_observation(&self, observation: PaneLayoutObservation) {
        let actor_bindings = self.actor_bindings();
        if !observation.report.synced
            && self
                .desired()
                .is_some_and(|desired| desired.generation == observation.generation)
            && observation.actor_bindings == actor_bindings
        {
            self.ctx.set(&self.structural_receipt, None);
        }
        self.ctx.set(&self.observed, Some(observation));
        self.waiters.notify_all();
    }

    #[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
    fn record_receipt(&self, receipt: PaneLayoutEffectReceipt) {
        self.ctx.set(&self.receipt, receipt);
        self.waiters.notify_all();
    }

    fn effect_file_panes(&self, generation: u64) -> Vec<(String, String)> {
        self.ctx
            .get(&self.applicable_receipt)
            .filter(|receipt| receipt.generation == generation)
            .map(|receipt| receipt.file_panes)
            .unwrap_or_default()
    }

    fn reusable_structural_receipt(
        &self,
        desired: &PaneLayoutDesired,
        actor_bindings: &[ControllerTmuxActorBinding],
    ) -> Option<PaneLayoutStructuralReceipt> {
        self.ctx.get(&self.structural_receipt).filter(|receipt| {
            receipt.structure == PaneLayoutStructure::from(&desired.invocation)
                && receipt.actor_bindings.as_slice() == actor_bindings
                && !receipt.file_panes.is_empty()
        })
    }

    fn record_structural_assignment(
        &self,
        desired: &PaneLayoutDesired,
        actor_bindings: Vec<ControllerTmuxActorBinding>,
        report: Option<ControllerTmuxLayoutSyncStateReport>,
        file_panes: Vec<(String, String)>,
    ) {
        if file_panes.is_empty() {
            return;
        }
        self.ctx.set(
            &self.structural_receipt,
            Some(PaneLayoutStructuralReceipt {
                structure: PaneLayoutStructure::from(&desired.invocation),
                actor_bindings,
                report: report.filter(|report| report.synced),
                file_panes,
            }),
        );
    }

    #[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
    fn await_generation(&self, generation: u64, timeout: Duration) -> PaneLayoutProjection {
        let deadline = Instant::now() + timeout;
        let mut guard = self.wait_lock.lock();
        loop {
            let projection = self.projection();
            let terminal = match &projection {
                PaneLayoutProjection::Converged(desired) => desired.generation == generation,
                PaneLayoutProjection::NeedsEffect(desired)
                | PaneLayoutProjection::Applying(desired)
                | PaneLayoutProjection::RetryPending(desired) => desired.generation != generation,
                PaneLayoutProjection::Absent => true,
            };
            if terminal || Instant::now() >= deadline {
                return projection;
            }
            self.waiters.wait_for(
                &mut guard,
                deadline.saturating_duration_since(Instant::now()),
            );
        }
    }
}

impl Drop for ControllerPaneLayoutGraph {
    fn drop(&mut self) {
        if let Some(effect) = self.effect.get_mut().take() {
            self.ctx.dispose_effect(&effect);
        }
    }
}

pub(crate) trait ControllerStatePlaneSink: Send + Sync + 'static {
    /// Projection delivery is at-least-once. Consumers fence on
    /// `frame.plane_version`, then publish their own observed/effect state.
    fn project(&self, frame: ControllerStatePlaneFrame);
}

/// Generic controller-lifetime cross-process Lazily state plane.
///
/// RPC is only the byte transport. Accepted Lazily graph messages become a
/// Source; a retained Effect projects the latest frame into registered domain
/// adapters. A covering Snapshot replaces prior history, while subsequent
/// Deltas are retained until the next Snapshot. This gives cold subscribers a
/// valid replay base and lets warm subscribers resume from `plane_version`.
struct ControllerStatePlaneGraph {
    ctx: ThreadSafeContext,
    /// Per-channel retained history. A subscriber to one authority document
    /// must never clone or depend on every other channel's frames.
    histories: lazily::ThreadSafeSourceMap<String, Vec<ControllerStatePlaneFrame>>,
    channel_effects: Mutex<BTreeMap<String, lazily::Effect>>,
    channel_dependencies: Arc<Mutex<BTreeMap<String, Arc<ControllerStatePlaneChannelDependency>>>>,
    sinks: Arc<Mutex<BTreeMap<String, Arc<dyn ControllerStatePlaneSink>>>>,
    next_plane_version: AtomicU64,
}

struct ControllerStatePlaneChannelDependency {
    changed: Condvar,
    change_lock: Mutex<()>,
    revision: AtomicU64,
}

impl ControllerStatePlaneChannelDependency {
    fn new() -> Self {
        Self {
            changed: Condvar::new(),
            change_lock: Mutex::new(()),
            revision: AtomicU64::new(0),
        }
    }

    fn project_change(&self) {
        let _guard = self.change_lock.lock();
        self.revision.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_all();
    }
}

impl ControllerStatePlaneGraph {
    #[cfg(test)]
    fn new_in(scope: &agent_doc_state_scope::ProcessScope) -> Self {
        Self::new_in_with_first_version(scope, 1)
    }

    fn new_in_with_first_version(
        scope: &agent_doc_state_scope::ProcessScope,
        first_plane_version: u64,
    ) -> Self {
        let ctx = scope.ctx().clone();
        let histories = lazily::ThreadSafeSourceMap::new(&ctx);
        let sinks: Arc<Mutex<BTreeMap<String, Arc<dyn ControllerStatePlaneSink>>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        Self {
            ctx,
            histories,
            channel_effects: Mutex::new(BTreeMap::new()),
            channel_dependencies: Arc::new(Mutex::new(BTreeMap::new())),
            sinks,
            next_plane_version: AtomicU64::new(first_plane_version.max(1)),
        }
    }

    fn install_sink(&self, channel: &str, sink: Arc<dyn ControllerStatePlaneSink>) {
        self.sinks.lock().insert(channel.to_string(), sink);
        let channel = channel.to_string();
        let mut effects = self.channel_effects.lock();
        if effects.contains_key(&channel) {
            drop(effects);
            if let Some(frame) = self
                .histories
                .observe(&self.ctx, &channel)
                .and_then(|frames| frames.last().cloned())
                && let Some(sink) = self.sinks.lock().get(&channel).cloned()
            {
                sink.project(frame);
            }
            return;
        }
        let history = self
            .histories
            .get_or_insert_handle(&self.ctx, channel.clone(), |_, _| Vec::new());
        let dependency = self.channel_dependency(&channel);
        let sinks = Arc::clone(&self.sinks);
        let effect_channel = channel.clone();
        let effect = self.ctx.effect(move |ctx| {
            let frame = ctx.get(&history).last().cloned();
            dependency.project_change();
            let Some(frame) = frame else {
                return;
            };
            let sink = sinks.lock().get(&effect_channel).cloned();
            if let Some(sink) = sink {
                sink.project(frame);
            }
        });
        effects.insert(channel, effect);
    }

    fn channel_dependency(&self, channel: &str) -> Arc<ControllerStatePlaneChannelDependency> {
        self.channel_dependencies
            .lock()
            .entry(channel.to_string())
            .or_insert_with(|| Arc::new(ControllerStatePlaneChannelDependency::new()))
            .clone()
    }

    fn ensure_channel_effect(&self, channel: &str) {
        let channel = channel.to_string();
        let mut effects = self.channel_effects.lock();
        if effects.contains_key(&channel) {
            return;
        }
        let history = self
            .histories
            .get_or_insert_handle(&self.ctx, channel.clone(), |_, _| Vec::new());
        let dependency = self.channel_dependency(&channel);
        let sinks = Arc::clone(&self.sinks);
        let effect_channel = channel.clone();
        let effect = self.ctx.effect(move |ctx| {
            let frame = ctx.get(&history).last().cloned();
            dependency.project_change();
            let Some(frame) = frame else {
                return;
            };
            if let Some(sink) = sinks.lock().get(&effect_channel).cloned() {
                sink.project(frame);
            }
        });
        effects.insert(channel, effect);
    }

    fn retire_channel(&self, channel: &str) {
        if let Some(effect) = self.channel_effects.lock().remove(channel) {
            self.ctx.dispose_effect(&effect);
        }
        self.sinks.lock().remove(channel);
        let channel_key = channel.to_string();
        self.histories.remove(&self.ctx, &channel_key);
        if let Some(dependency) = self.channel_dependencies.lock().remove(channel) {
            dependency.project_change();
        }
    }

    fn publish(
        &self,
        channel: String,
        producer_id: String,
        message_json: String,
        epoch: u64,
        base_epoch: Option<u64>,
    ) -> Result<(ControllerStatePlaneFrame, bool)> {
        anyhow::ensure!(!channel.trim().is_empty(), "state-plane channel is empty");
        anyhow::ensure!(
            !producer_id.trim().is_empty(),
            "state-plane producer_id is empty"
        );
        anyhow::ensure!(epoch > 0, "state-plane epoch must be positive");
        anyhow::ensure!(
            message_json.len() <= STATE_PLANE_MAX_MESSAGE_BYTES,
            "state-plane message exceeds {} bytes",
            STATE_PLANE_MAX_MESSAGE_BYTES
        );

        if !self.histories.is_present(&channel) {
            let authority_channel = channel.starts_with(DOCUMENT_AUTHORITY_STATE_CHANNEL_PREFIX);
            let used = self
                .histories
                .present_keys()
                .into_iter()
                .filter(|existing| {
                    existing.starts_with(DOCUMENT_AUTHORITY_STATE_CHANNEL_PREFIX)
                        == authority_channel
                })
                .count();
            let capacity = if authority_channel {
                DOCUMENT_AUTHORITY_MAX_CHANNELS
            } else {
                STATE_PLANE_MAX_CHANNELS
            };
            anyhow::ensure!(
                used < capacity,
                "{} state-plane channel capacity reached ({capacity}); close documents or retire an existing channel",
                if authority_channel {
                    "document-authority"
                } else {
                    "generic"
                }
            );
        }
        let mut history = self
            .histories
            .observe(&self.ctx, &channel)
            .unwrap_or_default();
        anyhow::ensure!(
            base_epoch.is_none() || history.len() < STATE_PLANE_MAX_RETAINED_FRAMES_PER_CHANNEL,
            "state-plane delta retention capacity reached; publish a covering Snapshot"
        );
        let latest = history.last();
        if let Some(latest) = latest
            && latest.producer_id == producer_id
            && latest.epoch == epoch
            && latest.message_json == message_json
        {
            return Ok((latest.clone(), true));
        }

        match (latest, base_epoch) {
            (Some(latest), Some(base)) => {
                anyhow::ensure!(
                    latest.producer_id == producer_id,
                    "state-plane Delta producer changed without a covering Snapshot"
                );
                anyhow::ensure!(
                    base == latest.epoch,
                    "state-plane Delta base_epoch mismatch: expected {}, got {base}",
                    latest.epoch
                );
                anyhow::ensure!(
                    epoch > latest.epoch,
                    "state-plane epoch did not advance: current={}, incoming={epoch}",
                    latest.epoch
                );
            }
            (Some(latest), None) if latest.producer_id == producer_id => {
                anyhow::ensure!(
                    epoch > latest.epoch,
                    "state-plane epoch did not advance: current={}, incoming={epoch}",
                    latest.epoch
                );
            }
            (_, None) => {
                // A Snapshot is a covering state cut. A new producer may claim
                // the channel by publishing one; no stale producer epoch leaks
                // across process restarts.
                history.clear();
            }
            (None, Some(_)) => {
                anyhow::bail!("state-plane Delta has no covering Snapshot");
            }
        }

        if base_epoch.is_none() {
            history.clear();
        }
        let plane_version = self.next_plane_version.fetch_add(1, Ordering::SeqCst);
        let frame = ControllerStatePlaneFrame {
            channel: channel.clone(),
            producer_id,
            epoch,
            base_epoch,
            plane_version,
            message_json,
        };
        history.push(frame.clone());
        self.histories.set(&self.ctx, channel, history);
        Ok((frame, false))
    }

    fn subscribe(
        &self,
        channel: &str,
        after_version: u64,
        reset_generationless_ahead_cursor: bool,
        timeout: Duration,
    ) -> ControllerStatePlaneSubscription {
        let deadline = Instant::now() + timeout;
        self.ensure_channel_effect(channel);
        let dependency = self.channel_dependency(channel);
        let mut guard = dependency.change_lock.lock();
        loop {
            let history = self
                .histories
                .observe(&self.ctx, &channel.to_string())
                .unwrap_or_default();
            let latest_version = history.last().map(|frame| frame.plane_version).unwrap_or(0);
            // A legacy subscriber cannot name the controller generation. If
            // its cursor is ahead of a replacement controller's non-empty
            // channel frontier, it is necessarily outside this cursor
            // namespace: plane versions are monotonic within one controller.
            // Cold replay now. Returning an empty timeout and lowering the
            // client cursor would skip the state edge that resumes the exact
            // retained operation.
            let effective_after_version = if reset_generationless_ahead_cursor
                && latest_version > 0
                && after_version > latest_version
            {
                0
            } else {
                after_version
            };
            if latest_version > effective_after_version || Instant::now() >= deadline {
                let frames = if effective_after_version == 0 {
                    history
                } else {
                    history
                        .into_iter()
                        .filter(|frame| frame.plane_version > effective_after_version)
                        .collect()
                };
                return ControllerStatePlaneSubscription {
                    channel: channel.to_string(),
                    controller_generation: 0,
                    latest_version,
                    timed_out: latest_version <= effective_after_version,
                    frames,
                };
            }
            dependency.changed.wait_for(
                &mut guard,
                deadline.saturating_duration_since(Instant::now()),
            );
        }
    }
}

/// Give pre-generation-aware subscribers a monotonic compatibility cursor
/// across controller replacement. New subscribers also carry the generation
/// explicitly, so this namespace is a rolling-upgrade bridge rather than the
/// sole correctness proof.
fn state_plane_first_version(controller_generation: u64) -> u64 {
    let generation = controller_generation.min(u32::MAX as u64);
    (generation << STATE_PLANE_VERSION_NAMESPACE_BITS).saturating_add(1)
}

fn state_plane_effective_after_version(
    after_controller_generation: Option<u64>,
    after_version: u64,
    controller_generation: u64,
) -> u64 {
    match after_controller_generation {
        Some(observed) if observed != controller_generation => 0,
        _ => after_version,
    }
}

impl Drop for ControllerStatePlaneGraph {
    fn drop(&mut self) {
        for effect in self.channel_effects.get_mut().values() {
            self.ctx.dispose_effect(effect);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerRouteAutoStartPolicy {
    WaitForReady,
    ProvisionOnly,
}

pub struct ControllerRouteAutoStartInvocation<'a> {
    pub tmux: &'a tmux_router::Tmux,
    pub file: &'a Path,
    pub session_id: &'a str,
    pub file_arg: &'a str,
    pub window: Option<&'a str>,
    pub policy: ControllerRouteAutoStartPolicy,
    pub resume: Option<agent_doc_harness::ResumeRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerAnsweredFreeTextStrikeInvocation {
    pub file: PathBuf,
    pub expected_content: String,
    pub target_content: String,
    pub projection_id: String,
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
    pub baseline_content: Option<String>,
    pub node_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerCompactProjectionCompletion {
    pub file: PathBuf,
    pub live_content: String,
    pub committed_content: String,
    pub target_component: Option<String>,
    pub commit: bool,
}

pub trait ProjectControllerRuntimeEffects: Send + Sync + 'static {
    fn consume_queue_prompt_force_disk(
        &self,
        file: &Path,
    ) -> Result<Option<ControllerQueueConsumptionOutcome>>;

    fn route_auto_start(
        &self,
        invocation: ControllerRouteAutoStartInvocation<'_>,
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

    /// Queue an exact answered-free-text target on the document's owner thread.
    ///
    /// This port must return after admission, not after editor/CRDT convergence:
    /// the controller Effect thread only projects state and must remain
    /// responsive while the per-document worker applies the target.
    fn project_answered_free_text_strike(
        &self,
        invocation: ControllerAnsweredFreeTextStrikeInvocation,
    ) -> Result<()>;

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

    fn compact_document(
        &self,
        file: &Path,
        invocation: ControllerCompactDocumentInvocation,
    ) -> Result<()>;

    /// Apply the snapshot/commit Effect selected by a durable compact
    /// continuation after the document projection clears its retained write.
    fn complete_compact_projection(
        &self,
        invocation: ControllerCompactProjectionCompletion,
    ) -> Result<String>;
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
        _invocation: ControllerRouteAutoStartInvocation<'_>,
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

    fn compact_document(
        &self,
        _file: &Path,
        _invocation: ControllerCompactDocumentInvocation,
    ) -> Result<()> {
        anyhow::bail!("project controller test runtime does not compact documents")
    }

    fn complete_compact_projection(
        &self,
        invocation: ControllerCompactProjectionCompletion,
    ) -> Result<String> {
        Ok(agent_doc_hash::content_hash(&invocation.live_content))
    }

    fn sync_tmux_layout(
        &self,
        _project_root: &Path,
        invocation: ControllerTmuxLayoutSyncInvocation,
    ) -> Result<ControllerTmuxLayoutSyncReceipt> {
        let routes_created_panes = invocation.routes_created_panes();
        Ok(ControllerTmuxLayoutSyncReceipt {
            applied: true,
            reason: "test_runtime".to_string(),
            columns: invocation.columns,
            window: invocation.window,
            focus: invocation.focus,
            no_autostart: invocation.no_autostart,
            exact_visible: invocation.exact_visible,
            routes_created_panes,
            file_panes: Vec::new(),
        })
    }

    fn project_answered_free_text_strike(
        &self,
        invocation: ControllerAnsweredFreeTextStrikeInvocation,
    ) -> Result<()> {
        let current = std::fs::read_to_string(&invocation.file)
            .context("project controller test free-text strike: failed to read document")?;
        anyhow::ensure!(
            current == invocation.expected_content,
            "project controller test free-text strike: authority changed"
        );
        std::fs::write(&invocation.file, &invocation.target_content)
            .context("project controller test free-text strike: failed to write document")
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
    state_ledger: agent_doc_state_backbone::EventLedger,
    /// Durable high-water captured from the exact rows folded into
    /// `state_ledger`. This must travel with the in-memory snapshot: reading a
    /// later SQLite MAX could acknowledge a concurrently appended event that
    /// the outgoing graph does not contain.
    state_document_versions: BTreeMap<String, u64>,
    state_projection: agent_doc_state_backbone::StateBackboneProjection,
    map_backend: &'static str,
}

impl ControllerMemoryState {
    fn load(project_root: &Path) -> Result<(Self, ControllerActorStore)> {
        let (state_ledger, state_document_versions) =
            load_state_event_ledger_with_versions(project_root)?;
        let state_projection = state_ledger.project();
        Ok((
            Self {
                state_ledger,
                state_document_versions,
                state_projection,
                map_backend: "std_btree_map",
            },
            load_actor_store(project_root)?,
        ))
    }
}

pub(crate) struct ControllerRuntime {
    bootstrap: Mutex<ControllerBootstrap>,
    memory: Mutex<ControllerMemoryState>,
    actor_graph: ControllerActorGraph,
    document_authority_graph: ControllerDocumentAuthorityGraph,
    coordination_graph: ControllerCoordinationGraph,
    supervisor_recycle_graph: ControllerSupervisorRecycleGraph,
    state_plane_graph: ControllerStatePlaneGraph,
    captured_finalize_wakes: Mutex<BTreeMap<String, rpc::CapturedFinalizeWakeProjection>>,
    pane_layout_graph: ControllerPaneLayoutGraph,
    /// Editor facts, history-dependent intent, and tmux consequences share the
    /// controller ProcessScope. Editors retain only transport/projection caches.
    editor_surface_graph: rpc::ControllerEditorSurfaceGraph,
    /// Retained old-path → new-path observations and their convergence
    /// receipts. Requests carry effects; this projection owns rename truth.
    document_path_transition_graph: rpc::ControllerDocumentPathTransitionGraph,
    /// Controller-owned reactive projection for accepted editor commands.
    ///
    /// The command worker writes accepted/terminal states into one SourceMap.
    /// Status reads and event-driven awaits consume that same projection; there
    /// is no client polling cache beside it.
    async_editor_commands: ControllerAsyncEditorCommandGraph,
    supervisor_recycle_waiters: Condvar,
    /// Serialize editor-op epoch read/derive/append transitions. The checkpoint
    /// fact contains the complete epoch, so two concurrent IDE callbacks must
    /// not derive from the same predecessor and overwrite one another.
    editor_op_capture_writes: Mutex<()>,
    /// `#lazily-hot-path` W1 — notified whenever the in-memory state projection
    /// advances (`apply_state_event` / `refresh_memory`). Bounded awaits park here
    /// instead of making every waiter re-poll the projection on its own timer, so
    /// one fact producer publishes once and all waiters react.
    state_projection_waiters: Condvar,
    /// `#ctlrecycle` R2 — set true by the `recycle` RPC (`agent-doc admin recycle`).
    /// The serve-loop idle poll honors it the same way it honors binary staleness:
    /// once no dispatch is in flight (debounced), the controller self-terminates and
    /// the next `connect_or_launch` relaunches the fresh binary.
    recycle_requested: AtomicBool,
    /// A rolling-upgrade incompatibility observed by a read-only recovery RPC.
    /// Unlike the normal five-second recycle debounce, this may recycle at the
    /// first exact idle cut. It still never stops while a client or durable
    /// dispatch is active.
    recycle_urgent: AtomicBool,
    /// `#recycleforce` — set true by the `recycle_force` RPC (`agent-doc admin
    /// recycle --force`). An explicit operator override: the serve-loop idle poll
    /// recycles WITHOUT waiting on the in-flight-dispatch idle gate, so a forced
    /// recycle takes effect at the next tick even mid-turn. Implies
    /// `recycle_requested`.
    recycle_forced: AtomicBool,
    /// `#stategraphjoin` / `#retainedsettlereactive` — one reactive graph per
    /// open document.
    ///
    /// `supervisor_recycle_graph` above is process-scoped because there is one
    /// supervisor. Retained-write settlement is **per document**, so each
    /// document owns a [`DocumentScope`](agent_doc_state_scope::DocumentScope)
    /// whose drop takes that document's cells with it.
    ///
    /// The point of the registry is lifetime. Before it, callers built a scope,
    /// set observations, read the verdict, and dropped the whole graph inside
    /// one function call — which is "constructing a whole context to answer one
    /// comparison", strictly worse than the comparison. Here the graph outlives
    /// the call, `apply_state_event` pushes facts into it, and the verdict
    /// updates because a fact arrived rather than because a caller remembered to
    /// reload SQLite and recompute.
    document_graphs: ControllerDocumentGraphs,
    /// `#stategraphjoin` — the controller process's reactive scope.
    ///
    /// Every graph below is built in this one scope, so controller-lifetime facts
    /// (claims, supervisor recycle) live in a single graph that can derive across
    /// them, instead of one private context per struct. Dropping the runtime drops
    /// the scope and every cell in it — teardown is the scope's lifetime.
    _scope: agent_doc_state_scope::ProcessScope,
}

const ASYNC_EDITOR_COMMAND_RESULT_TTL: Duration = Duration::from_secs(5 * 60);
const ASYNC_EDITOR_COMMAND_RESULT_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncEditorCommandPhase {
    Accepted,
    Terminal,
}

#[derive(Clone, Debug, PartialEq)]
struct AsyncEditorCommandProjection {
    response: serde_json::Value,
    phase: AsyncEditorCommandPhase,
    updated_at: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AsyncEditorFocusFence {
    pub(crate) project_root: PathBuf,
    pub(crate) command_id: String,
    pub(crate) expires_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AsyncEditorFocusFenceDecision {
    Current,
    Superseded,
    Expired,
}

/// Process-scoped command completion plane for editor gestures.
///
/// The Lazily SourceMaps are authoritative for completion and latest-focus
/// intent. The condition variable and effect gate carry no state: they only
/// park socket waiters and serialize the final external focus effect.
struct ControllerAsyncEditorCommandGraph {
    ctx: ThreadSafeContext,
    projections: lazily::ThreadSafeSourceMap<String, AsyncEditorCommandProjection>,
    focus_fences: lazily::ThreadSafeSourceMap<PathBuf, AsyncEditorFocusFence>,
    transition_gate: Mutex<()>,
    transition: Condvar,
    focus_effect_gate: Mutex<()>,
}

impl ControllerAsyncEditorCommandGraph {
    fn new_in(scope: &agent_doc_state_scope::ProcessScope) -> Self {
        Self {
            ctx: scope.ctx().clone(),
            projections: lazily::ThreadSafeSourceMap::new(scope.ctx()),
            focus_fences: lazily::ThreadSafeSourceMap::new(scope.ctx()),
            transition_gate: Mutex::new(()),
            transition: Condvar::new(),
            focus_effect_gate: Mutex::new(()),
        }
    }

    fn prune_expired_locked(&self, now: Instant) {
        for command_id in self.projections.present_keys() {
            let expired = self
                .projections
                .observe(&self.ctx, &command_id)
                .is_some_and(|projection| {
                    now.duration_since(projection.updated_at) > ASYNC_EDITOR_COMMAND_RESULT_TTL
                });
            if expired {
                self.projections.remove(&self.ctx, &command_id);
            }
        }
    }

    fn publish(
        &self,
        command_id: impl Into<String>,
        phase: AsyncEditorCommandPhase,
        response: serde_json::Value,
    ) {
        let command_id = command_id.into();
        let _transition = self.transition_gate.lock();
        let now = Instant::now();
        self.prune_expired_locked(now);
        if self.projections.present_count() >= ASYNC_EDITOR_COMMAND_RESULT_CAPACITY
            && !self.projections.is_present(&command_id)
            && let Some(oldest) = self
                .projections
                .present_keys()
                .into_iter()
                .filter_map(|candidate| {
                    self.projections
                        .observe(&self.ctx, &candidate)
                        .map(|projection| (candidate, projection.updated_at))
                })
                .min_by_key(|(_, updated_at)| *updated_at)
                .map(|(candidate, _)| candidate)
        {
            self.projections.remove(&self.ctx, &oldest);
        }
        self.projections.set(
            &self.ctx,
            command_id,
            AsyncEditorCommandProjection {
                response,
                phase,
                updated_at: now,
            },
        );
        self.transition.notify_all();
    }

    fn current(&self, command_id: &str) -> Option<serde_json::Value> {
        let projection = self
            .projections
            .observe(&self.ctx, &command_id.to_string())?;
        (projection.updated_at.elapsed() <= ASYNC_EDITOR_COMMAND_RESULT_TTL)
            .then_some(projection.response)
    }

    fn await_terminal(&self, command_id: &str, timeout: Duration) -> Result<serde_json::Value> {
        let deadline = Instant::now() + timeout;
        let command_id = command_id.to_string();
        let mut transition = self.transition_gate.lock();
        loop {
            self.prune_expired_locked(Instant::now());
            let projection = self.projections.observe(&self.ctx, &command_id);
            let Some(projection) = projection else {
                anyhow::bail!("unknown or expired async editor command: {command_id}");
            };
            if projection.phase == AsyncEditorCommandPhase::Terminal {
                return Ok(projection.response);
            }
            let now = Instant::now();
            if now >= deadline {
                anyhow::bail!(
                    "async editor command {command_id} did not publish a terminal projection within {}ms",
                    timeout.as_millis()
                );
            }
            self.transition.wait_for(&mut transition, deadline - now);
        }
    }

    fn remove(&self, command_id: &str) {
        let _transition = self.transition_gate.lock();
        self.projections.remove(&self.ctx, &command_id.to_string());
        self.transition.notify_all();
    }

    fn publish_focus_fence(&self, fence: AsyncEditorFocusFence) {
        let _effect = self.focus_effect_gate.lock();
        self.focus_fences
            .set(&self.ctx, fence.project_root.clone(), fence);
    }

    fn focus_fence_decision(&self, fence: &AsyncEditorFocusFence) -> AsyncEditorFocusFenceDecision {
        let current_command_id = self
            .focus_fences
            .observe(&self.ctx, &fence.project_root)
            .map(|current| current.command_id);
        if current_command_id.as_deref() != Some(fence.command_id.as_str()) {
            return AsyncEditorFocusFenceDecision::Superseded;
        }
        if fence
            .expires_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return AsyncEditorFocusFenceDecision::Expired;
        }
        AsyncEditorFocusFenceDecision::Current
    }

    fn require_focus_fence_current(&self, fence: Option<&AsyncEditorFocusFence>) -> Result<()> {
        let Some(fence) = fence else {
            return Ok(());
        };
        let decision = self.focus_fence_decision(fence);
        anyhow::ensure!(
            decision == AsyncEditorFocusFenceDecision::Current,
            "{}",
            decision.reason()
        );
        Ok(())
    }

    fn apply_focus_effect<T>(
        &self,
        fence: Option<&AsyncEditorFocusFence>,
        effect: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let Some(fence) = fence else {
            return effect();
        };
        let _effect = self.focus_effect_gate.lock();
        let decision = self.focus_fence_decision(fence);
        anyhow::ensure!(
            decision == AsyncEditorFocusFenceDecision::Current,
            "{}",
            decision.reason()
        );
        effect()
    }

    fn release_focus_fence(&self, fence: Option<&AsyncEditorFocusFence>) {
        let Some(fence) = fence else {
            return;
        };
        let _effect = self.focus_effect_gate.lock();
        if self
            .focus_fences
            .observe(&self.ctx, &fence.project_root)
            .is_some_and(|current| current.command_id == fence.command_id)
        {
            self.focus_fences.remove(&self.ctx, &fence.project_root);
        }
    }
}

impl AsyncEditorFocusFenceDecision {
    fn reason(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Superseded => "superseded_focus_intent",
            Self::Expired => "expired_focus_intent",
        }
    }
}

/// Per-document retained-write settlement as a **keyed reactive collection**
/// (`#reactivemap`, `#retainedsettlereactive`).
///
/// The first draft of this was a `Mutex<HashMap<String, DocumentGraph>>` whose
/// entries each held their own `DocumentScope`. The entries were reactive but
/// the registry was not: membership was imperative (`or_insert_with` on whoever
/// touched it first), nothing could derive across documents, and — because no
/// path ever removed a key — the "dropping the scope is the teardown" claim it
/// carried was simply not true.
///
/// A keyed reactive map is the shape that actually holds: `document_hash` is the
/// key dimension, the three observations are per-entry input cells, and the
/// verdict is a per-entry derived slot over them. Membership is the map's
/// present set rather than a side table, and a closed document is the value
/// `None` rather than a leaked graph.
struct ControllerDocumentGraphs {
    ctx: ThreadSafeContext,
    /// The fact seam: the document's state projection, set once per applied
    /// state event. Everything retained-write-shaped is *derived* from this.
    projection: lazily::ThreadSafeSourceMap<
        String,
        Option<agent_doc_state_backbone::DocumentStateProjection>,
    >,
    /// `#closeoutterminalreactive`: the durable closeout facts and observed
    /// clock edge are keyed controller sources. The timer may wake a waiter,
    /// but only the Computed gate decides whether the incumbent still blocks.
    closeout_cycle_id: lazily::ThreadSafeSourceMap<String, Option<String>>,
    closeout_owner: lazily::ThreadSafeSourceMap<
        String,
        Option<agent_doc_state_backbone::CloseoutOwnerProjection>,
    >,
    closeout_now_secs: lazily::ThreadSafeSourceMap<String, u64>,
    closeout_gate: lazily::ThreadSafeComputedMap<
        String,
        agent_doc_state_backbone::closeout_gate::CloseoutGate,
    >,
    /// Derived from [`Self::projection`] — **not** pushed.
    ///
    /// This was a cell map that `apply_state_event` computed and wrote into. A
    /// pushed value is only correct while every writer remembers to push it, and
    /// it forces the derivation to live at the call site rather than beside the
    /// data. As a derived slot it updates because the projection changed, and
    /// any other consumer can derive from the same projection cell instead of
    /// re-implementing `retained_intent_facts_from_projection` at its own seam.
    pending: lazily::ThreadSafeComputedMap<
        String,
        Option<agent_doc_state_backbone::retained_write::RetainedIntentFacts>,
    >,
    authority: lazily::ThreadSafeSourceMap<
        String,
        Option<agent_doc_state_backbone::retained_write::ContentObservation>,
    >,
    disk: lazily::ThreadSafeSourceMap<
        String,
        Option<agent_doc_state_backbone::retained_write::ContentObservation>,
    >,
    /// External CRDT/editor delivery ingress. Replica RPCs project their
    /// post-event relay state into this Source; retained closeout wake
    /// eligibility is derived from it beside the durable document projection.
    retained_delivery: lazily::ThreadSafeSourceMap<String, Option<RetainedDeliveryObservation>>,
    /// Controller-generation activation edge. A reconstructed graph first
    /// hydrates its durable inputs, then the sink installation advances this
    /// Source so already-satisfied effects rerun with a live durable sink.
    settle_generation: lazily::ThreadSafeSourceMap<String, u64>,
    verdict: lazily::ThreadSafeComputedMap<
        String,
        agent_doc_state_backbone::retained_write::SettlementVerdict,
    >,
    /// Exhaustive state of one retained Base -> Target transition. Durable
    /// transition facts, current visible delivery, and controller activation
    /// are Sources; this one Computed is the decision plane.
    retained_transition_state: lazily::ThreadSafeComputedMap<String, RetainedTransitionState>,
    /// The effect-bearing projection of [`Self::retained_transition_state`].
    /// Waiting and conflict states deliberately project no effect.
    retained_transition_effect:
        lazily::ThreadSafeComputedMap<String, Option<RetainedTransitionEffect>>,
    /// Controller-local record of the exact successfully published effect
    /// frontier. Delivery version and controller generation are part of the
    /// typed identity, so a changed frontier can retry while an unchanged one
    /// stays one-shot.
    retained_transition_published_frontier:
        lazily::ThreadSafeSourceMap<String, Option<RetainedTransitionEffect>>,
    /// Durable compact continuation whose retained document write has reached
    /// the projected authority+disk fixed point.
    compact_resume: lazily::ThreadSafeComputedMap<
        String,
        Option<agent_doc_state_backbone::DocumentCompactProjectionContinuation>,
    >,
    /// Controller-local receipt for the exact compact continuation Effect.
    compact_resume_applied: lazily::ThreadSafeSourceMap<String, Option<String>>,
    /// Authoritative markdown is ingress state. Terminal queue lifecycle facts
    /// are derived beside the durable document projection instead of being
    /// recorded by whichever mutation path happened to observe a strike.
    queue_authority: lazily::ThreadSafeSourceMap<String, Option<QueueAuthorityObservation>>,
    /// Captured response + authoritative markdown -> exact free-text queue
    /// strike target. This is the decision plane that replaces write/commit
    /// callbacks which used to attempt the mutation once and discard failure.
    answered_free_text_strike:
        lazily::ThreadSafeComputedMap<String, AnsweredFreeTextQueueStrikeProjection>,
    /// Controller-local admission receipt for the exact projected target. The
    /// per-document actor owns the potentially slow convergence work.
    answered_free_text_strike_submitted: lazily::ThreadSafeSourceMap<String, Option<String>>,
    queue_completion: lazily::ThreadSafeComputedMap<String, QueueCompletionProjection>,
    /// `#preflightreactive`: per-document read observations and the shared
    /// Computed projection consumed by the short-lived preflight CLI process.
    preflight_facts: lazily::ThreadSafeSourceMap<
        String,
        Option<agent_doc_state_backbone::preflight::PreflightReadFacts>,
    >,
    preflight_projection: lazily::ThreadSafeComputedMap<
        String,
        agent_doc_state_backbone::preflight::PreflightReadProjection,
    >,
    /// `#retainedclearreactive` — one settle [`lazily::Effect`] per document,
    /// subscribed to that document's [`Self::verdict`] slot.
    ///
    /// Holding the handles is what keeps the effects alive; nothing reads this
    /// map for its values. Minting is idempotent, so the effect exists from the
    /// first verdict query onward and fires whenever the slot changes.
    settle_effects: Mutex<BTreeMap<String, lazily::Effect>>,
    /// One retained-transition Effect per document. Held for the controller
    /// lifetime so every Source change is reduced by the same state machine.
    retained_transition_effects: Mutex<BTreeMap<String, lazily::Effect>>,
    /// One durable compact-completion Effect per document.
    compact_resume_effects: Mutex<BTreeMap<String, lazily::Effect>>,
    /// One answered-free-text projection Effect per document.
    answered_free_text_strike_effects: Mutex<BTreeMap<String, lazily::Effect>>,
    /// One durable queue-completion projection Effect per document.
    queue_completion_effects: Mutex<BTreeMap<String, lazily::Effect>>,
    /// Where a `Satisfied` verdict's clear is written.
    ///
    /// Installed once, after the runtime is in its `Arc` — the effect must reach
    /// the runtime and the runtime owns this graph, so the reference has to be
    /// weak and late-bound. An uninstalled sink makes every effect a logged
    /// no-op rather than a panic (test runtimes construct the graph without one).
    settle_sink: Arc<OnceLock<RetainedWriteSettleSink>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DocumentAuthorityKey {
    document_hash: String,
    document_id: String,
}

/// Controller-owned current-document authority.
///
/// The actor model is a process Source and the closeout document projection is
/// a per-document SourceMap. Their join is a ComputedMap, so neither editor
/// polling nor SQLite reads participate in the hot path.
struct ControllerDocumentAuthorityGraph {
    ctx: ThreadSafeContext,
    document_model_states:
        lazily::ThreadSafeSourceMap<String, Option<agent_doc_controller::actor::ActorState>>,
    document_projections: lazily::ThreadSafeSourceMap<
        String,
        Option<agent_doc_state_backbone::DocumentStateProjection>,
    >,
    projections: lazily::ThreadSafeComputedMap<
        DocumentAuthorityKey,
        agent_doc_turn::cp_projection::TurnProjection,
    >,
    effects: Mutex<BTreeMap<DocumentAuthorityKey, (lazily::Effect, usize)>>,
    runtime: Arc<OnceLock<std::sync::Weak<ControllerRuntime>>>,
    next_epoch: Arc<AtomicU64>,
}

impl ControllerDocumentAuthorityGraph {
    fn new_in(
        scope: &agent_doc_state_scope::ProcessScope,
        document_model_states: lazily::ThreadSafeSourceMap<
            String,
            Option<agent_doc_controller::actor::ActorState>,
        >,
        document_projections: lazily::ThreadSafeSourceMap<
            String,
            Option<agent_doc_state_backbone::DocumentStateProjection>,
        >,
    ) -> Self {
        Self {
            ctx: scope.ctx().clone(),
            document_model_states,
            document_projections,
            projections: lazily::ThreadSafeComputedMap::new(scope.ctx()),
            effects: Mutex::new(BTreeMap::new()),
            runtime: Arc::new(OnceLock::new()),
            next_epoch: Arc::new(AtomicU64::new(1)),
        }
    }

    fn projection_handle(
        &self,
        document_hash: &str,
        document_id: &str,
    ) -> Computed<agent_doc_turn::cp_projection::TurnProjection> {
        // Materialize the dependency reactive itself before the authority
        // Computed. `None` is the explicit unavailable state; the first real
        // projection is then an ordinary Source transition on this exact key.
        self.document_projections.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            |_, _| None,
        );
        self.document_model_states.get_or_insert_with(
            &self.ctx,
            document_id.to_string(),
            |_, _| None,
        );
        let document_model_states = self.document_model_states.clone();
        let document_projections = self.document_projections.clone();
        self.projections.get_or_insert_handle(
            &self.ctx,
            DocumentAuthorityKey {
                document_hash: document_hash.to_string(),
                document_id: document_id.to_string(),
            },
            move |ctx, key| {
                let model_state = document_model_states
                    .observe(ctx, &key.document_id)
                    .flatten();
                let closeout = document_projections
                    .observe(ctx, &key.document_hash)
                    .flatten()
                    .map(|document| document.closeout)
                    .unwrap_or_default();
                project_document_turn_authority(model_state, &closeout)
            },
        )
    }

    fn projection(
        &self,
        document_hash: &str,
        document_id: &str,
    ) -> agent_doc_turn::cp_projection::TurnProjection {
        self.ctx
            .get(&self.projection_handle(document_hash, document_id))
    }

    fn install_runtime(&self, runtime: &Arc<ControllerRuntime>) {
        let _ = self.runtime.set(Arc::downgrade(runtime));
    }

    /// Materialize the per-document Computed and retain one shared Effect while
    /// at least one persistent editor stream observes it.
    fn acquire_subscription(&self, document_hash: &str, document_id: &str) {
        let key = DocumentAuthorityKey {
            document_hash: document_hash.to_string(),
            document_id: document_id.to_string(),
        };
        let mut effects = self.effects.lock();
        if let Some((_, subscribers)) = effects.get_mut(&key) {
            *subscribers = subscribers.saturating_add(1);
            return;
        }
        let projection = self.projection_handle(document_hash, document_id);
        let runtime = Arc::clone(&self.runtime);
        let next_epoch = Arc::clone(&self.next_epoch);
        let effect_key = key.clone();
        let effect = self.ctx.effect(move |ctx| {
            let projected_turn = ctx.get(&projection);
            let Some(runtime) = runtime.get().and_then(std::sync::Weak::upgrade) else {
                return;
            };
            let epoch = next_epoch.fetch_add(1, Ordering::SeqCst);
            rpc::publish_document_turn_authority(
                &runtime,
                &effect_key.document_hash,
                epoch,
                &projected_turn,
            );
        });
        effects.insert(key, (effect, 1));
    }

    fn release_subscription(&self, document_hash: &str, document_id: &str) {
        let key = DocumentAuthorityKey {
            document_hash: document_hash.to_string(),
            document_id: document_id.to_string(),
        };
        let mut effects = self.effects.lock();
        let Some((_, subscribers)) = effects.get_mut(&key) else {
            return;
        };
        if *subscribers > 1 {
            *subscribers -= 1;
            return;
        }
        let Some((effect, _)) = effects.remove(&key) else {
            return;
        };
        self.ctx.dispose_effect(&effect);
        self.projections.remove(&self.ctx, &key);
        if let Some(runtime) = self.runtime.get().and_then(std::sync::Weak::upgrade) {
            runtime
                .state_plane_graph
                .retire_channel(&rpc::document_turn_authority_channel(document_hash));
        }
    }
}

fn project_document_turn_authority(
    model_state: Option<agent_doc_controller::actor::ActorState>,
    closeout: &agent_doc_state_backbone::CloseoutProjection,
) -> agent_doc_turn::cp_projection::TurnProjection {
    use agent_doc_controller::actor::ActorState;
    let projected_phase = closeout
        .phase
        .unwrap_or(agent_doc_turn::CyclePhase::Committed);
    let phase = match model_state {
        Some(ActorState::Busy | ActorState::Blocked) if projected_phase.is_open() => {
            projected_phase
        }
        Some(ActorState::Busy | ActorState::Blocked) => {
            agent_doc_turn::CyclePhase::PreflightStarted
        }
        Some(_) => agent_doc_turn::CyclePhase::Committed,
        None => projected_phase,
    };
    let projection = agent_doc_turn::cp_projection::TurnProjection::from_phase(phase);
    if projection.turn_in_flight {
        projection.with_realtime_steering(closeout.realtime_steering.clone())
    } else {
        projection
    }
}

/// The durable half of `#retainedclearreactive`: emit `DocumentWriteConverged`
/// for an intent the derived verdict proved `Satisfied`.
///
/// This is a plain projection of a decision the graph already made — Cells
/// decide, projection applies. It holds a [`std::sync::Weak`] because the
/// runtime owns the graph that owns the effect that calls it; a strong handle
/// would be a reference cycle that never drops the controller.
struct RetainedWriteSettleSink {
    project_root: PathBuf,
    runtime: std::sync::Weak<ControllerRuntime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RetainedDeliveryObservation {
    file: PathBuf,
    content: Arc<str>,
    content_hash: String,
    live_editors: usize,
    delivery_converged: bool,
    delivery_version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RetainedDeliveryActivation {
    intent_id: String,
    controller_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetainedResumeAction {
    ResumeExactDelivery,
    ReconcileMaterializedCapture,
}

const RETAINED_DELIVERY_REACTIVE_REASON: &str = "retained_delivery_reactive";

impl RetainedResumeAction {
    fn reason(self) -> &'static str {
        match self {
            Self::ResumeExactDelivery => RETAINED_DELIVERY_REACTIVE_REASON,
            Self::ReconcileMaterializedCapture => {
                "retained_materialized_capture_reconcile_reactive"
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RetainedResumeSignal {
    action: RetainedResumeAction,
    intent_id: String,
    target_hash: String,
    cycle_id: String,
    capture_id: String,
    response_sha256: String,
    delivery_version: u64,
    controller_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RetainedTransitionProjection {
    file: PathBuf,
    base_content: Arc<str>,
    target_content: Arc<str>,
    intent_id: String,
    target_hash: String,
    delivery_version: u64,
    controller_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetainedTransitionConflict {
    MissingBase,
    InvalidTargetHash,
    InvalidTargetStructure,
    DivergentVisibleProjection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RetainedTransitionState {
    NoProjection,
    Idle,
    AwaitingController {
        intent_id: String,
    },
    AwaitingDelivery(RetainedDeliveryActivation),
    AwaitingLiveEditor {
        intent_id: String,
        delivery_version: u64,
    },
    AwaitingConvergence {
        intent_id: String,
        delivery_version: u64,
    },
    ApplyTarget(RetainedTransitionProjection),
    TargetVisible {
        intent_id: String,
        target_hash: String,
        delivery_version: u64,
        controller_generation: u64,
        resume: Option<RetainedResumeSignal>,
    },
    ReconcileMaterializedCapture(RetainedResumeSignal),
    Conflict {
        intent_id: String,
        target_hash: String,
        visible_hash: Option<String>,
        delivery_version: Option<u64>,
        reason: RetainedTransitionConflict,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RetainedTransitionEffect {
    ObserveCurrentDelivery(RetainedDeliveryActivation),
    ApplyTarget(RetainedTransitionProjection),
    ResumeCloseout(RetainedResumeSignal),
    SettleMaterializedCapture(RetainedResumeSignal),
}

impl RetainedTransitionState {
    fn effect(&self) -> Option<RetainedTransitionEffect> {
        match self {
            Self::AwaitingDelivery(activation) => Some(
                RetainedTransitionEffect::ObserveCurrentDelivery(activation.clone()),
            ),
            Self::ApplyTarget(transition) => {
                Some(RetainedTransitionEffect::ApplyTarget(transition.clone()))
            }
            Self::TargetVisible {
                resume: Some(signal),
                ..
            } => Some(RetainedTransitionEffect::ResumeCloseout(signal.clone())),
            Self::ReconcileMaterializedCapture(signal) => Some(
                RetainedTransitionEffect::SettleMaterializedCapture(signal.clone()),
            ),
            Self::NoProjection
            | Self::Idle
            | Self::AwaitingController { .. }
            | Self::AwaitingLiveEditor { .. }
            | Self::AwaitingConvergence { .. }
            | Self::TargetVisible { resume: None, .. }
            | Self::Conflict { .. } => None,
        }
    }

    fn resume_signal(&self) -> Option<RetainedResumeSignal> {
        match self {
            Self::TargetVisible {
                resume: Some(signal),
                ..
            }
            | Self::ReconcileMaterializedCapture(signal) => Some(signal.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    fn transition_projection(&self) -> Option<RetainedTransitionProjection> {
        match self {
            Self::ApplyTarget(transition) => Some(transition.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    fn delivery_activation(&self) -> Option<RetainedDeliveryActivation> {
        match self {
            Self::AwaitingDelivery(activation) => Some(activation.clone()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QueueAuthorityObservation {
    file: PathBuf,
    content: String,
    content_hash: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AnsweredFreeTextQueueStrikeProjection {
    file: Option<PathBuf>,
    expected_content: String,
    target_content: String,
    projection_id: String,
    capture_id: String,
    response_sha256: String,
    response_body: String,
    baseline_content: Option<String>,
    node_keys: Vec<String>,
    error: Option<String>,
}

impl AnsweredFreeTextQueueStrikeProjection {
    fn has_target(&self) -> bool {
        self.error.is_none()
            && self.file.is_some()
            && !self.projection_id.is_empty()
            && self.target_content != self.expected_content
            && !self.node_keys.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct QueueCompletionProjection {
    content_hash: String,
    completions: Vec<agent_doc_queue::queue_projection::CompletedQueueHeadProjection>,
    error: Option<String>,
}

impl RetainedWriteSettleSink {
    /// Append + apply the convergence fact. Applying re-enters
    /// [`ControllerDocumentGraphs::set_projection`], which invalidates the
    /// verdict that triggered us; the rerun then sees `NoRetainedIntent` and
    /// stops. `flush_effects` is re-entrancy-guarded, so that second run is
    /// another iteration of the same drain, not recursion.
    fn settle(
        &self,
        document_hash: &str,
        intent_id: &str,
        target_hash: &str,
        source: &str,
        intent_source: &agent_doc_state_backbone::DocumentWriteSource,
    ) -> bool {
        let Some(runtime) = self.runtime.upgrade() else {
            // The controller is shutting down; the intent stays retained and the
            // next controller derives the same verdict from the same ledger.
            return false;
        };
        let event = agent_doc_state_backbone::StateEvent::new(
            format!("document-write-converged-{document_hash}-{intent_id}"),
            agent_doc_state_backbone::StateFact::DocumentWriteConverged {
                document_hash: document_hash.to_string(),
                intent_id: intent_id.to_string(),
                target_hash: target_hash.to_string(),
                source: source.to_string(),
                intent_source: intent_source.clone(),
            },
        );
        if let Err(e) = append_state_event(&self.project_root, &event) {
            eprintln!("[controller] retained-write settle append failed for {document_hash}: {e}");
            return false;
        }
        if let Err(e) = runtime.apply_state_event(&event) {
            eprintln!("[controller] retained-write settle apply failed for {document_hash}: {e}");
            return false;
        }
        true
    }

    /// Observe the already-retained editor/CRDT projection when a replacement
    /// controller activates after the editor's most recent replica event.
    fn observe_current_retained_delivery(
        &self,
        document_hash: &str,
    ) -> Option<RetainedDeliveryObservation> {
        match rpc::current_registered_retained_delivery_projection(
            &self.project_root,
            document_hash,
        ) {
            Ok(Some(observation)) => {
                agent_doc_ops_log_io::log_op(
                    &self.project_root,
                    &format!(
                        "retained_delivery_observed_on_activation document_hash={document_hash} content_hash={} delivery_version={} live_editors={}",
                        agent_doc_hash::content_hash(&observation.content),
                        observation.delivery_version,
                        observation.live_editors,
                    ),
                );
                Some(observation)
            }
            Ok(None) => None,
            Err(error) => {
                agent_doc_ops_log_io::log_op(
                    &self.project_root,
                    &format!(
                        "retained_delivery_activation_observation_deferred document_hash={document_hash} error={error:#}"
                    ),
                );
                None
            }
        }
    }

    /// Project a typed derived recovery identity into the existing
    /// captured-finalize state-plane channel.
    fn resume(&self, document_hash: &str, signal: &RetainedResumeSignal) -> bool {
        let Some(runtime) = self.runtime.upgrade() else {
            // A replacement controller will reconstruct the same Computed
            // signal after replica registration; retain the durable intent.
            return false;
        };
        let Ok(Some(projection)) = runtime.document_state_projection(document_hash) else {
            return false;
        };
        if !retained_resume_signal_matches_projection(signal, &projection) {
            // The durable projection advanced between derivation and effect
            // application. The new projection will invalidate the Computed.
            return false;
        }
        if !rpc::publish_pinned_captured_finalize_wake(
            &runtime,
            document_hash,
            &signal.cycle_id,
            &signal.capture_id,
            &signal.response_sha256,
            signal.action.reason(),
        ) {
            return false;
        }
        agent_doc_ops_log_io::log_op(
            &self.project_root,
            &format!(
                "retained_closeout_woken_from_derived_delivery document_hash={document_hash} action={:?} intent_id={} target_hash={} cycle_id={} capture_id={} controller_generation={}",
                signal.action,
                signal.intent_id,
                signal.target_hash,
                signal.cycle_id,
                signal.capture_id,
                signal.controller_generation,
            ),
        );
        true
    }

    /// Retire an obsolete post-commit reposition once the converged editor
    /// projection already contains the pinned response continuation.
    ///
    /// This is settlement of a derived state, not another finalize attempt:
    /// the owning cycle is already terminal and replaying its body would turn
    /// harmless layout residue into an operator-blocking duplicate closeout.
    fn settle_materialized_capture(
        &self,
        document_hash: &str,
        signal: &RetainedResumeSignal,
    ) -> bool {
        let Some(runtime) = self.runtime.upgrade() else {
            return false;
        };
        let Ok(Some(projection)) = runtime.document_state_projection(document_hash) else {
            return false;
        };
        if signal.action != RetainedResumeAction::ReconcileMaterializedCapture
            || !retained_resume_signal_matches_projection(signal, &projection)
        {
            return false;
        }
        let Some(intent) = projection.document.pending_write.as_ref() else {
            return false;
        };
        if intent.source != agent_doc_state_backbone::DocumentWriteSource::PostCommitReposition {
            return false;
        }
        let source = "controller_retained_materialized_capture_settlement_effect";
        if !self.settle(
            document_hash,
            &intent.intent_id,
            &intent.target_hash,
            source,
            &intent.source,
        ) {
            return false;
        }
        agent_doc_ops_log_io::log_op(
            &self.project_root,
            &format!(
                "retained_materialized_capture_settled_reactively document_hash={document_hash} intent_id={} target_hash={} cycle_id={} capture_id={} delivery_version={} controller_generation={}",
                signal.intent_id,
                signal.target_hash,
                signal.cycle_id,
                signal.capture_id,
                signal.delivery_version,
                signal.controller_generation,
            ),
        );
        true
    }

    /// Materialize one derived retained target in the controller-owned CRDT
    /// relay. The relay performs the final expected-base CAS, so a stale Effect
    /// can never overwrite operator typing that advanced after derivation.
    fn project_retained_transition(
        &self,
        document_hash: &str,
        transition: &RetainedTransitionProjection,
    ) -> bool {
        let Some(runtime) = self.runtime.upgrade() else {
            return false;
        };
        let Ok(Some(projection)) = runtime.document_state_projection(document_hash) else {
            return false;
        };
        let Some(intent) = projection.document.pending_write.as_ref() else {
            return false;
        };
        if intent.intent_id != transition.intent_id
            || !intent
                .target_hash
                .eq_ignore_ascii_case(&transition.target_hash)
            || intent.expected_content.as_deref() != Some(transition.base_content.as_ref())
            || intent.target_content != transition.target_content.as_ref()
        {
            return false;
        }
        let source = "retained_transition_projection_effect";
        match agent_doc_crdt_relay_io::apply_cp_write_for_file(
            &transition.file,
            transition.base_content.as_ref(),
            transition.target_content.as_ref(),
            source,
        ) {
            Ok(Some(outcome)) => {
                agent_doc_ops_log_io::log_op(
                    &self.project_root,
                    &format!(
                        "retained_transition_projected document_hash={document_hash} intent_id={} target_hash={} delivery_version={} controller_generation={} applied={} targets={} source={source}",
                        transition.intent_id,
                        transition.target_hash,
                        transition.delivery_version,
                        transition.controller_generation,
                        outcome.applied,
                        outcome.targets,
                    ),
                );
                true
            }
            Ok(None) => false,
            Err(error) => {
                agent_doc_ops_log_io::log_op(
                    &self.project_root,
                    &format!(
                        "retained_transition_projection_deferred document_hash={document_hash} intent_id={} target_hash={} delivery_version={} controller_generation={} source={source} error={error:#}",
                        transition.intent_id,
                        transition.target_hash,
                        transition.delivery_version,
                        transition.controller_generation,
                    ),
                );
                false
            }
        }
    }

    fn complete_compact(
        &self,
        document_hash: &str,
        continuation: &agent_doc_state_backbone::DocumentCompactProjectionContinuation,
    ) -> bool {
        let Some(runtime) = self.runtime.upgrade() else {
            return false;
        };
        let Ok(Some(projection)) = runtime.document_state_projection(document_hash) else {
            return false;
        };
        if projection.document.pending_write.is_some()
            || projection
                .document
                .pending_compact_projection
                .as_ref()
                .is_none_or(|pending| pending.continuation_id != continuation.continuation_id)
        {
            return false;
        }
        let invocation = ControllerCompactProjectionCompletion {
            file: PathBuf::from(&continuation.file),
            live_content: continuation.live_content.clone(),
            committed_content: continuation.committed_content.clone(),
            target_component: continuation.target_component.clone(),
            commit: continuation.commit,
        };
        let settled_hash = match runtime_effects()
            .and_then(|effects| effects.complete_compact_projection(invocation))
        {
            Ok(settled_hash) => settled_hash,
            Err(error) => {
                eprintln!(
                    "[controller] compact projection completion failed for {document_hash}: {error:#}"
                );
                return false;
            }
        };
        let event = agent_doc_state_backbone::StateEvent::new(
            format!(
                "compact-projection-settled:{document_hash}:{}",
                continuation.continuation_id
            ),
            agent_doc_state_backbone::StateFact::DocumentCompactProjectionSettled {
                document_hash: document_hash.to_string(),
                continuation_id: continuation.continuation_id.clone(),
                settled_hash,
            },
        );
        if let Err(error) = append_state_event(&self.project_root, &event) {
            eprintln!(
                "[controller] compact projection receipt append failed for {document_hash}: {error:#}"
            );
            return false;
        }
        if let Err(error) = runtime.apply_state_event(&event) {
            eprintln!(
                "[controller] compact projection receipt apply failed for {document_hash}: {error:#}"
            );
            return false;
        }
        true
    }

    /// Persist terminal queue lifecycle facts selected by the controller's
    /// queue-completion projection. Applying each event feeds the receipt back
    /// into `projection`, so the Computed candidate disappears durably.
    fn complete_queue_heads(
        &self,
        document_hash: &str,
        file: &Path,
        projection: &QueueCompletionProjection,
    ) {
        let Some(runtime) = self.runtime.upgrade() else {
            return;
        };
        for head in &projection.completions {
            let head_hash = agent_doc_hash::content_hash(&head.text);
            let event = agent_doc_state_backbone::StateEvent::new(
                format!(
                    "queue-head-completed:{document_hash}:{}:{}:{head_hash}",
                    head.node_key, head.index
                ),
                agent_doc_state_backbone::StateFact::QueueHeadCompleted {
                    document_hash: document_hash.to_string(),
                    node_key: head.node_key.clone(),
                    backlog_id: head.backlog_id.clone(),
                    hosting_epoch: None,
                },
            );
            match append_state_event(&self.project_root, &event) {
                Ok(_) => {
                    if let Err(error) = runtime.apply_state_event(&event) {
                        eprintln!(
                            "[controller] queue completion apply failed for {document_hash}: {error}"
                        );
                        return;
                    }
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_authority_completion_projected file={} event_id={} document_hash={} node_id={} authority_hash={}",
                            file.display(),
                            event.event_id,
                            document_hash,
                            head.node_key,
                            projection.content_hash,
                        ),
                    );
                }
                Err(error) => {
                    eprintln!(
                        "[controller] queue completion append failed for {document_hash}: {error}"
                    );
                    return;
                }
            }
        }
    }
}

impl ControllerDocumentGraphs {
    fn new_in(scope: &agent_doc_state_scope::ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        Self {
            projection: lazily::ThreadSafeSourceMap::new(&ctx),
            closeout_cycle_id: lazily::ThreadSafeSourceMap::new(&ctx),
            closeout_owner: lazily::ThreadSafeSourceMap::new(&ctx),
            closeout_now_secs: lazily::ThreadSafeSourceMap::new(&ctx),
            closeout_gate: lazily::ThreadSafeComputedMap::new(&ctx),
            pending: lazily::ThreadSafeComputedMap::new(&ctx),
            authority: lazily::ThreadSafeSourceMap::new(&ctx),
            disk: lazily::ThreadSafeSourceMap::new(&ctx),
            retained_delivery: lazily::ThreadSafeSourceMap::new(&ctx),
            settle_generation: lazily::ThreadSafeSourceMap::new(&ctx),
            verdict: lazily::ThreadSafeComputedMap::new(&ctx),
            retained_transition_state: lazily::ThreadSafeComputedMap::new(&ctx),
            retained_transition_effect: lazily::ThreadSafeComputedMap::new(&ctx),
            retained_transition_published_frontier: lazily::ThreadSafeSourceMap::new(&ctx),
            compact_resume: lazily::ThreadSafeComputedMap::new(&ctx),
            compact_resume_applied: lazily::ThreadSafeSourceMap::new(&ctx),
            queue_authority: lazily::ThreadSafeSourceMap::new(&ctx),
            answered_free_text_strike: lazily::ThreadSafeComputedMap::new(&ctx),
            answered_free_text_strike_submitted: lazily::ThreadSafeSourceMap::new(&ctx),
            queue_completion: lazily::ThreadSafeComputedMap::new(&ctx),
            preflight_facts: lazily::ThreadSafeSourceMap::new(&ctx),
            preflight_projection: lazily::ThreadSafeComputedMap::new(&ctx),
            settle_effects: Mutex::new(BTreeMap::new()),
            retained_transition_effects: Mutex::new(BTreeMap::new()),
            compact_resume_effects: Mutex::new(BTreeMap::new()),
            answered_free_text_strike_effects: Mutex::new(BTreeMap::new()),
            queue_completion_effects: Mutex::new(BTreeMap::new()),
            settle_sink: Arc::new(OnceLock::new()),
            ctx,
        }
    }

    fn projection_handle(
        &self,
    ) -> lazily::ThreadSafeSourceMap<
        String,
        Option<agent_doc_state_backbone::DocumentStateProjection>,
    > {
        self.projection.clone()
    }

    /// Bind the settle effects' durable sink. Called once, right after the
    /// runtime enters its `Arc`.
    fn install_settle_sink(&self, project_root: PathBuf, runtime: &Arc<ControllerRuntime>) {
        let _ = self.settle_sink.set(RetainedWriteSettleSink {
            project_root,
            runtime: Arc::downgrade(runtime),
        });
        let mut document_hashes = self
            .settle_effects
            .lock()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        document_hashes.extend(self.compact_resume_effects.lock().keys().cloned());
        document_hashes.extend(self.retained_transition_effects.lock().keys().cloned());
        self.ctx.batch(|ctx| {
            for document_hash in document_hashes {
                self.settle_generation.set(ctx, document_hash, 1);
            }
        });
    }

    /// Record the applied state projection. This is the only write into the
    /// retained-write graph; `pending` derives from it.
    ///
    /// Content observations are reconstructed from the durable per-plane
    /// authority facts. Their write ordinals fence them to the retained intent
    /// they postdate, so rebuilding the graph after a binary handoff can settle
    /// an exact target without letting a coincidental pre-intent hash clear a
    /// newer write.
    fn set_projection(
        &self,
        document_hash: &str,
        projection: Option<agent_doc_state_backbone::DocumentStateProjection>,
    ) {
        let (authority, disk) = projection
            .as_ref()
            .map(|document| {
                agent_doc_state_backbone::retained_write::durable_exact_observations(
                    &document.document,
                )
            })
            .unwrap_or((None, None));
        let has_retained_intent = projection
            .as_ref()
            .and_then(retained_intent_facts_from_projection)
            .is_some();
        let has_captured_response = projection
            .as_ref()
            .and_then(|document| document.closeout.captured_response.as_ref())
            .is_some();
        let has_compact_continuation = projection
            .as_ref()
            .and_then(|document| document.document.pending_compact_projection.as_ref())
            .is_some();
        let closeout_cycle_id = projection
            .as_ref()
            .and_then(|document| document.closeout.cycle_id.clone());
        let closeout_owner = projection
            .as_ref()
            .and_then(|document| document.closeout.owner.clone());
        let settle_generation = u64::from(self.settle_sink.get().is_some());
        self.ctx.batch(|ctx| {
            self.authority
                .set(ctx, document_hash.to_string(), authority);
            self.disk.set(ctx, document_hash.to_string(), disk);
            self.settle_generation
                .set(ctx, document_hash.to_string(), settle_generation);
            self.closeout_cycle_id
                .set(ctx, document_hash.to_string(), closeout_cycle_id);
            self.closeout_owner
                .set(ctx, document_hash.to_string(), closeout_owner);
            self.projection
                .set(ctx, document_hash.to_string(), projection);
        });
        if has_retained_intent {
            // A pending intent is itself the membership edge for the retained
            // settle Effect. Mint it while rebuilding the graph; sink
            // installation advances `settle_generation` once the runtime is
            // safely inside its Arc.
            self.current_verdict(document_hash);
            self.current_retained_transition_state(document_hash);
        }
        if has_captured_response {
            self.current_answered_free_text_strike(document_hash);
        }
        if has_compact_continuation {
            self.current_compact_resume(document_hash);
        }
    }

    /// Observe one clock edge beside the current durable closeout facts and
    /// return the shared derived gate.
    ///
    /// `timestamp_secs()` deliberately stays outside this graph. Reading the
    /// wall clock is an effect; publishing the reading is a Source update.
    /// Expiry is therefore a Computed state transition, not an imperative
    /// branch hidden inside the wait loop.
    fn closeout_gate(
        &self,
        document_hash: &str,
        closeout: &agent_doc_state_backbone::CloseoutProjection,
        now_secs: u64,
    ) -> agent_doc_state_backbone::closeout_gate::CloseoutGate {
        self.ctx.batch(|ctx| {
            self.closeout_cycle_id
                .set(ctx, document_hash.to_string(), closeout.cycle_id.clone());
            self.closeout_owner
                .set(ctx, document_hash.to_string(), closeout.owner.clone());
            self.closeout_now_secs
                .set(ctx, document_hash.to_string(), now_secs);
        });

        let cycle_id = self.closeout_cycle_id.clone();
        let owner = self.closeout_owner.clone();
        let observed_now_secs = self.closeout_now_secs.clone();
        self.closeout_gate.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                let cycle_id = cycle_id.observe(ctx, key).flatten();
                let owner = owner.observe(ctx, key).flatten();
                agent_doc_state_backbone::closeout_gate::closeout_gate(
                    cycle_id.as_deref(),
                    owner.as_ref(),
                    observed_now_secs.observe(ctx, key).unwrap_or_default(),
                    None,
                    false,
                )
            },
        )
    }

    /// Mint (or read) the derived retained-intent slot for `document_hash`.
    fn pending(
        &self,
        document_hash: &str,
    ) -> Option<agent_doc_state_backbone::retained_write::RetainedIntentFacts> {
        let projection = self.projection.clone();
        self.pending
            .get_or_insert_with(&self.ctx, document_hash.to_string(), move |ctx, key| {
                projection
                    .observe(ctx, key)
                    .flatten()
                    .as_ref()
                    .and_then(retained_intent_facts_from_projection)
            })
    }

    /// Read `document_hash`'s derived verdict, minting the slot on first access.
    ///
    /// The slot's body reads the three cell maps, which subscribes it to them, so
    /// a later `set` on any one invalidates this verdict instead of leaving a
    /// stale value behind.
    fn verdict(
        &self,
        document_hash: &str,
        _file: &Path,
        authority: Option<agent_doc_state_backbone::retained_write::ContentObservation>,
        disk: Option<agent_doc_state_backbone::retained_write::ContentObservation>,
    ) -> agent_doc_state_backbone::retained_write::SettlementVerdict {
        // One batch: the two planes are one observation, and a settle effect that
        // ran between them would be judging a fresh authority against a stale disk.
        self.ctx.batch(|ctx| {
            self.authority
                .set(ctx, document_hash.to_string(), authority);
            self.disk.set(ctx, document_hash.to_string(), disk);
        });

        self.current_verdict(document_hash)
    }

    /// Publish one live-authority edge without erasing the last disk edge.
    fn observe_authority(
        &self,
        document_hash: &str,
        _file: &Path,
        authority: agent_doc_state_backbone::retained_write::ContentObservation,
    ) -> agent_doc_state_backbone::retained_write::SettlementVerdict {
        self.authority
            .set(&self.ctx, document_hash.to_string(), Some(authority));
        self.current_verdict(document_hash)
    }

    /// Publish one editor-save edge without erasing the last authority edge.
    fn observe_disk(
        &self,
        document_hash: &str,
        _file: &Path,
        disk: agent_doc_state_backbone::retained_write::ContentObservation,
    ) -> agent_doc_state_backbone::retained_write::SettlementVerdict {
        self.disk
            .set(&self.ctx, document_hash.to_string(), Some(disk));
        self.current_verdict(document_hash)
    }

    /// Publish the post-event CRDT delivery frontier. This is the only write
    /// into the retained-resume delivery Source; the wake decision remains a
    /// Computed over this ingress and the durable projection.
    fn observe_retained_delivery(
        &self,
        document_hash: &str,
        observation: Option<RetainedDeliveryObservation>,
    ) -> Option<RetainedResumeSignal> {
        self.retained_delivery
            .set(&self.ctx, document_hash.to_string(), observation);
        self.current_retained_transition_state(document_hash)
            .resume_signal()
    }

    fn current_verdict(
        &self,
        document_hash: &str,
    ) -> agent_doc_state_backbone::retained_write::SettlementVerdict {
        // As of lazily 0.50 the slot factory receives the entry's own tracking
        // view (`Fn(&ThreadSafeContext, &K) -> V`), so reads through it register
        // dependency edges *on this entry* directly. Before that the only way to
        // derive across maps was to capture a context clone — which did work
        // (clones share the graph's inner state) but made "is this actually
        // Computed?" something you had to prove rather than read, hence
        // `slot_map_entry_tracks_a_cell_map_dependency_through_its_tracking_view`.
        // "Computed in name only" fails silently and surfaces as a stale value
        // much later, so taking the parameter the API now hands us beats
        // re-proving the workaround.
        //
        // Mint the derived `pending` slot before the verdict slot so the verdict's
        // recompute reads an already-materialized entry rather than minting one
        // mid-recompute.
        self.pending(document_hash);

        let pending = self.pending.clone();
        let authority_map = self.authority.clone();
        let disk_map = self.disk.clone();
        let settle_generation = self.settle_generation.clone();
        let verdict = self.verdict.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                let _generation = settle_generation.observe(ctx, key).unwrap_or_default();
                agent_doc_state_backbone::retained_write::settlement_verdict(
                    pending.observe(ctx, key).flatten().as_ref(),
                    authority_map.observe(ctx, key).flatten().as_ref(),
                    disk_map.observe(ctx, key).flatten().as_ref(),
                )
            },
        );
        // Minted after the slot it subscribes to, and only ever once per
        // document. On the first query it fires here; afterwards it has already
        // fired from the `set` above. Either way no caller decides to settle.
        self.ensure_settle_effect(document_hash);
        verdict
    }

    fn current_retained_transition_state(&self, document_hash: &str) -> RetainedTransitionState {
        let projection = self.projection.clone();
        let delivery = self.retained_delivery.clone();
        let settle_generation = self.settle_generation.clone();
        let state = self.retained_transition_state.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                // `observe` cannot subscribe to an entry that is not materialized yet.
                // Observe membership first so a replacement controller recomputes when
                // any cold retained-transition input appears.
                let projection = projection
                    .contains_key(ctx, key)
                    .then(|| projection.observe(ctx, key))
                    .flatten()
                    .flatten();
                let delivery = delivery
                    .contains_key(ctx, key)
                    .then(|| delivery.observe(ctx, key))
                    .flatten()
                    .flatten();
                let controller_generation = settle_generation
                    .contains_key(ctx, key)
                    .then(|| settle_generation.observe(ctx, key))
                    .flatten()
                    .unwrap_or_default();
                retained_transition_state(
                    projection.as_ref(),
                    delivery.as_ref(),
                    controller_generation,
                )
            },
        );
        self.current_retained_transition_effect(document_hash);
        state
    }

    fn current_retained_transition_effect(
        &self,
        document_hash: &str,
    ) -> Option<RetainedTransitionEffect> {
        let state = self.retained_transition_state.clone();
        let published_frontier = self.retained_transition_published_frontier.clone();
        let effect = self.retained_transition_effect.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                let candidate = state
                    .contains_key(ctx, key)
                    .then(|| state.observe(ctx, key))
                    .flatten()
                    .and_then(|state| state.effect());
                let published_frontier = published_frontier
                    .contains_key(ctx, key)
                    .then(|| published_frontier.observe(ctx, key))
                    .flatten()
                    .flatten();
                match candidate {
                    Some(candidate) if published_frontier.as_ref() != Some(&candidate) => {
                        Some(candidate)
                    }
                    _ => None,
                }
            },
        );
        self.ensure_retained_transition_effect(document_hash);
        effect
    }

    #[cfg(test)]
    fn current_retained_resume(&self, document_hash: &str) -> Option<RetainedResumeSignal> {
        self.current_retained_transition_state(document_hash);
        match self.current_retained_transition_effect(document_hash) {
            Some(RetainedTransitionEffect::ResumeCloseout(signal)) => Some(signal),
            _ => None,
        }
    }

    fn current_compact_resume(
        &self,
        document_hash: &str,
    ) -> Option<agent_doc_state_backbone::DocumentCompactProjectionContinuation> {
        let projection = self.projection.clone();
        let settle_generation = self.settle_generation.clone();
        let applied = self.compact_resume_applied.clone();
        let signal = self.compact_resume.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                let _projection_present = projection.contains_key(ctx, key);
                let _generation_present = settle_generation.contains_key(ctx, key);
                let _applied_present = applied.contains_key(ctx, key);
                let candidate = compact_resume_signal(
                    projection.observe(ctx, key).flatten().as_ref(),
                    settle_generation.observe(ctx, key).unwrap_or_default(),
                );
                match candidate {
                    Some(candidate)
                        if applied.observe(ctx, key).flatten().as_deref()
                            != Some(candidate.continuation_id.as_str()) =>
                    {
                        Some(candidate)
                    }
                    _ => None,
                }
            },
        );
        self.ensure_compact_resume_effect(document_hash);
        signal
    }

    /// Publish one authoritative markdown frontier and read its derived
    /// terminal queue projection. The caller supplies evidence only; the
    /// controller-owned Computed decides which durable lifecycle facts are
    /// missing, and its Effect persists them.
    fn observe_queue_authority(
        &self,
        document_hash: &str,
        file: &Path,
        content: String,
    ) -> Result<usize> {
        let observation = QueueAuthorityObservation {
            file: file.to_path_buf(),
            content_hash: agent_doc_hash::content_hash(&content),
            content,
        };
        self.queue_authority
            .set(&self.ctx, document_hash.to_string(), Some(observation));
        let strike = self.current_answered_free_text_strike(document_hash);
        if let Some(error) = strike.error {
            anyhow::bail!("{error}");
        }
        let projection = self.current_queue_completion(document_hash);
        if let Some(error) = projection.error {
            anyhow::bail!("{error}");
        }
        Ok(projection.completions.len())
    }

    fn current_answered_free_text_strike(
        &self,
        document_hash: &str,
    ) -> AnsweredFreeTextQueueStrikeProjection {
        let authority = self.queue_authority.clone();
        let durable_projection = self.projection.clone();
        let projection = self.answered_free_text_strike.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                // SourceMap value dependencies only exist after the key does.
                // Observe membership as its own dependency so a capture that
                // arrives before the first authority publication still wakes
                // when that document authority slot is inserted.
                let _authority_present = authority.contains_key(ctx, key);
                let _projection_present = durable_projection.contains_key(ctx, key);
                let Some(observation) = authority.observe(ctx, key).flatten() else {
                    return AnsweredFreeTextQueueStrikeProjection::default();
                };
                let Some(captured) = durable_projection
                    .observe(ctx, key)
                    .flatten()
                    .and_then(|document| document.closeout.captured_response)
                else {
                    return AnsweredFreeTextQueueStrikeProjection::default();
                };
                if !agent_doc_turn::response_replay::response_materialized_in_content(
                    &captured.response_body,
                    &observation.content,
                ) {
                    return AnsweredFreeTextQueueStrikeProjection::default();
                }
                match agent_doc_queue::queue_consume::project_answered_free_text_strike(
                    &observation.content,
                    &captured.response_body,
                    captured.baseline_content.as_deref(),
                ) {
                    Ok(Some(target)) => {
                        let projection_id = agent_doc_hash::content_hash(&format!(
                            "{}:{}:{}",
                            captured.response_sha256,
                            observation.content_hash,
                            target.node_keys.join(",")
                        ));
                        AnsweredFreeTextQueueStrikeProjection {
                            file: Some(observation.file),
                            expected_content: observation.content,
                            target_content: target.target_content,
                            projection_id,
                            capture_id: captured.capture_id,
                            response_sha256: captured.response_sha256,
                            response_body: captured.response_body,
                            baseline_content: captured.baseline_content,
                            node_keys: target.node_keys,
                            error: None,
                        }
                    }
                    Ok(None) => AnsweredFreeTextQueueStrikeProjection::default(),
                    Err(error) => AnsweredFreeTextQueueStrikeProjection {
                        error: Some(format!(
                            "answered free-text queue projection failed for {key}: {error}"
                        )),
                        ..AnsweredFreeTextQueueStrikeProjection::default()
                    },
                }
            },
        );
        self.ensure_answered_free_text_strike_effect(document_hash);
        projection
    }

    fn current_queue_completion(&self, document_hash: &str) -> QueueCompletionProjection {
        let authority = self.queue_authority.clone();
        let durable_projection = self.projection.clone();
        let projection = self.queue_completion.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                let Some(observation) = authority.observe(ctx, key).flatten() else {
                    return QueueCompletionProjection::default();
                };
                match agent_doc_queue::queue_projection::completed_queue_head_projections(
                    &observation.content,
                ) {
                    Ok(completions) => {
                        let completed_heads = durable_projection
                            .observe(ctx, key)
                            .flatten()
                            .map(|projection| projection.queue.completed_heads)
                            .unwrap_or_default();
                        QueueCompletionProjection {
                            content_hash: observation.content_hash,
                            completions: completions
                                .into_iter()
                                .filter(|head| !completed_heads.contains(&head.node_key))
                                .collect(),
                            error: None,
                        }
                    }
                    Err(error) => QueueCompletionProjection {
                        content_hash: observation.content_hash,
                        completions: Vec::new(),
                        error: Some(error.to_string()),
                    },
                }
            },
        );
        self.ensure_queue_completion_effect(document_hash);
        projection
    }

    /// Refresh this document's preflight read observations and return the
    /// controller-owned Computed projection. The slot stays subscribed across
    /// successive preflight CLI invocations for the controller lifetime.
    fn preflight_projection(
        &self,
        document_hash: &str,
        facts: agent_doc_state_backbone::preflight::PreflightReadFacts,
    ) -> agent_doc_state_backbone::preflight::PreflightReadProjection {
        self.preflight_facts
            .set(&self.ctx, document_hash.to_string(), Some(facts));
        let facts_map = self.preflight_facts.clone();
        self.preflight_projection.get_or_insert_with(
            &self.ctx,
            document_hash.to_string(),
            move |ctx, key| {
                facts_map
                    .observe(ctx, key)
                    .flatten()
                    .as_ref()
                    .map(agent_doc_state_backbone::preflight::derive_read_projection)
                    .unwrap_or_default()
            },
        )
    }

    /// `#retainedclearreactive` — subscribe this document's clear to its verdict.
    ///
    /// Before this, clearing a `Satisfied` intent was a `settle_*` call two
    /// separate consumers had to remember to make; `#preflightsettleparity` had
    /// already been one round of "add the second call site", and a third consumer
    /// would have reintroduced the same class of bug. Gated on the derived signal
    /// in the graph that owns it, the clear fires whenever the signal says so and
    /// is a no-op for every verdict other than `Satisfied` — so there is no
    /// moment for a caller to miss.
    fn ensure_settle_effect(&self, document_hash: &str) {
        if self.settle_effects.lock().contains_key(document_hash) {
            return;
        }
        let key = document_hash.to_string();
        let verdict_map = self.verdict.clone();
        let settle_generation = self.settle_generation.clone();
        let sink = self.settle_sink.clone();
        let effect_key = key.clone();
        let effect = self.ctx.effect(move |ctx| {
            // Subscribe directly to the controller-generation Source. The
            // verdict value may remain `Satisfied` across reconstruction, so
            // relying only on value propagation through the Computed would
            // leave the pre-sink no-op effect dormant.
            let _generation = settle_generation
                .observe(ctx, &effect_key)
                .unwrap_or_default();
            // Reading through the map is what subscribes this effect; a verdict
            // fetched any other way would make it fire exactly once.
            let Some(agent_doc_state_backbone::retained_write::SettlementVerdict::Satisfied {
                intent_id,
                retained_target_hash,
                settled_hash,
                proof,
                intent_source,
            }) = verdict_map.observe(ctx, &effect_key)
            else {
                return;
            };
            let Some(sink) = sink.get() else {
                // No sink bound (test runtime): say so rather than silently
                // dropping a settlement.
                eprintln!(
                    "[controller] retained-write settle skipped for {effect_key}: no sink installed"
                );
                return;
            };
            let source = "controller_retained_write_settlement_effect";
            if !sink.settle(
                &effect_key,
                &intent_id,
                &retained_target_hash,
                source,
                &intent_source,
            ) {
                return;
            }
            agent_doc_ops_log_io::log_op(
                &sink.project_root,
                &format!(
                    "retained_write_settled_from_derived_verdict document_hash={effect_key} intent_id={intent_id} retained_target_hash={retained_target_hash} settled_hash={settled_hash} proof={} source={source}",
                    proof.token(),
                ),
            );
        });
        // Losing the mint race means another thread already installed an
        // equivalent effect; drop ours rather than leaving two subscribed.
        let mut effects = self.settle_effects.lock();
        if effects.contains_key(&key) {
            drop(effects);
            self.ctx.dispose_effect(&effect);
            return;
        }
        effects.insert(key, effect);
    }

    fn ensure_compact_resume_effect(&self, document_hash: &str) {
        if self
            .compact_resume_effects
            .lock()
            .contains_key(document_hash)
        {
            return;
        }
        let key = document_hash.to_string();
        let signal_map = self.compact_resume.clone();
        let settle_generation = self.settle_generation.clone();
        let applied = self.compact_resume_applied.clone();
        let sink = self.settle_sink.clone();
        let effect_key = key.clone();
        let effect = self.ctx.effect(move |ctx| {
            let _generation_present = settle_generation.contains_key(ctx, &effect_key);
            let _generation = settle_generation
                .observe(ctx, &effect_key)
                .unwrap_or_default();
            let Some(continuation) = signal_map.observe(ctx, &effect_key).flatten() else {
                return;
            };
            let Some(sink) = sink.get() else {
                return;
            };
            if sink.complete_compact(&effect_key, &continuation) {
                applied.set(
                    ctx,
                    effect_key.clone(),
                    Some(continuation.continuation_id.clone()),
                );
            }
        });
        let mut effects = self.compact_resume_effects.lock();
        if effects.contains_key(&key) {
            drop(effects);
            self.ctx.dispose_effect(&effect);
            return;
        }
        effects.insert(key, effect);
    }

    /// Subscribe durable queue completion to the authoritative markdown
    /// projection. Mutation, cleanup, and preflight callers cannot forget a
    /// companion lifecycle write because they do not own that decision.
    fn ensure_answered_free_text_strike_effect(&self, document_hash: &str) {
        if self
            .answered_free_text_strike_effects
            .lock()
            .contains_key(document_hash)
        {
            return;
        }
        let key = document_hash.to_string();
        let projection_map = self.answered_free_text_strike.clone();
        let submitted_map = self.answered_free_text_strike_submitted.clone();
        let effect_key = key.clone();
        let effect = self.ctx.effect(move |ctx| {
            let Some(projection) = projection_map.observe(ctx, &effect_key) else {
                return;
            };
            if !projection.has_target() {
                return;
            }
            if submitted_map.observe(ctx, &effect_key).flatten().as_deref()
                == Some(projection.projection_id.as_str())
            {
                return;
            }
            // Claim this exact target before admitting the slow document work.
            // A state event unrelated to queue authority may invalidate the
            // Computed while the worker is still converging; the receipt keeps
            // that invalidation from enqueueing a duplicate write.
            submitted_map.set(
                ctx,
                effect_key.clone(),
                Some(projection.projection_id.clone()),
            );
            let Some(file) = projection.file.clone() else {
                return;
            };
            let invocation = ControllerAnsweredFreeTextStrikeInvocation {
                file: file.clone(),
                expected_content: projection.expected_content.clone(),
                target_content: projection.target_content.clone(),
                projection_id: projection.projection_id.clone(),
                capture_id: projection.capture_id.clone(),
                response_sha256: projection.response_sha256.clone(),
                response_body: projection.response_body.clone(),
                baseline_content: projection.baseline_content.clone(),
                node_keys: projection.node_keys.clone(),
            };
            match runtime_effects()
                .and_then(|effects| effects.project_answered_free_text_strike(invocation))
            {
                Ok(()) => agent_doc_ops_log_io::log_op(
                    &file,
                    &format!(
                        "answered_free_text_strike_projected file={} projection_id={} capture_id={} nodes={}",
                        file.display(),
                        projection.projection_id,
                        projection.capture_id,
                        projection.node_keys.len(),
                    ),
                ),
                Err(error) => eprintln!(
                    "[controller] answered free-text strike admission failed for {}: {error}",
                    file.display()
                ),
            }
        });
        let mut effects = self.answered_free_text_strike_effects.lock();
        if effects.contains_key(&key) {
            drop(effects);
            self.ctx.dispose_effect(&effect);
            return;
        }
        effects.insert(key, effect);
    }

    fn ensure_queue_completion_effect(&self, document_hash: &str) {
        if self
            .queue_completion_effects
            .lock()
            .contains_key(document_hash)
        {
            return;
        }
        let key = document_hash.to_string();
        let projection_map = self.queue_completion.clone();
        let authority_map = self.queue_authority.clone();
        let sink = self.settle_sink.clone();
        let effect_key = key.clone();
        let effect = self.ctx.effect(move |ctx| {
            let Some(projection) = projection_map.observe(ctx, &effect_key) else {
                return;
            };
            if projection.error.is_some() || projection.completions.is_empty() {
                return;
            }
            let Some(observation) = authority_map.observe(ctx, &effect_key).flatten() else {
                return;
            };
            let Some(sink) = sink.get() else {
                return;
            };
            sink.complete_queue_heads(&effect_key, &observation.file, &projection);
        });
        let mut effects = self.queue_completion_effects.lock();
        if effects.contains_key(&key) {
            drop(effects);
            self.ctx.dispose_effect(&effect);
            return;
        }
        effects.insert(key, effect);
    }

    /// Apply the sole effect-bearing projection of the retained-transition
    /// state table. Replica RPCs publish Sources only; this Effect is the one
    /// place that observes activation state, submits a guarded Base -> Target
    /// transition, or wakes retained closeout reconciliation.
    fn ensure_retained_transition_effect(&self, document_hash: &str) {
        if self
            .retained_transition_effects
            .lock()
            .contains_key(document_hash)
        {
            return;
        }
        let key = document_hash.to_string();
        let effect_map = self.retained_transition_effect.clone();
        let delivery = self.retained_delivery.clone();
        let published_frontier = self.retained_transition_published_frontier.clone();
        let sink = self.settle_sink.clone();
        let effect_key = key.clone();
        let effect = self.ctx.effect(move |ctx| {
            let Some(projected_effect) = effect_map.observe(ctx, &effect_key).flatten() else {
                return;
            };
            let Some(sink) = sink.get() else {
                return;
            };
            match &projected_effect {
                RetainedTransitionEffect::ObserveCurrentDelivery(_) => {
                    let Some(observation) = sink.observe_current_retained_delivery(&effect_key)
                    else {
                        return;
                    };
                    published_frontier.set(ctx, effect_key.clone(), Some(projected_effect));
                    delivery.set(ctx, effect_key.clone(), Some(observation));
                }
                RetainedTransitionEffect::ApplyTarget(transition) => {
                    if sink.project_retained_transition(&effect_key, transition) {
                        published_frontier.set(ctx, effect_key.clone(), Some(projected_effect));
                    }
                }
                RetainedTransitionEffect::ResumeCloseout(signal) => {
                    // Advance the frontier only after publication. A stale or
                    // missing continuation remains eligible for a later
                    // projection edge.
                    if sink.resume(&effect_key, signal) {
                        published_frontier.set(ctx, effect_key.clone(), Some(projected_effect));
                    }
                }
                RetainedTransitionEffect::SettleMaterializedCapture(signal) => {
                    if sink.settle_materialized_capture(&effect_key, signal) {
                        published_frontier.set(ctx, effect_key.clone(), Some(projected_effect));
                    }
                }
            }
        });
        let mut effects = self.retained_transition_effects.lock();
        if effects.contains_key(&key) {
            drop(effects);
            self.ctx.dispose_effect(&effect);
            return;
        }
        effects.insert(key, effect);
    }
}

fn retained_transition_state(
    projection: Option<&agent_doc_state_backbone::DocumentStateProjection>,
    delivery: Option<&RetainedDeliveryObservation>,
    controller_generation: u64,
) -> RetainedTransitionState {
    let Some(projection) = projection else {
        return RetainedTransitionState::NoProjection;
    };
    let Some(intent) = projection.document.pending_write.as_ref() else {
        return RetainedTransitionState::Idle;
    };
    if controller_generation == 0 {
        return RetainedTransitionState::AwaitingController {
            intent_id: intent.intent_id.clone(),
        };
    }
    let Some(delivery) = delivery else {
        return RetainedTransitionState::AwaitingDelivery(RetainedDeliveryActivation {
            intent_id: intent.intent_id.clone(),
            controller_generation,
        });
    };
    if delivery.live_editors == 0 {
        return RetainedTransitionState::AwaitingLiveEditor {
            intent_id: intent.intent_id.clone(),
            delivery_version: delivery.delivery_version,
        };
    }
    if !delivery.delivery_converged {
        return RetainedTransitionState::AwaitingConvergence {
            intent_id: intent.intent_id.clone(),
            delivery_version: delivery.delivery_version,
        };
    }

    if agent_doc_element::element::structural_corruption_reason(&intent.target_content).is_some() {
        return RetainedTransitionState::Conflict {
            intent_id: intent.intent_id.clone(),
            target_hash: intent.target_hash.clone(),
            visible_hash: Some(delivery.content_hash.clone()),
            delivery_version: Some(delivery.delivery_version),
            reason: RetainedTransitionConflict::InvalidTargetStructure,
        };
    }

    let resume = derive_retained_resume_signal(projection, delivery, controller_generation);
    if delivery
        .content_hash
        .eq_ignore_ascii_case(&intent.target_hash)
    {
        return RetainedTransitionState::TargetVisible {
            intent_id: intent.intent_id.clone(),
            target_hash: intent.target_hash.clone(),
            delivery_version: delivery.delivery_version,
            controller_generation,
            resume,
        };
    }
    if let Some(
        signal @ RetainedResumeSignal {
            action: RetainedResumeAction::ReconcileMaterializedCapture,
            ..
        },
    ) = resume
    {
        return RetainedTransitionState::ReconcileMaterializedCapture(signal);
    }

    let Some(base_content) = intent.expected_content.as_deref() else {
        return RetainedTransitionState::Conflict {
            intent_id: intent.intent_id.clone(),
            target_hash: intent.target_hash.clone(),
            visible_hash: Some(delivery.content_hash.clone()),
            delivery_version: Some(delivery.delivery_version),
            reason: RetainedTransitionConflict::MissingBase,
        };
    };
    if delivery.content.as_ref() == base_content {
        if intent.target_content == base_content
            || !agent_doc_hash::content_hash(&intent.target_content)
                .eq_ignore_ascii_case(&intent.target_hash)
        {
            return RetainedTransitionState::Conflict {
                intent_id: intent.intent_id.clone(),
                target_hash: intent.target_hash.clone(),
                visible_hash: Some(delivery.content_hash.clone()),
                delivery_version: Some(delivery.delivery_version),
                reason: RetainedTransitionConflict::InvalidTargetHash,
            };
        }
        return RetainedTransitionState::ApplyTarget(RetainedTransitionProjection {
            file: delivery.file.clone(),
            base_content: Arc::from(base_content),
            target_content: Arc::from(intent.target_content.as_str()),
            intent_id: intent.intent_id.clone(),
            target_hash: intent.target_hash.clone(),
            delivery_version: delivery.delivery_version,
            controller_generation,
        });
    }

    RetainedTransitionState::Conflict {
        intent_id: intent.intent_id.clone(),
        target_hash: intent.target_hash.clone(),
        visible_hash: Some(delivery.content_hash.clone()),
        delivery_version: Some(delivery.delivery_version),
        reason: RetainedTransitionConflict::DivergentVisibleProjection,
    }
}

fn derive_retained_resume_signal(
    projection: &agent_doc_state_backbone::DocumentStateProjection,
    delivery: &RetainedDeliveryObservation,
    controller_generation: u64,
) -> Option<RetainedResumeSignal> {
    let intent = projection.document.pending_write.as_ref()?;
    let capture = intent
        .continuation
        .as_ref()
        .or(projection.closeout.captured_response.as_ref())?;
    let cycle_id = capture.cycle_id.as_str();
    let action = if delivery
        .content_hash
        .eq_ignore_ascii_case(&intent.target_hash)
    {
        RetainedResumeAction::ResumeExactDelivery
    } else if intent.source == agent_doc_state_backbone::DocumentWriteSource::PostCommitReposition
        && agent_doc_element::element::structural_corruption_reason(&delivery.content).is_none()
        && agent_doc_turn::response_replay::response_materialized_in_content(
            &capture.response_body,
            delivery.content.as_ref(),
        )
    {
        // A post-commit reposition carries layout cleanup, not a missing
        // response body. If a newer converged projection already contains the
        // durable response, the reactive settlement Effect can retire the
        // obsolete layout transition directly. The already-terminal owner
        // cycle must not replay its response body through the supervisor.
        RetainedResumeAction::ReconcileMaterializedCapture
    } else {
        return None;
    };
    Some(RetainedResumeSignal {
        action,
        intent_id: intent.intent_id.clone(),
        target_hash: intent.target_hash.clone(),
        cycle_id: cycle_id.to_string(),
        capture_id: capture.capture_id.clone(),
        response_sha256: capture.response_sha256.clone(),
        delivery_version: delivery.delivery_version,
        controller_generation,
    })
}

#[cfg(test)]
fn retained_delivery_activation(
    projection: Option<&agent_doc_state_backbone::DocumentStateProjection>,
    delivery: Option<&RetainedDeliveryObservation>,
    controller_generation: u64,
) -> Option<RetainedDeliveryActivation> {
    retained_transition_state(projection, delivery, controller_generation).delivery_activation()
}

#[cfg(test)]
fn retained_transition_projection(
    projection: Option<&agent_doc_state_backbone::DocumentStateProjection>,
    delivery: Option<&RetainedDeliveryObservation>,
    controller_generation: u64,
) -> Option<RetainedTransitionProjection> {
    retained_transition_state(projection, delivery, controller_generation).transition_projection()
}

#[cfg(test)]
fn retained_resume_signal(
    projection: Option<&agent_doc_state_backbone::DocumentStateProjection>,
    delivery: Option<&RetainedDeliveryObservation>,
    controller_generation: u64,
) -> Option<RetainedResumeSignal> {
    retained_transition_state(projection, delivery, controller_generation).resume_signal()
}

fn compact_resume_signal(
    projection: Option<&agent_doc_state_backbone::DocumentStateProjection>,
    controller_generation: u64,
) -> Option<agent_doc_state_backbone::DocumentCompactProjectionContinuation> {
    if controller_generation == 0 {
        return None;
    }
    let projection = projection?;
    if projection.document.pending_write.is_some() {
        return None;
    }
    projection.document.pending_compact_projection.clone()
}

fn retained_resume_signal_matches_projection(
    signal: &RetainedResumeSignal,
    projection: &agent_doc_state_backbone::DocumentStateProjection,
) -> bool {
    let Some(intent) = projection.document.pending_write.as_ref() else {
        return false;
    };
    if intent.intent_id != signal.intent_id
        || !intent.target_hash.eq_ignore_ascii_case(&signal.target_hash)
    {
        return false;
    }
    intent
        .continuation
        .as_ref()
        .or(projection.closeout.captured_response.as_ref())
        .is_some_and(|capture| {
            capture.cycle_id == signal.cycle_id
                && capture.capture_id == signal.capture_id
                && capture.response_sha256 == signal.response_sha256
        })
}

/// Project a document's retained-write intent into the facts settlement needs.
///
/// `carries_response_payload` asks whether the intent's own target contains the
/// captured response: only such an intent can be proven landed by that response
/// appearing in a rebased document.
///
/// That test is deliberately narrow — it is hash equality against the closeout's
/// response cell — so a closeout's *second* write does not qualify. A closeout
/// writes the response cell, then a `pending_write` carrying response+backlog,
/// then the `pending_add_sync` queue mirror; only the first has the response
/// cell's hash. An interrupt between the last two used to leave the middle
/// intent with `carries_response_payload == false` and a target the queue mirror
/// had already superseded, so nothing could ever settle it and every later cycle
/// was refused. `carries_content_delta` is the proof that covers it: whatever
/// the intent was adding is present in the converged content.
fn retained_intent_facts_from_projection(
    document: &agent_doc_state_backbone::DocumentStateProjection,
) -> Option<agent_doc_state_backbone::retained_write::RetainedIntentFacts> {
    let pending = document.document.pending_write.as_ref()?;
    // The same identity `DocumentWriteConverged` already uses to prove a write
    // through to `DiskProjected`: the closeout's response cell and this intent's
    // target are the same content, so the intent *is* the response write.
    let carries_response_payload = document
        .closeout
        .response_cell
        .as_ref()
        .is_some_and(|cell| cell.content_hash.eq_ignore_ascii_case(&pending.target_hash));
    let carries_content_delta = !agent_doc_state_backbone::retained_write::intent_added_lines(
        pending.expected_content.as_deref(),
        &pending.target_content,
    )
    .is_empty();
    Some(
        agent_doc_state_backbone::retained_write::RetainedIntentFacts {
            intent_id: pending.intent_id.clone(),
            target_hash: pending.target_hash.clone(),
            reason: pending.reason.clone(),
            source: pending.source.clone(),
            // Answered by the projection, which owns the write ordinals
            // (`#adwritesourceenum`). Settlement itself observes only content
            // planes, so it cannot tell a later stage from an older cycle's.
            superseding_stage: document.document.superseding_closeout_stage(pending),
            carries_response_payload,
            carries_content_delta,
        },
    )
}

impl ControllerRuntime {
    fn new(bootstrap: ControllerBootstrap) -> Result<Self> {
        if controller_restart_recovery_needed(
            bootstrap.controller_generation,
            bootstrap.previous_controller_pid,
        ) {
            recover_controller_after_restart(&bootstrap)?;
        }
        let (memory, actor_store) = ControllerMemoryState::load(&bootstrap.project_root)?;
        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph = ControllerActorGraph::new_in(&scope, actor_store);
        let persisted_layout = load_layout_state(&bootstrap.project_root).unwrap_or_default();
        let supervisor_recycle_graph = ControllerSupervisorRecycleGraph::new_in(
            &scope,
            memory.state_projection.project_supervisor_recycle(),
        );
        let coordination_graph = ControllerCoordinationGraph::new_in(&scope);
        let document_graphs = ControllerDocumentGraphs::new_in(&scope);
        let document_authority_graph = ControllerDocumentAuthorityGraph::new_in(
            &scope,
            actor_graph.document_model_states_handle(),
            document_graphs.projection_handle(),
        );
        let state_plane_graph = ControllerStatePlaneGraph::new_in_with_first_version(
            &scope,
            state_plane_first_version(bootstrap.controller_generation),
        );
        let pane_layout_graph = ControllerPaneLayoutGraph::new_in(
            &scope,
            persisted_layout,
            actor_graph.live_bindings_handle(),
        );
        let async_editor_commands = ControllerAsyncEditorCommandGraph::new_in(&scope);
        let editor_surface_graph =
            rpc::ControllerEditorSurfaceGraph::new(Arc::new(rpc::run_controller_editor_intent));
        let document_path_transition_graph =
            rpc::ControllerDocumentPathTransitionGraph::new_in(&scope);
        for (document_hash, projection) in &memory.state_projection.documents {
            document_graphs.set_projection(document_hash, Some(projection.clone()));
        }
        Ok(Self {
            bootstrap: Mutex::new(bootstrap),
            memory: Mutex::new(memory),
            actor_graph,
            document_authority_graph,
            coordination_graph,
            supervisor_recycle_graph,
            state_plane_graph,
            captured_finalize_wakes: Mutex::new(BTreeMap::new()),
            pane_layout_graph,
            editor_surface_graph,
            document_path_transition_graph,
            async_editor_commands,
            supervisor_recycle_waiters: Condvar::new(),
            editor_op_capture_writes: Mutex::new(()),
            state_projection_waiters: Condvar::new(),
            document_graphs,
            recycle_requested: AtomicBool::new(false),
            recycle_urgent: AtomicBool::new(false),
            recycle_forced: AtomicBool::new(false),
            _scope: scope,
        })
    }

    /// Build the runtime already inside its `Arc` and bind the retained-write
    /// settle sink to it (`#retainedclearreactive`).
    ///
    /// The sink needs the runtime and the runtime owns the graph that owns the
    /// effect that calls the sink, so it cannot be wired in [`Self::new`]. Every
    /// production path constructs through here so no controller runs with
    /// settle effects that have nowhere to write.
    pub(crate) fn new_arc(bootstrap: ControllerBootstrap) -> Result<Arc<Self>> {
        let project_root = bootstrap.project_root.clone();
        let runtime = Arc::new(Self::new(bootstrap)?);
        runtime.document_authority_graph.install_runtime(&runtime);
        runtime
            .document_graphs
            .install_settle_sink(project_root, &runtime);
        rpc::install_state_plane_projection_sinks(&runtime);
        #[cfg(not(any(test, feature = "test-support")))]
        rpc::install_pane_layout_projection_sink(&runtime);
        Ok(runtime)
    }

    /// `#ctlrecycle` R2 — mark this controller to recycle at the next idle boundary.
    fn request_recycle(&self) {
        self.recycle_requested.store(true, Ordering::SeqCst);
    }

    fn recycle_urgent(&self) -> bool {
        self.recycle_urgent.load(Ordering::SeqCst)
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
        Ok(self.bootstrap.lock().clone())
    }

    fn actor_record(
        &self,
        document_id: &str,
    ) -> Result<Option<agent_doc_controller::actor::ActorRecord>> {
        Ok(self.actor_graph.record(document_id))
    }

    fn actor_store_snapshot(&self) -> BTreeMap<String, agent_doc_controller::actor::ActorRecord> {
        self.actor_graph.records()
    }

    fn acquire_document_turn_authority_subscription(&self, document_hash: &str, document_id: &str) {
        self.document_authority_graph
            .acquire_subscription(document_hash, document_id);
    }

    fn release_document_turn_authority_subscription(&self, document_hash: &str, document_id: &str) {
        self.document_authority_graph
            .release_subscription(document_hash, document_id);
    }

    fn document_turn_authority_projection(
        &self,
        document_hash: &str,
        document_id: &str,
    ) -> agent_doc_turn::cp_projection::TurnProjection {
        self.document_authority_graph
            .projection(document_hash, document_id)
    }

    fn apply_actor_store_write(&self, write: &agent_doc_controller::actor::ActorStoreWrite) {
        self.actor_graph.apply_store_write(write);
    }

    fn install_state_plane_sink(&self, channel: &str, sink: Arc<dyn ControllerStatePlaneSink>) {
        self.state_plane_graph.install_sink(channel, sink);
    }

    fn publish_state_plane_frame(
        &self,
        channel: String,
        producer_id: String,
        message_json: String,
        epoch: u64,
        base_epoch: Option<u64>,
    ) -> Result<(ControllerStatePlaneFrame, bool)> {
        self.state_plane_graph
            .publish(channel, producer_id, message_json, epoch, base_epoch)
    }

    fn subscribe_state_plane(
        &self,
        channel: &str,
        after_controller_generation: Option<u64>,
        after_version: u64,
        timeout: Duration,
    ) -> ControllerStatePlaneSubscription {
        let controller_generation = self.bootstrap.lock().controller_generation;
        let reset_generationless_ahead_cursor = after_controller_generation.is_none();
        let after_version = state_plane_effective_after_version(
            after_controller_generation,
            after_version,
            controller_generation,
        );
        let mut subscription = self.state_plane_graph.subscribe(
            channel,
            after_version,
            reset_generationless_ahead_cursor,
            timeout,
        );
        subscription.controller_generation = controller_generation;
        subscription
    }

    fn set_pane_layout_desired(
        &self,
        invocation: ControllerTmuxLayoutSyncInvocation,
        source_plane_version: Option<u64>,
    ) -> PaneLayoutDesired {
        self.pane_layout_graph
            .set_desired(invocation, source_plane_version)
    }

    fn pane_layout_desired(&self) -> Option<PaneLayoutDesired> {
        self.pane_layout_graph.desired()
    }

    #[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
    fn pane_layout_projection(&self) -> PaneLayoutProjection {
        self.pane_layout_graph.projection()
    }

    fn pane_layout_state_projection(&self) -> Option<ControllerPaneLayoutStateProjection> {
        self.pane_layout_graph.state_projection()
    }

    fn record_pane_layout_observation(&self, observation: PaneLayoutObservation) {
        self.pane_layout_graph.record_observation(observation);
    }

    #[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
    fn record_pane_layout_effect_receipt(&self, receipt: PaneLayoutEffectReceipt) {
        self.pane_layout_graph.record_receipt(receipt);
    }

    fn pane_layout_effect_file_panes(&self, generation: u64) -> Vec<(String, String)> {
        self.pane_layout_graph.effect_file_panes(generation)
    }

    fn reusable_pane_layout_structure(
        &self,
        desired: &PaneLayoutDesired,
        actor_bindings: &[ControllerTmuxActorBinding],
    ) -> Option<PaneLayoutStructuralReceipt> {
        self.pane_layout_graph
            .reusable_structural_receipt(desired, actor_bindings)
    }

    fn record_pane_layout_structural_assignment(
        &self,
        desired: &PaneLayoutDesired,
        actor_bindings: Vec<ControllerTmuxActorBinding>,
        report: Option<ControllerTmuxLayoutSyncStateReport>,
        file_panes: Vec<(String, String)>,
    ) {
        self.pane_layout_graph.record_structural_assignment(
            desired,
            actor_bindings,
            report,
            file_panes,
        );
    }

    fn pane_layout_actor_bindings(&self) -> Vec<ControllerTmuxActorBinding> {
        self.pane_layout_graph.actor_bindings()
    }

    #[cfg_attr(any(test, feature = "test-support"), allow(dead_code))]
    fn await_pane_layout_generation(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> PaneLayoutProjection {
        self.pane_layout_graph.await_generation(generation, timeout)
    }

    fn try_claim_coordination(&self, scopes: &[String], owner_token: &str, owner_pid: u32) -> bool {
        self.coordination_graph
            .try_claim(scopes, owner_token, owner_pid)
    }

    fn release_coordination(&self, scopes: &[String], owner_token: &str) {
        self.coordination_graph.release(scopes, owner_token);
    }

    fn refresh_memory(&self) -> Result<()> {
        let project_root = self.bootstrap_snapshot()?.project_root;
        let (next, next_actor_store) = ControllerMemoryState::load(&project_root)?;
        let recycle = next.state_projection.project_supervisor_recycle();
        let next_documents = next.state_projection.documents.clone();
        let mut memory = self.memory.lock();
        let previous_documents = memory.state_projection.documents.clone();
        *memory = next;
        drop(memory);
        self.actor_graph.set(next_actor_store);
        self.supervisor_recycle_graph.set(recycle);
        for document_hash in previous_documents
            .keys()
            .chain(next_documents.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            if previous_documents.get(document_hash) != next_documents.get(document_hash) {
                self.document_graphs
                    .set_projection(document_hash, next_documents.get(document_hash).cloned());
            }
        }
        self.supervisor_recycle_waiters.notify_all();
        self.state_projection_waiters.notify_all();
        Ok(())
    }

    fn apply_state_event(&self, event: &agent_doc_state_backbone::StateEvent) -> Result<()> {
        let document_hash = event.fact.document_hash().to_string();
        let (recycle, document_projection) = {
            let mut memory = self.memory.lock();
            memory.state_ledger.append(event.clone());
            memory.state_projection.apply(event);
            let document_projection = memory.state_projection.document(&document_hash).cloned();
            (
                memory.state_projection.project_supervisor_recycle(),
                document_projection,
            )
        };
        let captured_finalize_wake_reason = match &event.fact {
            agent_doc_state_backbone::StateFact::ResponseCaptured { .. } => {
                Some("response_captured")
            }
            agent_doc_state_backbone::StateFact::WriteApplied { .. } => Some("write_applied"),
            agent_doc_state_backbone::StateFact::DocumentWriteConverged {
                intent_source: agent_doc_state_backbone::DocumentWriteSource::PostCommitReposition,
                ..
            } => None,
            agent_doc_state_backbone::StateFact::DocumentWriteConverged { .. } => {
                Some("document_write_converged")
            }
            _ => None,
        };
        let captured_finalize_committed = matches!(
            &event.fact,
            agent_doc_state_backbone::StateFact::CommitObserved { .. }
        );
        let captured_finalize_wake_projection =
            captured_finalize_wake_reason.and_then(|_| document_projection.as_ref().cloned());
        self.supervisor_recycle_graph.set(recycle);
        // `#retainedsettlereactive`: publish the applied *projection* as the fact
        // lands; the retained-intent facts and the settlement verdict are derived
        // from it. Pushing a pre-computed intent here instead would put the
        // derivation at this call site, where it is correct only while every
        // writer remembers to run it. Done outside the memory lock: setting a
        // cell recomputes dependents, which must not re-enter the projection.
        self.document_graphs
            .set_projection(&document_hash, document_projection);
        if let (Some(reason), Some(projection)) = (
            captured_finalize_wake_reason,
            captured_finalize_wake_projection.as_ref(),
        ) {
            rpc::publish_captured_finalize_wake(self, projection, reason);
        }
        if captured_finalize_committed {
            rpc::clear_captured_finalize_wake(self, &document_hash);
        }
        self.supervisor_recycle_waiters.notify_all();
        self.state_projection_waiters.notify_all();
        Ok(())
    }

    /// The document's derived settlement verdict, given content observations the
    /// caller has resolved.
    ///
    /// Both `preflight` and `session-check` reach this one cell, so they cannot
    /// answer "is a retained write outstanding?" differently — which is the
    /// contradiction that deadlocked closeout before `#retainedsettlereactive`.
    /// `#retainedclearreactive`: reading the verdict is also what settles a
    /// `Satisfied` intent — the per-document settle effect is subscribed to this
    /// slot, so the clear happens because the fact changed, not because a caller
    /// invoked a `settle_*` companion. The returned verdict is the one derived
    /// from the caller's observations; the clear it triggers is recorded in
    /// `ops.log`.
    pub(crate) fn document_retained_write_verdict(
        &self,
        document_hash: &str,
        file: &Path,
        authority: Option<agent_doc_state_backbone::retained_write::ContentObservation>,
        disk: Option<agent_doc_state_backbone::retained_write::ContentObservation>,
    ) -> agent_doc_state_backbone::retained_write::SettlementVerdict {
        self.document_graphs
            .verdict(document_hash, file, authority, disk)
    }

    pub(crate) fn document_retained_write_observe_authority(
        &self,
        document_hash: &str,
        file: &Path,
        authority: agent_doc_state_backbone::retained_write::ContentObservation,
    ) -> agent_doc_state_backbone::retained_write::SettlementVerdict {
        self.document_graphs
            .observe_authority(document_hash, file, authority)
    }

    pub(crate) fn document_retained_write_observe_disk(
        &self,
        document_hash: &str,
        file: &Path,
        disk: agent_doc_state_backbone::retained_write::ContentObservation,
    ) -> agent_doc_state_backbone::retained_write::SettlementVerdict {
        self.document_graphs.observe_disk(document_hash, file, disk)
    }

    fn document_retained_write_observe_delivery(
        &self,
        document_hash: &str,
        observation: Option<RetainedDeliveryObservation>,
    ) {
        let _ = self
            .document_graphs
            .observe_retained_delivery(document_hash, observation);
    }

    pub(crate) fn document_preflight_projection(
        &self,
        document_hash: &str,
        facts: agent_doc_state_backbone::preflight::PreflightReadFacts,
    ) -> agent_doc_state_backbone::preflight::PreflightReadProjection {
        self.document_graphs
            .preflight_projection(document_hash, facts)
    }

    pub(crate) fn document_queue_authority_observe(
        &self,
        document_hash: &str,
        file: &Path,
        content: String,
    ) -> Result<usize> {
        self.document_graphs
            .observe_queue_authority(document_hash, file, content)
    }

    /// `#lazily-hot-path` W1 — bounded await for the visible-write receipt of
    /// `(document_hash, patch_id)`.
    ///
    /// The in-memory Lazily projection stays the authority: it is re-read on every
    /// wake, so a missed notify degrades to a slower wait (bounded by the caller's
    /// deadline) and never wedges. This exists so the CLI-side convergence wait is
    /// a *push* from the process that records the fact, instead of every waiter
    /// re-folding the durable ledger on its own timer.
    fn wait_for_visible_write_commit_candidate_patch(
        &self,
        document_hash: &str,
        patch_id: &str,
        timeout: Duration,
    ) -> Option<agent_doc_state_backbone::VisibleWriteCommitCandidateProjection> {
        let started = Instant::now();
        let mut memory = self.memory.lock();
        loop {
            if let Some(proof) = memory
                .state_projection
                .document(document_hash)
                .and_then(|document| document.applied_visible_write_candidate_for_patch(patch_id))
                .cloned()
            {
                return Some(proof);
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return None;
            }
            self.state_projection_waiters
                .wait_for(&mut memory, timeout.saturating_sub(elapsed));
        }
    }

    /// Wait for the closeout that currently owns `cycle_id` to stop blocking a
    /// recovery attempt.
    ///
    /// The owner PID may belong to a long-lived route supervisor, so process
    /// liveness cannot identify the end of one closeout request. The controller
    /// instead observes the exact cycle projection and wakes on terminal-cycle
    /// or owner-release facts.
    fn wait_for_closeout_cycle_progress(
        &self,
        document_hash: &str,
        cycle_id: &str,
        timeout: Duration,
    ) -> rpc::CloseoutCycleWaitOutcome {
        let started = Instant::now();
        let mut observed_owner_id: Option<String> = None;
        let mut memory = self.memory.lock();
        loop {
            let Some(closeout) = memory
                .state_projection
                .document(document_hash)
                .map(|document| &document.closeout)
            else {
                return rpc::CloseoutCycleWaitOutcome::Superseded;
            };
            if closeout.cycle_id.as_deref() != Some(cycle_id) {
                return rpc::CloseoutCycleWaitOutcome::Superseded;
            }
            if !closeout
                .phase
                .is_some_and(agent_doc_turn::CyclePhase::is_open)
            {
                return rpc::CloseoutCycleWaitOutcome::Terminal;
            }

            let current_owner = closeout.owner.as_ref();
            let current_owner_id = current_owner.map(|owner| owner.owner_id.as_str());
            match observed_owner_id.as_deref() {
                Some(observed) if current_owner_id != Some(observed) => {
                    return rpc::CloseoutCycleWaitOutcome::OwnerReleased;
                }
                None => {
                    let Some(current_owner_id) = current_owner_id else {
                        return rpc::CloseoutCycleWaitOutcome::OwnerReleased;
                    };
                    observed_owner_id = Some(current_owner_id.to_string());
                }
                _ => {}
            }

            // No durable fact is appended until the next claimant reconciles an
            // expired stopgap, so schedule a wake at the lease boundary and
            // observe that clock edge into the controller's Lazily graph. The
            // timer is only an effect: the Computed closeout gate below owns the
            // decision that the incumbent no longer blocks.
            let now_secs = timestamp_secs();
            let gate = self
                .document_graphs
                .closeout_gate(document_hash, closeout, now_secs);
            if !gate.blocks_claim() {
                return rpc::CloseoutCycleWaitOutcome::OwnerReleased;
            }
            let lease_wait = current_owner
                .map(|owner| Duration::from_secs(owner.expires_secs.saturating_sub(now_secs)));

            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return rpc::CloseoutCycleWaitOutcome::TimedOut;
            }
            let remaining = timeout.saturating_sub(elapsed);
            let wait = lease_wait.map_or(remaining, |lease| remaining.min(lease));
            self.state_projection_waiters.wait_for(&mut memory, wait);
        }
    }

    fn state_subscribe(
        &self,
        document_hash: &str,
        last_epoch: u64,
    ) -> Result<(agent_doc_state_wire::WireSubscribe, u64)> {
        let memory = self.memory.lock();
        Ok((
            agent_doc_state_wire::subscribe(&memory.state_ledger, document_hash, last_epoch),
            memory
                .state_document_versions
                .get(document_hash)
                .copied()
                .unwrap_or(0),
        ))
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
        Ok(self
            .memory
            .lock()
            .state_projection
            .document(document_hash)
            .cloned())
    }

    fn wait_for_supervisor_recycle_settle(
        &self,
        timeout: Duration,
    ) -> Result<agent_doc_state_backbone::SupervisorRecycleProjection> {
        let started = Instant::now();
        let mut memory = self.memory.lock();
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
            self.supervisor_recycle_waiters
                .wait_for(&mut memory, remaining);
        }
    }

    fn memory_categories(&self) -> Result<BTreeMap<String, usize>> {
        let memory = self.memory.lock();
        Ok(agent_doc_controller::status::status_categories([
            ("actor_records", self.actor_graph.records().len()),
            ("coordination_claims", self.coordination_graph.claim_count()),
            (
                "state_backbone_documents",
                memory.state_projection.documents.len(),
            ),
            (
                "map_backend_std_btree_map",
                usize::from(memory.map_backend == "std_btree_map"),
            ),
            ("reactive_actor_source", 1),
            ("durable_sqlite_sink", 1),
        ]))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoordinationOwner {
    token: String,
    pid: u32,
}

/// Ephemeral controller-owned coordination state.
///
/// Claims are live process facts: they are held in Lazily state and disappear
/// when explicitly released or when the owning process is no longer alive.
/// SQLite remains a durable effect sink and is intentionally absent here.
struct ControllerCoordinationGraph {
    ctx: ThreadSafeContext,
    claims: Source<BTreeMap<String, CoordinationOwner>>,
    mutation: Mutex<()>,
}

impl ControllerCoordinationGraph {
    /// `#stategraphjoin` — claims live for the controller process, so they join the
    /// controller's [`ProcessScope`] rather than a private context. Sharing the scope
    /// with the recycle graph is the point: both are process facts, and a derivation
    /// across them is now possible instead of being blocked by a graph boundary.
    fn new_in(scope: &agent_doc_state_scope::ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        let claims = ctx.source(BTreeMap::new());
        Self {
            ctx,
            claims,
            mutation: Mutex::new(()),
        }
    }

    fn try_claim(&self, scopes: &[String], owner_token: &str, owner_pid: u32) -> bool {
        let _mutation = self.mutation.lock();
        let mut claims = self.ctx.get(&self.claims);
        claims.retain(|_, owner| process_is_alive(owner.pid));
        if scopes.iter().any(|scope| {
            claims
                .get(scope)
                .is_some_and(|owner| owner.token != owner_token)
        }) {
            self.ctx.set(&self.claims, claims);
            return false;
        }
        let owner = CoordinationOwner {
            token: owner_token.to_string(),
            pid: owner_pid,
        };
        for scope in scopes {
            claims.insert(scope.clone(), owner.clone());
        }
        self.ctx.set(&self.claims, claims);
        true
    }

    fn release(&self, scopes: &[String], owner_token: &str) {
        let _mutation = self.mutation.lock();
        let mut claims = self.ctx.get(&self.claims);
        for scope in scopes {
            if claims
                .get(scope)
                .is_some_and(|owner| owner.token == owner_token)
            {
                claims.remove(scope);
            }
        }
        self.ctx.set(&self.claims, claims);
    }

    fn claim_count(&self) -> usize {
        self.ctx.get(&self.claims).len()
    }
}

/// `#stategraphjoin` — controller supervisor-recycle state, joined to the controller's
/// [`ProcessScope`] instead of a private context.
///
/// The recycle projection outlives every document and every turn: it is a fact about
/// the controller process. Naming that lifetime in the type is what stops it from
/// being rebuilt in a document or turn graph, where it would be torn down under a
/// caller still reading it.
struct ControllerSupervisorRecycleGraph {
    ctx: ThreadSafeContext,
    projection: Source<agent_doc_state_backbone::SupervisorRecycleProjection>,
    in_flight: Computed<bool>,
}

impl ControllerSupervisorRecycleGraph {
    fn new_in(
        scope: &agent_doc_state_scope::ProcessScope,
        initial: agent_doc_state_backbone::SupervisorRecycleProjection,
    ) -> Self {
        let ctx = scope.ctx().clone();
        let projection = ctx.source(initial);
        // `#lzcellkernel`: a derived value is a `Computed` read with `get`. The old
        // `signal`/`get_signal` pair is the pre-kernel two-node shape (memo slot plus
        // a puller effect) and is not the vocabulary this codebase derives in.
        let in_flight = ctx.computed(move |ctx| {
            matches!(
                ctx.get(&projection).phase,
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
        self.ctx.set(&self.projection, projection);
    }

    fn projection(&self) -> agent_doc_state_backbone::SupervisorRecycleProjection {
        self.ctx.get(&self.projection)
    }

    fn in_flight(&self) -> bool {
        self.ctx.get(&self.in_flight)
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
    store: &BTreeMap<String, agent_doc_controller::actor::ActorRecord>,
    stats: &mut CrashRecoveryStats,
) -> Result<()> {
    let now = timestamp_secs();
    for record in store.values() {
        if record.state == agent_doc_controller::actor::ActorState::Closed {
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
    pub state: agent_doc_controller::actor::ActorState,
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
    pub record: agent_doc_controller::actor::ActorRecord,
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
    pub record: agent_doc_controller::actor::ActorRecord,
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
    pub record: Option<agent_doc_controller::actor::ActorRecord>,
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
    pub record: Option<agent_doc_controller::actor::ActorRecord>,
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
    pub record: Option<agent_doc_controller::actor::ActorRecord>,
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

impl ControllerRequest {
    /// Build a `command_plane_submit` request carrying a lazily [`CommandSubmit`]
    /// envelope (serialized as JSON in `diagnostic_payload`, the same channel
    /// `closeout_owner_claim`/`release` already use for structured payloads). The
    /// controller routes it by `(namespace, name)` to the domain authority and
    /// returns the terminal [`lazily::CausalReceipt`] — the command-plane's
    /// terminal authority, never a transport ACK.
    pub(crate) fn command_plane_submit(submit_json: String) -> Self {
        Self {
            command: "command_plane_submit".to_string(),
            diagnostic_payload: Some(submit_json),
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
        }
    }
}

#[derive(Debug, Deserialize)]
struct ControllerEnvelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

pub struct LaunchClaim {
    _listener: interprocess::local_socket::Listener,
    waited: bool,
}

impl LaunchClaim {
    /// Non-blocking acquire: fails immediately if another launch owns the
    /// process-lifetime bootstrap endpoint.
    pub fn acquire(project_root: &Path) -> Result<Self> {
        Self::acquire_inner(project_root, None)
    }

    /// Bounded blocking acquire. Bootstrap-claim contention is **not** a hard error:
    /// it means another agent-doc process (a concurrent `start`, a sibling
    /// document's controller launch for the same project root, or a
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
        let canonical_root = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        let root_key = agent_doc_hash::path_string_hash(canonical_root.to_string_lossy().as_ref());
        let claim_name = format!("agent-doc-controller-launch-{root_key}");
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut waited = false;
        loop {
            let name = claim_name
                .as_str()
                .to_ns_name::<GenericNamespaced>()
                .context("failed to map controller bootstrap claim name")?;
            match ListenerOptions::new().name(name).create_sync() {
                Ok(listener) => {
                    return Ok(Self {
                        _listener: listener,
                        waited,
                    });
                }
                Err(err) => {
                    let contended = matches!(
                        err.kind(),
                        std::io::ErrorKind::AddrInUse | std::io::ErrorKind::WouldBlock
                    );
                    match deadline {
                        Some(deadline) if contended && Instant::now() < deadline => {
                            waited = true;
                            std::thread::sleep(LAUNCH_CLAIM_POLL);
                            continue;
                        }
                        _ => {
                            return Err(err).with_context(|| {
                                format!(
                                    "controller launch already in progress for bootstrap claim {claim_name}"
                                )
                            });
                        }
                    }
                }
            }
        }
    }
}

pub fn read_bootstrap(project_root: &Path) -> Result<Option<ControllerBootstrap>> {
    let conn = open_state_db(project_root)?;
    state_store::load_controller_bootstrap_json_from_db(&conn, CONTROLLER_BOOTSTRAP_SCOPE)?
        .map(|json| {
            serde_json::from_str::<ControllerBootstrap>(&json)
                .context("failed to parse controller bootstrap from state.db")
        })
        .transpose()
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
    let conn = open_state_db(&bootstrap.project_root)?;
    let json = serde_json::to_string(bootstrap)?;
    state_store::store_controller_bootstrap_json_in_db(&conn, CONTROLLER_BOOTSTRAP_SCOPE, &json)
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
    record: &agent_doc_controller::actor::ActorRecord,
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

pub(crate) fn append_state_event(
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

/// Direct durable-ledger insertion for isolated projector fixtures.
///
/// Runtime code must use [`publish_state_event`] so the controller appends and
/// applies the fact to its live reactive graph in the same serialized turn.
#[doc(hidden)]
pub fn append_state_event_for_test(
    project_root: &Path,
    event: &agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    append_state_event(project_root, event)
}

pub fn load_state_event_ledger(
    project_root: &Path,
) -> Result<agent_doc_state_backbone::EventLedger> {
    Ok(load_state_event_ledger_with_versions(project_root)?.0)
}

fn load_state_event_ledger_with_versions(
    project_root: &Path,
) -> Result<(agent_doc_state_backbone::EventLedger, BTreeMap<String, u64>)> {
    let conn = open_state_db(project_root)?;
    let mut ledger = agent_doc_state_backbone::EventLedger::new();
    let mut document_versions: BTreeMap<String, u64> = BTreeMap::new();
    for row in load_state_events_from_db(&conn, None)? {
        document_versions
            .entry(row.document_hash.clone())
            .and_modify(|version| *version = (*version).max(row.document_version))
            .or_insert(row.document_version);
        let event: agent_doc_state_backbone::StateEvent = serde_json::from_str(&row.payload_json)
            .with_context(|| {
            format!(
                "parse state backbone event {} from controller state",
                row.event_id
            )
        })?;
        ledger.append(event);
    }
    Ok((ledger, document_versions))
}

pub fn load_state_backbone_projection(
    project_root: &Path,
) -> Result<agent_doc_state_backbone::StateBackboneProjection> {
    Ok(load_state_event_ledger(project_root)?.project())
}

pub fn upsert_coordination_lease(
    project_root: &Path,
    lease: &state_store::CoordinationLeaseRecord,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
    state_store::upsert_coordination_lease_in_db(&conn, lease)
}

pub fn load_coordination_lease(
    project_root: &Path,
    scope_kind: &str,
    scope_id: &str,
) -> Result<Option<state_store::CoordinationLeaseRecord>> {
    let conn = open_state_db(project_root)?;
    state_store::load_coordination_lease_from_db(&conn, scope_kind, scope_id)
}

pub fn clear_coordination_lease(
    project_root: &Path,
    scope_kind: &str,
    scope_id: &str,
) -> Result<bool> {
    let conn = open_state_db(project_root)?;
    state_store::clear_coordination_lease_in_db(&conn, scope_kind, scope_id)
}

pub use agent_doc_sqlite::state_store::EditorTransportHealthRecord;

pub fn upsert_editor_transport_health(
    project_root: &Path,
    health: &EditorTransportHealthRecord,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
    state_store::upsert_editor_transport_health_in_db(&conn, health)
}

pub fn load_editor_transport_health(
    project_root: &Path,
    document_hash: &str,
) -> Result<Option<EditorTransportHealthRecord>> {
    let conn = open_state_db(project_root)?;
    state_store::load_editor_transport_health_from_db(&conn, document_hash)
}

pub fn clear_editor_transport_health(project_root: &Path, document_hash: &str) -> Result<bool> {
    let conn = open_state_db(project_root)?;
    state_store::clear_editor_transport_health_in_db(&conn, document_hash)
}

pub fn load_actor_store(
    project_root: &Path,
) -> Result<BTreeMap<String, agent_doc_controller::actor::ActorRecord>> {
    let conn = open_state_db(project_root)?;
    load_actor_store_from_db(&conn)
}

pub fn load_actor_record(
    project_root: &Path,
    document_id: &str,
) -> Result<Option<agent_doc_controller::actor::ActorRecord>> {
    let conn = open_state_db(project_root)?;
    load_actor_record_from_db(&conn, document_id)
}

pub fn store_actor_record(
    project_root: &Path,
    expected_prior_generation: Option<u64>,
    record: &agent_doc_controller::actor::ActorRecord,
) -> Result<agent_doc_controller::actor::ActorRecord> {
    Ok(store_actor_record_write(project_root, expected_prior_generation, record)?.record)
}

fn store_actor_record_write(
    project_root: &Path,
    expected_prior_generation: Option<u64>,
    record: &agent_doc_controller::actor::ActorRecord,
) -> Result<agent_doc_controller::actor::ActorStoreWrite> {
    let mut conn = open_state_db(project_root)?;
    let (launch_mode, controller_epoch) = actor_document_bootstrap_columns(project_root)?;
    state_store::store_actor_record_tx(
        &mut conn,
        expected_prior_generation,
        record,
        launch_mode,
        controller_epoch,
    )
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
        if record.state != agent_doc_controller::actor::ActorState::Starting {
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
        next.state = agent_doc_controller::actor::ActorState::Closed;
        next.last_transition = agent_doc_controller::actor::ActorLastTransition {
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
        if record.state == agent_doc_controller::actor::ActorState::Closed
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
        next.state = agent_doc_controller::actor::ActorState::Closed;
        next.pane_id.clear();
        next.window_id.clear();
        next.last_transition = agent_doc_controller::actor::ActorLastTransition {
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
        if record.state != agent_doc_controller::actor::ActorState::Closed {
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
        next.state = agent_doc_controller::actor::ActorState::Closed;
        next.pane_id.clear();
        next.window_id.clear();
        next.last_transition = agent_doc_controller::actor::ActorLastTransition {
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

pub fn load_layout_state(project_root: &Path) -> Result<Vec<String>> {
    let conn = open_state_db(project_root)?;
    load_layout_state_from_db(&conn, DEFAULT_LAYOUT_SCOPE)
}

pub fn store_layout_state(project_root: &Path, columns: &[String]) -> Result<()> {
    let conn = open_state_db(project_root)?;
    store_layout_state_in_db(&conn, DEFAULT_LAYOUT_SCOPE, columns)
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

#[cfg(test)]
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
        if status(project_root)
            .map(|controller| controller.active)
            .unwrap_or(false)
        {
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

    struct CountingStatePlaneSink(Arc<AtomicUsize>);

    impl ControllerStatePlaneSink for CountingStatePlaneSink {
        fn project(&self, _frame: ControllerStatePlaneFrame) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

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
    }

    #[test]
    fn manual_and_projection_autostart_sync_route_created_panes() {
        let invocation =
            |caller_kind: &str, no_autostart: bool| ControllerTmuxLayoutSyncInvocation {
                columns: vec!["tasks/one.md".to_string()],
                window: None,
                focus: Some("tasks/one.md".to_string()),
                no_autostart,
                exact_visible: true,
                caller_kind: caller_kind.to_string(),
                actor_bindings: Vec::new(),
            };

        assert!(invocation("manual", false).routes_created_panes());
        assert!(invocation("projection", false).routes_created_panes());
        assert!(!invocation("automatic", false).routes_created_panes());
        assert!(!invocation("manual", true).routes_created_panes());
    }

    fn pane_layout_desired_for_test(generation: u64) -> PaneLayoutDesired {
        PaneLayoutDesired {
            generation,
            source_plane_version: Some(41),
            invocation: ControllerTmuxLayoutSyncInvocation {
                columns: vec!["tasks/one.md".to_string(), "tasks/two.md".to_string()],
                window: Some("agent-doc".to_string()),
                focus: Some("tasks/two.md".to_string()),
                no_autostart: false,
                exact_visible: true,
                caller_kind: "projection".to_string(),
                actor_bindings: Vec::new(),
            },
        }
    }

    fn actor_record_for_test(
        document_id: &str,
        pane_id: &str,
        state: agent_doc_controller::actor::ActorState,
    ) -> agent_doc_controller::actor::ActorRecord {
        agent_doc_controller::actor::ActorRecord {
            document_id: document_id.to_string(),
            session_id: format!("session-{pane_id}"),
            generation: 7,
            pane_id: pane_id.to_string(),
            window_id: "@1".to_string(),
            harness: "codex".to_string(),
            state,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
                caller: "test".to_string(),
                reason: "reactive projection".to_string(),
                timestamp: 1,
                prior_generation: 6,
                new_generation: 7,
            },
        }
    }

    #[test]
    fn pane_layout_actor_authority_is_a_reactive_join() {
        let root = tempfile::TempDir::new().unwrap();
        let document = root.path().join("tasks/one.md");
        let document_string = document.display().to_string();
        let document_id =
            agent_doc_session_actor_io::canonical_document_id_in(root.path(), &document_string);
        let ready = actor_record_for_test(
            &document_id,
            "%41",
            agent_doc_controller::actor::ActorState::Ready,
        );

        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph =
            ControllerActorGraph::new_in(&scope, BTreeMap::from([(document_id.clone(), ready)]));
        let pane_graph = ControllerPaneLayoutGraph::new_in(
            &scope,
            Vec::new(),
            actor_graph.live_bindings_handle(),
        );
        pane_graph.set_desired(
            ControllerTmuxLayoutSyncInvocation {
                columns: vec![document_string.clone()],
                window: Some("@1".to_string()),
                focus: Some(document_string.clone()),
                no_autostart: true,
                exact_visible: true,
                caller_kind: "automatic".to_string(),
                actor_bindings: Vec::new(),
            },
            None,
        );

        assert_eq!(
            pane_graph.actor_bindings(),
            vec![ControllerTmuxActorBinding {
                document_path: document_string,
                session_id: "session-%41".to_string(),
                pane_id: "%41".to_string(),
                generation: 7,
            }]
        );

        let closed = actor_record_for_test(
            &document_id,
            "%41",
            agent_doc_controller::actor::ActorState::Closed,
        );
        actor_graph.set(BTreeMap::from([(document_id, closed)]));
        assert!(
            pane_graph.actor_bindings().is_empty(),
            "closing the actor Source must invalidate the pane-layout authority Computed"
        );
    }

    #[test]
    fn document_turn_authority_reacts_to_actor_and_closeout_sources() {
        use agent_doc_turn::cp_projection::TurnState;

        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph = ControllerActorGraph::new_in(&scope, BTreeMap::new());
        let document_graphs = ControllerDocumentGraphs::new_in(&scope);
        let authority_graph = ControllerDocumentAuthorityGraph::new_in(
            &scope,
            actor_graph.document_model_states_handle(),
            document_graphs.projection_handle(),
        );
        let document_hash = "authority-reactive";
        let document_id = "document-authority-reactive";

        assert_eq!(
            authority_graph.projection(document_hash, document_id).state,
            TurnState::Idle
        );

        actor_graph.set(BTreeMap::from([(
            document_id.to_string(),
            actor_record_for_test(
                document_id,
                "%42",
                agent_doc_controller::actor::ActorState::Busy,
            ),
        )]));
        assert_eq!(
            authority_graph.projection(document_hash, document_id).state,
            TurnState::AwaitingResponse
        );

        let mut document = agent_doc_state_backbone::DocumentStateProjection::new(document_hash);
        document.closeout.phase = Some(agent_doc_turn::CyclePhase::WriteApplied);
        document_graphs.set_projection(document_hash, Some(document));
        assert_eq!(
            authority_graph.projection(document_hash, document_id).state,
            TurnState::Persisting
        );

        actor_graph.set(BTreeMap::from([(
            document_id.to_string(),
            actor_record_for_test(
                document_id,
                "%42",
                agent_doc_controller::actor::ActorState::Ready,
            ),
        )]));
        assert_eq!(
            authority_graph.projection(document_hash, document_id).state,
            TurnState::Idle
        );
    }

    #[test]
    fn document_turn_authority_does_not_recompute_for_an_unrelated_actor() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let document_a = "document-authority-a";
        let document_b = "document-authority-b";
        let actor_graph = ControllerActorGraph::new_in(
            &scope,
            BTreeMap::from([
                (
                    document_a.to_string(),
                    actor_record_for_test(
                        document_a,
                        "%41",
                        agent_doc_controller::actor::ActorState::Ready,
                    ),
                ),
                (
                    document_b.to_string(),
                    actor_record_for_test(
                        document_b,
                        "%42",
                        agent_doc_controller::actor::ActorState::Ready,
                    ),
                ),
            ]),
        );
        let document_graphs = ControllerDocumentGraphs::new_in(&scope);
        let authority_graph = ControllerDocumentAuthorityGraph::new_in(
            &scope,
            actor_graph.document_model_states_handle(),
            document_graphs.projection_handle(),
        );
        let projection_a = authority_graph.projection_handle("authority-a", document_a);
        let projection_b = authority_graph.projection_handle("authority-b", document_b);
        let runs_a = Arc::new(AtomicUsize::new(0));
        let runs_b = Arc::new(AtomicUsize::new(0));
        let _effect_a = {
            let runs = Arc::clone(&runs_a);
            scope.ctx().effect(move |ctx| {
                let _ = ctx.get(&projection_a);
                runs.fetch_add(1, Ordering::SeqCst);
            })
        };
        let _effect_b = {
            let runs = Arc::clone(&runs_b);
            scope.ctx().effect(move |ctx| {
                let _ = ctx.get(&projection_b);
                runs.fetch_add(1, Ordering::SeqCst);
            })
        };
        assert_eq!(runs_a.load(Ordering::SeqCst), 1);
        assert_eq!(runs_b.load(Ordering::SeqCst), 1);

        actor_graph.set(BTreeMap::from([
            (
                document_a.to_string(),
                actor_record_for_test(
                    document_a,
                    "%41",
                    agent_doc_controller::actor::ActorState::Busy,
                ),
            ),
            (
                document_b.to_string(),
                actor_record_for_test(
                    document_b,
                    "%42",
                    agent_doc_controller::actor::ActorState::Ready,
                ),
            ),
        ]));

        assert_eq!(runs_a.load(Ordering::SeqCst), 2);
        assert_eq!(
            runs_b.load(Ordering::SeqCst),
            1,
            "one actor transition must not invalidate another document's authority",
        );
    }

    #[test]
    fn document_turn_authority_effect_is_shared_and_released_with_last_stream() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph = ControllerActorGraph::new_in(&scope, BTreeMap::new());
        let document_graphs = ControllerDocumentGraphs::new_in(&scope);
        let authority_graph = ControllerDocumentAuthorityGraph::new_in(
            &scope,
            actor_graph.document_model_states_handle(),
            document_graphs.projection_handle(),
        );

        authority_graph.acquire_subscription("shared-authority", "shared-document");
        authority_graph.acquire_subscription("shared-authority", "shared-document");
        assert_eq!(authority_graph.effects.lock().len(), 1);
        assert_eq!(authority_graph.projections.present_count(), 1);

        authority_graph.release_subscription("shared-authority", "shared-document");
        assert_eq!(authority_graph.effects.lock().len(), 1);
        authority_graph.release_subscription("shared-authority", "shared-document");
        assert!(authority_graph.effects.lock().is_empty());
        assert_eq!(authority_graph.projections.present_count(), 0);
    }

    #[test]
    fn pane_layout_projection_is_derived_from_desired_observed_and_effect_state() {
        let desired = pane_layout_desired_for_test(7);
        let actor_bindings = vec![ControllerTmuxActorBinding {
            document_path: "tasks/one.md".to_string(),
            session_id: "session-one".to_string(),
            pane_id: "%1".to_string(),
            generation: 3,
        }];
        assert_eq!(
            derive_pane_layout_projection(
                None,
                actor_bindings.clone(),
                None,
                PaneLayoutEffectReceipt::default(),
            ),
            PaneLayoutProjection::Absent
        );
        assert_eq!(
            derive_pane_layout_projection(
                Some(desired.clone()),
                actor_bindings.clone(),
                None,
                PaneLayoutEffectReceipt::default(),
            ),
            PaneLayoutProjection::NeedsEffect(desired.clone())
        );
        assert_eq!(
            derive_pane_layout_projection(
                Some(desired.clone()),
                actor_bindings.clone(),
                None,
                PaneLayoutEffectReceipt {
                    generation: 7,
                    actor_bindings: actor_bindings.clone(),
                    attempt: 1,
                    phase: PaneLayoutEffectPhase::InFlight,
                    reason: "applying".to_string(),
                    file_panes: Vec::new(),
                    focus_required: true,
                    focus_applied: false,
                },
            ),
            PaneLayoutProjection::Applying(desired.clone())
        );

        let mismatched = PaneLayoutObservation {
            generation: 7,
            actor_bindings: actor_bindings.clone(),
            report: ControllerTmuxLayoutSyncStateReport {
                synced: false,
                reason: "pane_order_mismatch".to_string(),
                expected_documents: desired.invocation.columns.clone(),
                actual_documents: vec!["tasks/two.md".to_string()],
                panes: vec!["%1".to_string()],
                session_name: Some("agent-doc".to_string()),
                window_id: Some("@1".to_string()),
                window_name: Some("agent-doc".to_string()),
                focus: desired.invocation.focus.clone(),
            },
        };
        assert_eq!(
            derive_pane_layout_projection(
                Some(desired.clone()),
                actor_bindings.clone(),
                Some(mismatched.clone()),
                PaneLayoutEffectReceipt {
                    generation: 7,
                    actor_bindings: actor_bindings.clone(),
                    attempt: 1,
                    phase: PaneLayoutEffectPhase::RetryPending,
                    reason: "retry_scheduled".to_string(),
                    file_panes: Vec::new(),
                    focus_required: true,
                    focus_applied: false,
                },
            ),
            PaneLayoutProjection::RetryPending(desired.clone())
        );
        let changed_actor_bindings = vec![ControllerTmuxActorBinding {
            generation: 4,
            ..actor_bindings[0].clone()
        }];
        assert_eq!(
            derive_pane_layout_projection(
                Some(desired.clone()),
                changed_actor_bindings,
                Some(mismatched),
                PaneLayoutEffectReceipt {
                    generation: 7,
                    actor_bindings: actor_bindings.clone(),
                    attempt: 1,
                    phase: PaneLayoutEffectPhase::RetryPending,
                    reason: "retry_scheduled".to_string(),
                    file_panes: Vec::new(),
                    focus_required: true,
                    focus_applied: false,
                },
            ),
            PaneLayoutProjection::NeedsEffect(desired.clone()),
            "a changed actor-binding projection must reactivate the exact layout effect without a timer",
        );

        let converged = PaneLayoutObservation {
            generation: 7,
            actor_bindings: actor_bindings.clone(),
            report: ControllerTmuxLayoutSyncStateReport {
                synced: true,
                reason: "synced".to_string(),
                expected_documents: desired.invocation.columns.clone(),
                actual_documents: desired.invocation.columns.clone(),
                panes: vec!["%1".to_string(), "%2".to_string()],
                session_name: Some("agent-doc".to_string()),
                window_id: Some("@1".to_string()),
                window_name: Some("agent-doc".to_string()),
                focus: desired.invocation.focus.clone(),
            },
        };
        assert_eq!(
            derive_pane_layout_projection(
                Some(desired.clone()),
                actor_bindings.clone(),
                Some(converged.clone()),
                PaneLayoutEffectReceipt {
                    generation: 7,
                    actor_bindings: actor_bindings.clone(),
                    attempt: 1,
                    phase: PaneLayoutEffectPhase::RetryPending,
                    reason: "focus_pane_not_found".to_string(),
                    file_panes: Vec::new(),
                    focus_required: true,
                    focus_applied: false,
                },
            ),
            PaneLayoutProjection::RetryPending(desired.clone()),
            "matching columns alone must not retire the projection while the newly selected document is still unfocused",
        );
        assert_eq!(
            derive_pane_layout_projection(
                Some(desired.clone()),
                actor_bindings.clone(),
                Some(converged),
                PaneLayoutEffectReceipt {
                    generation: 7,
                    actor_bindings,
                    attempt: 2,
                    phase: PaneLayoutEffectPhase::Converged,
                    reason: "observed_layout_and_focus_convergence".to_string(),
                    file_panes: Vec::new(),
                    focus_required: true,
                    focus_applied: true,
                },
            ),
            PaneLayoutProjection::Converged(desired),
        );
    }

    #[test]
    fn pane_layout_status_correlates_to_the_exact_desired_plane_version() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph = ControllerActorGraph::new_in(&scope, BTreeMap::new());
        let graph = ControllerPaneLayoutGraph::new_in(
            &scope,
            Vec::new(),
            actor_graph.live_bindings_handle(),
        );
        let desired = pane_layout_desired_for_test(1);
        graph.set_desired(desired.invocation, Some(73));

        let status = graph.state_projection().unwrap();
        assert_eq!(status.source_plane_version, Some(73));
        assert_eq!(status.phase, ControllerPaneLayoutPhase::NeedsEffect);
    }

    #[test]
    fn identical_pane_layout_desired_is_deduplicated_without_resetting_projection() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph = ControllerActorGraph::new_in(&scope, BTreeMap::new());
        let graph = ControllerPaneLayoutGraph::new_in(
            &scope,
            Vec::new(),
            actor_graph.live_bindings_handle(),
        );
        let invocation = pane_layout_desired_for_test(1).invocation;
        let first = graph.set_desired(invocation.clone(), Some(81));
        let duplicate = graph.set_desired(invocation, Some(82));

        assert_eq!(duplicate.generation, first.generation);
        assert_eq!(duplicate.source_plane_version, Some(82));
        assert_eq!(
            graph.state_projection().unwrap().source_plane_version,
            Some(82)
        );
    }

    #[test]
    fn structurally_converged_layout_is_reused_for_a_focus_only_generation() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph = ControllerActorGraph::new_in(&scope, BTreeMap::new());
        let graph = ControllerPaneLayoutGraph::new_in(
            &scope,
            Vec::new(),
            actor_graph.live_bindings_handle(),
        );
        let first = graph.set_desired(pane_layout_desired_for_test(1).invocation, Some(81));
        let assignment = vec![
            ("tasks/one.md".to_string(), "%1".to_string()),
            ("tasks/two.md".to_string(), "%2".to_string()),
        ];
        let report = ControllerTmuxLayoutSyncStateReport {
            synced: true,
            reason: "synced".to_string(),
            expected_documents: first.invocation.columns.clone(),
            actual_documents: first.invocation.columns.clone(),
            panes: vec!["%1".to_string(), "%2".to_string()],
            session_name: Some("agent-doc".to_string()),
            window_id: Some("@1".to_string()),
            window_name: Some("agent-doc".to_string()),
            focus: first.invocation.focus.clone(),
        };
        graph.record_structural_assignment(
            &first,
            graph.actor_bindings(),
            Some(report.clone()),
            assignment.clone(),
        );

        let mut focus_only = first.invocation.clone();
        focus_only.focus = Some("tasks/one.md".to_string());
        let second = graph.set_desired(focus_only, Some(82));
        let reusable = graph
            .reusable_structural_receipt(&second, &graph.actor_bindings())
            .unwrap();

        assert_eq!(reusable.file_panes, assignment);
        assert_eq!(reusable.report, Some(report));
        let changed_actor_bindings = vec![ControllerTmuxActorBinding {
            document_path: "tasks/one.md".to_string(),
            session_id: "session-one".to_string(),
            pane_id: "%9".to_string(),
            generation: 9,
        }];
        assert!(
            graph
                .reusable_structural_receipt(&second, &changed_actor_bindings)
                .is_none(),
            "a structural assignment cannot be reused after its actor-binding projection changes",
        );

        let mut structural_change = second.invocation.clone();
        structural_change.columns.reverse();
        let third = graph.set_desired(structural_change, Some(83));
        assert!(
            graph
                .reusable_structural_receipt(&third, &graph.actor_bindings())
                .is_none()
        );
    }

    #[test]
    fn pane_layout_effect_assignment_is_fenced_by_desired_generation() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let actor_graph = ControllerActorGraph::new_in(&scope, BTreeMap::new());
        let graph = ControllerPaneLayoutGraph::new_in(
            &scope,
            Vec::new(),
            actor_graph.live_bindings_handle(),
        );
        let first = graph.set_desired(pane_layout_desired_for_test(1).invocation, Some(81));
        let first_assignment = vec![("tasks/primary.md".to_string(), "%1".to_string())];
        graph.record_receipt(PaneLayoutEffectReceipt {
            generation: first.generation,
            actor_bindings: graph.actor_bindings(),
            attempt: 1,
            phase: PaneLayoutEffectPhase::Converged,
            reason: "observed_convergence".to_string(),
            file_panes: first_assignment.clone(),
            focus_required: true,
            focus_applied: true,
        });
        assert_eq!(graph.effect_file_panes(first.generation), first_assignment);

        let mut second_invocation = pane_layout_desired_for_test(2).invocation;
        second_invocation.focus = Some("tasks/one.md".to_string());
        let second = graph.set_desired(second_invocation, Some(82));
        graph.record_receipt(PaneLayoutEffectReceipt {
            generation: first.generation,
            actor_bindings: graph.actor_bindings(),
            attempt: 2,
            phase: PaneLayoutEffectPhase::Converged,
            reason: "late_prior_generation".to_string(),
            file_panes: vec![("tasks/primary.md".to_string(), "%1".to_string())],
            focus_required: true,
            focus_applied: true,
        });

        assert!(
            graph.effect_file_panes(second.generation).is_empty(),
            "a late receipt from the prior desired generation cannot identify panes for the new layout"
        );
    }

    #[test]
    fn state_plane_requires_causal_deltas_and_replays_from_covering_snapshot() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let plane = ControllerStatePlaneGraph::new_in(&scope);
        let (snapshot, duplicate) = plane
            .publish(
                "test/layout".to_string(),
                "editor-a".to_string(),
                r#"{"Snapshot":{"epoch":1}}"#.to_string(),
                1,
                None,
            )
            .unwrap();
        assert!(!duplicate);
        assert_eq!(snapshot.plane_version, 1);

        let (_, duplicate) = plane
            .publish(
                "test/layout".to_string(),
                "editor-a".to_string(),
                r#"{"Snapshot":{"epoch":1}}"#.to_string(),
                1,
                None,
            )
            .unwrap();
        assert!(duplicate);

        let (delta, duplicate) = plane
            .publish(
                "test/layout".to_string(),
                "editor-a".to_string(),
                r#"{"Delta":{"epoch":2,"base_epoch":1}}"#.to_string(),
                2,
                Some(1),
            )
            .unwrap();
        assert!(!duplicate);
        assert_eq!(delta.plane_version, 2);
        let replay = plane.subscribe("test/layout", 0, false, Duration::ZERO);
        assert_eq!(replay.frames, vec![snapshot, delta]);

        let error = plane
            .publish(
                "test/layout".to_string(),
                "editor-b".to_string(),
                r#"{"Delta":{"epoch":2,"base_epoch":1}}"#.to_string(),
                2,
                Some(1),
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("producer changed without a covering Snapshot")
        );

        let (replacement, _) = plane
            .publish(
                "test/layout".to_string(),
                "editor-b".to_string(),
                r#"{"Snapshot":{"epoch":1}}"#.to_string(),
                1,
                None,
            )
            .unwrap();
        let cold_replay = plane.subscribe("test/layout", 0, false, Duration::ZERO);
        assert_eq!(cold_replay.frames, vec![replacement.clone()]);
        assert!(
            plane
                .subscribe(
                    "test/layout",
                    replacement.plane_version,
                    false,
                    Duration::ZERO,
                )
                .timed_out
        );
        let legacy_replacement_replay =
            plane.subscribe("test/layout", u64::MAX, true, Duration::ZERO);
        assert_eq!(legacy_replacement_replay.frames, vec![replacement]);
        assert!(!legacy_replacement_replay.timed_out);
    }

    #[test]
    fn state_plane_channel_update_projects_only_that_channel() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let plane = ControllerStatePlaneGraph::new_in(&scope);
        let channel_a_runs = Arc::new(AtomicUsize::new(0));
        let channel_b_runs = Arc::new(AtomicUsize::new(0));
        plane.install_sink(
            "test/channel-a",
            Arc::new(CountingStatePlaneSink(Arc::clone(&channel_a_runs))),
        );
        plane.install_sink(
            "test/channel-b",
            Arc::new(CountingStatePlaneSink(Arc::clone(&channel_b_runs))),
        );
        let channel_a_dependency = plane.channel_dependency("test/channel-a");
        let channel_b_dependency = plane.channel_dependency("test/channel-b");
        let channel_a_revision = channel_a_dependency.revision.load(Ordering::SeqCst);
        let channel_b_revision = channel_b_dependency.revision.load(Ordering::SeqCst);

        plane
            .publish(
                "test/channel-a".to_string(),
                "producer-a".to_string(),
                r#"{"Snapshot":{"epoch":1}}"#.to_string(),
                1,
                None,
            )
            .unwrap();
        assert_eq!(channel_a_runs.load(Ordering::SeqCst), 1);
        assert_eq!(channel_b_runs.load(Ordering::SeqCst), 0);
        assert!(channel_a_dependency.revision.load(Ordering::SeqCst) > channel_a_revision);
        assert_eq!(
            channel_b_dependency.revision.load(Ordering::SeqCst),
            channel_b_revision,
            "publishing one channel must not wake another channel's subscribers",
        );

        let channel_a_revision = channel_a_dependency.revision.load(Ordering::SeqCst);
        plane
            .publish(
                "test/channel-b".to_string(),
                "producer-b".to_string(),
                r#"{"Snapshot":{"epoch":1}}"#.to_string(),
                1,
                None,
            )
            .unwrap();
        assert_eq!(
            channel_a_runs.load(Ordering::SeqCst),
            1,
            "publishing one channel must not replay every other sink",
        );
        assert_eq!(
            channel_a_dependency.revision.load(Ordering::SeqCst),
            channel_a_revision,
            "publishing one channel must not wake another channel's subscribers",
        );
        assert_eq!(channel_b_runs.load(Ordering::SeqCst), 1);

        plane.retire_channel("test/channel-a");
        assert!(!plane.histories.is_present(&"test/channel-a".to_string()));
        assert!(!plane.channel_effects.lock().contains_key("test/channel-a"));
        assert!(
            !plane
                .channel_dependencies
                .lock()
                .contains_key("test/channel-a")
        );
        assert!(!plane.sinks.lock().contains_key("test/channel-a"));
    }

    #[test]
    fn state_plane_versions_advance_across_controller_generation_namespaces() {
        let prior = state_plane_first_version(41);
        let replacement = state_plane_first_version(42);
        assert_eq!(prior, 41_u64 << STATE_PLANE_VERSION_NAMESPACE_BITS | 1);
        assert_eq!(
            replacement,
            42_u64 << STATE_PLANE_VERSION_NAMESPACE_BITS | 1
        );
        assert!(
            replacement > prior.saturating_add(u32::MAX as u64),
            "a replacement controller must outrank every version in the prior generation"
        );
    }

    #[test]
    fn state_plane_subscription_resets_cursor_on_controller_replacement() {
        assert_eq!(state_plane_effective_after_version(Some(41), 99, 42), 0);
        assert_eq!(state_plane_effective_after_version(Some(42), 99, 42), 99);
        assert_eq!(state_plane_effective_after_version(None, 99, 42), 99);
    }

    #[test]
    fn controller_replacement_replays_current_state_below_a_stale_cursor() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut bootstrap = test_bootstrap(&dir);
        bootstrap.controller_generation = 42;
        let runtime = ControllerRuntime::new_arc(bootstrap).unwrap();
        let (frame, duplicate) = runtime
            .publish_state_plane_frame(
                "test/replacement".to_string(),
                "replacement-controller".to_string(),
                r#"{"Snapshot":{"epoch":1}}"#.to_string(),
                1,
                None,
            )
            .unwrap();
        assert!(!duplicate);

        let replay =
            runtime.subscribe_state_plane("test/replacement", Some(41), u64::MAX, Duration::ZERO);
        assert_eq!(replay.controller_generation, 42);
        assert_eq!(replay.frames, vec![frame.clone()]);
        assert!(!replay.timed_out);

        let legacy_replay =
            runtime.subscribe_state_plane("test/replacement", None, u64::MAX, Duration::ZERO);
        assert_eq!(legacy_replay.controller_generation, 42);
        assert_eq!(legacy_replay.frames, vec![frame.clone()]);
        assert!(!legacy_replay.timed_out);

        let warm = runtime.subscribe_state_plane(
            "test/replacement",
            Some(42),
            frame.plane_version,
            Duration::ZERO,
        );
        assert!(warm.frames.is_empty());
        assert!(warm.timed_out);
    }

    #[test]
    fn controller_projection_starts_after_retiring_removed_state_facts() {
        let dir = tempfile::TempDir::new().unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        conn.execute("DELETE FROM state_schema_migrations", [])
            .unwrap();
        agent_doc_sqlite::state_store::insert_state_event_in_db(
            &conn,
            &agent_doc_sqlite::state_store::StateEventInsert {
                event_id: "legacy-pending-response-captured",
                document_hash: "doc-hash",
                domain: "closeout",
                fact_type: "pending_response_captured",
                payload_json: r#"{"event_id":"legacy-pending-response-captured","fact":{"type":"pending_response_captured"}}"#,
            },
        )
        .unwrap();
        drop(conn);

        let projection = load_state_backbone_projection(dir.path()).unwrap();
        assert!(projection.documents.is_empty());
    }

    #[test]
    fn write_then_read_bootstrap_roundtrips() {
        let dir = tempfile::TempDir::new().unwrap();
        let bootstrap = test_bootstrap(&dir);
        write_bootstrap_state(&bootstrap).unwrap();
        let read = read_bootstrap(dir.path())
            .unwrap()
            .expect("bootstrap present after write");
        assert_eq!(read.controller_generation, bootstrap.controller_generation);
        assert_eq!(read.project_root, bootstrap.project_root);
        assert!(!dir.path().join(".agent-doc/controller-state.json").exists());
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
        record.state = agent_doc_controller::actor::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let summary =
            checkpoint_route_owned_documents_for_project(dir.path(), "test_recycle").unwrap();
        assert_eq!(summary.detached, 1);
        assert_eq!(summary.failed, 0);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_crdt_checkpoint_skipped"));
        assert!(ops_log.contains("reason=detached_authority"));
    }

    fn seed_reliable_sync_editor_open(doc: &std::path::Path, tag: &str) {
        let document_hash = agent_doc_hash::document_id_for_path(doc);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: format!("{tag}:{}", doc.display()),
            }]);
    }

    #[test]
    fn crdt_projection_is_unavailable_without_controller_model() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/editor.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        seed_reliable_sync_editor_open(&doc, "jetbrains-test-deferred");
        let mut record = actor_record(&document_id, "%41", "@1");
        record.state = agent_doc_controller::actor::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let summary =
            checkpoint_route_owned_documents_for_project(dir.path(), "test_recycle").unwrap();
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.detached, 0);
        assert_eq!(summary.skipped, 1);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_crdt_checkpoint"));
        assert!(ops_log.contains("status=unavailable"));
        assert!(ops_log.contains("recovery=retained_lazily_projection"));
        assert!(!ops_log.contains("supervisor_crdt_checkpoint"));
    }

    #[test]
    fn recycle_controller_continues_when_projection_is_unavailable() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/recycle-editor.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let document_id = doc.to_string_lossy().to_string();
        seed_reliable_sync_editor_open(&doc, "jetbrains-test-recycle");
        let mut record = actor_record(&document_id, "%41", "@1");
        record.state = agent_doc_controller::actor::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let recycled = recycle_controller_force(dir.path(), false).unwrap();

        assert!(!recycled, "no live controller was present to recycle");
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("controller_crdt_checkpoint"));
        assert!(ops_log.contains("status=unavailable"));
        assert!(ops_log.contains("recovery=retained_lazily_projection"));
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
        seed_reliable_sync_editor_open(&doc, "jetbrains-test-current");
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, "intellij:test")
            .unwrap()
            .expect("editor-attached register should allocate model");
        let mut record = actor_record(&document_id, "%41", "@1");
        record.state = agent_doc_controller::actor::ActorState::Ready;
        store_actor_record(dir.path(), Some(0), &record).unwrap();

        let summary =
            checkpoint_route_owned_documents_for_project(dir.path(), "test_recycle").unwrap();
        assert_eq!(summary.failed, 0);
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();

        // `#ctrlchkptiso`: the observation runs through the controller's own
        // retained Lazily projection and never delegates to the supervisor or
        // consults a process-global hub-owner flag.
        assert!(ops_log.contains("controller_crdt_checkpoint"));
        assert!(
            ops_log.contains("authority=cp_model")
                && ops_log.contains("transport=local_document_model"),
            "the projection observation must use the controller document model: {ops_log}"
        );
        assert!(!ops_log.contains("supervisor_crdt_checkpoint"));
        assert!(ops_log.contains("status=checkpointed"));
    }

    fn actor_record(
        document_id: &str,
        pane: &str,
        window: &str,
    ) -> agent_doc_controller::actor::ActorRecord {
        agent_doc_controller::actor::ActorRecord {
            document_id: document_id.to_string(),
            session_id: "session-1".to_string(),
            generation: 1,
            pane_id: pane.to_string(),
            window_id: window.to_string(),
            harness: "codex".to_string(),
            state: agent_doc_controller::actor::ActorState::Starting,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
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
    ) -> agent_doc_controller::actor::ActorRecord {
        agent_doc_controller::actor::ActorRecord {
            document_id: document_id.to_string(),
            session_id: "session-clear".to_string(),
            generation: 1,
            pane_id: String::new(),
            window_id: String::new(),
            harness: "codex".to_string(),
            state: agent_doc_controller::actor::ActorState::Closed,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
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
        dead.state = agent_doc_controller::actor::ActorState::Ready;
        let mut live = actor_record(&live_id, "%live", "@1");
        live.state = agent_doc_controller::actor::ActorState::Busy;
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
            agent_doc_controller::actor::ActorState::Closed
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
            agent_doc_controller::actor::ActorState::Busy
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
        record.state = agent_doc_controller::actor::ActorState::Ready;
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
            agent_doc_controller::actor::ActorState::Ready
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
    fn layout_state_roundtrips_in_sqlite_without_file_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        store_layout_state(dir.path(), &["tasks/current.md".to_string()]).unwrap();

        let loaded = load_layout_state(dir.path()).unwrap();

        assert_eq!(loaded, vec!["tasks/current.md"]);
        assert!(!dir.path().join(".agent-doc/last_layout.json").exists());
    }
    #[test]
    fn singleton_launch_claim_rejects_concurrent_holder() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = LaunchClaim::acquire(dir.path()).unwrap();
        let second = LaunchClaim::acquire(dir.path());
        assert!(second.is_err());
        drop(first);
        assert!(LaunchClaim::acquire(dir.path()).is_ok());
    }
    #[test]
    fn blocking_launch_claim_waits_for_holder_then_acquires() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = LaunchClaim::acquire(dir.path()).unwrap();
        let root = dir.path().to_path_buf();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(first);
        });
        // Times out far enough above the holder's release that contention resolves
        // into a successful acquire rather than an error.
        let acquired = LaunchClaim::acquire_blocking(dir.path(), Duration::from_secs(2));
        assert!(
            acquired.is_ok(),
            "blocking acquire should wait out the holder"
        );
        releaser.join().unwrap();
        let _ = root;
    }
    #[test]
    fn blocking_launch_claim_times_out_when_holder_never_releases() {
        let dir = tempfile::TempDir::new().unwrap();
        let _held = LaunchClaim::acquire(dir.path()).unwrap();
        let acquired = LaunchClaim::acquire_blocking(dir.path(), Duration::from_millis(100));
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(
            record.state,
            agent_doc_controller::actor::ActorState::Starting
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(record.state, agent_doc_controller::actor::ActorState::Ready);
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok);
        let record = envelope.data.unwrap();
        assert_eq!(
            record.state,
            agent_doc_controller::actor::ActorState::Closed
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
            agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Closed,
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
        assert_eq!(record.state, agent_doc_controller::actor::ActorState::Ready);

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
            agent_doc_controller::actor::ActorState::Closed,
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
            agent_doc_controller::actor::ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();
        assert_eq!(record.generation, 1);

        let conn = open_state_db(dir.path()).unwrap();
        let boot_timestamp = crate::process::system_boot_timestamp_secs(timestamp_secs())
            .expect("/proc/uptime should be available in tests");
        // `#ctrliotestflake`: these margins used to be 2s and 1s, which is inside
        // the jitter of the value they are compared against. `system_boot_timestamp_secs`
        // derives boot time as `now - /proc/uptime`, and `handle_dispatch`
        // re-derives it a moment later with a different `now`, so the two
        // computations routinely differ by a second. When that drift exceeded the
        // margin the "pre-boot" pause no longer read as pre-boot, the stale pause
        // was not cleared, dispatch was refused, and the `.unwrap()` below
        // panicked — intermittently, depending on where the run landed inside a
        // second. Both records only need to sit unambiguously before the last
        // boot, so put them far outside that jitter.
        let old_transition_timestamp = boot_timestamp.saturating_sub(600);
        let preboot_pause_timestamp = boot_timestamp.saturating_sub(300);
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
            agent_doc_controller::actor::ActorState::Closed
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
            agent_doc_controller::actor::ActorState::Starting
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
            agent_doc_controller::actor::ActorState::Closed
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
            agent_doc_controller::actor::ActorState::Closed
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(
            envelope.ok,
            "same-pane stale generation should succeed: {:?}",
            envelope.error
        );
        assert_eq!(
            envelope.data.unwrap().state,
            agent_doc_controller::actor::ActorState::Ready
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
            agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Ready,
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
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
            agent_doc_controller::actor::ActorState::Ready,
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
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
            agent_doc_controller::actor::ActorState::Ready,
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
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
            agent_doc_controller::actor::ActorState::Ready,
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
                agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Closed
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
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &TEST_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        assert!(
            !agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .is_open()
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
    fn controller_restart_recovery_rebuilds_memory_from_state_db() {
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

        assert_eq!(
            load_layout_state(dir.path()).unwrap(),
            vec!["tasks/restart.md"]
        );
        assert!(!dir.path().join(".agent-doc/last_layout.json").exists());

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
        let record = agent_doc_controller::actor::ActorRecord {
            document_id: document_id.clone(),
            session_id: "session-restart-flood".to_string(),
            generation: 1,
            pane_id: "%88".to_string(),
            window_id: "@8".to_string(),
            harness: "codex".to_string(),
            state: agent_doc_controller::actor::ActorState::Ready,
            last_transition: agent_doc_controller::actor::ActorLastTransition {
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
            agent_doc_controller::actor::ActorState::Closed,
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
            agent_doc_controller::actor::ActorState::Blocked,
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
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
        let envelope: ControllerEnvelope<agent_doc_controller::actor::ActorRecord> =
            serde_json::from_str(&response).unwrap();
        assert!(envelope.ok, "mark_lifecycle with relative path failed");
        assert_eq!(
            envelope.data.unwrap().state,
            agent_doc_controller::actor::ActorState::Ready
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
        let scope = agent_doc_state_scope::ProcessScope::new();
        let supervisor_recycle_graph = ControllerSupervisorRecycleGraph::new_in(
            &scope,
            state_projection.project_supervisor_recycle(),
        );
        let coordination_graph = ControllerCoordinationGraph::new_in(&scope);
        let actor_graph = ControllerActorGraph::new_in(&scope, BTreeMap::new());
        let document_graphs = ControllerDocumentGraphs::new_in(&scope);
        let document_authority_graph = ControllerDocumentAuthorityGraph::new_in(
            &scope,
            actor_graph.document_model_states_handle(),
            document_graphs.projection_handle(),
        );
        let pane_layout_graph = ControllerPaneLayoutGraph::new_in(
            &scope,
            Vec::new(),
            actor_graph.live_bindings_handle(),
        );
        let async_editor_commands = ControllerAsyncEditorCommandGraph::new_in(&scope);
        let editor_surface_graph =
            rpc::ControllerEditorSurfaceGraph::new(Arc::new(rpc::run_controller_editor_intent));
        let document_path_transition_graph =
            rpc::ControllerDocumentPathTransitionGraph::new_in(&scope);
        ControllerRuntime {
            bootstrap: Mutex::new(bootstrap),
            memory: Mutex::new(ControllerMemoryState {
                state_ledger,
                state_document_versions: BTreeMap::new(),
                state_projection,
                map_backend: "std_btree_map",
            }),
            actor_graph,
            document_authority_graph,
            supervisor_recycle_graph,
            coordination_graph,
            supervisor_recycle_waiters: Condvar::new(),
            editor_op_capture_writes: Mutex::new(()),
            state_projection_waiters: Condvar::new(),
            document_graphs,
            state_plane_graph: ControllerStatePlaneGraph::new_in(&scope),
            captured_finalize_wakes: Mutex::new(BTreeMap::new()),
            pane_layout_graph,
            editor_surface_graph,
            document_path_transition_graph,
            async_editor_commands,
            recycle_requested: AtomicBool::new(false),
            recycle_urgent: AtomicBool::new(false),
            recycle_forced: AtomicBool::new(false),
            _scope: scope,
        }
    }

    #[test]
    fn editor_command_await_reacts_to_the_terminal_source_transition() {
        let scope = agent_doc_state_scope::ProcessScope::new();
        let graph = Arc::new(ControllerAsyncEditorCommandGraph::new_in(&scope));
        graph.publish(
            "cmd-reactive",
            AsyncEditorCommandPhase::Accepted,
            serde_json::json!({"phase": "accepted"}),
        );
        let (waiting, waiting_observer) = std::sync::mpsc::channel();
        let waiter_graph = Arc::clone(&graph);
        let waiter = std::thread::spawn(move || {
            waiting.send(()).unwrap();
            waiter_graph
                .await_terminal("cmd-reactive", Duration::from_secs(1))
                .unwrap()
        });
        waiting_observer.recv().unwrap();

        graph.publish(
            "cmd-reactive",
            AsyncEditorCommandPhase::Terminal,
            serde_json::json!({"phase": "terminal"}),
        );

        assert_eq!(waiter.join().unwrap()["phase"], "terminal");
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
        let _env = agent_doc_harness::prompt_source::TEST_ENV_LOCK.lock();
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
            agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Ready,
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
            agent_doc_controller::actor::ActorState::Ready,
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
                sentinel tests' processes under nextest concurrency. Runs serially in \
                the `make tmux-ci` ignored-test leg."]
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

    // -----------------------------------------------------------------------
    // `#retainedclearreactive` — the retained-write clear as a subscribed
    // `Effect`, not a `settle_*` call every consumer has to remember.
    // -----------------------------------------------------------------------

    fn retained_test_document(dir: &tempfile::TempDir) -> (PathBuf, String) {
        let file = dir.path().join("session.md");
        std::fs::write(&file, "# Session\n").unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&file);
        (file, document_hash)
    }

    fn defer_document_write(
        runtime: &Arc<ControllerRuntime>,
        project_root: &Path,
        document_hash: &str,
        intent_id: &str,
        target_hash: &str,
    ) {
        let event = deferred_document_write_event(document_hash, intent_id, target_hash);
        append_state_event(project_root, &event).unwrap();
        runtime.apply_state_event(&event).unwrap();
    }

    fn deferred_document_write_event(
        document_hash: &str,
        intent_id: &str,
        target_hash: &str,
    ) -> agent_doc_state_backbone::StateEvent {
        agent_doc_state_backbone::StateEvent::new(
            format!("document-write-deferred-{document_hash}-{intent_id}"),
            agent_doc_state_backbone::StateFact::DocumentWriteDeferred {
                document_hash: document_hash.to_string(),
                intent_id: intent_id.to_string(),
                expected_hash: "expected".to_string(),
                expected_content: None,
                target_hash: target_hash.to_string(),
                target_content: format!("content-for-{target_hash}"),
                source: agent_doc_state_backbone::DocumentWriteSource::PendingWrite,
                reason:
                    agent_doc_state_backbone::DocumentWriteDeferredReason::CrdtDeliveryAckPending,
            },
        )
    }

    fn preflight_started_event(document_hash: &str) -> agent_doc_state_backbone::StateEvent {
        agent_doc_state_backbone::StateEvent::new(
            format!("preflight-started-{document_hash}"),
            agent_doc_state_backbone::StateFact::PreflightStarted {
                document_hash: document_hash.to_string(),
                cycle_id: "cycle-1".to_string(),
                session_id: None,
                tracked_work_maintenance_required: None,
            },
        )
    }

    fn response_captured_event(document_hash: &str) -> agent_doc_state_backbone::StateEvent {
        agent_doc_state_backbone::StateEvent::new(
            format!("response-captured-{document_hash}"),
            agent_doc_state_backbone::StateFact::ResponseCaptured {
                document_hash: document_hash.to_string(),
                cycle_id: "cycle-1".to_string(),
                capture_id: "capture-1".to_string(),
                response_sha256: "response-1".to_string(),
                response_body: Some("response body".to_string()),
                intent_body: None,
                mutation_plan_json: None,
                file_hash: None,
                snapshot_hash: None,
                baseline_content: None,
            },
        )
    }

    fn capture_response(
        runtime: &Arc<ControllerRuntime>,
        project_root: &Path,
        document_hash: &str,
    ) {
        for event in [
            preflight_started_event(document_hash),
            response_captured_event(document_hash),
        ] {
            append_state_event(project_root, &event).unwrap();
            runtime.apply_state_event(&event).unwrap();
        }
        // ResponseCaptured deliberately wakes ordinary closeout processing.
        // Clear that independent edge so retained-delivery tests observe only
        // the derived resume signal under test.
        rpc::clear_captured_finalize_wake(runtime, document_hash);
    }

    fn retained_resume_projection(
        document_hash: &str,
    ) -> agent_doc_state_backbone::DocumentStateProjection {
        let mut projection = agent_doc_state_backbone::DocumentStateProjection::new(document_hash);
        for event in [
            preflight_started_event(document_hash),
            response_captured_event(document_hash),
            deferred_document_write_event(document_hash, "intent-1", "target"),
        ] {
            projection.apply_fact(&event.fact);
        }
        projection
    }

    fn document_authority_event(
        document_hash: &str,
        authority: agent_doc_state_backbone::DocumentAuthority,
        authority_epoch: u64,
        content_hash: &str,
    ) -> agent_doc_state_backbone::StateEvent {
        agent_doc_state_backbone::StateEvent::new(
            format!(
                "document-authority-{document_hash}-{authority_epoch}-{}",
                if authority.editor_active() {
                    "editor"
                } else {
                    "disk"
                }
            ),
            agent_doc_state_backbone::StateFact::DocumentAuthorityObserved {
                document_hash: document_hash.to_string(),
                authority,
                authority_epoch,
                source: "controller-generation-test".to_string(),
                reason: "exact_target_observed".to_string(),
                content_hash: Some(content_hash.to_string()),
                editor_id: None,
            },
        )
    }

    fn pending_intent_id(runtime: &Arc<ControllerRuntime>, document_hash: &str) -> Option<String> {
        runtime
            .memory
            .lock()
            .state_projection
            .document(document_hash)
            .and_then(|document| document.document.pending_write.as_ref())
            .map(|pending| pending.intent_id.clone())
    }

    fn observation(hash: &str) -> agent_doc_state_backbone::retained_write::ContentObservation {
        agent_doc_state_backbone::retained_write::ContentObservation {
            content_hash: hash.to_string(),
            payload_materialized: true,
            intent_delta_materialized: false,
        }
    }

    #[test]
    fn controller_document_graph_keeps_one_reactive_preflight_projection() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (_file, document_hash) = retained_test_document(&dir);

        let first = runtime.document_preflight_projection(
            &document_hash,
            agent_doc_state_backbone::preflight::PreflightReadFacts {
                document_hash: "doc-a".to_string(),
                baseline_hash: "base".to_string(),
                config_hash: "config".to_string(),
                diff: Some("+prompt a".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(first.diff.as_deref(), Some("+prompt a"));

        let second = runtime.document_preflight_projection(
            &document_hash,
            agent_doc_state_backbone::preflight::PreflightReadFacts {
                document_hash: "doc-b".to_string(),
                baseline_hash: "base".to_string(),
                config_hash: "config".to_string(),
                diff: Some("+prompt b".to_string()),
                ..Default::default()
            },
        );
        assert!(second.current);
        assert_eq!(second.document_hash, "doc-b");
        assert_eq!(
            second.diff.as_deref(),
            Some("+prompt b"),
            "updating the source slot must invalidate the existing Computed entry"
        );
    }

    /// The property the item exists for: **nobody calls settle**. Reading the
    /// derived verdict is the only thing this test does, and the intent is gone
    /// afterwards — durably and in the live projection.
    ///
    /// Before this, the clear was `settle_retained_write_through_derived_verdict`,
    /// a public companion that `session-check` called and `preflight` did not
    /// (`#preflightsettleparity` fixed that by adding a *second* call site, which
    /// is the imperative shape `#idlerevisionreactive` names). A third consumer
    /// would have reintroduced the same class of bug.
    #[test]
    fn reading_the_verdict_settles_a_satisfied_intent_with_no_settle_call() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (file, document_hash) = retained_test_document(&dir);
        defer_document_write(&runtime, dir.path(), &document_hash, "intent-1", "target");
        assert_eq!(
            pending_intent_id(&runtime, &document_hash).as_deref(),
            Some("intent-1"),
        );

        // The planes converged on exactly the stamped target: `Satisfied`.
        runtime.document_retained_write_verdict(
            &document_hash,
            &file,
            Some(observation("target")),
            Some(observation("target")),
        );

        assert_eq!(
            pending_intent_id(&runtime, &document_hash),
            None,
            "the subscribed clear must fire off the verdict, with no caller invoking a settle"
        );
        let ledger = load_state_event_ledger(dir.path()).unwrap();
        assert!(
            ledger.events().iter().any(|event| matches!(
                &event.fact,
                agent_doc_state_backbone::StateFact::DocumentWriteConverged { intent_id, .. }
                    if intent_id == "intent-1"
            )),
            "the clear must be durable, not just an in-memory projection edit"
        );
    }

    /// An `Unsettled` intent must survive the read. Making the gate settle must
    /// not turn it into a rubber stamp.
    #[test]
    fn reading_the_verdict_leaves_an_unsettled_intent_alone() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (file, document_hash) = retained_test_document(&dir);
        defer_document_write(&runtime, dir.path(), &document_hash, "intent-1", "target");

        // Authority and disk disagree: delivery is still in flight.
        let verdict = runtime.document_retained_write_verdict(
            &document_hash,
            &file,
            Some(observation("authority")),
            Some(observation("disk")),
        );
        assert!(verdict.blocks_new_cycle());
        assert_eq!(
            pending_intent_id(&runtime, &document_hash).as_deref(),
            Some("intent-1"),
        );
    }

    #[test]
    fn authority_edge_preserves_the_last_disk_source_observation() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (file, document_hash) = retained_test_document(&dir);
        defer_document_write(&runtime, dir.path(), &document_hash, "intent-1", "target");

        let initial = runtime.document_retained_write_verdict(
            &document_hash,
            &file,
            Some(observation("authority-before-save")),
            Some(observation("target")),
        );
        assert!(initial.blocks_new_cycle());

        runtime.document_retained_write_observe_authority(
            &document_hash,
            &file,
            observation("target"),
        );
        assert_eq!(
            pending_intent_id(&runtime, &document_hash),
            None,
            "a new authority edge must combine with the retained disk Source instead of erasing it"
        );
    }

    /// A verdict is only as fresh as its least fresh input.
    ///
    /// The observations are supplied per query, but the pending intent arrives
    /// asynchronously through `apply_state_event`. Without dropping the planes
    /// when a new fact lands, a *later* intent would be judged against planes
    /// nobody had looked at since — and a coincidental hash match would clear a
    /// write that never landed. `#idlerevisionreactive`: "I did not look" is a
    /// distinct outcome, so the verdict must fall back to `Unobserved`.
    #[test]
    fn a_new_projection_fact_invalidates_the_observations_it_predates() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (file, document_hash) = retained_test_document(&dir);
        defer_document_write(&runtime, dir.path(), &document_hash, "intent-1", "target");
        runtime.document_retained_write_verdict(
            &document_hash,
            &file,
            Some(observation("target")),
            Some(observation("target")),
        );
        assert_eq!(pending_intent_id(&runtime, &document_hash), None);

        // A second intent stamped at the SAME target hash the last observation
        // reported. Nothing has looked at either plane since it was deferred.
        defer_document_write(&runtime, dir.path(), &document_hash, "intent-2", "target");
        assert_eq!(
            pending_intent_id(&runtime, &document_hash).as_deref(),
            Some("intent-2"),
            "a stale observation must not be able to settle an intent that postdates it"
        );
    }

    #[test]
    fn controller_document_graph_is_seeded_from_durable_projection_at_startup() {
        let dir = tempfile::TempDir::new().unwrap();
        let (file, document_hash) = retained_test_document(&dir);
        let event = deferred_document_write_event(&document_hash, "intent-before-start", "target");
        append_state_event(dir.path(), &event).unwrap();

        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let verdict = runtime.document_retained_write_verdict(
            &document_hash,
            &file,
            Some(observation("authority")),
            Some(observation("disk")),
        );

        assert!(matches!(
            verdict,
            agent_doc_state_backbone::retained_write::SettlementVerdict::Unsettled {
                intent_id,
                ..
            } if intent_id == "intent-before-start"
        ));
    }

    #[test]
    fn rebuilt_controller_rehydrates_exact_planes_and_settles_without_polling() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_file, document_hash) = retained_test_document(&dir);
        let deferred =
            deferred_document_write_event(&document_hash, "intent-before-rebuild", "target");
        let editor = document_authority_event(
            &document_hash,
            agent_doc_state_backbone::DocumentAuthority::EditorRelay,
            10,
            "target",
        );
        let disk = document_authority_event(
            &document_hash,
            agent_doc_state_backbone::DocumentAuthority::DiskReplica,
            11,
            "target",
        );
        for event in [&deferred, &editor, &disk] {
            append_state_event(dir.path(), event).unwrap();
        }

        // Constructing the replacement controller is the installed-build
        // generation edge. No verdict query, retry, or session-check follows.
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();

        assert_eq!(
            pending_intent_id(&runtime, &document_hash),
            None,
            "the rebuilt reactive graph must settle exact durable editor+disk observations"
        );
        let ledger = load_state_event_ledger(dir.path()).unwrap();
        assert!(ledger.events().iter().any(|event| matches!(
            &event.fact,
            agent_doc_state_backbone::StateFact::DocumentWriteConverged { intent_id, .. }
            if intent_id == "intent-before-rebuild"
        )));
    }

    #[test]
    fn rebuilt_controller_does_not_reuse_planes_that_predate_the_intent() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_file, document_hash) = retained_test_document(&dir);
        let editor = document_authority_event(
            &document_hash,
            agent_doc_state_backbone::DocumentAuthority::EditorRelay,
            10,
            "target",
        );
        let disk = document_authority_event(
            &document_hash,
            agent_doc_state_backbone::DocumentAuthority::DiskReplica,
            11,
            "target",
        );
        let deferred =
            deferred_document_write_event(&document_hash, "intent-after-observation", "target");
        for event in [&editor, &disk, &deferred] {
            append_state_event(dir.path(), event).unwrap();
        }

        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();

        assert_eq!(
            pending_intent_id(&runtime, &document_hash).as_deref(),
            Some("intent-after-observation"),
            "a matching hash from before the write is not settlement evidence"
        );
    }

    #[test]
    fn refresh_memory_publishes_an_externally_appended_retained_intent() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (file, document_hash) = retained_test_document(&dir);
        let event = deferred_document_write_event(&document_hash, "intent-after-start", "target");
        append_state_event(dir.path(), &event).unwrap();

        runtime.refresh_memory().unwrap();
        let verdict = runtime.document_retained_write_verdict(
            &document_hash,
            &file,
            Some(observation("authority")),
            Some(observation("disk")),
        );

        assert!(matches!(
                verdict,
                agent_doc_state_backbone::retained_write::SettlementVerdict::Unsettled {
                    intent_id,
                    ..
                } if intent_id == "intent-after-start"
        ));
    }

    fn retained_transition_state_tag(state: &RetainedTransitionState) -> &'static str {
        match state {
            RetainedTransitionState::NoProjection => "no_projection",
            RetainedTransitionState::Idle => "idle",
            RetainedTransitionState::AwaitingController { .. } => "awaiting_controller",
            RetainedTransitionState::AwaitingDelivery(_) => "awaiting_delivery",
            RetainedTransitionState::AwaitingLiveEditor { .. } => "awaiting_live_editor",
            RetainedTransitionState::AwaitingConvergence { .. } => "awaiting_convergence",
            RetainedTransitionState::ApplyTarget(_) => "apply_target",
            RetainedTransitionState::TargetVisible { .. } => "target_visible",
            RetainedTransitionState::ReconcileMaterializedCapture(_) => {
                "reconcile_materialized_capture"
            }
            RetainedTransitionState::Conflict {
                reason: RetainedTransitionConflict::MissingBase,
                ..
            } => "conflict_missing_base",
            RetainedTransitionState::Conflict {
                reason: RetainedTransitionConflict::InvalidTargetHash,
                ..
            } => "conflict_invalid_target_hash",
            RetainedTransitionState::Conflict {
                reason: RetainedTransitionConflict::InvalidTargetStructure,
                ..
            } => "conflict_invalid_target_structure",
            RetainedTransitionState::Conflict {
                reason: RetainedTransitionConflict::DivergentVisibleProjection,
                ..
            } => "conflict_divergent_visible_projection",
        }
    }

    fn retained_transition_effect_tag(state: &RetainedTransitionState) -> &'static str {
        match state.effect() {
            None => "none",
            Some(RetainedTransitionEffect::ObserveCurrentDelivery(_)) => "observe_current_delivery",
            Some(RetainedTransitionEffect::ApplyTarget(_)) => "apply_target",
            Some(RetainedTransitionEffect::ResumeCloseout(_)) => "resume_closeout",
            Some(RetainedTransitionEffect::SettleMaterializedCapture(_)) => {
                "settle_materialized_capture"
            }
        }
    }

    #[test]
    fn retained_transition_state_table_covers_every_state_and_effect() {
        let base = "# Queue\n";
        let target = "# Queue\n\n### Re: done\n";
        let mut transition = retained_resume_projection("doc-retained-state-table");
        {
            let intent = transition.document.pending_write.as_mut().unwrap();
            intent.expected_content = Some(base.to_string());
            intent.expected_hash = agent_doc_hash::content_hash(base);
            intent.target_content = target.to_string();
            intent.target_hash = agent_doc_hash::content_hash(target);
        }
        let delivery = |content: &str, live_editors: usize, delivery_converged: bool| {
            RetainedDeliveryObservation {
                file: PathBuf::from("/work/task.md"),
                content: Arc::from(content),
                content_hash: agent_doc_hash::content_hash(content),
                live_editors,
                delivery_converged,
                delivery_version: 11,
            }
        };

        let idle = agent_doc_state_backbone::DocumentStateProjection::new("doc-retained-idle");
        let mut recycled_cycle = transition.clone();
        recycled_cycle.closeout.captured_response = None;
        let mut legacy_target_without_capture = recycled_cycle.clone();
        legacy_target_without_capture
            .document
            .pending_write
            .as_mut()
            .unwrap()
            .continuation = None;
        let mut materialized = transition.clone();
        materialized.document.pending_write.as_mut().unwrap().source =
            agent_doc_state_backbone::DocumentWriteSource::PostCommitReposition;
        let mut missing_base = transition.clone();
        missing_base
            .document
            .pending_write
            .as_mut()
            .unwrap()
            .expected_content = None;
        let mut invalid_hash = transition.clone();
        invalid_hash
            .document
            .pending_write
            .as_mut()
            .unwrap()
            .target_hash = "not-the-target".into();
        let mut invalid_structure = transition.clone();
        {
            let intent = invalid_structure.document.pending_write.as_mut().unwrap();
            intent.target_content = "# Queue\n-->\n".to_string();
            intent.target_hash = agent_doc_hash::content_hash(&intent.target_content);
        }

        let cases = vec![
            ("no projection", None, None, 1, "no_projection", "none"),
            (
                "no retained transition",
                Some(idle),
                None,
                1,
                "idle",
                "none",
            ),
            (
                "controller not active",
                Some(transition.clone()),
                None,
                0,
                "awaiting_controller",
                "none",
            ),
            (
                "delivery source absent",
                Some(transition.clone()),
                None,
                1,
                "awaiting_delivery",
                "observe_current_delivery",
            ),
            (
                "no live editor",
                Some(transition.clone()),
                Some(delivery(base, 0, true)),
                1,
                "awaiting_live_editor",
                "none",
            ),
            (
                "delivery not converged",
                Some(transition.clone()),
                Some(delivery(base, 1, false)),
                1,
                "awaiting_convergence",
                "none",
            ),
            (
                "base visible",
                Some(transition.clone()),
                Some(delivery(base, 1, true)),
                1,
                "apply_target",
                "apply_target",
            ),
            (
                "target visible with captured closeout",
                Some(transition.clone()),
                Some(delivery(target, 1, true)),
                1,
                "target_visible",
                "resume_closeout",
            ),
            (
                "target visible after the current cycle capture was recycled",
                Some(recycled_cycle),
                Some(delivery(target, 1, true)),
                1,
                "target_visible",
                "resume_closeout",
            ),
            (
                "legacy target visible without any retained continuation",
                Some(legacy_target_without_capture),
                Some(delivery(target, 1, true)),
                1,
                "target_visible",
                "none",
            ),
            (
                "newer projection materializes retained response",
                Some(materialized),
                Some(delivery("operator queue edit\nresponse body\n", 1, true)),
                1,
                "reconcile_materialized_capture",
                "settle_materialized_capture",
            ),
            (
                "legacy transition has no base",
                Some(missing_base),
                Some(delivery("divergent\n", 1, true)),
                1,
                "conflict_missing_base",
                "none",
            ),
            (
                "target hash is invalid",
                Some(invalid_hash),
                Some(delivery(base, 1, true)),
                1,
                "conflict_invalid_target_hash",
                "none",
            ),
            (
                "target structure is invalid",
                Some(invalid_structure),
                Some(delivery(base, 1, true)),
                1,
                "conflict_invalid_target_structure",
                "none",
            ),
            (
                "visible projection diverged from base and target",
                Some(transition),
                Some(delivery("operator typing\n", 1, true)),
                1,
                "conflict_divergent_visible_projection",
                "none",
            ),
        ];

        for (name, projection, delivery, generation, expected_state, expected_effect) in cases {
            let state =
                retained_transition_state(projection.as_ref(), delivery.as_ref(), generation);
            assert_eq!(
                retained_transition_state_tag(&state),
                expected_state,
                "state: {name}"
            );
            assert_eq!(
                retained_transition_effect_tag(&state),
                expected_effect,
                "effect: {name}"
            );
        }
    }

    #[test]
    fn retained_resume_is_a_typed_computed_delivery_gate() {
        let projection = retained_resume_projection("doc-retained-resume");
        let exact = RetainedDeliveryObservation {
            file: PathBuf::from("/work/task.md"),
            content: Arc::from("target"),
            content_hash: "target".to_string(),
            live_editors: 1,
            delivery_converged: true,
            delivery_version: 7,
        };

        assert!(
            retained_resume_signal(Some(&projection), Some(&exact), 0).is_none(),
            "the pre-sink controller generation cannot apply an effect"
        );
        assert!(
            retained_resume_signal(
                Some(&projection),
                Some(&RetainedDeliveryObservation {
                    live_editors: 0,
                    ..exact.clone()
                }),
                1,
            )
            .is_none(),
            "zero-member convergence is not visible-write proof"
        );
        assert!(
            retained_resume_signal(
                Some(&projection),
                Some(&RetainedDeliveryObservation {
                    delivery_converged: false,
                    ..exact.clone()
                }),
                1,
            )
            .is_none()
        );
        assert!(
            retained_resume_signal(
                Some(&projection),
                Some(&RetainedDeliveryObservation {
                    content: Arc::from("other"),
                    content_hash: "other".to_string(),
                    ..exact.clone()
                }),
                1,
            )
            .is_none()
        );

        let signal = retained_resume_signal(Some(&projection), Some(&exact), 1).unwrap();
        assert_eq!(signal.action, RetainedResumeAction::ResumeExactDelivery);
        assert_eq!(signal.intent_id, "intent-1");
        assert_eq!(signal.target_hash, "target");
        assert_eq!(signal.cycle_id, "cycle-1");
        assert_eq!(signal.capture_id, "capture-1");
        assert!(retained_resume_signal_matches_projection(
            &signal,
            &projection
        ));
        let mut recycled = projection.clone();
        recycled.closeout.cycle_id = Some("cycle-2".into());
        recycled.closeout.captured_response = None;
        assert!(
            retained_resume_signal_matches_projection(&signal, &recycled),
            "the effect fence must follow the retained transition continuation, not current-cycle state",
        );
        recycled
            .document
            .pending_write
            .as_mut()
            .unwrap()
            .continuation
            .as_mut()
            .unwrap()
            .capture_id = "different-capture".into();
        assert!(
            !retained_resume_signal_matches_projection(&signal, &recycled),
            "a genuinely different retained continuation must remain fenced",
        );
    }

    #[test]
    fn retained_transition_is_a_guarded_computed_projection_not_an_ack_request() {
        let mut projection = retained_resume_projection("doc-retained-target");
        let intent = projection.document.pending_write.as_mut().unwrap();
        let base = "# Queue\n";
        let target = "# Queue\n\n### Re: done\n";
        intent.expected_content = Some(base.to_string());
        intent.expected_hash = agent_doc_hash::content_hash(base);
        intent.target_content = target.to_string();
        intent.target_hash = agent_doc_hash::content_hash(target);
        let delivery = RetainedDeliveryObservation {
            file: PathBuf::from("/work/task.md"),
            content: Arc::from(base),
            content_hash: agent_doc_hash::content_hash(base),
            live_editors: 1,
            delivery_converged: true,
            delivery_version: 8,
        };

        let transition =
            retained_transition_projection(Some(&projection), Some(&delivery), 1).unwrap();
        assert_eq!(transition.base_content.as_ref(), base);
        assert_eq!(transition.target_content.as_ref(), target);
        assert_eq!(transition.delivery_version, 8);

        assert!(
            retained_transition_projection(
                Some(&projection),
                Some(&RetainedDeliveryObservation {
                    content: Arc::from("# operator typing\n"),
                    content_hash: agent_doc_hash::content_hash("# operator typing\n"),
                    ..delivery.clone()
                }),
                1,
            )
            .is_none(),
            "a divergent visible authority must never be overwritten"
        );
        assert!(
            retained_transition_projection(Some(&projection), Some(&delivery), 0).is_none(),
            "a pre-sink controller cannot submit the projection"
        );

        let malformed = "# Queue\n-->\n";
        let intent = projection.document.pending_write.as_mut().unwrap();
        intent.target_content = malformed.to_string();
        intent.target_hash = agent_doc_hash::content_hash(malformed);
        assert!(
            retained_transition_projection(Some(&projection), Some(&delivery), 1).is_none(),
            "structurally invalid targets remain fail-closed"
        );
    }

    #[test]
    fn controller_activation_derives_delivery_observation_without_requesting_an_ack() {
        let projection = retained_resume_projection("doc-retained-hydration");
        let activation = retained_delivery_activation(Some(&projection), None, 7).unwrap();
        assert_eq!(activation.intent_id, "intent-1");
        assert_eq!(activation.controller_generation, 7);
        assert!(
            retained_delivery_activation(Some(&projection), None, 0).is_none(),
            "a graph without its Effect sink cannot observe activation state"
        );
        assert!(
            retained_delivery_activation(
                Some(&projection),
                Some(&RetainedDeliveryObservation {
                    file: PathBuf::from("/work/task.md"),
                    content: Arc::from("target"),
                    content_hash: "target".to_string(),
                    live_editors: 1,
                    delivery_converged: true,
                    delivery_version: 1,
                }),
                7,
            )
            .is_none(),
            "an existing Source observation needs no activation hydration"
        );
    }

    #[test]
    fn retained_post_commit_reposition_reacts_to_a_materialized_newer_projection() {
        let mut projection = retained_resume_projection("doc-retained-reposition");
        projection.document.pending_write.as_mut().unwrap().source =
            agent_doc_state_backbone::DocumentWriteSource::PostCommitReposition;
        let materialized = RetainedDeliveryObservation {
            file: PathBuf::from("/work/task.md"),
            content: Arc::from("operator queue edit\nresponse body\n"),
            content_hash: "newer-projection".to_string(),
            live_editors: 1,
            delivery_converged: true,
            delivery_version: 9,
        };

        let signal = retained_resume_signal(Some(&projection), Some(&materialized), 1).unwrap();
        assert_eq!(
            signal.action,
            RetainedResumeAction::ReconcileMaterializedCapture
        );
        assert_eq!(signal.delivery_version, 9);

        projection.document.pending_write.as_mut().unwrap().source =
            agent_doc_state_backbone::DocumentWriteSource::PendingWrite;
        assert!(
            retained_resume_signal(Some(&projection), Some(&materialized), 1).is_none(),
            "ordinary divergent writes cannot be treated as a settled reposition"
        );
    }

    #[test]
    fn retained_post_commit_reposition_does_not_wake_for_malformed_visible_content() {
        let mut projection = retained_resume_projection("doc-retained-malformed");
        projection.document.pending_write.as_mut().unwrap().source =
            agent_doc_state_backbone::DocumentWriteSource::PostCommitReposition;
        let malformed = RetainedDeliveryObservation {
            file: PathBuf::from("/work/task.md"),
            content: Arc::from("response body\n-->\n"),
            content_hash: "malformed-projection".to_string(),
            live_editors: 1,
            delivery_converged: true,
            delivery_version: 10,
        };

        assert!(
            retained_resume_signal(Some(&projection), Some(&malformed), 1).is_none(),
            "structurally invalid visible authority stays blocked for operator-safe recovery"
        );
    }

    #[test]
    fn compact_resume_is_derived_only_after_the_retained_write_clears() {
        let document_hash = "doc-compact-resume";
        let mut projection = agent_doc_state_backbone::DocumentStateProjection::new(document_hash);
        let deferred =
            deferred_document_write_event(document_hash, "compact-intent", "compact-target");
        projection.apply_fact(&deferred.fact);
        projection.apply_fact(
            &agent_doc_state_backbone::StateFact::DocumentCompactProjectionRetained {
                document_hash: document_hash.to_string(),
                continuation_id: "compact-continuation".to_string(),
                file: "/work/task.md".to_string(),
                live_content: "live compact target".to_string(),
                committed_content: "committed compact target".to_string(),
                target_component: Some("exchange".to_string()),
                commit: true,
            },
        );

        assert!(compact_resume_signal(Some(&projection), 0).is_none());
        assert!(
            compact_resume_signal(Some(&projection), 1).is_none(),
            "admission alone cannot run snapshot/commit before write convergence"
        );

        projection.apply_fact(
            &agent_doc_state_backbone::StateFact::DocumentWriteConverged {
                document_hash: document_hash.to_string(),
                intent_id: "compact-intent".to_string(),
                target_hash: "compact-target".to_string(),
                source: "editor_document_state_projection".to_string(),
                intent_source: agent_doc_state_backbone::DocumentWriteSource::PendingWrite,
            },
        );
        let continuation = compact_resume_signal(Some(&projection), 1).unwrap();
        assert_eq!(continuation.continuation_id, "compact-continuation");
        assert!(continuation.commit);
    }

    #[test]
    fn exact_editor_projection_receipt_completes_retained_compact_once() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (file, document_hash) = retained_test_document(&dir);
        let target_content = "# Session\n\nCompacted exchange.\n";
        let target_hash = agent_doc_hash::content_hash(target_content);
        let continuation_id = "compact-continuation";

        defer_document_write(
            &runtime,
            dir.path(),
            &document_hash,
            "compact-intent",
            &target_hash,
        );
        let retained = agent_doc_state_backbone::StateEvent::new(
            format!("{document_hash}:{continuation_id}"),
            agent_doc_state_backbone::StateFact::DocumentCompactProjectionRetained {
                document_hash: document_hash.clone(),
                continuation_id: continuation_id.to_string(),
                file: file.to_string_lossy().into_owned(),
                live_content: target_content.to_string(),
                committed_content: target_content.to_string(),
                target_component: Some("exchange".to_string()),
                commit: true,
            },
        );
        append_state_event(dir.path(), &retained).unwrap();
        runtime.apply_state_event(&retained).unwrap();

        runtime.document_retained_write_observe_authority(
            &document_hash,
            &file,
            observation(&target_hash),
        );
        assert_eq!(
            pending_intent_id(&runtime, &document_hash).as_deref(),
            Some("compact-intent"),
            "one projection plane cannot release compact closeout"
        );

        runtime.document_retained_write_observe_disk(
            &document_hash,
            &file,
            observation(&target_hash),
        );
        let projection = runtime
            .document_state_projection(&document_hash)
            .unwrap()
            .unwrap();
        assert!(
            projection.document.pending_write.is_none(),
            "the exact authority+disk projection must settle the retained write"
        );
        assert!(
            projection.document.pending_compact_projection.is_none(),
            "settlement must reactively run and receipt the compact completion Effect"
        );

        let settled_count = || {
            load_state_event_ledger(dir.path())
                .unwrap()
                .events()
                .iter()
                .filter(|event| {
                    matches!(
                        &event.fact,
                        agent_doc_state_backbone::StateFact::DocumentCompactProjectionSettled {
                            continuation_id: settled_id,
                            ..
                        } if settled_id == continuation_id
                    )
                })
                .count()
        };
        assert_eq!(settled_count(), 1);

        runtime.document_retained_write_observe_authority(
            &document_hash,
            &file,
            observation(&target_hash),
        );
        runtime.document_retained_write_observe_disk(
            &document_hash,
            &file,
            observation(&target_hash),
        );
        assert_eq!(
            settled_count(),
            1,
            "replayed exact projection receipts cannot duplicate compact completion"
        );
    }

    #[test]
    fn retained_resume_reacts_when_delivery_arrives_after_the_intent() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (_file, document_hash) = retained_test_document(&dir);
        capture_response(&runtime, dir.path(), &document_hash);
        defer_document_write(
            &runtime,
            dir.path(),
            &document_hash,
            "intent-after-capture",
            "target",
        );

        assert!(
            !runtime
                .captured_finalize_wakes
                .lock()
                .contains_key(&document_hash)
        );
        runtime.document_graphs.observe_retained_delivery(
            &document_hash,
            Some(RetainedDeliveryObservation {
                file: PathBuf::from("/work/task.md"),
                content: Arc::from("target"),
                content_hash: "target".to_string(),
                live_editors: 1,
                delivery_converged: true,
                delivery_version: 1,
            }),
        );

        let wakes = runtime.captured_finalize_wakes.lock();
        let wake = wakes.get(&document_hash).unwrap();
        assert_eq!(wake.reason, "retained_delivery_reactive");
        assert_eq!(wake.capture_id, "capture-1");
    }

    #[test]
    fn retained_materialized_capture_is_settled_without_supervisor_replay() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (_file, document_hash) = retained_test_document(&dir);
        capture_response(&runtime, dir.path(), &document_hash);
        let mut event =
            deferred_document_write_event(&document_hash, "post-commit-reposition", "old-target");
        let agent_doc_state_backbone::StateFact::DocumentWriteDeferred { source, .. } =
            &mut event.fact
        else {
            unreachable!();
        };
        *source = agent_doc_state_backbone::DocumentWriteSource::PostCommitReposition;
        append_state_event(dir.path(), &event).unwrap();
        runtime.apply_state_event(&event).unwrap();
        rpc::clear_captured_finalize_wake(&runtime, &document_hash);

        let materialized = RetainedDeliveryObservation {
            file: PathBuf::from("/work/task.md"),
            content: Arc::from("operator queue edit\nresponse body\n"),
            content_hash: "newer-projection".to_string(),
            live_editors: 1,
            delivery_converged: true,
            delivery_version: 12,
        };
        runtime
            .document_graphs
            .observe_retained_delivery(&document_hash, Some(materialized.clone()));

        assert!(
            !runtime
                .captured_finalize_wakes
                .lock()
                .contains_key(&document_hash),
            "a terminal post-commit reposition is settlement, not a finalize wake"
        );
        assert_eq!(
            pending_intent_id(&runtime, &document_hash),
            None,
            "the reactive settlement effect must retire the obsolete reposition"
        );
        assert!(
            runtime
                .document_graphs
                .current_retained_resume(&document_hash)
                .is_none(),
            "settlement must not leave a supervisor replay candidate"
        );

        runtime
            .document_graphs
            .observe_retained_delivery(&document_hash, Some(materialized));
        assert!(
            runtime
                .document_graphs
                .current_retained_resume(&document_hash)
                .is_none(),
            "repeated delivery projections cannot recreate the settled transition"
        );
    }

    #[test]
    fn pinned_retained_wake_does_not_consult_the_current_closeout_cycle() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (_file, document_hash) = retained_test_document(&dir);

        assert!(rpc::publish_pinned_captured_finalize_wake(
            &runtime,
            &document_hash,
            "retained-cycle",
            "retained-capture",
            "retained-response-sha",
            "retained_materialized_capture_reconcile_reactive",
        ));
        let wakes = runtime.captured_finalize_wakes.lock();
        let wake = wakes.get(&document_hash).unwrap();
        assert_eq!(wake.cycle_id, "retained-cycle");
        assert_eq!(wake.capture_id, "retained-capture");
        assert_eq!(wake.response_sha256, "retained-response-sha");
        assert_eq!(
            wake.reason,
            "retained_materialized_capture_reconcile_reactive"
        );
    }

    #[test]
    fn retained_resume_reacts_when_the_intent_arrives_after_delivery() {
        let dir = tempfile::TempDir::new().unwrap();
        let runtime = ControllerRuntime::new_arc(test_bootstrap(&dir)).unwrap();
        let (_file, document_hash) = retained_test_document(&dir);
        capture_response(&runtime, dir.path(), &document_hash);
        runtime.document_retained_write_observe_delivery(
            &document_hash,
            Some(RetainedDeliveryObservation {
                file: PathBuf::from("/work/task.md"),
                content: Arc::from("target"),
                content_hash: "target".to_string(),
                live_editors: 1,
                delivery_converged: true,
                delivery_version: 1,
            }),
        );
        assert!(
            !runtime
                .captured_finalize_wakes
                .lock()
                .contains_key(&document_hash)
        );

        defer_document_write(
            &runtime,
            dir.path(),
            &document_hash,
            "intent-after-delivery",
            "target",
        );

        assert_eq!(
            runtime
                .captured_finalize_wakes
                .lock()
                .get(&document_hash)
                .map(|wake| wake.reason.as_str()),
            Some("retained_delivery_reactive")
        );
    }

    #[test]
    fn controller_activation_replays_an_already_eligible_retained_resume() {
        let dir = tempfile::TempDir::new().unwrap();
        let (_file, document_hash) = retained_test_document(&dir);
        for event in [
            preflight_started_event(&document_hash),
            response_captured_event(&document_hash),
            deferred_document_write_event(
                &document_hash,
                "intent-before-controller-activation",
                "target",
            ),
        ] {
            append_state_event(dir.path(), &event).unwrap();
        }

        // Build the graph before its sink exists, then publish the external
        // delivery edge. Generation zero must retain the signal without
        // applying it.
        let runtime = Arc::new(ControllerRuntime::new(test_bootstrap(&dir)).unwrap());
        runtime.document_retained_write_observe_delivery(
            &document_hash,
            Some(RetainedDeliveryObservation {
                file: PathBuf::from("/work/task.md"),
                content: Arc::from("target"),
                content_hash: "target".to_string(),
                live_editors: 1,
                delivery_converged: true,
                delivery_version: 1,
            }),
        );
        assert!(
            !runtime
                .captured_finalize_wakes
                .lock()
                .contains_key(&document_hash)
        );

        runtime
            .document_graphs
            .install_settle_sink(dir.path().to_path_buf(), &runtime);

        assert_eq!(
            runtime
                .captured_finalize_wakes
                .lock()
                .get(&document_hash)
                .map(|wake| wake.reason.as_str()),
            Some("retained_delivery_reactive"),
            "controller activation must apply the already-eligible Computed without another ACK"
        );
    }

    #[test]
    fn authoritative_markdown_reactively_projects_terminal_queue_lifecycle() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- ~~do [#completed-work]~~\n",
            "- do [#ready-work]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&file, content).unwrap();
        let canonical = file.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let runtime = Arc::new(ControllerRuntime::new(test_bootstrap(&dir)).unwrap());
        runtime
            .document_graphs
            .install_settle_sink(dir.path().to_path_buf(), &runtime);

        let projected = runtime
            .document_queue_authority_observe(&document_hash, &canonical, content.to_string())
            .unwrap();

        assert_eq!(projected, 1);
        let state = runtime
            .document_state_projection(&document_hash)
            .unwrap()
            .unwrap();
        assert!(
            state
                .queue
                .completed_heads
                .contains("queue:0:completed-work:0")
        );
        assert!(!state.queue.completed_heads.contains("queue:1:ready-work:0"));
    }

    #[test]
    fn captured_response_and_authority_reactively_project_answered_free_text_strike() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("session.md");
        let response = concat!(
            "### Re: staging deploy — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> staging deploy\n\n",
            "Deployed and verified.\n",
        );
        let content = format!(
            concat!(
                "---\n",
                "agent_doc_session: queue-projection\n",
                "queue_active: false\n",
                "---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "{}",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue -->\n",
                "- staging deploy\n",
                "- do [#production-deploy]\n",
                "<!-- /agent:queue -->\n",
            ),
            response
        );
        std::fs::write(&file, &content).unwrap();
        let canonical = file.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let runtime = Arc::new(ControllerRuntime::new(test_bootstrap(&dir)).unwrap());
        runtime
            .document_graphs
            .install_settle_sink(dir.path().to_path_buf(), &runtime);

        let captured = agent_doc_state_backbone::StateEvent::new(
            "response-captured-free-text",
            agent_doc_state_backbone::StateFact::ResponseCaptured {
                document_hash: document_hash.clone(),
                cycle_id: "cycle-1".to_string(),
                capture_id: "capture-free-text".to_string(),
                response_sha256: agent_doc_hash::content_hash(response),
                response_body: Some(response.to_string()),
                intent_body: None,
                mutation_plan_json: None,
                file_hash: None,
                snapshot_hash: None,
                baseline_content: Some(content.clone()),
            },
        );
        for event in [preflight_started_event(&document_hash), captured] {
            append_state_event(dir.path(), &event).unwrap();
            runtime.apply_state_event(&event).unwrap();
        }
        let durable = runtime
            .document_state_projection(&document_hash)
            .unwrap()
            .unwrap();
        assert!(
            durable.closeout.captured_response.is_some(),
            "the fixture must expose the captured response to the Computed: {durable:?}"
        );
        assert!(
            agent_doc_turn::response_replay::response_materialized_in_content(response, &content)
        );

        runtime
            .document_queue_authority_observe(&document_hash, &canonical, content)
            .unwrap();
        let strike = runtime
            .document_graphs
            .current_answered_free_text_strike(&document_hash);
        assert!(
            strike.has_target(),
            "captured response plus authority must derive a target: {strike:?}"
        );

        let projected = std::fs::read_to_string(&canonical).unwrap();
        assert!(
            projected
                .contains("- ~~staging deploy~~ — auto-struck: answered this cycle (#ftstrike)"),
            "the Effect should apply the Computed target without a caller requesting a strike"
        );
        assert!(projected.contains("queue: start"));
        assert!(projected.contains("- do [#production-deploy]"));
    }
}
