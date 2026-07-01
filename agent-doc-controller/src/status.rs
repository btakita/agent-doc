//! Pure controller status and freshness projections.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// True when a recorded long-lived process identity matches the freshly
/// resolved controller binary identity. Missing identity is a fail-open
/// mismatch.
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

pub fn default_control_plane_status() -> ControlPlaneStatus {
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
}
