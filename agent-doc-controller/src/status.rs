//! Pure controller status and freshness projections.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_STALE_PREPARING_CONTROLLER_SECS: u64 = 45;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Managed,
    Lazy,
}

impl LaunchMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Lazy => "lazy",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, ParseLaunchModeError> {
        match raw {
            "managed" => Ok(Self::Managed),
            "lazy" => Ok(Self::Lazy),
            other => Err(ParseLaunchModeError {
                raw: other.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseLaunchModeError {
    raw: String,
}

impl fmt::Display for ParseLaunchModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown controller launch mode: {}", self.raw)
    }
}

impl Error for ParseLaunchModeError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerBinaryIdentity {
    pub path: PathBuf,
    pub version: String,
    pub len: u64,
    pub modified_secs: u64,
    pub modified_nanos: u32,
}

/// True only when a recorded long-lived process identity exactly matches the
/// freshly resolved controller binary identity. Missing identity is not a
/// match; callers that need fail-open stale detection should use
/// `process_binary_is_stale`.
pub fn controller_binary_identity_matches(
    recorded: Option<&ControllerBinaryIdentity>,
    current: Option<&ControllerBinaryIdentity>,
) -> bool {
    matches!((recorded, current), (Some(recorded), Some(current)) if recorded == current)
}

/// True when a long-lived process's recorded launch identity differs from the
/// freshly resolved controller binary identity. Missing identities are
/// fail-open and therefore not stale.
pub fn process_binary_is_stale(
    recorded: Option<&ControllerBinaryIdentity>,
    current: Option<&ControllerBinaryIdentity>,
) -> bool {
    matches!((recorded, current), (Some(recorded), Some(current)) if recorded != current)
}

/// Return true only when `candidate` is provably newer than `recorded`.
///
/// Replacement is directional: an older CLI must adopt a newer controller,
/// while a newer install (including a same-version rebuild with a later mtime)
/// may recycle the older long-lived process.
pub fn controller_binary_identity_is_newer(
    candidate: Option<&ControllerBinaryIdentity>,
    recorded: Option<&ControllerBinaryIdentity>,
) -> bool {
    let (Some(candidate), Some(recorded)) = (candidate, recorded) else {
        return false;
    };
    if candidate.version != recorded.version {
        return match (
            normalized_release_version(&candidate.version),
            normalized_release_version(&recorded.version),
        ) {
            (Some(candidate), Some(recorded)) => candidate > recorded,
            _ => false,
        };
    }
    (candidate.modified_secs, candidate.modified_nanos)
        > (recorded.modified_secs, recorded.modified_nanos)
}

fn normalized_release_version(version: &str) -> Option<Vec<u64>> {
    let normalized = version.strip_prefix('v').unwrap_or(version);
    let release = normalized
        .split_once('-')
        .map(|(release, _)| release)
        .unwrap_or(normalized);
    let mut parts = release
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    while parts.last() == Some(&0) {
        parts.pop();
    }
    Some(parts)
}

pub const fn default_controller_generation() -> u64 {
    1
}

pub const fn controller_restart_recovery_needed(
    controller_generation: u64,
    previous_controller_pid: Option<u32>,
) -> bool {
    controller_generation > default_controller_generation() || previous_controller_pid.is_some()
}

/// Resolve the version stamped into a controller binary identity.
///
/// Long-lived controllers prefer the top-level binary version injected by the
/// process entrypoint. Library-only callers and tests fall back to the calling
/// crate's package version supplied at the orchestration boundary.
pub fn resolve_controller_identity_version(
    injected_binary_version: Option<&str>,
    fallback_crate_version: &str,
) -> String {
    injected_binary_version
        .map(str::to_string)
        .unwrap_or_else(|| fallback_crate_version.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerProcessFreshness {
    pub role: String,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub running_inode: Option<u64>,
    #[serde(default)]
    pub installed_inode: Option<u64>,
    #[serde(default)]
    pub matches_installed: Option<bool>,
    pub stale: bool,
    pub guidance: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerFreshnessStatus {
    #[serde(default)]
    pub installed_binary: Option<ControllerBinaryIdentity>,
    #[serde(default)]
    pub installed_inode: Option<u64>,
    pub controller: ControllerProcessFreshness,
    #[serde(default)]
    pub route_owned_supervisor: Option<ControllerProcessFreshness>,
    pub guidance: String,
}

pub fn controller_process_freshness_label(process: &ControllerProcessFreshness) -> &'static str {
    match process.matches_installed {
        Some(true) => "fresh",
        Some(false) => "stale",
        None => "unknown",
    }
}

pub fn controller_freshness_summary(freshness: Option<&ControllerFreshnessStatus>) -> String {
    let Some(freshness) = freshness else {
        return "unknown".to_string();
    };
    let controller = controller_process_freshness_label(&freshness.controller);
    let supervisor = freshness
        .route_owned_supervisor
        .as_ref()
        .map(controller_process_freshness_label)
        .unwrap_or("n/a");
    format!("controller:{controller},supervisor:{supervisor}")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerFreshnessFacts {
    pub installed_binary: Option<ControllerBinaryIdentity>,
    pub installed_inode: Option<u64>,
    pub controller_pid: Option<u32>,
    pub controller_running_inode: Option<u64>,
    pub route_owned_supervisor_pid: Option<u32>,
    pub route_owned_supervisor_running_inode: Option<u64>,
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
    #[serde(default)]
    pub freshness: Option<ControllerFreshnessStatus>,
    #[serde(default = "default_control_plane_status")]
    pub control_plane: ControlPlaneStatus,
}

/// `#fccsupwarn` — read-only operator WARN message when the LIVE controller/supervisor
/// hosting this document is serving a stale agent-doc binary. The caller owns resolving
/// the current binary identity at its IO boundary; this pure policy renders only when
/// the recorded launch identity differs from that current identity.
pub fn supervisor_stale_warning_message(
    status: &ControllerStatus,
    current_binary: Option<&ControllerBinaryIdentity>,
) -> Option<String> {
    if !status.active {
        return None;
    }
    if !process_binary_is_stale(status.controller_binary.as_ref(), current_binary) {
        return None;
    }
    let pid = status
        .pid
        .map(|pid| format!(" (pid {pid})"))
        .unwrap_or_default();
    let launched = status
        .controller_binary
        .as_ref()
        .map(|id| format!(" launched as {}", id.version))
        .unwrap_or_default();
    Some(format!(
        "the live session controller/supervisor{pid}{launched} is running a STALE agent-doc binary \
         (a newer build is installed) — restart it so the latest fixes take effect: \
         `agent-doc admin recycle` (recycles at the next idle boundary) or \
         `agent-doc session restart-supervisor <FILE>`. A stale supervisor silently keeps \
         producing File Cache Conflict / IPC-drift dialogs (#fcc0/#ipcdrift)."
    ))
}

/// `#fccsupwarn3` — user-facing warning for a stale route-owned host supervisor.
/// Point at the routine non-destructive refresh: idle-boundary recycle or normal
/// file-scoped restart.
pub fn host_supervisor_stale_warning_message(supervisor_pid: u32) -> String {
    format!(
        "the route-owned host supervisor (pid {supervisor_pid}) serving this document is mapping \
         a STALE agent-doc binary while a newer build is installed, so it can keep producing File \
         Cache Conflict / IPC-drift dialogs (#fcc0/#ipcdrift). Refresh it without discarding the \
         live turn: `agent-doc admin recycle` (recycles at the next idle boundary) or \
         `agent-doc session restart-supervisor <FILE>` (refuses busy panes)."
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerBootstrapStatusFacts {
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
    pub launch_mode: LaunchMode,
    pub bootstrap_epoch: u64,
    pub pid: u32,
    pub controller_binary: Option<ControllerBinaryIdentity>,
    pub controller_generation: u64,
    pub handoff_state: ControllerHandoffState,
    pub handoff_started_at: Option<u64>,
    pub previous_controller_pid: Option<u32>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    pub const fn total_authoritative_rows(&self) -> usize {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CrashRecoveryStats {
    pub actor_records: usize,
    pub supervisor_reattached: usize,
    pub supervisor_stale: usize,
    pub dispatch_retryable: usize,
    pub dispatch_blocked: usize,
    pub open_cycles_preserved: usize,
    pub projections_emitted: usize,
}

impl CrashRecoveryStats {
    pub fn new(actor_records: usize) -> Self {
        Self {
            actor_records,
            ..Self::default()
        }
    }

    pub fn record_supervisor_lease_reconcile(&mut self, fresh: bool) -> &'static str {
        if fresh {
            self.supervisor_reattached += 1;
            "reattached"
        } else {
            self.supervisor_stale += 1;
            "stale"
        }
    }

    pub fn record_dispatch_receipt_reconcile(
        &mut self,
        dispatch_start_proven: bool,
    ) -> &'static str {
        if dispatch_start_proven {
            self.dispatch_blocked += 1;
            "blocked"
        } else {
            self.dispatch_retryable += 1;
            "retryable"
        }
    }

    pub fn record_open_closeout_preserved(&mut self) -> &'static str {
        self.open_cycles_preserved += 1;
        "preserved"
    }

    pub fn record_projection_emitted(&mut self) {
        self.projections_emitted += 1;
    }

    pub fn completion_payload(&self) -> String {
        format!(
            "actor_records={} supervisor_reattached={} supervisor_stale={} dispatch_retryable={} dispatch_blocked={} open_cycles_preserved={} projections_emitted={}",
            self.actor_records,
            self.supervisor_reattached,
            self.supervisor_stale,
            self.dispatch_retryable,
            self.dispatch_blocked,
            self.open_cycles_preserved,
            self.projections_emitted
        )
    }
}

pub fn supervisor_lease_reconcile_payload(
    session_id: &str,
    pane_id: &str,
    runtime_state: Option<&str>,
    last_heartbeat: Option<u64>,
) -> String {
    format!(
        "session={} pane={} runtime_state={} heartbeat={}",
        session_id,
        pane_id,
        runtime_state.unwrap_or("unknown"),
        last_heartbeat
            .map(|timestamp| timestamp.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    )
}

pub fn dispatch_receipt_reconcile_payload(
    receipt_id: u64,
    command_kind: &str,
    result_status: Option<&str>,
    proof_scope: Option<&str>,
    dispatch_start_proven: bool,
) -> String {
    format!(
        "receipt_id={} command_kind={} result_status={} proof_scope={} dispatch_start_proven={}",
        receipt_id,
        command_kind,
        result_status.unwrap_or("unknown"),
        proof_scope.unwrap_or("unknown"),
        dispatch_start_proven
    )
}

pub fn open_closeout_preserved_payload(
    cycle_id: &str,
    state: &str,
    queue_head_id: Option<&str>,
) -> String {
    format!(
        "cycle_id={} state={} queue_head_id={}",
        cycle_id,
        state,
        queue_head_id.unwrap_or("none")
    )
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

impl ControllerHandoffState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Preparing => "preparing",
            Self::Promoted => "promoted",
            Self::Retiring => "retiring",
            Self::Failed => "failed",
        }
    }
}

/// Pure staleness predicate for stuck controller handoffs. A controller stuck
/// in `Preparing` (or `Promoted` but not finalized) past `stale_after` is
/// wedged. Stable terminal states and records without a handoff start are
/// never stale.
pub fn preparing_controller_is_stale(
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

/// Resolve the stuck-`Preparing` controller staleness threshold from the raw
/// environment override value. Callers own environment IO.
pub fn stale_preparing_controller_threshold_from_env_value(raw: Option<&str>) -> Duration {
    let secs = raw
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_STALE_PREPARING_CONTROLLER_SECS);
    Duration::from_secs(secs.max(1))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerWatchdogFacts {
    pub handoff_state: ControllerHandoffState,
    pub handoff_started_at: Option<u64>,
    pub now: u64,
    pub stale_after: Duration,
    pub is_handoff_replacement: bool,
    pub handoff_replacement_socket_exists: bool,
    pub launched_elapsed: Duration,
}

/// Pure self-watchdog policy for a controller process.
///
/// A controller should exit when its in-memory handoff state is wedged past the
/// threshold, or when a handoff replacement has been promoted in memory but is
/// still stranded on its temporary socket past the same threshold. Callers own
/// collecting runtime/IO facts such as wall-clock time and socket existence.
pub fn controller_watchdog_should_suicide(facts: ControllerWatchdogFacts) -> bool {
    preparing_controller_is_stale(
        facts.handoff_state,
        facts.handoff_started_at,
        facts.now,
        facts.stale_after,
    ) || preparing_without_a_handoff_clock(
        facts.handoff_state,
        facts.handoff_started_at,
        facts.launched_elapsed,
        facts.stale_after,
    ) || handoff_replacement_is_stranded(
        facts.is_handoff_replacement,
        facts.handoff_replacement_socket_exists,
        facts.launched_elapsed,
        facts.stale_after,
    )
}

/// A controller stuck in a handoff that never recorded when the handoff began
/// (`#preparingnoclockwedge`).
///
/// `preparing_controller_is_stale` needs `handoff_started_at` and returns
/// `false` without it; `handoff_replacement_is_stranded` needs the temporary
/// socket to still exist. A controller whose handoff clock was never written
/// and whose socket is gone satisfies neither, so nothing could ever reap it —
/// and both facts are missing exactly when the wedge is worst. Observed live on
/// 2026-08-10: two controllers `Preparing` against DEAD predecessors, sockets
/// absent, one burning 95% CPU for minutes, while the 45s self-reap never fired.
/// The same shape was recorded earlier as a wedge that survived three hours on a
/// current binary with no explanation.
///
/// This is the `#idlerevisionreactive` inversion again: "I never looked" and
/// "there is nothing wrong" must not collapse into the same answer. The process
/// age is the one clock that is ALWAYS known, so it is the honest fallback — no
/// handoff can legitimately be preparing for longer than the staleness threshold
/// regardless of what got recorded.
pub fn preparing_without_a_handoff_clock(
    handoff_state: ControllerHandoffState,
    handoff_started_at: Option<u64>,
    launched_elapsed: Duration,
    stale_after: Duration,
) -> bool {
    matches!(
        handoff_state,
        ControllerHandoffState::Preparing | ControllerHandoffState::Promoted
    ) && handoff_started_at.is_none()
        && launched_elapsed > stale_after
}

/// Pure structural predicate for a promoted-but-stranded handoff replacement.
/// Runtime code supplies whether this process is a handoff replacement and
/// whether its temporary socket still exists.
pub fn handoff_replacement_is_stranded(
    is_handoff_replacement: bool,
    handoff_replacement_socket_exists: bool,
    launched_elapsed: Duration,
    stale_after: Duration,
) -> bool {
    is_handoff_replacement && handoff_replacement_socket_exists && launched_elapsed > stale_after
}

/// `#stuckhandoffselfheal` — a promoted-but-stranded replacement should COMPLETE its
/// own promotion (rename its temp handoff socket onto the canonical public path)
/// rather than suicide. It already received `promote_handoff` (in-memory `Stable`),
/// so it holds the authoritative generation; its handoff client merely died before
/// performing the final rename. Self-promoting preserves the live controller (and any
/// in-flight turn) instead of forcing a relaunch, and — crucially — restores
/// `controller.sock` so clients that resolve via the recorded `socket_path` (e.g.
/// finalize/commit) stop timing out and never fall back to an out-of-band disk write
/// the IPC snapshot cannot adopt (the drift root cause).
///
/// A short `grace` — not the full stale threshold — bounds the canonical-socket
/// outage while still never racing a *healthy* client, which renames within
/// milliseconds of `promote_handoff`. Only `Stable` replacements self-promote; a
/// replacement still `Preparing` (never promoted) has no authority and is left to the
/// stranded/suicide watchdog so a clean generation binds the public socket instead.
pub fn handoff_replacement_should_self_promote(
    is_handoff_replacement: bool,
    handoff_replacement_socket_exists: bool,
    handoff_state: ControllerHandoffState,
    launched_elapsed: Duration,
    grace: Duration,
) -> bool {
    is_handoff_replacement
        && handoff_replacement_socket_exists
        && handoff_state == ControllerHandoffState::Stable
        && launched_elapsed > grace
}

pub fn supervisor_lease_is_fresh_and_alive(
    last_heartbeat: Option<u64>,
    supervisor_process_alive: bool,
    now: u64,
    stale_after: Duration,
) -> bool {
    let fresh_heartbeat = last_heartbeat
        .map(|timestamp| now.saturating_sub(timestamp) <= stale_after.as_secs())
        .unwrap_or(false);
    fresh_heartbeat && supervisor_process_alive
}

pub const fn supervisor_lease_pid_is_foreign(supervisor_pid: Option<u32>, self_pid: u32) -> bool {
    match supervisor_pid {
        Some(pid) => pid != self_pid,
        None => false,
    }
}

pub fn default_control_plane_status() -> ControlPlaneStatus {
    ControlPlaneStatus {
        process_model: "project_scoped_single_process".to_string(),
        external_boundary: "controller_ipc".to_string(),
        state_authority: ".agent-doc/state.db".to_string(),
        projection_authority: "cold_recovery_output".to_string(),
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
            authority: "cold_recovery_projection".to_string(),
            state: "unknown".to_string(),
            owned_items: 0,
            categories: BTreeMap::new(),
        },
    }
}

pub fn status_categories<const N: usize>(pairs: [(&str, usize); N]) -> BTreeMap<String, usize> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

pub fn control_plane_status(
    active: bool,
    counts: ControlPlaneStoreCounts,
    memory_categories: Option<BTreeMap<String, usize>>,
) -> ControlPlaneStatus {
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

    ControlPlaneStatus {
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
    }
}

pub fn controller_status_from_bootstrap(
    bootstrap: &ControllerBootstrapStatusFacts,
    active: bool,
    stale_duplicate_pids: Vec<u32>,
    freshness: ControllerFreshnessStatus,
    control_plane: ControlPlaneStatus,
) -> ControllerStatus {
    ControllerStatus {
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
        stale_duplicate_pids,
        freshness: Some(freshness),
        control_plane,
    }
}

pub fn inactive_controller_status(
    project_root: &Path,
    socket_path: PathBuf,
    bootstrap: Option<&ControllerBootstrapStatusFacts>,
    stale_duplicate_pids: Vec<u32>,
    freshness: ControllerFreshnessStatus,
    control_plane: ControlPlaneStatus,
) -> ControllerStatus {
    ControllerStatus {
        active: false,
        project_root: project_root.to_path_buf(),
        socket_path,
        launch_mode: bootstrap.map(|state| state.launch_mode),
        bootstrap_epoch: bootstrap.map(|state| state.bootstrap_epoch),
        pid: bootstrap.map(|state| state.pid),
        controller_binary: bootstrap.and_then(|state| state.controller_binary.clone()),
        controller_generation: bootstrap.map(|state| state.controller_generation),
        handoff_state: bootstrap.map(|state| state.handoff_state),
        handoff_started_at: bootstrap.and_then(|state| state.handoff_started_at),
        previous_controller_pid: bootstrap.and_then(|state| state.previous_controller_pid),
        stale_duplicate_pids,
        freshness: Some(freshness),
        control_plane,
    }
}

pub fn controller_freshness_status(facts: ControllerFreshnessFacts) -> ControllerFreshnessStatus {
    let controller = controller_process_freshness_from_inodes(
        "controller",
        facts.controller_pid,
        facts.controller_running_inode,
        facts.installed_inode,
    );
    let route_owned_supervisor = facts.route_owned_supervisor_pid.map(|pid| {
        controller_process_freshness_from_inodes(
            "route_owned_supervisor",
            Some(pid),
            facts.route_owned_supervisor_running_inode,
            facts.installed_inode,
        )
    });
    let mut processes = vec![&controller];
    if let Some(supervisor) = route_owned_supervisor.as_ref() {
        processes.push(supervisor);
    }
    let guidance = if processes.iter().any(|process| process.stale) {
        "stale: one or more long-running agent-doc processes map a different binary inode; recycle or restart at an idle boundary".to_string()
    } else if processes
        .iter()
        .all(|process| process.matches_installed == Some(true))
    {
        "fresh: controller/supervisor inode identity matches the installed agent-doc binary"
            .to_string()
    } else {
        "partial: inode proof unavailable for one or more processes; restart only if behavior remains stale".to_string()
    };
    ControllerFreshnessStatus {
        installed_binary: facts.installed_binary,
        installed_inode: facts.installed_inode,
        controller,
        route_owned_supervisor,
        guidance,
    }
}

pub fn controller_process_freshness_from_inodes(
    role: &str,
    pid: Option<u32>,
    running_inode: Option<u64>,
    installed_inode: Option<u64>,
) -> ControllerProcessFreshness {
    let matches_installed = match (running_inode, installed_inode) {
        (Some(running), Some(installed)) => Some(running == installed),
        _ => None,
    };
    let stale = matches_installed == Some(false);
    let guidance = match matches_installed {
        Some(true) => {
            format!("fresh: {role} running inode matches the installed agent-doc binary")
        }
        Some(false) => {
            format!(
                "stale: {role} running inode differs from the installed agent-doc binary; recycle or restart at an idle boundary"
            )
        }
        None => {
            format!(
                "unknown: {role} running or installed inode unavailable; inspect /proc/<pid>/exe on Linux or restart if behavior remains stale"
            )
        }
    };
    ControllerProcessFreshness {
        role: role.to_string(),
        pid,
        running_inode,
        installed_inode,
        matches_installed,
        stale,
        guidance,
    }
}

pub fn parse_handoff_state(
    raw: &str,
) -> Result<ControllerHandoffState, ParseControllerHandoffStateError> {
    match raw {
        "stable" => Ok(ControllerHandoffState::Stable),
        "preparing" => Ok(ControllerHandoffState::Preparing),
        "promoted" => Ok(ControllerHandoffState::Promoted),
        "retiring" => Ok(ControllerHandoffState::Retiring),
        "failed" => Ok(ControllerHandoffState::Failed),
        other => Err(ParseControllerHandoffStateError {
            raw: other.to_string(),
        }),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseControllerHandoffStateError {
    raw: String,
}

impl fmt::Display for ParseControllerHandoffStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown controller handoff state: {}", self.raw)
    }
}

impl Error for ParseControllerHandoffStateError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(version: &str, len: u64) -> ControllerBinaryIdentity {
        ControllerBinaryIdentity {
            path: PathBuf::from("/tmp/agent-doc"),
            version: version.to_string(),
            len,
            modified_secs: 1_000,
            modified_nanos: 7,
        }
    }

    fn test_controller_status(
        active: bool,
        controller_binary: Option<ControllerBinaryIdentity>,
    ) -> ControllerStatus {
        ControllerStatus {
            active,
            project_root: PathBuf::from("/tmp/project"),
            socket_path: PathBuf::from("/tmp/project/.agent-doc/controller.sock"),
            launch_mode: Some(LaunchMode::Lazy),
            bootstrap_epoch: Some(1),
            pid: Some(166599),
            controller_binary,
            controller_generation: Some(1),
            handoff_state: Some(ControllerHandoffState::Stable),
            handoff_started_at: None,
            previous_controller_pid: None,
            stale_duplicate_pids: Vec::new(),
            freshness: None,
            control_plane: default_control_plane_status(),
        }
    }

    #[test]
    fn controller_binary_identity_match_is_strict_and_fail_open() {
        let current = identity("1.2.3", 42);
        let mut changed = current.clone();
        changed.modified_nanos = changed.modified_nanos.wrapping_add(1);

        assert!(controller_binary_identity_matches(
            Some(&current),
            Some(&current)
        ));
        assert!(!controller_binary_identity_matches(None, Some(&current)));
        assert!(!controller_binary_identity_matches(Some(&current), None));
        assert!(!controller_binary_identity_matches(
            Some(&changed),
            Some(&current)
        ));
    }

    #[test]
    fn process_binary_staleness_compares_recorded_and_current_identity() {
        let current = identity("1.2.3", 42);
        let stale = identity("1.2.2", 41);

        assert!(!process_binary_is_stale(None, Some(&current)));
        assert!(!process_binary_is_stale(Some(&current), None));
        assert!(!process_binary_is_stale(Some(&current), Some(&current)));
        assert!(process_binary_is_stale(Some(&stale), Some(&current)));
    }

    #[test]
    fn binary_replacement_is_directional_and_accepts_same_version_rebuilds() {
        let recorded = identity("1.2.3", 42);
        let newer_version = identity("1.2.4", 41);
        let older_version = identity("1.2.2", 99);
        let mut newer_build = recorded.clone();
        newer_build.modified_nanos += 1;
        let mut older_build = recorded.clone();
        older_build.modified_nanos -= 1;

        assert!(controller_binary_identity_is_newer(
            Some(&newer_version),
            Some(&recorded)
        ));
        assert!(!controller_binary_identity_is_newer(
            Some(&older_version),
            Some(&recorded)
        ));
        assert!(controller_binary_identity_is_newer(
            Some(&newer_build),
            Some(&recorded)
        ));
        assert!(!controller_binary_identity_is_newer(
            Some(&older_build),
            Some(&recorded)
        ));
        assert!(!controller_binary_identity_is_newer(None, Some(&recorded)));
    }

    #[test]
    fn default_controller_generation_is_initial_generation() {
        assert_eq!(default_controller_generation(), 1);
    }

    #[test]
    fn controller_restart_recovery_needed_requires_generation_or_previous_pid() {
        assert!(!controller_restart_recovery_needed(0, None));
        assert!(!controller_restart_recovery_needed(
            default_controller_generation(),
            None
        ));
        assert!(controller_restart_recovery_needed(
            default_controller_generation() + 1,
            None
        ));
        assert!(controller_restart_recovery_needed(
            default_controller_generation(),
            Some(42)
        ));
    }

    #[test]
    fn crash_recovery_stats_classify_markers_and_summary_payload() {
        let mut stats = CrashRecoveryStats::new(3);

        assert_eq!(stats.record_supervisor_lease_reconcile(true), "reattached");
        assert_eq!(stats.record_supervisor_lease_reconcile(false), "stale");
        assert_eq!(stats.record_dispatch_receipt_reconcile(false), "retryable");
        assert_eq!(stats.record_dispatch_receipt_reconcile(true), "blocked");
        assert_eq!(stats.record_open_closeout_preserved(), "preserved");
        stats.record_projection_emitted();

        assert_eq!(
            stats,
            CrashRecoveryStats {
                actor_records: 3,
                supervisor_reattached: 1,
                supervisor_stale: 1,
                dispatch_retryable: 1,
                dispatch_blocked: 1,
                open_cycles_preserved: 1,
                projections_emitted: 1,
            }
        );
        assert_eq!(
            stats.completion_payload(),
            "actor_records=3 supervisor_reattached=1 supervisor_stale=1 dispatch_retryable=1 dispatch_blocked=1 open_cycles_preserved=1 projections_emitted=1"
        );
    }

    #[test]
    fn crash_recovery_marker_payloads_fill_unknowns() {
        assert_eq!(
            supervisor_lease_reconcile_payload("session-1", "%1", Some("ready"), Some(42)),
            "session=session-1 pane=%1 runtime_state=ready heartbeat=42"
        );
        assert_eq!(
            supervisor_lease_reconcile_payload("session-1", "%1", None, None),
            "session=session-1 pane=%1 runtime_state=unknown heartbeat=unknown"
        );
        assert_eq!(
            dispatch_receipt_reconcile_payload(
                7,
                "dispatch",
                Some("accepted"),
                Some("accepted_only"),
                false,
            ),
            "receipt_id=7 command_kind=dispatch result_status=accepted proof_scope=accepted_only dispatch_start_proven=false"
        );
        assert_eq!(
            dispatch_receipt_reconcile_payload(8, "dispatch", None, None, true),
            "receipt_id=8 command_kind=dispatch result_status=unknown proof_scope=unknown dispatch_start_proven=true"
        );
        assert_eq!(
            open_closeout_preserved_payload("cycle-1", "closing", Some("head-1")),
            "cycle_id=cycle-1 state=closing queue_head_id=head-1"
        );
        assert_eq!(
            open_closeout_preserved_payload("cycle-1", "closing", None),
            "cycle_id=cycle-1 state=closing queue_head_id=none"
        );
    }

    #[test]
    fn identity_version_prefers_injected_binary_version() {
        assert_eq!(
            resolve_controller_identity_version(Some("0.34.30"), "0.1.0"),
            "0.34.30"
        );
    }

    #[test]
    fn identity_version_falls_back_to_crate_version() {
        assert_eq!(
            resolve_controller_identity_version(None, "0.34.66"),
            "0.34.66"
        );
    }

    #[test]
    fn stale_supervisor_warning_message_fires_for_active_stale_controller() {
        let current = identity("1.2.3", 42);
        let stale = identity("1.2.2", 41);
        let status = test_controller_status(true, Some(stale));

        let msg = supervisor_stale_warning_message(&status, Some(&current))
            .expect("an active host on a stale binary must warn");
        assert!(msg.contains("pid 166599"), "message: {msg}");
        assert!(msg.contains("launched as 1.2.2"), "message: {msg}");
        assert!(msg.contains("STALE"), "message: {msg}");
        assert!(msg.contains("agent-doc admin recycle"), "message: {msg}");
        assert!(
            msg.contains("agent-doc session restart-supervisor <FILE>"),
            "message: {msg}"
        );
        assert!(!msg.contains("--force"), "message: {msg}");
        assert!(!msg.contains("interrupt-clear"), "message: {msg}");
    }

    #[test]
    fn stale_supervisor_warning_message_is_none_for_inactive_or_fresh() {
        let current = identity("1.2.3", 42);
        let stale = identity("1.2.2", 41);

        assert!(
            supervisor_stale_warning_message(
                &test_controller_status(false, Some(stale)),
                Some(&current)
            )
            .is_none()
        );
        assert!(
            supervisor_stale_warning_message(
                &test_controller_status(true, Some(current.clone())),
                Some(&current)
            )
            .is_none()
        );
        assert!(
            supervisor_stale_warning_message(&test_controller_status(true, None), Some(&current))
                .is_none()
        );
        assert!(
            supervisor_stale_warning_message(&test_controller_status(true, Some(current)), None)
                .is_none()
        );
    }

    #[test]
    fn stale_supervisor_host_warning_message_uses_non_destructive_refresh() {
        let msg = host_supervisor_stale_warning_message(166599);
        assert!(msg.contains("pid 166599"), "message: {msg}");
        assert!(msg.contains("STALE"), "message: {msg}");
        assert!(msg.contains("agent-doc admin recycle"), "message: {msg}");
        assert!(
            msg.contains("agent-doc session restart-supervisor <FILE>"),
            "message: {msg}"
        );
        assert!(msg.contains("idle boundary"), "message: {msg}");
        assert!(msg.contains("refuses busy panes"), "message: {msg}");
        assert!(!msg.contains("--force"), "message: {msg}");
        assert!(!msg.contains("interrupt-clear"), "message: {msg}");
    }

    #[test]
    fn controller_process_freshness_classifies_inode_identity() {
        let fresh =
            controller_process_freshness_from_inodes("controller", Some(7), Some(11), Some(11));
        assert_eq!(fresh.matches_installed, Some(true));
        assert!(!fresh.stale);
        assert!(fresh.guidance.contains("fresh"));

        let stale = controller_process_freshness_from_inodes(
            "route_owned_supervisor",
            Some(8),
            Some(10),
            Some(11),
        );
        assert_eq!(stale.matches_installed, Some(false));
        assert!(stale.stale);
        assert!(stale.guidance.contains("stale"));

        let unknown =
            controller_process_freshness_from_inodes("controller", Some(9), None, Some(11));
        assert_eq!(unknown.matches_installed, None);
        assert!(!unknown.stale);
        assert!(unknown.guidance.contains("unknown"));
    }

    #[test]
    fn parses_handoff_state_wire_values() {
        assert_eq!(
            parse_handoff_state("stable").unwrap(),
            ControllerHandoffState::Stable
        );
        assert_eq!(
            parse_handoff_state("preparing").unwrap(),
            ControllerHandoffState::Preparing
        );
        assert!(parse_handoff_state("wedged").is_err());
    }

    #[test]
    fn preparing_controller_staleness_truth_table() {
        let stale_after = Duration::from_secs(45);
        let now = 10_000u64;

        assert!(preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 100),
            now,
            stale_after,
        ));
        assert!(preparing_controller_is_stale(
            ControllerHandoffState::Promoted,
            Some(now - 100),
            now,
            stale_after,
        ));
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 5),
            now,
            stale_after,
        ));
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            Some(now - 45),
            now,
            stale_after,
        ));
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
        assert!(!preparing_controller_is_stale(
            ControllerHandoffState::Preparing,
            None,
            now,
            stale_after,
        ));
    }

    #[test]
    fn stale_preparing_controller_threshold_defaults_on_missing_or_invalid_env_value() {
        assert_eq!(
            stale_preparing_controller_threshold_from_env_value(None),
            Duration::from_secs(45)
        );
        assert_eq!(
            stale_preparing_controller_threshold_from_env_value(Some("")),
            Duration::from_secs(45)
        );
        assert_eq!(
            stale_preparing_controller_threshold_from_env_value(Some("bogus")),
            Duration::from_secs(45)
        );
        assert_eq!(
            stale_preparing_controller_threshold_from_env_value(Some("-3")),
            Duration::from_secs(45)
        );
    }

    #[test]
    fn stale_preparing_controller_threshold_trims_and_clamps_to_one_second() {
        assert_eq!(
            stale_preparing_controller_threshold_from_env_value(Some(" 90 ")),
            Duration::from_secs(90)
        );
        assert_eq!(
            stale_preparing_controller_threshold_from_env_value(Some("0")),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn handoff_replacement_stranded_policy_requires_replacement_socket_and_age() {
        let stale_after = Duration::from_secs(45);

        assert!(handoff_replacement_is_stranded(
            true,
            true,
            Duration::from_secs(46),
            stale_after,
        ));
        assert!(!handoff_replacement_is_stranded(
            false,
            true,
            Duration::from_secs(46),
            stale_after,
        ));
        assert!(!handoff_replacement_is_stranded(
            true,
            false,
            Duration::from_secs(46),
            stale_after,
        ));
        assert!(!handoff_replacement_is_stranded(
            true,
            true,
            Duration::from_secs(45),
            stale_after,
        ));
    }

    #[test]
    fn handoff_replacement_self_promote_policy_requires_stable_socket_and_grace() {
        let grace = Duration::from_secs(3);

        // Promoted (Stable) + still on temp socket + past grace → self-promote.
        assert!(handoff_replacement_should_self_promote(
            true,
            true,
            ControllerHandoffState::Stable,
            Duration::from_secs(4),
            grace,
        ));
        // Not yet promoted (still Preparing) → leave to the stranded/suicide watchdog,
        // never self-promote a replacement that has no authority.
        assert!(!handoff_replacement_should_self_promote(
            true,
            true,
            ControllerHandoffState::Preparing,
            Duration::from_secs(4),
            grace,
        ));
        // Temp socket already renamed away (healthy client finished) → nothing to do.
        assert!(!handoff_replacement_should_self_promote(
            true,
            false,
            ControllerHandoffState::Stable,
            Duration::from_secs(4),
            grace,
        ));
        // Initial controller on the public socket (not a replacement) → never.
        assert!(!handoff_replacement_should_self_promote(
            false,
            true,
            ControllerHandoffState::Stable,
            Duration::from_secs(4),
            grace,
        ));
        // Within grace → don't race a healthy client mid-rename.
        assert!(!handoff_replacement_should_self_promote(
            true,
            true,
            ControllerHandoffState::Stable,
            Duration::from_secs(3),
            grace,
        ));
    }

    #[test]
    fn controller_watchdog_policy_combines_handoff_and_stranded_replacement() {
        let stale_after = Duration::from_secs(45);
        let now = 10_000u64;
        let base = ControllerWatchdogFacts {
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            now,
            stale_after,
            is_handoff_replacement: false,
            handoff_replacement_socket_exists: false,
            launched_elapsed: Duration::from_secs(0),
        };

        assert!(!controller_watchdog_should_suicide(base));
        assert!(controller_watchdog_should_suicide(
            ControllerWatchdogFacts {
                handoff_state: ControllerHandoffState::Preparing,
                handoff_started_at: Some(now - 100),
                ..base
            }
        ));
        assert!(controller_watchdog_should_suicide(
            ControllerWatchdogFacts {
                is_handoff_replacement: true,
                handoff_replacement_socket_exists: true,
                launched_elapsed: Duration::from_secs(46),
                ..base
            }
        ));
        assert!(!controller_watchdog_should_suicide(
            ControllerWatchdogFacts {
                is_handoff_replacement: true,
                handoff_replacement_socket_exists: false,
                launched_elapsed: Duration::from_secs(46),
                ..base
            }
        ));
    }

    #[test]
    fn supervisor_lease_freshness_requires_recent_heartbeat_and_live_process() {
        let stale_after = Duration::from_secs(60);
        let now = 1_000u64;

        assert!(supervisor_lease_is_fresh_and_alive(
            Some(now - 60),
            true,
            now,
            stale_after
        ));
        assert!(!supervisor_lease_is_fresh_and_alive(
            Some(now - 61),
            true,
            now,
            stale_after
        ));
        assert!(!supervisor_lease_is_fresh_and_alive(
            Some(now - 1),
            false,
            now,
            stale_after
        ));
        assert!(!supervisor_lease_is_fresh_and_alive(
            None,
            true,
            now,
            stale_after
        ));
    }

    #[test]
    fn supervisor_lease_foreign_pid_requires_present_different_pid() {
        assert!(supervisor_lease_pid_is_foreign(Some(42), 7));
        assert!(!supervisor_lease_pid_is_foreign(Some(7), 7));
        assert!(!supervisor_lease_pid_is_foreign(None, 7));
    }
}

#[cfg(test)]
mod preparing_without_a_handoff_clock_tests {
    use super::*;

    fn facts(
        handoff_state: ControllerHandoffState,
        handoff_started_at: Option<u64>,
        launched_elapsed: Duration,
    ) -> ControllerWatchdogFacts {
        ControllerWatchdogFacts {
            handoff_state,
            handoff_started_at,
            now: 1_000,
            stale_after: Duration::from_secs(45),
            is_handoff_replacement: true,
            // The wedge that motivated this: the temporary socket is GONE, so
            // `handoff_replacement_is_stranded` cannot fire either.
            handoff_replacement_socket_exists: false,
            launched_elapsed,
        }
    }

    /// The live 2026-08-10 wedge: `Preparing`, no recorded handoff clock, socket
    /// absent, minutes of wall time, 95% CPU. Neither existing escape could fire.
    #[test]
    fn a_preparing_controller_with_no_clock_reaps_on_process_age() {
        assert!(controller_watchdog_should_suicide(facts(
            ControllerHandoffState::Preparing,
            None,
            Duration::from_secs(200),
        )));
    }

    /// Promoted-but-unfinalized has the same hole.
    #[test]
    fn a_promoted_controller_with_no_clock_reaps_on_process_age() {
        assert!(controller_watchdog_should_suicide(facts(
            ControllerHandoffState::Promoted,
            None,
            Duration::from_secs(200),
        )));
    }

    /// Young processes are left alone: a handoff legitimately takes time, and
    /// reaping inside the threshold would kill healthy startups.
    #[test]
    fn a_young_handoff_without_a_clock_is_left_alone() {
        assert!(!controller_watchdog_should_suicide(facts(
            ControllerHandoffState::Preparing,
            None,
            Duration::from_secs(10),
        )));
    }

    /// A terminal state is never reaped by this rule, however old the process is.
    /// A long-lived healthy controller is exactly what `Stable` means.
    #[test]
    fn a_stable_controller_is_never_reaped_by_process_age() {
        for state in [
            ControllerHandoffState::Stable,
            ControllerHandoffState::Retiring,
            ControllerHandoffState::Failed,
        ] {
            assert!(
                !controller_watchdog_should_suicide(facts(
                    state,
                    None,
                    Duration::from_secs(100_000),
                )),
                "{state:?} must not be reaped on age"
            );
        }
    }

    /// When the handoff clock IS recorded, the existing predicate owns the
    /// decision and this fallback must not widen it.
    #[test]
    fn a_recorded_clock_leaves_the_decision_to_the_existing_predicate() {
        assert!(!preparing_without_a_handoff_clock(
            ControllerHandoffState::Preparing,
            Some(500),
            Duration::from_secs(100_000),
            Duration::from_secs(45),
        ));
    }
}
