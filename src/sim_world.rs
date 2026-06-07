//! Test-only deterministic workflow simulator.
//!
//! The simulator deliberately stays small: it models the closeout state that is
//! cheap to exercise in memory, and delegates document semantics to production
//! parsers/classifiers wherever possible.

use anyhow::{Result, anyhow, bail};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FAST_CORPUS_SEEDS: std::ops::Range<u64> = 0..512;
const FAST_CORPUS_STEPS: usize = 24;
const FAST_CORPUS_BUDGET: Duration = Duration::from_secs(3);
const MEDIUM_CORPUS_SEEDS: std::ops::Range<u64> = 0..2_048;
const MEDIUM_CORPUS_STEPS: usize = 32;
const MEDIUM_CORPUS_BUDGET: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CyclePhase {
    Idle,
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Interrupted(FaultPoint),
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FaultPoint {
    SnapshotSave,
    FallbackPatchWrite,
    IpcDelivery,
    TemplateMerge,
    WorkingTreeWrite,
    IndexUpdate,
    GitCommit,
    PostCommitBoundaryReposition,
    SessionCheck,
}

impl FaultPoint {
    const ALL: [FaultPoint; 9] = [
        FaultPoint::SnapshotSave,
        FaultPoint::FallbackPatchWrite,
        FaultPoint::IpcDelivery,
        FaultPoint::TemplateMerge,
        FaultPoint::WorkingTreeWrite,
        FaultPoint::IndexUpdate,
        FaultPoint::GitCommit,
        FaultPoint::PostCommitBoundaryReposition,
        FaultPoint::SessionCheck,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimCommand {
    EditPrompt,
    EditLaterPrompt,
    AddMalformedBacklogItem,
    CaptureResponse,
    CaptureFallbackResponse,
    ApplyCapturedResponse,
    Commit,
    FailCommit,
    RepairBoundary,
    DuplicateVisibleResponse,
    CrashAt(FaultPoint),
    Recover,
    SessionClear,
    SessionRestart,
    SessionRestartForce,
    SessionRestartForcePreInterruptIdle,
    BindRouteOwner,
    SupervisorReady,
    SupervisorBusy,
    SupervisorWaitingInput,
    SupervisorBlocked,
    SupervisorClosed,
    DispatchRoutePrompt,
    ProveDispatchAccepted,
    StaleSupervisorUpdate,
    ObserveStalePane,
    ObserveMissingPane,
    DriftProjection,
    RepairProjection,
    PromoteStartingPromptReady,
    BusyInterruptRecoveryReady,
    RepairBusyProjectionWithReadyPrompt,
    SyncProtectedGrowthManual,
    SyncProtectedGrowthPassive,
    SyncProtectedGrowthFocusVisible,
    SyncDetachableReplaceManual,
    SyncDetachableReplacePassive,
    SyncVisibleFocusPreserve,
    SyncRerequestVisibleEditorManual,
    SyncRerequestVisibleEditorPassive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupervisorLifecycle {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Blocked,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartInterruptOutcome {
    StillBusy,
    IdleBeforeForceKill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActorState {
    generation: u64,
    session_id: String,
    pane_id: Option<String>,
    lifecycle: SupervisorLifecycle,
}

impl ActorState {
    fn initial() -> Self {
        Self {
            generation: 1,
            session_id: "session-1".to_string(),
            pane_id: Some("%1".to_string()),
            lifecycle: SupervisorLifecycle::Starting,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DispatchReceipt {
    generation: u64,
    session_id: String,
    pane_id: String,
    proved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncMode {
    Full,
    SafePassive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SyncOutcome {
    PreservedLayoutAndFocused,
    ReplacedDetachable(usize),
    AttachedAroundProtected(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FullContentReplacementSource {
    CompactExchange,
    FullContentRepair,
    IpcTimeoutRecovery,
}

impl FullContentReplacementSource {
    const ALL: [FullContentReplacementSource; 3] = [
        FullContentReplacementSource::CompactExchange,
        FullContentReplacementSource::FullContentRepair,
        FullContentReplacementSource::IpcTimeoutRecovery,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::CompactExchange => "compact_exchange",
            Self::FullContentRepair => "full_content_repair",
            Self::IpcTimeoutRecovery => "ipc_timeout_recovery",
        }
    }
}

#[derive(Debug, Clone)]
struct SyncProjection {
    visible: Vec<String>,
    protected_open_cycle: BTreeSet<String>,
    stashed: BTreeSet<String>,
    active: Option<String>,
}

impl SyncProjection {
    fn protected_growth_case() -> Self {
        Self {
            visible: vec!["protected".to_string(), "sibling".to_string()],
            protected_open_cycle: BTreeSet::from(["protected".to_string()]),
            stashed: BTreeSet::from(["requested".to_string()]),
            active: Some("sibling".to_string()),
        }
    }

    fn detachable_replacement_case() -> Self {
        Self {
            visible: vec!["protected".to_string(), "detachable".to_string()],
            protected_open_cycle: BTreeSet::from(["protected".to_string()]),
            stashed: BTreeSet::from(["requested".to_string()]),
            active: Some("protected".to_string()),
        }
    }

    /// The editor document is already visible under one pane. This models the
    /// state in which a duplicate-claim / pane-id churn makes sync re-request the
    /// same document that is already on screen (see
    /// `duplicate_live_pane_claim` in `agent-doc-orchestration::sync`). The
    /// reconciler must recognize the document as already present and must NOT
    /// attach a second editor pane for it (no-duplicate-editor-pane invariant).
    fn rerequested_visible_editor_case() -> Self {
        Self {
            visible: vec!["editor".to_string()],
            protected_open_cycle: BTreeSet::new(),
            stashed: BTreeSet::new(),
            active: Some("editor".to_string()),
        }
    }

    fn apply_requested_projection(
        &mut self,
        requested_docs: &[&str],
        focus_doc: &str,
        mode: SyncMode,
    ) -> SyncOutcome {
        let _mode_label = match mode {
            SyncMode::Full => "manual",
            SyncMode::SafePassive => "safe-passive",
        };
        let requested: BTreeSet<String> = requested_docs
            .iter()
            .map(|doc| (*doc).to_string())
            .collect();
        let visible: BTreeSet<String> = self.visible.iter().cloned().collect();
        let missing: Vec<String> = requested
            .iter()
            .filter(|doc| !visible.contains(*doc))
            .cloned()
            .collect();
        let detachable_unwanted: Vec<usize> = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(index, doc)| {
                if requested.contains(doc) || self.protected_open_cycle.contains(doc) {
                    None
                } else {
                    Some(index)
                }
            })
            .collect();

        if missing.len() > detachable_unwanted.len() {
            let attach_count = missing.len();
            for doc in missing {
                self.stashed.remove(&doc);
                self.visible.push(doc);
            }
            if self.visible.iter().any(|doc| doc == focus_doc) {
                self.active = Some(focus_doc.to_string());
            }
            return SyncOutcome::AttachedAroundProtected(attach_count);
        }

        let replacement_count = missing.len();
        for (doc, index) in missing.into_iter().zip(detachable_unwanted.into_iter()) {
            self.stashed.remove(&doc);
            self.stashed.insert(self.visible[index].clone());
            self.visible[index] = doc;
        }
        if self.visible.iter().any(|doc| doc == focus_doc) {
            self.active = Some(focus_doc.to_string());
        }
        SyncOutcome::ReplacedDetachable(replacement_count)
    }
}

impl Default for SyncProjection {
    fn default() -> Self {
        Self::protected_growth_case()
    }
}

#[derive(Debug, Clone)]
struct RouteModel {
    durable: ActorState,
    projection: ActorState,
    pending_dispatch: Option<DispatchReceipt>,
    starting_timeout: Option<(u64, String)>,
}

impl RouteModel {
    fn new() -> Self {
        let durable = ActorState::initial();
        Self {
            projection: durable.clone(),
            durable,
            pending_dispatch: None,
            starting_timeout: None,
        }
    }
}

#[derive(Debug, Default)]
struct Coverage {
    unresolved_prompt_blocks: usize,
    malformed_backlog_blocks: usize,
    uncommitted_response_blocks: usize,
    duplicate_patchback_blocks: usize,
    boundary_repairs: usize,
    fault_fail_closed: usize,
    fault_recoveries: usize,
    fault_noops: usize,
    fault_points_hit: BTreeSet<FaultPoint>,
    route_generation_rebinds: usize,
    supervisor_lifecycle_updates: usize,
    route_dispatch_acceptances: usize,
    route_dispatch_proofs: usize,
    session_clears: usize,
    session_restart_busy_refusals: usize,
    session_restart_force_used: usize,
    session_restart_busy_pre_interrupt_idle: usize,
    session_restart_busy_force_killed: usize,
    session_restarts: usize,
    starting_dispatch_blocks: usize,
    starting_timeout_records: usize,
    starting_timeout_coalesces: usize,
    prompt_duplicate_repairs: usize,
    normalization_repair_patches: usize,
    sidecar_normalization_divergences: usize,
    stale_source_buffer_skips: usize,
    ipc_snapshot_live_prompt_blocks: usize,
    ack_sidecar_only_repairs: usize,
    visible_duplicate_repairs: usize,
    post_commit_follow_up_handoffs: usize,
    starting_prompt_promotions: usize,
    busy_dispatch_blocks: usize,
    closed_dispatch_blocks: usize,
    busy_interrupt_recoveries: usize,
    busy_projection_ready_repairs: usize,
    stale_generation_blocks: usize,
    stale_pane_blocks: usize,
    missing_pane_blocks: usize,
    projection_drift_blocks: usize,
    projection_repairs: usize,
    sync_preserve_layout_blocks: usize,
    sync_detachable_replacements: usize,
    sync_protected_expansions: usize,
    sync_focus_handoffs: usize,
    commits: usize,
}

impl Coverage {
    fn record_block(&mut self, message: &str) {
        if message.contains("unresolved prompt_target") {
            self.unresolved_prompt_blocks += 1;
        }
        if message.contains("malformed tracked checklist item") {
            self.malformed_backlog_blocks += 1;
        }
        if message.contains("response captured but not committed")
            || message.contains("response write applied but not committed")
        {
            self.uncommitted_response_blocks += 1;
        }
        if message.contains("duplicate response patchback") {
            self.duplicate_patchback_blocks += 1;
        }
        if message.contains("fault point") {
            self.fault_fail_closed += 1;
        }
        if message.contains("stale actor generation") {
            self.stale_generation_blocks += 1;
        }
        if message.contains("stale pane observation") {
            self.stale_pane_blocks += 1;
        }
        if message.contains("missing pane observation") {
            self.missing_pane_blocks += 1;
        }
        if message.contains("projection drift") {
            self.projection_drift_blocks += 1;
        }
        if message.contains("supervisor lifecycle Busy cannot accept route dispatch") {
            self.busy_dispatch_blocks += 1;
        }
        if message.contains("supervisor lifecycle Starting cannot accept route dispatch") {
            self.starting_dispatch_blocks += 1;
        }
        if message.contains("supervisor lifecycle Closed cannot accept route dispatch") {
            self.closed_dispatch_blocks += 1;
        }
        if message.contains("session_restart refused") {
            self.session_restart_busy_refusals += 1;
        }
    }

    fn merge(&mut self, other: Coverage) {
        self.unresolved_prompt_blocks += other.unresolved_prompt_blocks;
        self.malformed_backlog_blocks += other.malformed_backlog_blocks;
        self.uncommitted_response_blocks += other.uncommitted_response_blocks;
        self.duplicate_patchback_blocks += other.duplicate_patchback_blocks;
        self.boundary_repairs += other.boundary_repairs;
        self.fault_fail_closed += other.fault_fail_closed;
        self.fault_recoveries += other.fault_recoveries;
        self.fault_noops += other.fault_noops;
        self.fault_points_hit.extend(other.fault_points_hit);
        self.route_generation_rebinds += other.route_generation_rebinds;
        self.supervisor_lifecycle_updates += other.supervisor_lifecycle_updates;
        self.route_dispatch_acceptances += other.route_dispatch_acceptances;
        self.route_dispatch_proofs += other.route_dispatch_proofs;
        self.session_clears += other.session_clears;
        self.session_restart_busy_refusals += other.session_restart_busy_refusals;
        self.session_restart_force_used += other.session_restart_force_used;
        self.session_restart_busy_pre_interrupt_idle +=
            other.session_restart_busy_pre_interrupt_idle;
        self.session_restart_busy_force_killed += other.session_restart_busy_force_killed;
        self.session_restarts += other.session_restarts;
        self.starting_dispatch_blocks += other.starting_dispatch_blocks;
        self.starting_timeout_records += other.starting_timeout_records;
        self.starting_timeout_coalesces += other.starting_timeout_coalesces;
        self.prompt_duplicate_repairs += other.prompt_duplicate_repairs;
        self.normalization_repair_patches += other.normalization_repair_patches;
        self.sidecar_normalization_divergences += other.sidecar_normalization_divergences;
        self.stale_source_buffer_skips += other.stale_source_buffer_skips;
        self.ack_sidecar_only_repairs += other.ack_sidecar_only_repairs;
        self.visible_duplicate_repairs += other.visible_duplicate_repairs;
        self.post_commit_follow_up_handoffs += other.post_commit_follow_up_handoffs;
        self.starting_prompt_promotions += other.starting_prompt_promotions;
        self.busy_dispatch_blocks += other.busy_dispatch_blocks;
        self.closed_dispatch_blocks += other.closed_dispatch_blocks;
        self.busy_interrupt_recoveries += other.busy_interrupt_recoveries;
        self.busy_projection_ready_repairs += other.busy_projection_ready_repairs;
        self.stale_generation_blocks += other.stale_generation_blocks;
        self.stale_pane_blocks += other.stale_pane_blocks;
        self.missing_pane_blocks += other.missing_pane_blocks;
        self.projection_drift_blocks += other.projection_drift_blocks;
        self.projection_repairs += other.projection_repairs;
        self.sync_preserve_layout_blocks += other.sync_preserve_layout_blocks;
        self.sync_detachable_replacements += other.sync_detachable_replacements;
        self.sync_protected_expansions += other.sync_protected_expansions;
        self.sync_focus_handoffs += other.sync_focus_handoffs;
        self.commits += other.commits;
    }
}

#[derive(Debug)]
struct SimWorld {
    seed: u64,
    trace: Vec<SimCommand>,
    doc: String,
    snapshot: String,
    phase: CyclePhase,
    captured_response: Option<String>,
    pending_fault: Option<FaultPoint>,
    route: RouteModel,
    sync: SyncProjection,
    next_prompt: usize,
    coverage: Coverage,
}

impl SimWorld {
    fn new(seed: u64) -> Self {
        let doc = template_doc("");
        Self {
            seed,
            trace: Vec::new(),
            snapshot: doc.clone(),
            doc,
            phase: CyclePhase::Idle,
            captured_response: None,
            pending_fault: None,
            route: RouteModel::new(),
            sync: SyncProjection::default(),
            next_prompt: 1,
            coverage: Coverage::default(),
        }
    }

    fn run_seed(seed: u64, steps: usize) -> Result<Coverage> {
        let mut rng = DeterministicRng::new(seed);
        let mut world = Self::new(seed);
        for _ in 0..steps {
            let command = match rng.next_usize(48) {
                0 => SimCommand::EditPrompt,
                1 => SimCommand::EditLaterPrompt,
                2 => SimCommand::AddMalformedBacklogItem,
                3 => SimCommand::CaptureResponse,
                4 => SimCommand::CaptureFallbackResponse,
                5 => SimCommand::ApplyCapturedResponse,
                6 => SimCommand::Commit,
                7 => SimCommand::FailCommit,
                8 => SimCommand::RepairBoundary,
                9 => SimCommand::DuplicateVisibleResponse,
                10..=18 => {
                    let index = rng.next_usize(FaultPoint::ALL.len());
                    SimCommand::CrashAt(FaultPoint::ALL[index])
                }
                19 => SimCommand::Recover,
                20 => SimCommand::SessionClear,
                21 => SimCommand::SessionRestart,
                22 => SimCommand::SessionRestartForce,
                23 => SimCommand::SessionRestartForcePreInterruptIdle,
                24 => SimCommand::BindRouteOwner,
                25 => SimCommand::SupervisorReady,
                26 => SimCommand::SupervisorBusy,
                27 => SimCommand::SupervisorWaitingInput,
                28 => SimCommand::SupervisorBlocked,
                29 => SimCommand::SupervisorClosed,
                30 => SimCommand::DispatchRoutePrompt,
                31 => SimCommand::ProveDispatchAccepted,
                32 => SimCommand::StaleSupervisorUpdate,
                33 => SimCommand::ObserveStalePane,
                34 => SimCommand::ObserveMissingPane,
                35 => SimCommand::DriftProjection,
                36 => SimCommand::RepairProjection,
                37 => SimCommand::PromoteStartingPromptReady,
                38 => SimCommand::BusyInterruptRecoveryReady,
                39 => SimCommand::SyncProtectedGrowthManual,
                40 => SimCommand::SyncProtectedGrowthPassive,
                41 => SimCommand::SyncProtectedGrowthFocusVisible,
                42 => SimCommand::SyncDetachableReplaceManual,
                43 => SimCommand::SyncDetachableReplacePassive,
                44 => SimCommand::RepairBusyProjectionWithReadyPrompt,
                45 => SimCommand::SyncRerequestVisibleEditorManual,
                46 => SimCommand::SyncRerequestVisibleEditorPassive,
                _ => SimCommand::SyncVisibleFocusPreserve,
            };
            world.apply(command)?;
            world.assert_structural_invariants()?;
        }
        if let Err(err) = world.strict_closeout_invariants() {
            world.coverage.record_block(&err.to_string());
        }
        Ok(world.coverage)
    }

    fn run_seed_corpus(seeds: std::ops::Range<u64>, steps: usize) -> Result<CorpusRun> {
        let started = Instant::now();
        let mut coverage = Coverage::default();
        let mut schedules = 0usize;
        for seed in seeds {
            let seed_coverage = SimWorld::run_seed(seed, steps).unwrap_or_else(|err| {
                panic!("seed {seed} failed structurally: {err}");
            });
            coverage.merge(seed_coverage);
            schedules += 1;
        }
        Ok(CorpusRun {
            coverage,
            schedules,
            steps,
            elapsed: started.elapsed(),
        })
    }

    fn apply(&mut self, command: SimCommand) -> Result<()> {
        self.trace.push(command);
        match command {
            SimCommand::EditPrompt => {
                let prompt = format!(
                    "❯ do #sim{}. spec-test-build-install-commit-push\n",
                    self.next_prompt
                );
                self.next_prompt += 1;
                self.append_to_exchange(&prompt)?;
                if matches!(self.phase, CyclePhase::Idle | CyclePhase::Committed) {
                    self.phase = CyclePhase::PreflightStarted;
                }
            }
            SimCommand::EditLaterPrompt => {
                let prompt = format!("❯ later follow-up #sim{}\n", self.next_prompt);
                self.next_prompt += 1;
                self.append_to_exchange(&prompt)?;
            }
            SimCommand::AddMalformedBacklogItem => {
                self.replace_component_content(
                    "backlog",
                    "_- [ ] [#tigersim] malformed prefix keeps the item parse-hidden\n",
                )?;
            }
            SimCommand::CaptureResponse => {
                self.captured_response = Some(response_patch("sim closeout"));
                self.phase = CyclePhase::ResponseCaptured;
            }
            SimCommand::CaptureFallbackResponse => {
                self.captured_response = Some(fallback_response("sim closeout"));
                self.phase = CyclePhase::ResponseCaptured;
            }
            SimCommand::ApplyCapturedResponse => {
                if let Err(err) = self.apply_captured_response() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::Commit => match self.try_commit() {
                Ok(()) => self.coverage.commits += 1,
                Err(err) => self.coverage.record_block(&err.to_string()),
            },
            SimCommand::FailCommit => {
                if matches!(
                    self.phase,
                    CyclePhase::ResponseCaptured | CyclePhase::WriteApplied
                ) {
                    let message = self.strict_closeout_invariants().unwrap_err().to_string();
                    self.coverage.record_block(&message);
                }
            }
            SimCommand::RepairBoundary => {
                self.doc = crate::template::reposition_boundary_to_end_clean_with_id(
                    &self.doc,
                    Some("sim-boundary"),
                );
                self.coverage.boundary_repairs += 1;
            }
            SimCommand::DuplicateVisibleResponse => {
                let duplicate = "### Re: sim closeout — gpt-5\n\nDuplicate visible response.\n";
                self.append_to_exchange(duplicate)?;
            }
            SimCommand::CrashAt(fault) => {
                self.pending_fault = Some(fault);
                self.coverage.fault_points_hit.insert(fault);
            }
            SimCommand::Recover => {
                if let Err(err) = self.recover_after_fault() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SessionClear => {
                if let Err(err) = self.clear_session_context() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SessionRestart => {
                if let Err(err) = self.restart_supervisor(false, RestartInterruptOutcome::StillBusy)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SessionRestartForce => {
                if let Err(err) = self.restart_supervisor(true, RestartInterruptOutcome::StillBusy)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SessionRestartForcePreInterruptIdle => {
                if let Err(err) =
                    self.restart_supervisor(true, RestartInterruptOutcome::IdleBeforeForceKill)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::BindRouteOwner => {
                self.bind_route_owner();
            }
            SimCommand::SupervisorReady => {
                if let Err(err) = self.transition_supervisor(
                    self.route.durable.generation,
                    SupervisorLifecycle::Ready,
                ) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SupervisorBusy => {
                if let Err(err) = self
                    .transition_supervisor(self.route.durable.generation, SupervisorLifecycle::Busy)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SupervisorWaitingInput => {
                if let Err(err) = self.transition_supervisor(
                    self.route.durable.generation,
                    SupervisorLifecycle::WaitingInput,
                ) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SupervisorBlocked => {
                if let Err(err) = self.transition_supervisor(
                    self.route.durable.generation,
                    SupervisorLifecycle::Blocked,
                ) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SupervisorClosed => {
                if let Err(err) = self.transition_supervisor(
                    self.route.durable.generation,
                    SupervisorLifecycle::Closed,
                ) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::DispatchRoutePrompt => {
                if let Err(err) = self.dispatch_route_prompt() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::ProveDispatchAccepted => {
                if let Err(err) = self.prove_dispatch_accepted() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::StaleSupervisorUpdate => {
                let stale_generation = self.route.durable.generation.saturating_sub(1);
                if let Err(err) =
                    self.transition_supervisor(stale_generation, SupervisorLifecycle::Ready)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::ObserveStalePane => {
                self.route.projection.pane_id = Some("%stale".to_string());
            }
            SimCommand::ObserveMissingPane => {
                self.route.projection.pane_id = None;
            }
            SimCommand::DriftProjection => {
                self.route.projection.generation = self.route.durable.generation + 1;
            }
            SimCommand::RepairProjection => {
                self.route.projection = self.route.durable.clone();
                self.coverage.projection_repairs += 1;
            }
            SimCommand::PromoteStartingPromptReady => {
                if let Err(err) = self.promote_starting_prompt_ready() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::BusyInterruptRecoveryReady => {
                if let Err(err) = self.recover_busy_interrupt_to_ready() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::RepairBusyProjectionWithReadyPrompt => {
                if let Err(err) = self.repair_busy_projection_with_ready_prompt() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SyncProtectedGrowthManual => {
                self.apply_sync_protected_growth(SyncMode::Full);
            }
            SimCommand::SyncProtectedGrowthPassive => {
                self.apply_sync_protected_growth(SyncMode::SafePassive);
            }
            SimCommand::SyncProtectedGrowthFocusVisible => {
                self.apply_sync_protected_growth_focus_visible();
            }
            SimCommand::SyncDetachableReplaceManual => {
                self.apply_sync_detachable_replace(SyncMode::Full);
            }
            SimCommand::SyncDetachableReplacePassive => {
                self.apply_sync_detachable_replace(SyncMode::SafePassive);
            }
            SimCommand::SyncVisibleFocusPreserve => {
                self.apply_sync_visible_focus_preserve(SyncMode::SafePassive);
            }
            SimCommand::SyncRerequestVisibleEditorManual => {
                self.apply_sync_rerequest_visible_editor(SyncMode::Full);
            }
            SimCommand::SyncRerequestVisibleEditorPassive => {
                self.apply_sync_rerequest_visible_editor(SyncMode::SafePassive);
            }
        }
        Ok(())
    }

    fn record_sync_outcome(&mut self, outcome: SyncOutcome) {
        match outcome {
            SyncOutcome::PreservedLayoutAndFocused => {
                self.coverage.sync_preserve_layout_blocks += 1;
                self.coverage.sync_focus_handoffs += 1;
            }
            SyncOutcome::ReplacedDetachable(count) => {
                self.coverage.sync_detachable_replacements += count;
                if self.sync.active.is_some() {
                    self.coverage.sync_focus_handoffs += 1;
                }
            }
            SyncOutcome::AttachedAroundProtected(count) => {
                self.coverage.sync_protected_expansions += count;
                if self.sync.active.is_some() {
                    self.coverage.sync_focus_handoffs += 1;
                }
            }
        }
    }

    fn apply_sync_protected_growth(&mut self, mode: SyncMode) {
        self.sync = SyncProjection::protected_growth_case();
        let outcome =
            self.sync
                .apply_requested_projection(&["requested", "sibling"], "requested", mode);
        self.record_sync_outcome(outcome);
    }

    fn apply_sync_protected_growth_focus_visible(&mut self) {
        self.sync = SyncProjection::protected_growth_case();
        self.sync.active = Some("protected".to_string());
        let outcome = self.sync.apply_requested_projection(
            &["requested", "sibling"],
            "sibling",
            SyncMode::SafePassive,
        );
        self.record_sync_outcome(outcome);
    }

    fn apply_sync_detachable_replace(&mut self, mode: SyncMode) {
        self.sync = SyncProjection::detachable_replacement_case();
        let outcome = self
            .sync
            .apply_requested_projection(&["requested"], "requested", mode);
        self.record_sync_outcome(outcome);
    }

    fn apply_sync_visible_focus_preserve(&mut self, mode: SyncMode) {
        let _ = mode;
        self.sync = SyncProjection::protected_growth_case();
        self.sync.active = Some("sibling".to_string());
        self.record_sync_outcome(SyncOutcome::PreservedLayoutAndFocused);
    }

    fn apply_sync_rerequest_visible_editor(&mut self, mode: SyncMode) {
        self.sync = SyncProjection::rerequested_visible_editor_case();
        // Re-requesting a document that is already visible must be a no-op for
        // pane cardinality: the editor stays a single pane. Attaching a second
        // pane here is the duplicate-editor-pane regression.
        let outcome = self
            .sync
            .apply_requested_projection(&["editor"], "editor", mode);
        self.record_sync_outcome(outcome);
    }

    fn bind_route_owner(&mut self) {
        let generation = self.route.durable.generation + 1;
        self.route.durable = ActorState {
            generation,
            session_id: format!("session-{generation}"),
            pane_id: Some(format!("%{generation}")),
            lifecycle: SupervisorLifecycle::Starting,
        };
        self.route.projection = self.route.durable.clone();
        self.route.pending_dispatch = None;
        self.coverage.route_generation_rebinds += 1;
    }

    fn clear_session_context(&mut self) -> Result<()> {
        self.current_dispatch_pane()?;
        self.route.pending_dispatch = None;
        self.coverage.session_clears += 1;
        Ok(())
    }

    fn restart_supervisor(
        &mut self,
        force: bool,
        interrupt_outcome: RestartInterruptOutcome,
    ) -> Result<()> {
        let pane = self.current_dispatch_pane()?;
        if matches!(self.route.durable.lifecycle, SupervisorLifecycle::Starting) && !force {
            self.coverage.session_restart_busy_refusals += 1;
            bail!(
                "{}; seed={} trace={:?}",
                crate::session_actor_cmd::restart_starting_refusal_message(
                    Path::new("sim.md"),
                    "the document changed after the last committed cycle"
                ),
                self.seed,
                self.trace
            );
        }
        if self.restart_live_pane_is_busy() {
            if !force {
                self.coverage.session_restart_busy_refusals += 1;
                bail!(
                    "{}; seed={} trace={:?}",
                    crate::session_actor_cmd::restart_busy_refusal_message(
                        Path::new("sim.md"),
                        &pane,
                        "authoritative_actor",
                        "agent-doc",
                        Some("• Working (1m 34s · esc to interrupt)"),
                        "Working..."
                    ),
                    self.seed,
                    self.trace
                );
            }

            self.coverage.session_restart_force_used += 1;
            match interrupt_outcome {
                RestartInterruptOutcome::IdleBeforeForceKill => {
                    self.route.durable.lifecycle = SupervisorLifecycle::Ready;
                    self.route.projection.lifecycle = SupervisorLifecycle::Ready;
                    self.coverage.session_restart_busy_pre_interrupt_idle += 1;
                }
                RestartInterruptOutcome::StillBusy => {
                    self.coverage.session_restart_busy_force_killed += 1;
                }
            }
        }

        self.route.pending_dispatch = None;
        self.route.durable.lifecycle = SupervisorLifecycle::Starting;
        self.route.projection = self.route.durable.clone();
        self.coverage.session_restarts += 1;
        Ok(())
    }

    fn restart_live_pane_is_busy(&self) -> bool {
        matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Busy
                | SupervisorLifecycle::WaitingInput
                | SupervisorLifecycle::Blocked
        )
    }

    fn repair_ipc_snapshot_duplicate_prompts(&mut self, before: &str, file: &Path) -> Result<()> {
        let (repaired, changed) = agent_doc_orchestration::write::dedupe_ipc_snapshot_content(
            file,
            Some(before),
            &self.doc,
            "sim_ipc",
        )?;
        if changed {
            self.doc = repaired;
            self.coverage.prompt_duplicate_repairs += 1;
        }
        Ok(())
    }

    fn route_style_duplicate_prompt_cleanup(
        content: &str,
        preserve_docs: &[&str],
    ) -> Result<String> {
        let mut cleaned = content.to_string();
        if let Some(tail_cleaned) =
            crate::template::remove_duplicate_answered_exchange_prompt_tail(&cleaned)
        {
            cleaned = tail_cleaned;
        }
        if let Some(comment_cleaned) =
            crate::template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
                &cleaned,
                preserve_docs,
            )
        {
            cleaned = comment_cleaned;
        }
        crate::template::guard_no_duplicate_prompt_residue_outside_exchange(&cleaned)?;
        Ok(cleaned)
    }

    fn apply_narrow_normalization_repair(&mut self, normalize_prefix_lines: &[String]) {
        let repaired = agent_doc_orchestration::write::normalize_exchange_prefixes_for_targets(
            &self.doc,
            normalize_prefix_lines,
        );
        if repaired != self.doc {
            self.doc = repaired;
            self.coverage.normalization_repair_patches += 1;
        }
    }

    fn stale_full_content_visible_replacement(
        &mut self,
        source: FullContentReplacementSource,
    ) -> agent_doc_orchestration::flow::document_mutation::FullContentVisibleReplacementDecision
    {
        let proof =
            agent_doc_orchestration::flow::document_mutation::FullContentSourceProof::from_content(
                &self.doc,
            );
        let replacement = template_doc(&format!(
            "### Re: replacement from {} — gpt-5\n\nDone.\n",
            source.as_str()
        ));
        self.append_to_exchange("❯ live prompt typed before full-content apply\n")
            .expect("template doc should keep an exchange component");
        let decision = agent_doc_orchestration::flow::document_mutation::decide_full_content_visible_replacement(
            &self.doc,
            Some(&proof),
        );
        if decision == agent_doc_orchestration::flow::document_mutation::FullContentVisibleReplacementDecision::Apply
        {
            self.doc = replacement;
        } else if decision
            == agent_doc_orchestration::flow::document_mutation::FullContentVisibleReplacementDecision::RejectStaleSourceBuffer
        {
            self.coverage.stale_source_buffer_skips += 1;
        }
        decision
    }

    fn apply_sidecar_normalization_fallback(
        &mut self,
        sidecar: &str,
        content_ours: &str,
        normalize_prefix_lines: &[String],
    ) {
        if !agent_doc_orchestration::write::verify_sidecar_normalization(
            sidecar,
            normalize_prefix_lines,
        ) {
            let fallback = agent_doc_orchestration::write::normalize_exchange_prefixes_for_targets(
                content_ours,
                normalize_prefix_lines,
            );
            self.snapshot = fallback;
            self.coverage.sidecar_normalization_divergences += 1;
        }
    }

    fn adopt_ipc_snapshot_candidate(
        &mut self,
        baseline: &str,
        content_ours: &str,
        snapshot_candidate: &str,
    ) {
        self.doc = snapshot_candidate.to_string();
        if agent_doc_orchestration::write::ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
            baseline,
            snapshot_candidate,
            content_ours,
        ) {
            self.snapshot = content_ours.to_string();
            self.coverage.ipc_snapshot_live_prompt_blocks += 1;
        } else {
            self.snapshot = snapshot_candidate.to_string();
        }
    }

    fn apply_ack_sidecar_only_repair(&mut self, ack_content: &str) {
        self.snapshot = ack_content.to_string();
        self.coverage.ack_sidecar_only_repairs += 1;
    }

    fn repair_visible_duplicate_response(&mut self) {
        let repaired = agent_doc_orchestration::dedupe::dedupe_responses(&self.doc);
        if repaired != self.doc {
            self.doc = repaired;
            self.coverage.visible_duplicate_repairs += 1;
        }
    }

    fn record_post_commit_follow_up_handoff(&mut self) -> Result<()> {
        if self.phase != CyclePhase::Committed {
            bail!(
                "post-commit follow-up handoff requires committed phase; seed={} trace={:?}",
                self.seed,
                self.trace
            );
        }
        let Some(diff_text) =
            agent_doc_orchestration::diff::unified_diff_from_contents(&self.snapshot, &self.doc)
        else {
            return Ok(());
        };
        let has_follow_up =
            agent_doc_orchestration::diff::classify_prompt_bearing_changes(&diff_text)
                .into_iter()
                .any(|change| {
                    matches!(
                        change.kind,
                        agent_doc_orchestration::diff::PromptBearingChangeKind::PromptTarget
                    )
                });
        if has_follow_up {
            self.coverage.post_commit_follow_up_handoffs += 1;
        }
        Ok(())
    }

    fn transition_supervisor(
        &mut self,
        generation: u64,
        lifecycle: SupervisorLifecycle,
    ) -> Result<()> {
        if generation != self.route.durable.generation {
            bail!(
                "stale actor generation rejected: observed={} current={}; seed={} trace={:?}",
                generation,
                self.route.durable.generation,
                self.seed,
                self.trace
            );
        }
        let projection_was_current = self.projection_identity_matches_durable();
        self.route.durable.lifecycle = lifecycle;
        if projection_was_current {
            self.route.projection.lifecycle = lifecycle;
        }
        if matches!(
            lifecycle,
            SupervisorLifecycle::Ready | SupervisorLifecycle::Closed | SupervisorLifecycle::Blocked
        ) {
            self.route.starting_timeout = None;
        }
        self.coverage.supervisor_lifecycle_updates += 1;
        Ok(())
    }

    fn dispatch_route_prompt(&mut self) -> Result<()> {
        let pane_id = self.current_dispatch_pane()?;
        if self.route.durable.lifecycle == SupervisorLifecycle::Starting {
            let current = (self.route.durable.generation, pane_id.clone());
            if self.route.starting_timeout.as_ref() == Some(&current) {
                self.coverage.starting_timeout_coalesces += 1;
            } else {
                self.route.starting_timeout = Some(current);
                self.coverage.starting_timeout_records += 1;
            }
        }
        if self.route.durable.lifecycle != SupervisorLifecycle::Ready {
            bail!(
                "supervisor lifecycle {:?} cannot accept route dispatch; seed={} trace={:?}",
                self.route.durable.lifecycle,
                self.seed,
                self.trace
            );
        }
        self.route.pending_dispatch = Some(DispatchReceipt {
            generation: self.route.durable.generation,
            session_id: self.route.durable.session_id.clone(),
            pane_id,
            proved: false,
        });
        self.coverage.route_dispatch_acceptances += 1;
        Ok(())
    }

    fn promote_starting_prompt_ready(&mut self) -> Result<()> {
        self.current_dispatch_pane()?;
        if self.route.durable.lifecycle != SupervisorLifecycle::Starting {
            bail!(
                "starting prompt promotion requires Starting lifecycle; found {:?}; seed={} trace={:?}",
                self.route.durable.lifecycle,
                self.seed,
                self.trace
            );
        }
        self.transition_supervisor(self.route.durable.generation, SupervisorLifecycle::Ready)?;
        self.coverage.starting_prompt_promotions += 1;
        Ok(())
    }

    fn recover_busy_interrupt_to_ready(&mut self) -> Result<()> {
        self.current_dispatch_pane()?;
        match self.route.durable.lifecycle {
            SupervisorLifecycle::WaitingInput | SupervisorLifecycle::Blocked => {
                self.transition_supervisor(
                    self.route.durable.generation,
                    SupervisorLifecycle::Ready,
                )?;
                self.coverage.busy_interrupt_recoveries += 1;
                Ok(())
            }
            lifecycle => bail!(
                "busy interrupt recovery requires WaitingInput or Blocked lifecycle; found {:?}; seed={} trace={:?}",
                lifecycle,
                self.seed,
                self.trace
            ),
        }
    }

    /// `#snrun` / `#run-agent-doc-stale-busy-replay`: a dispatch-only reroute
    /// finds the authoritative actor projected `Busy`, but the live pane proves a
    /// dispatch-ready prompt on the current generation — the busy/lease projection
    /// is stale. Direct idle evidence repairs it: promote `Busy` -> `Ready` so the
    /// next dispatch goes to the proven-ready pane instead of queuing into
    /// `agent:queue auto`. Gated by the production predicate
    /// `busy_projection_repaired_by_ready_prompt` (idle evidence repairs; a busy
    /// projection WITHOUT a proven ready prompt stays fail-closed).
    fn repair_busy_projection_with_ready_prompt(&mut self) -> Result<()> {
        self.current_dispatch_pane()?;
        if self.route.durable.lifecycle != SupervisorLifecycle::Busy {
            bail!(
                "stale busy projection repair requires Busy lifecycle; found {:?}; seed={} trace={:?}",
                self.route.durable.lifecycle,
                self.seed,
                self.trace
            );
        }
        // The pane proves a dispatch-ready prompt (prompt_ready=true); defer the
        // promote-vs-fail-closed decision to the production predicate.
        let repaired =
            agent_doc_orchestration::flow::routed_reopen::busy_projection_repaired_by_ready_prompt(
                agent_doc_orchestration::flow::routed_reopen::ActorDispatchState::Busy,
                true,
            );
        if !repaired {
            bail!(
                "production predicate refused stale busy projection repair; seed={} trace={:?}",
                self.seed,
                self.trace
            );
        }
        self.transition_supervisor(self.route.durable.generation, SupervisorLifecycle::Ready)?;
        self.coverage.busy_projection_ready_repairs += 1;
        Ok(())
    }

    fn prove_dispatch_accepted(&mut self) -> Result<()> {
        let Some(receipt) = self.route.pending_dispatch.as_mut() else {
            return Ok(());
        };
        if receipt.generation != self.route.durable.generation
            || receipt.session_id != self.route.durable.session_id
            || Some(&receipt.pane_id) != self.route.durable.pane_id.as_ref()
        {
            bail!(
                "stale pane observation blocks dispatch proof: receipt={:?} durable={:?}; seed={} trace={:?}",
                receipt,
                self.route.durable,
                self.seed,
                self.trace
            );
        }
        receipt.proved = true;
        self.coverage.route_dispatch_proofs += 1;
        Ok(())
    }

    fn current_dispatch_pane(&self) -> Result<String> {
        match (
            self.route.durable.pane_id.as_ref(),
            self.route.projection.pane_id.as_ref(),
        ) {
            (Some(durable), Some(projected))
                if self.route.projection.generation == self.route.durable.generation
                    && self.route.projection.session_id == self.route.durable.session_id
                    && durable != projected =>
            {
                bail!(
                    "stale pane observation blocks route dispatch: durable={} projected={}; seed={} trace={:?}",
                    durable,
                    projected,
                    self.seed,
                    self.trace
                )
            }
            (Some(_), None)
                if self.route.projection.generation == self.route.durable.generation
                    && self.route.projection.session_id == self.route.durable.session_id =>
            {
                bail!(
                    "missing pane observation blocks route dispatch: durable={:?} projected={:?}; seed={} trace={:?}",
                    self.route.durable,
                    self.route.projection,
                    self.seed,
                    self.trace
                )
            }
            (None, _) => {
                bail!(
                    "missing pane observation blocks route dispatch: durable={:?}; seed={} trace={:?}",
                    self.route.durable,
                    self.seed,
                    self.trace
                )
            }
            _ => {}
        }

        if !self.projection_identity_matches_durable() {
            bail!(
                "projection drift between durable actor state and projection: durable={:?} projection={:?}; seed={} trace={:?}",
                self.route.durable,
                self.route.projection,
                self.seed,
                self.trace
            );
        }
        self.route
            .durable
            .pane_id
            .clone()
            .ok_or_else(|| anyhow!("missing pane observation blocks route dispatch"))
    }

    fn projection_identity_matches_durable(&self) -> bool {
        self.route.projection.generation == self.route.durable.generation
            && self.route.projection.session_id == self.route.durable.session_id
            && self.route.projection.pane_id == self.route.durable.pane_id
    }

    fn apply_captured_response(&mut self) -> Result<()> {
        if self.take_fault(FaultPoint::TemplateMerge) {
            self.phase = CyclePhase::Interrupted(FaultPoint::TemplateMerge);
            bail!(
                "fault point {:?} interrupted template merge; seed={} trace={:?}",
                FaultPoint::TemplateMerge,
                self.seed,
                self.trace
            );
        }
        let response = self.captured_response.clone().unwrap_or_default();
        if response.is_empty() {
            return Ok(());
        }
        let (patches, unmatched) = crate::template::parse_patches(&response)?;
        let next_doc =
            crate::template::apply_patches(&self.doc, &patches, &unmatched, Path::new("sim.md"))?;
        if patches.is_empty() && self.take_fault(FaultPoint::FallbackPatchWrite) {
            self.phase = CyclePhase::Interrupted(FaultPoint::FallbackPatchWrite);
            bail!(
                "fault point {:?} interrupted fallback patch write; seed={} trace={:?}",
                FaultPoint::FallbackPatchWrite,
                self.seed,
                self.trace
            );
        }
        if self.take_fault(FaultPoint::WorkingTreeWrite) {
            self.phase = CyclePhase::Interrupted(FaultPoint::WorkingTreeWrite);
            bail!(
                "fault point {:?} interrupted working-tree write; seed={} trace={:?}",
                FaultPoint::WorkingTreeWrite,
                self.seed,
                self.trace
            );
        }
        self.doc = next_doc;
        self.phase = CyclePhase::WriteApplied;
        Ok(())
    }

    fn try_commit(&mut self) -> Result<()> {
        self.pre_commit_invariants()?;
        if self.take_fault(FaultPoint::IndexUpdate) {
            self.phase = CyclePhase::Interrupted(FaultPoint::IndexUpdate);
            bail!(
                "fault point {:?} interrupted index update; seed={} trace={:?}",
                FaultPoint::IndexUpdate,
                self.seed,
                self.trace
            );
        }
        if self.take_fault(FaultPoint::GitCommit) {
            self.phase = CyclePhase::Interrupted(FaultPoint::GitCommit);
            bail!(
                "fault point {:?} interrupted git commit; seed={} trace={:?}",
                FaultPoint::GitCommit,
                self.seed,
                self.trace
            );
        }
        self.doc =
            crate::template::reposition_boundary_to_end_clean_with_id(&self.doc, Some("committed"));
        if self.take_fault(FaultPoint::PostCommitBoundaryReposition) {
            self.phase = CyclePhase::Interrupted(FaultPoint::PostCommitBoundaryReposition);
            bail!(
                "fault point {:?} interrupted post-commit boundary reposition; seed={} trace={:?}",
                FaultPoint::PostCommitBoundaryReposition,
                self.seed,
                self.trace
            );
        }
        if self.take_fault(FaultPoint::SnapshotSave) {
            self.phase = CyclePhase::Interrupted(FaultPoint::SnapshotSave);
            bail!(
                "fault point {:?} interrupted snapshot save; seed={} trace={:?}",
                FaultPoint::SnapshotSave,
                self.seed,
                self.trace
            );
        }
        self.snapshot = self.doc.clone();
        if self.take_fault(FaultPoint::SessionCheck) {
            self.phase = CyclePhase::Interrupted(FaultPoint::SessionCheck);
            bail!(
                "fault point {:?} interrupted session-check; seed={} trace={:?}",
                FaultPoint::SessionCheck,
                self.seed,
                self.trace
            );
        }
        if self.take_fault(FaultPoint::IpcDelivery) {
            self.coverage.fault_noops += 1;
        }
        self.phase = CyclePhase::Committed;
        Ok(())
    }

    fn strict_closeout_invariants(&self) -> Result<()> {
        self.closeout_invariants(true)
    }

    fn pre_commit_invariants(&self) -> Result<()> {
        self.closeout_invariants(false)
    }

    fn closeout_invariants(&self, require_committed: bool) -> Result<()> {
        if let CyclePhase::Interrupted(fault) = self.phase {
            bail!(
                "fault point {:?} left closeout interrupted; seed={} trace={:?}",
                fault,
                self.seed,
                self.trace
            );
        }

        match self.phase {
            CyclePhase::ResponseCaptured => {
                bail!(
                    "response captured but not committed; seed={} trace={:?}",
                    self.seed,
                    self.trace
                )
            }
            CyclePhase::WriteApplied => {
                if self.has_duplicate_response_heading() {
                    bail!(
                        "duplicate response patchback before commit; seed={} trace={:?}",
                        self.seed,
                        self.trace
                    );
                }
            }
            _ => {}
        }

        let components = crate::component::parse(&self.doc)?;
        let malformed = components
            .iter()
            .filter(|component| crate::component::is_tracked_work_component(&component.name))
            .flat_map(|component| {
                agent_doc_orchestration::pending::detect_malformed_item_lines(
                    component.content(&self.doc),
                )
            })
            .collect::<Vec<_>>();
        if !malformed.is_empty() {
            let refs = malformed
                .iter()
                .map(|item| item.reference())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("malformed tracked checklist item(s): {refs}");
        }

        if let Some(diff_text) =
            agent_doc_orchestration::diff::unified_diff_from_contents(&self.snapshot, &self.doc)
        {
            let prompt_targets =
                agent_doc_orchestration::diff::classify_prompt_bearing_changes(&diff_text)
                    .into_iter()
                    .filter(|change| {
                        matches!(
                            change.kind,
                            agent_doc_orchestration::diff::PromptBearingChangeKind::PromptTarget
                        )
                    })
                    .map(|change| change.text)
                    .collect::<Vec<_>>();
            if !prompt_targets.is_empty() {
                bail!(
                    "unresolved prompt_target(s) after closeout: {}; seed={} trace={:?}",
                    prompt_targets.join(" | "),
                    self.seed,
                    self.trace
                );
            }
        }

        if require_committed && matches!(self.phase, CyclePhase::WriteApplied) {
            bail!(
                "response write applied but not committed; seed={} trace={:?}",
                self.seed,
                self.trace
            );
        }
        Ok(())
    }

    fn recover_after_fault(&mut self) -> Result<()> {
        let fault = match self.phase {
            CyclePhase::Interrupted(fault) => fault,
            _ => {
                if self.pending_fault.take().is_some() {
                    self.coverage.fault_noops += 1;
                }
                return Ok(());
            }
        };

        match fault {
            FaultPoint::TemplateMerge
            | FaultPoint::FallbackPatchWrite
            | FaultPoint::WorkingTreeWrite => {
                self.phase = CyclePhase::ResponseCaptured;
                self.apply_captured_response()?;
                self.try_commit()?;
            }
            FaultPoint::IndexUpdate
            | FaultPoint::GitCommit
            | FaultPoint::PostCommitBoundaryReposition => {
                self.phase = CyclePhase::WriteApplied;
                self.try_commit()?;
            }
            FaultPoint::SnapshotSave => {
                self.snapshot = self.doc.clone();
                self.phase = CyclePhase::Committed;
            }
            FaultPoint::SessionCheck => {
                self.phase = CyclePhase::Committed;
            }
            FaultPoint::IpcDelivery => {
                self.phase = CyclePhase::Committed;
                self.coverage.fault_noops += 1;
            }
        }
        self.coverage.fault_recoveries += 1;
        Ok(())
    }

    fn take_fault(&mut self, fault: FaultPoint) -> bool {
        if self.pending_fault == Some(fault) {
            self.pending_fault = None;
            true
        } else {
            false
        }
    }

    fn assert_structural_invariants(&self) -> Result<()> {
        let components = crate::component::parse(&self.doc)?;
        let exchange = components
            .iter()
            .find(|component| component.name == "exchange")
            .ok_or_else(|| anyhow!("sim document lost exchange component"))?;
        let exchange_body = exchange.content(&self.doc);
        let boundary_count = exchange_body.matches("<!-- agent:boundary:").count();
        if boundary_count > 1 {
            bail!(
                "exchange has multiple boundary markers: count={boundary_count}; seed={} trace={:?}",
                self.seed,
                self.trace
            );
        }
        // No-duplicate-editor-pane cardinality invariant: a sync projection must
        // never present the same document under two visible panes. This is the
        // in-memory guard for the "3 tmux panes with 2 editor panes" regression
        // class. Ownership of this invariant lives in the sync claim-tracking +
        // tmux-router column dedup, NOT in any lazily-rs cache.
        let mut seen_visible = BTreeSet::new();
        for pane in &self.sync.visible {
            if !seen_visible.insert(pane.clone()) {
                bail!(
                    "sync projection presents a duplicate visible pane `{pane}` for the same document (duplicate editor pane regression): visible={:?}; seed={} trace={:?}",
                    self.sync.visible,
                    self.seed,
                    self.trace
                );
            }
        }
        Ok(())
    }

    fn has_duplicate_response_heading(&self) -> bool {
        self.doc.matches("### Re: sim closeout").count() > 1
    }

    fn append_to_exchange(&mut self, text: &str) -> Result<()> {
        let body = self.component_content("exchange")?.to_string();
        let boundary = body.find("<!-- agent:boundary:");
        let next = if let Some(pos) = boundary {
            format!("{}{}{}", &body[..pos], text, &body[pos..])
        } else {
            format!("{body}{text}")
        };
        self.replace_component_content("exchange", &next)
    }

    fn component_content(&self, name: &str) -> Result<&str> {
        crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == name)
            .map(|component| component.content(&self.doc))
            .ok_or_else(|| anyhow!("missing component `{name}`"))
    }

    fn replace_component_content(&mut self, name: &str, content: &str) -> Result<()> {
        let component = crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == name)
            .ok_or_else(|| anyhow!("missing component `{name}`"))?;
        self.doc = component.replace_content(&self.doc, content);
        Ok(())
    }

    fn insert_after_exchange(&mut self, content: &str) -> Result<()> {
        let exchange = crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == "exchange")
            .ok_or_else(|| anyhow!("missing component `exchange`"))?;
        self.doc.insert_str(exchange.close_end, content);
        Ok(())
    }
}

#[derive(Debug)]
struct DeterministicRng(u64);

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_usize(&mut self, modulo: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.0 >> 32) as usize) % modulo
    }
}

fn template_doc(exchange_body: &str) -> String {
    format!(
        "---\nagent_doc_session: sim\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
         ## Exchange\n\n\
         <!-- agent:exchange patch=append -->\n\
         {exchange_body}<!-- agent:boundary:initial -->\n\
         <!-- /agent:exchange -->\n\n\
         ## Pending / Not Built\n\n\
         <!-- agent:backlog -->\n\
         - [ ] [#tigersim] Implement the simulator MVP\n\
         <!-- /agent:backlog -->\n\n\
         <!-- agent:icebox -->\n\
         <!-- /agent:icebox -->\n"
    )
}

fn response_patch(topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->\n"
    )
}

fn fallback_response(topic: &str) -> String {
    format!("### Re: {topic} — gpt-5\n\nImplemented and verified through fallback.\n")
}

fn post_exchange_scratch_comment(prompt: &str) -> String {
    format!(
        "\n###\n\n<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nUse route, preflight, write, IPC, repair, and compact no-delete coverage.\n-->\n"
    )
}

fn assert_owned_scratch_comment_preserved(doc: &str, prompt: &str) {
    assert!(
        doc.contains(&post_exchange_scratch_comment(prompt)),
        "owned post-exchange scratch comment must survive:\n{doc}"
    );
}

fn setup_baseline_drift_capture(
    seed: u64,
    response: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    agent_doc_orchestration::capture::CaptureRecord,
    SimWorld,
) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
    let doc = dir.path().join("doc.md");
    let mut world = SimWorld::new(seed);
    world.apply(SimCommand::EditPrompt).unwrap();
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_orchestration::snapshot::save(&doc, &world.doc).unwrap();
    let capture = agent_doc_orchestration::capture::capture_response(&doc, response).unwrap();
    (dir, doc, capture, world)
}

fn apply_response_and_save_current(doc: &Path, world: &mut SimWorld, response: &str) -> Result<()> {
    world.captured_response = Some(response.to_string());
    world.apply_captured_response()?;
    world.apply(SimCommand::Commit)?;
    std::fs::write(doc, &world.doc)?;
    agent_doc_orchestration::snapshot::save(doc, &world.doc)?;
    Ok(())
}

#[derive(Debug)]
struct CorpusRun {
    coverage: Coverage,
    schedules: usize,
    steps: usize,
    elapsed: Duration,
}

impl CorpusRun {
    fn command_count(&self) -> usize {
        self.schedules * self.steps
    }

    fn assert_within_budget(&self, budget: Duration, label: &str) {
        eprintln!(
            "{label}: schedules={} steps={} commands={} elapsed_ms={} budget_ms={}",
            self.schedules,
            self.steps,
            self.command_count(),
            self.elapsed.as_millis(),
            budget.as_millis()
        );
        assert!(
            self.elapsed <= budget,
            "{label} exceeded runtime budget: elapsed={:?} budget={:?}",
            self.elapsed,
            budget
        );
    }
}

#[test]
fn closeout_sim_fixed_seed_corpus_exercises_recent_failure_classes() {
    let run = SimWorld::run_seed_corpus(FAST_CORPUS_SEEDS, FAST_CORPUS_STEPS).unwrap();
    run.assert_within_budget(FAST_CORPUS_BUDGET, "fast simulator corpus");
    let coverage = run.coverage;

    assert!(
        coverage.commits > 0,
        "seed corpus must include valid committed closeouts"
    );
    assert!(
        coverage.boundary_repairs > 0,
        "seed corpus must exercise deterministic boundary repair"
    );
    assert!(
        coverage.unresolved_prompt_blocks > 0,
        "seed corpus must exercise unresolved prompt closeout blocks"
    );
    assert!(
        coverage.malformed_backlog_blocks > 0,
        "seed corpus must exercise malformed backlog closeout blocks"
    );
    assert!(
        coverage.uncommitted_response_blocks > 0,
        "seed corpus must exercise captured/write-applied uncommitted closeout blocks"
    );
    assert!(
        coverage.duplicate_patchback_blocks > 0,
        "seed corpus must exercise duplicate visible response blocks"
    );
    assert!(
        coverage.fault_fail_closed > 0,
        "seed corpus must exercise fault-triggered fail-closed interruptions"
    );
    assert!(
        coverage.fault_recoveries > 0,
        "seed corpus must exercise deterministic fault recovery"
    );
    assert!(
        coverage.fault_noops > 0,
        "seed corpus must exercise invariant-preserving fault no-ops"
    );
    assert_eq!(
        coverage.fault_points_hit.len(),
        FaultPoint::ALL.len(),
        "seed corpus must inject every named closeout fault point"
    );
    assert!(
        coverage.route_generation_rebinds > 0,
        "seed corpus must exercise route owner generation changes"
    );
    assert!(
        coverage.supervisor_lifecycle_updates > 0,
        "seed corpus must exercise supervisor lifecycle facts"
    );
    assert!(
        coverage.route_dispatch_acceptances > 0,
        "seed corpus must exercise accepted route dispatch"
    );
    assert!(
        coverage.route_dispatch_proofs > 0,
        "seed corpus must exercise dispatch proof"
    );
    assert!(
        coverage.busy_dispatch_blocks > 0,
        "seed corpus must block busy/bootstrap route dispatch before current ready proof"
    );
    assert!(
        coverage.stale_generation_blocks > 0,
        "seed corpus must reject stale actor generations"
    );
    assert!(
        coverage.stale_pane_blocks > 0,
        "seed corpus must block stale pane observations"
    );
    assert!(
        coverage.missing_pane_blocks > 0,
        "seed corpus must block missing pane observations"
    );
    assert!(
        coverage.projection_drift_blocks > 0,
        "seed corpus must diagnose projection drift"
    );
    assert!(
        coverage.projection_repairs > 0,
        "seed corpus must repair projection drift from durable actor state"
    );
    assert!(
        coverage.sync_protected_expansions > 0,
        "seed corpus must exercise protected-closeout sync expansion cases"
    );
    assert!(
        coverage.sync_detachable_replacements > 0,
        "seed corpus must exercise detachable-pane sync replacement cases"
    );
    assert!(
        coverage.sync_focus_handoffs > 0,
        "seed corpus must exercise sync focus handoffs"
    );
}

#[test]
#[ignore = "run by `make check` as the medium deterministic simulator corpus"]
fn closeout_sim_medium_seed_corpus_runs_wider_deterministic_budget() {
    let run = SimWorld::run_seed_corpus(MEDIUM_CORPUS_SEEDS, MEDIUM_CORPUS_STEPS).unwrap();
    run.assert_within_budget(MEDIUM_CORPUS_BUDGET, "medium simulator corpus");
    let coverage = run.coverage;

    assert!(
        coverage.commits >= 10,
        "medium seed corpus should include many valid committed closeouts"
    );
    assert!(
        coverage.fault_points_hit.len() == FaultPoint::ALL.len(),
        "medium seed corpus must keep every named closeout fault point covered"
    );
    assert!(
        coverage.route_dispatch_acceptances > 0 && coverage.route_dispatch_proofs > 0,
        "medium seed corpus must keep route dispatch/proof coverage"
    );
    assert!(
        coverage.projection_drift_blocks > 0 && coverage.projection_repairs > 0,
        "medium seed corpus must keep projection drift and repair coverage"
    );
    assert!(
        coverage.sync_protected_expansions > 0 && coverage.sync_detachable_replacements > 0,
        "medium seed corpus must keep sync expansion/replacement coverage"
    );
}

#[test]
fn post_exchange_comment_ownership_sim_covers_cleanup_and_handoff_paths() {
    let prompt = "The post-exchange scratch ownership prompt should not be deleted by duplicate cleanup. #spec-test-build-install-commit-push";
    let file = Path::new("sim.md");
    let mut world = SimWorld::new(9_901);
    world.append_to_exchange(&format!("❯ {prompt}\n")).unwrap();
    world
        .insert_after_exchange(&post_exchange_scratch_comment(prompt))
        .unwrap();
    world.snapshot = world.doc.clone();

    let route_cleaned =
        SimWorld::route_style_duplicate_prompt_cleanup(&world.doc, &[world.doc.as_str()]).unwrap();
    assert_eq!(
        route_cleaned, world.doc,
        "route-style cleanup must treat the visible document as comment ownership proof"
    );

    let preflight_cleaned = SimWorld::route_style_duplicate_prompt_cleanup(
        &world.doc,
        &[world.doc.as_str(), world.snapshot.as_str()],
    )
    .unwrap();
    assert_eq!(
        preflight_cleaned, world.doc,
        "preflight-style recovery must not delete visible scratch comments"
    );

    let direct_write =
        agent_doc_orchestration::write::normalize_template_structure_or_fail_preserving(
            &world.doc,
            file,
            Some(&world.snapshot),
        )
        .unwrap();
    assert_owned_scratch_comment_preserved(&direct_write, prompt);

    let (ipc_handoff, changed) = agent_doc_orchestration::write::dedupe_ipc_snapshot_content(
        file,
        Some(&world.snapshot),
        &direct_write,
        "sim_ipc",
    )
    .unwrap();
    assert!(
        !changed,
        "IPC/plugin handoff must not rewrite owned scratch comments"
    );
    assert_owned_scratch_comment_preserved(&ipc_handoff, prompt);

    let mut repair_world = world;
    repair_world.captured_response = Some(response_patch("comment ownership"));
    repair_world.apply_captured_response().unwrap();
    let repaired_write =
        agent_doc_orchestration::write::normalize_template_structure_or_fail_preserving(
            &repair_world.doc,
            file,
            Some(&repair_world.snapshot),
        )
        .unwrap();
    assert_owned_scratch_comment_preserved(&repaired_write, prompt);

    let compacted_exchange = "### Session Summary\n\nCompacted content archived.\n";
    repair_world
        .replace_component_content("exchange", compacted_exchange)
        .unwrap();
    assert_owned_scratch_comment_preserved(&repair_world.doc, prompt);

    let generated = {
        let mut generated_world = SimWorld::new(9_902);
        generated_world
            .append_to_exchange(&format!("❯ {prompt}\n"))
            .unwrap();
        let before = generated_world.doc.clone();
        generated_world
            .insert_after_exchange(&post_exchange_scratch_comment(prompt))
            .unwrap();
        (before, generated_world.doc)
    };
    let (scrubbed, changed) = agent_doc_orchestration::write::dedupe_ipc_snapshot_content(
        file,
        Some(&generated.0),
        &generated.1,
        "sim_generated",
    )
    .unwrap();
    assert!(
        changed,
        "generated duplicate comment residue without ownership proof must still be scrubbed"
    );
    assert!(
        !scrubbed.contains(&format!("<!--\n{prompt}")),
        "generated duplicate prompt text should be removed from post-exchange comment residue:\n{scrubbed}"
    );
    assert!(
        scrubbed
            .contains("Use route, preflight, write, IPC, repair, and compact no-delete coverage."),
        "unrelated scratch lines in generated mixed comments must remain:\n{scrubbed}"
    );
}

#[test]
fn closeout_sim_blocks_later_prompt_after_response_write() {
    let mut world = SimWorld::new(42);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::EditLaterPrompt).unwrap();

    let err = world.try_commit().unwrap_err();
    assert!(
        err.to_string().contains("unresolved prompt_target"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_blocks_malformed_tracked_backlog_line() {
    let mut world = SimWorld::new(7);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::AddMalformedBacklogItem).unwrap();

    let err = world.try_commit().unwrap_err();
    assert!(
        err.to_string().contains("malformed tracked checklist item"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_requires_captured_response_to_cross_commit_boundary() {
    let mut world = SimWorld::new(11);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();

    let err = world.strict_closeout_invariants().unwrap_err();
    assert!(
        err.to_string()
            .contains("response captured but not committed"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_collapses_boundary_drift_to_single_marker() {
    let mut world = SimWorld::new(99);
    world
        .append_to_exchange("<!-- agent:boundary:stale-one -->\n")
        .unwrap();
    world
        .append_to_exchange("<!-- agent:boundary:stale-two -->\n")
        .unwrap();

    world.apply(SimCommand::RepairBoundary).unwrap();
    world.assert_structural_invariants().unwrap();
    assert!(world.doc.contains("<!-- agent:boundary:sim-boundary -->"));
}

#[test]
fn closeout_sim_fault_points_fail_closed_then_recover() {
    for (index, fault) in FaultPoint::ALL.into_iter().enumerate() {
        let mut world = SimWorld::new(1_000 + index as u64);
        match fault {
            FaultPoint::TemplateMerge | FaultPoint::WorkingTreeWrite => {
                world.apply(SimCommand::CaptureResponse).unwrap();
                world.apply(SimCommand::CrashAt(fault)).unwrap();
                world.apply(SimCommand::ApplyCapturedResponse).unwrap();
            }
            FaultPoint::FallbackPatchWrite => {
                world.apply(SimCommand::CaptureFallbackResponse).unwrap();
                world.apply(SimCommand::CrashAt(fault)).unwrap();
                world.apply(SimCommand::ApplyCapturedResponse).unwrap();
            }
            _ => {
                world.apply(SimCommand::CaptureResponse).unwrap();
                world.apply(SimCommand::ApplyCapturedResponse).unwrap();
                world.apply(SimCommand::CrashAt(fault)).unwrap();
                world.apply(SimCommand::Commit).unwrap();
            }
        }

        if fault == FaultPoint::IpcDelivery {
            assert_eq!(world.phase, CyclePhase::Committed);
            assert!(
                world.coverage.fault_noops > 0,
                "IPC delivery faults should be invariant-preserving no-ops"
            );
        } else {
            assert!(
                matches!(world.phase, CyclePhase::Interrupted(observed) if observed == fault),
                "fault {fault:?} should fail closed before recovery; phase={:?}",
                world.phase
            );
            assert!(
                world.coverage.fault_fail_closed > 0,
                "fault {fault:?} should be recorded as a fail-closed interruption"
            );
            world.apply(SimCommand::Recover).unwrap();
            assert_eq!(
                world.phase,
                CyclePhase::Committed,
                "fault {fault:?} should recover to a committed closeout"
            );
            assert_eq!(world.snapshot, world.doc);
        }
    }
}

#[test]
fn route_sim_accepts_dispatch_only_after_current_supervisor_ready() {
    let mut world = SimWorld::new(2_001);
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_promotes_starting_prompt_ready_before_dispatch() {
    let mut world = SimWorld::new(2_004);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::PromoteStartingPromptReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.starting_prompt_promotions, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_repairs_stale_busy_projection_with_ready_prompt_then_dispatches() {
    // #run-agent-doc-stale-busy-replay / #snrun: a dispatch-only reroute that
    // finds the actor projected Busy but the live pane proving a dispatch-ready
    // prompt must repair the stale projection and DISPATCH, not enqueue into
    // agent:queue auto. Replays the stale-busy projection end-to-end through the
    // SimWorld actor model (the pure predicate is unit-tested separately).
    let mut world = SimWorld::new(2_026);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    // Stale busy projection: the actor reports Busy, so a dispatch fails closed
    // (queues) rather than dispatching.
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.busy_dispatch_blocks, 1,
        "a busy projection without a proven ready prompt must fail closed (queue)"
    );
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    // The live pane proves a dispatch-ready prompt on the current generation:
    // direct idle evidence repairs the stale busy projection to Ready.
    world
        .apply(SimCommand::RepairBusyProjectionWithReadyPrompt)
        .unwrap();
    assert_eq!(world.coverage.busy_projection_ready_repairs, 1);

    // After repair the reroute dispatches to the proven-ready pane instead of
    // enqueuing.
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "after the stale busy projection is repaired the prompt must dispatch, not enqueue"
    );
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_stale_busy_repair_requires_busy_lifecycle() {
    // The repair is fail-closed: it only promotes a genuinely Busy projection.
    // A Ready actor is not a stale-busy case, so the repair must be rejected
    // (recorded as a block) and leave dispatch counts untouched.
    let mut world = SimWorld::new(2_027);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world
        .apply(SimCommand::RepairBusyProjectionWithReadyPrompt)
        .unwrap();
    assert_eq!(
        world.coverage.busy_projection_ready_repairs, 0,
        "repair must not fire when the actor is not projected Busy"
    );
}

#[test]
fn route_sim_blocks_starting_to_busy_bootstrap_until_current_ready_prompt() {
    let mut world = SimWorld::new(2_006);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.busy_dispatch_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn restart_supervisor_sim_refuses_alive_busy_without_force_then_force_kills() {
    let mut world = SimWorld::new(2_016);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();

    let err = world
        .restart_supervisor(false, RestartInterruptOutcome::StillBusy)
        .expect_err("busy restart without --force must fail closed");
    assert!(err.to_string().contains("session_restart refused"));
    assert!(err.to_string().contains("pane %2 is alive-busy"));
    assert!(err.to_string().contains("pass `--force`"));
    assert_eq!(world.coverage.session_restart_busy_refusals, 1);
    assert_eq!(world.coverage.session_restarts, 0);

    world
        .restart_supervisor(true, RestartInterruptOutcome::StillBusy)
        .unwrap();

    assert_eq!(world.coverage.session_restart_force_used, 1);
    assert_eq!(world.coverage.session_restart_busy_force_killed, 1);
    assert_eq!(world.coverage.session_restart_busy_pre_interrupt_idle, 0);
    assert_eq!(world.coverage.session_restarts, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.starting_dispatch_blocks, 1,
        "route must remain prompt-gated while the restarted supervisor is booting"
    );
}

#[test]
fn restart_supervisor_sim_records_pre_interrupt_idle_before_restart() {
    let mut world = SimWorld::new(2_017);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();

    world
        .restart_supervisor(true, RestartInterruptOutcome::IdleBeforeForceKill)
        .unwrap();

    assert_eq!(world.coverage.session_restart_force_used, 1);
    assert_eq!(world.coverage.session_restart_busy_pre_interrupt_idle, 1);
    assert_eq!(world.coverage.session_restart_busy_force_killed, 0);
    assert_eq!(world.coverage.session_restarts, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);
}

#[test]
fn restart_supervisor_sim_force_allows_starting_owner_restart() {
    let mut world = SimWorld::new(2_018);
    world.apply(SimCommand::BindRouteOwner).unwrap();

    let err = world
        .restart_supervisor(false, RestartInterruptOutcome::StillBusy)
        .expect_err("starting restart without --force must fail closed");
    assert!(err.to_string().contains("session_restart refused"));
    assert!(
        err.to_string()
            .contains("the authoritative actor is still starting")
    );
    assert!(
        err.to_string()
            .contains("the document changed after the last committed cycle")
    );
    assert!(err.to_string().contains("Pass `--force`"));
    assert_eq!(world.coverage.session_restart_busy_refusals, 1);
    assert_eq!(world.coverage.session_restarts, 0);

    world
        .restart_supervisor(true, RestartInterruptOutcome::StillBusy)
        .unwrap();

    assert_eq!(world.coverage.session_restarts, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Starting);
}

#[test]
fn route_sim_coalesces_repeated_starting_timeouts_for_same_generation() {
    let mut world = SimWorld::new(2_009);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();

    assert_eq!(world.coverage.starting_timeout_records, 1);
    assert_eq!(world.coverage.starting_timeout_coalesces, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::PromoteStartingPromptReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_blocks_starting_actor_that_closes_before_ready_prompt() {
    let mut world = SimWorld::new(2_007);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorClosed).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();

    assert_eq!(world.coverage.closed_dispatch_blocks, 1);
    assert_eq!(world.coverage.starting_prompt_promotions, 0);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);
    assert_eq!(world.coverage.route_dispatch_proofs, 0);
}

#[test]
fn jetbrains_clear_session_sim_keeps_starting_route_guard_and_repairs_prompt_duplicate() {
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(temp.path().join(".agent-doc/logs")).unwrap();
    let doc_path = temp.path().join("jetbrains-clear-session.md");

    let mut world = SimWorld::new(2_008);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::EditPrompt).unwrap();
    let before_ipc = world.doc.clone();

    world.apply(SimCommand::SessionClear).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();

    assert_eq!(world.coverage.session_clears, 1);
    assert_eq!(
        world.coverage.starting_dispatch_blocks, 1,
        "Run Agent Doc after clear must stay prompt-gated while the actor is still starting"
    );
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world
        .append_to_exchange("do #sim1. spec-test-build-install-commit-push\n")
        .unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world
        .repair_ipc_snapshot_duplicate_prompts(&before_ipc, &doc_path)
        .unwrap();

    assert_eq!(world.coverage.prompt_duplicate_repairs, 1);
    assert_eq!(
        world
            .component_content("exchange")
            .unwrap()
            .matches("do #sim1. spec-test-build-install-commit-push")
            .count(),
        1,
        "IPC snapshot repair should keep the normalized live prompt once"
    );

    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
}

#[test]
fn full_content_source_proof_sim_rejects_stale_editor_buffers() {
    for source in FullContentReplacementSource::ALL {
        let mut world = SimWorld::new(2_010);
        let original = world.doc.clone();

        let decision = world.stale_full_content_visible_replacement(source);

        assert_eq!(
            decision,
            agent_doc_orchestration::flow::document_mutation::FullContentVisibleReplacementDecision::RejectStaleSourceBuffer,
            "{source:?} must reject full-content replacement when the editor buffer drifted"
        );
        assert!(
            world
                .doc
                .contains("❯ live prompt typed before full-content apply"),
            "{source:?} must preserve live editor text on stale proof rejection"
        );
        assert_ne!(
            world.doc, original,
            "{source:?} should model live prompt drift before the full-content apply attempt"
        );
        assert_eq!(
            world.coverage.stale_source_buffer_skips, 1,
            "{source:?} should count the stale full-content replacement skip"
        );
        assert!(
            !world
                .doc
                .contains(&format!("replacement from {}", source.as_str())),
            "{source:?} must not apply stale compact/repair/timeout replacement content"
        );
    }
}

#[test]
fn ipc_snapshot_adoption_sim_blocks_live_prompt_drift_after_preflight() {
    let mut world = SimWorld::new(2_021);
    world.append_to_exchange("❯ Please reply\n").unwrap();
    let baseline = world.doc.clone();
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: Please reply — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
    );
    let snapshot_candidate = baseline.replace(
        "<!-- /agent:exchange -->",
        "❯ New prompt typed during closeout\n### Re: Please reply — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
    );

    world.adopt_ipc_snapshot_candidate(&baseline, &content_ours, &snapshot_candidate);

    assert_eq!(world.coverage.ipc_snapshot_live_prompt_blocks, 1);
    assert_eq!(
        world.snapshot, content_ours,
        "IPC snapshot adoption must keep the committed snapshot to agent-owned response content"
    );
    assert!(
        world.doc.contains("❯ New prompt typed during closeout"),
        "the live prompt remains visible for the next response cycle"
    );
    assert!(
        !world.snapshot.contains("New prompt typed during closeout"),
        "the live prompt must not be absorbed into the snapshot"
    );
    assert_eq!(
        world
            .doc
            .matches("❯ New prompt typed during closeout")
            .count(),
        1,
        "the live prompt must remain exactly once"
    );
    assert_eq!(
        world.doc.matches("### Re: Please reply — gpt-5").count(),
        1,
        "the response heading must not be duplicated"
    );
}

#[test]
fn sidecar_normalization_divergence_sim_uses_normalized_content_ours() {
    let mut world = SimWorld::new(2_015);
    let sidecar = template_doc(
        "do #sidecardiv. spec-test-build-install-commit-push\n### Re: #sidecardiv — gpt-5\n\nDone.\n",
    );
    let content_ours = sidecar.clone();
    let normalize_prefix_lines =
        vec!["do #sidecardiv. spec-test-build-install-commit-push".to_string()];

    world.apply_sidecar_normalization_fallback(&sidecar, &content_ours, &normalize_prefix_lines);

    assert_eq!(world.coverage.sidecar_normalization_divergences, 1);
    assert!(
        world
            .snapshot
            .contains("❯ do #sidecardiv. spec-test-build-install-commit-push"),
        "rejected sidecar snapshot should fall back to normalized content_ours"
    );
    assert_eq!(
        world
            .snapshot
            .matches("### Re: #sidecardiv — gpt-5")
            .count(),
        1,
        "normalization fallback must not duplicate the assistant response"
    );
}

#[test]
fn ack_sidecar_only_repair_sim_uses_authoritative_sidecar_snapshot() {
    let mut world = SimWorld::new(2_012);
    world.doc = template_doc(
        "❯ do #acksidecar. spec-test-build-install-commit-push\n<!-- agent:boundary:live -->\n",
    );
    world.snapshot = template_doc("<!-- agent:boundary:base -->\n");
    let ack_content = template_doc(
        "❯ do #acksidecar. spec-test-build-install-commit-push\n### Re: #acksidecar — gpt-5\n\nDone.\n<!-- agent:boundary:ack -->\n",
    );

    world.apply_ack_sidecar_only_repair(&ack_content);

    assert_eq!(world.coverage.ack_sidecar_only_repairs, 1);
    assert_eq!(
        world.snapshot, ack_content,
        "ack-content sidecar should be the committed snapshot proof even when the local visible file still lags"
    );
    assert!(
        !world.doc.contains("### Re: #acksidecar — gpt-5"),
        "the sim must keep the sidecar-only distinction from ordinary disk repair"
    );
}

#[test]
fn visible_duplicate_repair_sim_dedupes_real_response_block() {
    let mut world = SimWorld::new(2_013);
    world.doc = template_doc(
        "❯ do #dupvis. spec-test-build-install-commit-push\n### Re: #dupvis — gpt-5\n\nDone.\n### Re: #dupvis — gpt-5\n\nDone.\n<!-- agent:boundary:dup -->\n",
    );

    world.repair_visible_duplicate_response();

    assert_eq!(world.coverage.visible_duplicate_repairs, 1);
    assert_eq!(
        world.doc.matches("### Re: #dupvis — gpt-5").count(),
        1,
        "visible duplicate repair should remove the real repeated response block"
    );
    world.assert_structural_invariants().unwrap();
}

#[test]
fn normalization_repair_sim_uses_narrow_patch_for_prefix_only_divergence() {
    let mut world = SimWorld::new(2_011);
    world.doc = template_doc(
        "do #simnorm. spec-test-build-install-commit-push\n### Re: #simnorm — gpt-5\n\nDone.\n",
    );

    world.apply_narrow_normalization_repair(&[
        "do #simnorm. spec-test-build-install-commit-push".to_string()
    ]);

    assert_eq!(world.coverage.normalization_repair_patches, 1);
    assert!(
        world
            .doc
            .contains("❯ do #simnorm. spec-test-build-install-commit-push"),
        "narrow normalization repair should prefix the live prompt"
    );
    assert_eq!(
        world.doc.matches("### Re: #simnorm — gpt-5").count(),
        1,
        "narrow repair must not duplicate the assistant response"
    );
}

#[test]
fn post_commit_follow_up_sim_keeps_prompt_out_of_snapshot() {
    let mut world = SimWorld::new(2_014);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    let committed_snapshot = world.snapshot.clone();

    world.apply(SimCommand::EditLaterPrompt).unwrap();
    world.record_post_commit_follow_up_handoff().unwrap();

    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.coverage.post_commit_follow_up_handoffs, 1);
    assert_eq!(
        world.snapshot, committed_snapshot,
        "terminal follow-up drift should not be absorbed into the committed snapshot"
    );
    assert!(
        world.doc.contains("later follow-up"),
        "the user follow-up stays visible for the next response cycle"
    );
}

#[test]
fn route_sim_recovers_busy_interrupt_to_ready_once() {
    let mut world = SimWorld::new(2_005);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorWaitingInput).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::BusyInterruptRecoveryReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();

    assert_eq!(world.coverage.busy_interrupt_recoveries, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn route_sim_rejects_stale_generation_and_pane_observations() {
    let mut world = SimWorld::new(2_002);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::StaleSupervisorUpdate).unwrap();
    assert_eq!(world.coverage.stale_generation_blocks, 1);

    world.apply(SimCommand::ObserveStalePane).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.stale_pane_blocks, 1);

    world.apply(SimCommand::RepairProjection).unwrap();
    world.apply(SimCommand::ObserveMissingPane).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.missing_pane_blocks, 1);
}

#[test]
fn route_sim_repairs_projection_drift_from_durable_actor_state() {
    let mut world = SimWorld::new(2_003);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DriftProjection).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.projection_drift_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::RepairProjection).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(world.coverage.projection_repairs, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);
}

#[test]
fn sync_sim_tmuxbudget_seed_3001_attaches_requested_pane_around_protected_cycle() {
    let mut world = SimWorld::new(3_001);
    world.apply(SimCommand::SyncProtectedGrowthManual).unwrap();

    assert_eq!(
        world.sync.visible,
        vec![
            "protected".to_string(),
            "sibling".to_string(),
            "requested".to_string()
        ],
        "manual sync should attach the requested pane around a protected closeout owner instead of deferring"
    );
    assert_eq!(world.coverage.sync_protected_expansions, 1);
    assert_eq!(world.coverage.sync_focus_handoffs, 1);

    world.apply(SimCommand::SyncProtectedGrowthPassive).unwrap();
    assert_eq!(
        world.sync.visible,
        vec![
            "protected".to_string(),
            "sibling".to_string(),
            "requested".to_string()
        ],
        "safe-passive sync should share the same protected-closeout attach decision"
    );
    assert_eq!(world.coverage.sync_protected_expansions, 2);
    assert_eq!(world.coverage.sync_focus_handoffs, 2);
}

#[test]
fn sync_sim_tmuxbudget_seed_3002_replaces_detachable_pane_while_protected_cycle_remains() {
    let mut world = SimWorld::new(3_002);
    world
        .apply(SimCommand::SyncDetachableReplaceManual)
        .unwrap();

    assert_eq!(
        world.sync.visible,
        vec!["protected".to_string(), "requested".to_string()],
        "manual sync should displace an unprotected unwanted pane instead of treating any protected visible pane as a no-op"
    );
    assert_eq!(world.sync.active.as_deref(), Some("requested"));
    assert_eq!(world.coverage.sync_detachable_replacements, 1);

    world
        .apply(SimCommand::SyncDetachableReplacePassive)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["protected".to_string(), "requested".to_string()],
        "safe-passive sync should share the same detachable-pane replacement decision"
    );
    assert_eq!(world.coverage.sync_detachable_replacements, 2);
    assert_eq!(world.coverage.sync_focus_handoffs, 2);
}

#[test]
fn sync_sim_tmuxbudget_seed_3003_preserve_layout_still_reselects_visible_focus() {
    let mut world = SimWorld::new(3_003);
    world.apply(SimCommand::SyncVisibleFocusPreserve).unwrap();

    assert_eq!(
        world.sync.visible,
        vec!["protected".to_string(), "sibling".to_string()],
        "preserve-layout sync should not mutate the visible pane set"
    );
    assert_eq!(world.sync.active.as_deref(), Some("sibling"));
    assert_eq!(world.coverage.sync_preserve_layout_blocks, 1);
    assert_eq!(world.coverage.sync_focus_handoffs, 1);
}

#[test]
fn sync_sim_tmuxbudget_seed_3005_rejects_duplicate_editor_pane_for_rerequested_document() {
    // Regression guard for "3 tmux panes with 2 editor panes": when the editor
    // document is already visible and sync re-requests the same document (the
    // duplicate-claim / pane-id churn surface), the projection must keep a single
    // editor pane instead of attaching a second one.
    let mut world = SimWorld::new(3_005);
    world
        .apply(SimCommand::SyncRerequestVisibleEditorManual)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["editor".to_string()],
        "manual sync must not attach a second editor pane when the document is already visible"
    );
    assert_eq!(world.sync.active.as_deref(), Some("editor"));
    // The structural cardinality invariant must hold: no duplicate visible pane.
    world.assert_structural_invariants().unwrap();

    world
        .apply(SimCommand::SyncRerequestVisibleEditorPassive)
        .unwrap();
    assert_eq!(
        world.sync.visible,
        vec!["editor".to_string()],
        "safe-passive sync must share the same single-editor-pane decision"
    );
    world.assert_structural_invariants().unwrap();
}

#[test]
fn sync_sim_tmuxbudget_seed_3004_attaches_hidden_requested_pane_and_focuses_visible_sibling() {
    let mut world = SimWorld::new(3_004);
    world
        .apply(SimCommand::SyncProtectedGrowthFocusVisible)
        .unwrap();

    assert_eq!(
        world.sync.visible,
        vec![
            "protected".to_string(),
            "sibling".to_string(),
            "requested".to_string()
        ],
        "safe-passive sync should attach the hidden requested pane without detaching the protected closeout owner"
    );
    assert_eq!(
        world.sync.active.as_deref(),
        Some("sibling"),
        "safe-passive sync should still focus an already-visible requested sibling"
    );
    assert_eq!(world.coverage.sync_protected_expansions, 1);
    assert_eq!(world.coverage.sync_focus_handoffs, 1);
}

#[test]
fn finalize_with_typing_in_post_exchange_comment_and_already_applied_ack_does_not_duplicate_response()
 {
    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 1 (deterministic SimWorld repro) + Phase 5 (regression coverage).
    //
    // Models the finalize-time IPC corruption + duplicate-response race:
    //   1. Document baseline = exchange with a prompt + an HTML scratch comment
    //      below `</agent:exchange>` that the user is actively typing into.
    //   2. Agent runs finalize and the plugin had already applied the response
    //      patch to its live buffer (e.g. via a prior socket retry whose ack
    //      write was slow).
    //   3. The plugin's retry ack is the protocol's dedupe signal:
    //      `{"type":"ack","status":"error","reason":"already_applied"}`.
    //   4. The binary recognizes that signal through
    //      `ipc_socket::is_already_applied_error` and skips the file-IPC
    //      fallback so it does not re-apply the same response on top of the
    //      live buffer (which would land a duplicate `### Re:` heading and
    //      collide with the user's in-flight typing inside the scratch
    //      comment).
    let prompt = "do #ipcdup. spec-test-build-install-commit-push";
    let file = Path::new("sim-ipc-already-applied.md");
    let mut world = SimWorld::new(2_020);
    world.append_to_exchange(&format!("❯ {prompt}\n")).unwrap();
    world
        .insert_after_exchange(&post_exchange_scratch_comment(prompt))
        .unwrap();
    world.snapshot = world.doc.clone();

    let response_block = "\n### Re: closeout — gpt-5\n\nImplemented.\n";
    world
        .append_to_exchange(response_block)
        .expect("plugin already inserted the response patch into the live buffer");
    let live_after_plugin_apply = world.doc.clone();

    let already_applied_ack = r#"{"type":"ack","status":"error","reason":"already_applied"}"#;
    assert_eq!(
        agent_doc_orchestration::ipc_socket::classify_ack(already_applied_ack),
        agent_doc_orchestration::ipc_socket::AckClassification::AlreadyApplied,
        "protocol contract: status=error + reason=already_applied is the dedupe signal"
    );
    let send_err = anyhow!("IPC ack already_applied: {}", already_applied_ack);
    assert!(
        agent_doc_orchestration::ipc_socket::is_already_applied_error(&send_err),
        "send_message wraps already_applied acks in an error the write path can recognize"
    );

    assert_eq!(
        world.doc.matches("### Re: closeout — gpt-5").count(),
        1,
        "with the already_applied gate, the file-IPC fallback must not re-apply the patch and double the response heading"
    );
    assert_owned_scratch_comment_preserved(&world.doc, prompt);

    let mut counterfactual = world.doc.clone();
    counterfactual.push_str(response_block);
    assert_eq!(
        counterfactual.matches("### Re: closeout — gpt-5").count(),
        2,
        "without the already_applied gate the file-IPC fallback would land a duplicate response heading"
    );
    let (deduped, changed) = agent_doc_orchestration::write::dedupe_ipc_snapshot_content(
        file,
        Some(&live_after_plugin_apply),
        &counterfactual,
        "sim_ipc_already_applied_counterfactual",
    )
    .unwrap();
    assert!(
        changed,
        "dedupe recovery must collapse the counterfactual duplicate response"
    );
    assert_eq!(
        deduped.matches("### Re: closeout — gpt-5").count(),
        1,
        "dedupe recovery must collapse the duplicated response heading down to one"
    );
    assert_owned_scratch_comment_preserved(&deduped, prompt);
}

#[test]
fn cycle_1779845677327_scratch_directives_survive_already_applied_ipc_race() {
    let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first that fail and fix the implementation.";
    let scratch_directive = "#spec-test-build-install-commit-push";
    let scratch_dispatch = "dispatch #spec-test-build-install-commit-push";
    let scratch_comment =
        format!("\n###\n\n<!--\n{prompt}\n{scratch_directive}\n---\n{scratch_dispatch}\n-->\n");
    let file = Path::new("cycle-1779845677327.md");
    let mut world = SimWorld::new(1_779_845_677_327);
    world
        .append_to_exchange(&format!("❯ do [#liveipcrace]\n❯ {scratch_directive}\n"))
        .unwrap();
    world.insert_after_exchange(&scratch_comment).unwrap();
    world.snapshot = world.doc.clone();

    let response_block = "\n### Re: cycle 1779845677327 IPC race — gpt-5\n\nImplemented.\n";
    world
        .append_to_exchange(response_block)
        .expect("plugin already inserted the response patch into the live buffer");
    let live_after_plugin_apply = world.doc.clone();

    let already_applied_ack = r#"{"type":"ack","status":"error","reason":"already_applied"}"#;
    assert_eq!(
        agent_doc_orchestration::ipc_socket::classify_ack(already_applied_ack),
        agent_doc_orchestration::ipc_socket::AckClassification::AlreadyApplied,
        "editor plugins must use already_applied so the binary skips file IPC fallback"
    );

    assert_eq!(
        world
            .doc
            .matches("### Re: cycle 1779845677327 IPC race — gpt-5")
            .count(),
        1,
        "already_applied must leave one response heading"
    );
    assert_eq!(
        world.doc.matches(prompt).count(),
        1,
        "scratch prompt text should remain visible exactly once"
    );
    assert!(
        world.doc.contains(&scratch_comment),
        "scratch prompt preset and dispatch directives must remain inside the ordinary comment:\n{}",
        world.doc
    );

    let mut counterfactual = world.doc.clone();
    counterfactual.push_str(response_block);
    let (deduped, changed) = agent_doc_orchestration::write::dedupe_ipc_snapshot_content(
        file,
        Some(&live_after_plugin_apply),
        &counterfactual,
        "cycle_1779845677327_counterfactual",
    )
    .unwrap();
    assert!(
        changed,
        "dedupe recovery must collapse the counterfactual duplicate response"
    );
    assert_eq!(
        deduped
            .matches("### Re: cycle 1779845677327 IPC race — gpt-5")
            .count(),
        1,
        "dedupe recovery must collapse the duplicated response heading down to one"
    );
    assert!(
        deduped.contains(&scratch_comment),
        "dedupe recovery must not discard prompt preset/directive text in the scratch comment:\n{deduped}"
    );
}

#[test]
fn ipc_snapshot_guard_blocks_live_queue_drift_after_preflight() {
    let mut world = SimWorld::new(2_022);
    world
        .insert_after_exchange("\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n")
        .unwrap();
    let baseline = world.doc.clone();
    let response = response_patch("live queue IPC race");
    let (patches, unmatched) = crate::template::parse_patches(&response).unwrap();
    let content_ours =
        crate::template::apply_patches(&baseline, &patches, &unmatched, Path::new("sim.md"))
            .unwrap();
    let live_queue_prompt = "- do #liveipcrace. #spec-test-build-install-commit-push";
    let ack_candidate = content_ours.replace(
        "<!-- agent:queue -->\n<!-- /agent:queue -->",
        &format!("<!-- agent:queue -->\n{live_queue_prompt}\n<!-- /agent:queue -->"),
    );

    assert!(
        agent_doc_orchestration::write::ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
            &baseline,
            &ack_candidate,
            &content_ours,
        ),
        "IPC snapshot adoption must classify live queue edits typed after preflight as next-cycle drift"
    );

    world.doc = ack_candidate;
    world.snapshot = content_ours;
    assert!(
        world.doc.contains(live_queue_prompt),
        "live queue prompt should remain visible in the working tree"
    );
    assert!(
        !world.snapshot.contains(live_queue_prompt),
        "snapshot should stay on content_ours so the queue prompt remains a future diff"
    );
}

// -------- #queue-strike-on-halt: a halt/refusal response must not strike the
// active auto-queue head; only an explicit closeout flag advances it. The Codex
// Stop-hook heading path is exact-match only. See
// tasks/agent-doc/plan-queue-strike-on-halt-response.md.

#[test]
fn halt_response_does_not_strike_queue_head_but_done_flag_does() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#alpha]\n",
        "- do [#beta]\n",
        "<!-- /agent:queue -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    agent_doc_orchestration::snapshot::save(&doc, content).unwrap();

    // A halt response that names the head with a trailing modifier must NOT
    // register as targeting the head (exact-topic match only).
    let halt = "### Re: do [#alpha] halt — opus-4-8\n\nBacklog left intact; not executing.\n";
    assert!(
        !agent_doc_orchestration::write::response_explicitly_targets_active_queue_head(&doc, halt)
            .unwrap(),
        "halt heading must not target the queue head"
    );
    // An exact-topic heading still registers, preserving the Codex auto-loop on a
    // clean completion that titles the response with the head prompt verbatim.
    let exact = "### Re: do [#alpha] — opus-4-8\n\nDone.\n";
    assert!(
        agent_doc_orchestration::write::response_explicitly_targets_active_queue_head(&doc, exact)
            .unwrap(),
        "exact-topic heading should still target the queue head"
    );

    // An explicit --done strikes the head, leaving #beta as the next head.
    let outcome = agent_doc_orchestration::write::consume_queue_prompts_for_done_ids_with_outcome(
        &doc,
        &["alpha".to_string()],
    )
    .unwrap()
    .expect("explicit --done should consume the queue head");
    assert_eq!(outcome.consumed_count, 1);
    assert_eq!(outcome.remaining, 1);
    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(after.contains("- ~~do [#alpha]~~"), "alpha struck:\n{after}");
    assert!(
        after.contains("- do [#beta]"),
        "beta remains the head:\n{after}"
    );
}

// -------- #adoc-bdauc-simworld: deterministic SimWorld coverage for baseline
// drift after a manual user commit. See
// tasks/agent-doc/plan-baseline-drift-after-user-commit.md.

#[test]
fn baseline_drift_benign_user_commit_outside_response_auto_refreshes() {
    let response = response_patch("baseline drift");
    let (dir, doc, capture, mut world) = setup_baseline_drift_capture(2026_05_25 + 10, &response);
    apply_response_and_save_current(&doc, &mut world, &response).unwrap();

    world
        .replace_component_content(
            "backlog",
            "- [ ] [#tigersim] Implement the simulator MVP\n- [ ] [#manual] User-added follow-up outside the captured response\n",
        )
        .unwrap();
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_orchestration::snapshot::save(&doc, &world.doc).unwrap();

    agent_doc_orchestration::capture::validate_replay(&doc, &capture)
        .expect("benign user commit outside response must auto-refresh");

    let refreshed = agent_doc_orchestration::capture::load_active(&doc)
        .unwrap()
        .unwrap();
    assert_eq!(
        refreshed.file_hash.as_deref(),
        Some(agent_doc_orchestration::ops_log::content_hash(&world.doc).as_str()),
        "file hash should refresh to the user-committed document"
    );
    assert_eq!(
        refreshed.snapshot_hash.as_deref(),
        Some(agent_doc_orchestration::ops_log::content_hash(&world.doc).as_str()),
        "snapshot hash should refresh to the user-committed baseline"
    );
    let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("capture_baseline_refreshed_for_benign_drift"),
        "benign drift refresh should be auditable:\n{ops_log}"
    );
}

#[test]
fn baseline_drift_user_edit_inside_committed_response_fails_closed() {
    let response = response_patch("baseline drift");
    let (_dir, doc, capture, mut world) = setup_baseline_drift_capture(2026_05_25 + 11, &response);
    apply_response_and_save_current(&doc, &mut world, &response).unwrap();

    world.doc = world.doc.replace(
        "Implemented and verified.",
        "User rewrote the committed response.",
    );
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_orchestration::snapshot::save(&doc, &world.doc).unwrap();

    let err = agent_doc_orchestration::capture::validate_replay(&doc, &capture)
        .expect_err("editing the committed response body must fail closed");
    assert!(
        err.to_string().contains("baseline no longer matches")
            || err.to_string().contains("snapshot no longer matches"),
        "expected fail-closed baseline drift error; got: {err}"
    );
}

#[test]
fn baseline_drift_user_edit_matches_normalized_response_adopts() {
    let response = "<!-- patch:exchange -->\n### Re: baseline drift normalized — gpt-5\n\nImplemented and verified.\n❯ Submodule pointer updated.\n<!-- /patch:exchange -->\n";
    let (_dir, doc, capture, mut world) = setup_baseline_drift_capture(2026_05_25 + 12, response);
    apply_response_and_save_current(&doc, &mut world, response).unwrap();

    world.doc = world
        .doc
        .replace("❯ Submodule pointer updated.", "Submodule pointer updated.");
    std::fs::write(&doc, &world.doc).unwrap();
    agent_doc_orchestration::snapshot::save(&doc, &world.doc).unwrap();

    agent_doc_orchestration::capture::validate_replay(&doc, &capture)
        .expect("user-normalized response body should be adopted");

    let refreshed = agent_doc_orchestration::capture::load_active(&doc)
        .unwrap()
        .unwrap();
    assert_eq!(
        refreshed.file_hash.as_deref(),
        Some(agent_doc_orchestration::ops_log::content_hash(&world.doc).as_str()),
        "file hash should reflect the normalized user-cleaned response"
    );
    assert_eq!(
        refreshed.snapshot_hash.as_deref(),
        Some(agent_doc_orchestration::ops_log::content_hash(&world.doc).as_str()),
        "snapshot hash should reflect the normalized user-cleaned response"
    );
}

// -------- #jbccc1: deterministic SimWorld scenario for the JB File Cache Conflict
// cancel wedge. See tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md.
//
// The scenario models three branches of the JetBrains "File Cache Conflict"
// dialog that surfaces during IPC patch delivery while the document is open
// in IntelliJ:
//
// 1. **No dialog** — happy path; IPC apply commits cleanly.
// 2. **Accept** — user accepts the conflict resolution; IPC apply still commits
//    cleanly. From the binary's perspective indistinguishable from (1).
// 3. **Cancel** — user cancels; the response is already written to the working
//    tree by the time the dialog appears, but the binary's IPC callback returns
//    failure. The cycle wedges at `WriteApplied`: response in doc, snapshot ≠
//    HEAD-equivalent, no commit landed.
//
// The cancel branch is the failing baseline that Phase 3 (#jbccc3) will recover
// from inside `preflight` / `repair` by auto-detecting the
// `cycle_phase=WriteApplied + working-tree-response-matches-captured-body +
// snapshot != HEAD` signature and running the equivalent of `write --commit`.

/// Apply the captured response to the document the way a successful JB cache
/// conflict acceptance would: VFS writes the patch into the working tree and
/// the binary receives the ack. From the binary's perspective this is
/// indistinguishable from `SimCommand::ApplyCapturedResponse`.
fn apply_jb_cache_conflict_accept(world: &mut SimWorld) {
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
}

/// Apply the captured response to the document the way a JB cache conflict
/// **cancel** does: the VFS write has already happened (the plugin queued it
/// before showing the dialog), but the binary's IPC callback returns failure
/// and the cycle never reaches `Committed`. The world stops at `WriteApplied`
/// with the response visible in `self.doc` and the snapshot still pointing at
/// the pre-response state.
///
/// This is the root-cause hypothesis 1/2 from
/// `tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md`.
fn apply_jb_cache_conflict_cancel(world: &mut SimWorld) {
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    // No SimCommand::Commit — the cancel branch is exactly the absence of the
    // commit half. Phase 3 (#jbccc3) will close this in the binary.
}

#[test]
fn jb_cache_conflict_no_dialog_commits_cleanly() {
    let mut world = SimWorld::new(2026_05_25);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

#[test]
fn jb_cache_conflict_accept_branch_commits_cleanly() {
    let mut world = SimWorld::new(2026_05_25 + 1);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_accept(&mut world);
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

#[test]
fn jb_cache_conflict_cancel_branch_wedges_at_write_applied_today() {
    // FAILING BASELINE for #jbccc1.
    //
    // Today the cancel branch leaves the cycle at WriteApplied with the
    // response visible in the document but snapshot ≠ doc. This test pins
    // that behavior so that #jbccc3 (binary auto-recovery) has a clear
    // before/after assertion to flip.
    //
    // When #jbccc3 lands, this test should be replaced (or its assertion
    // inverted) by `jb_cache_conflict_cancel_branch_auto_recovers_via_preflight`
    // that drives a recovery and asserts phase == Committed without a manual
    // `write --commit`.
    let mut world = SimWorld::new(2026_05_25 + 2);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_cancel(&mut world);

    assert_eq!(
        world.phase,
        CyclePhase::WriteApplied,
        "cancel branch must wedge at WriteApplied today"
    );
    assert_ne!(
        world.snapshot, world.doc,
        "cancel branch must leave snapshot behind the visible response today"
    );
    assert!(
        world.doc.contains("### Re: sim closeout"),
        "cancel branch must leave the response visible in the working-tree doc today"
    );

    let err = world
        .strict_closeout_invariants()
        .expect_err("cancel-branch wedge must trip strict closeout today");
    assert!(
        err.to_string()
            .contains("response write applied but not committed"),
        "expected wedge to surface as 'response write applied but not committed'; got: {err}"
    );
}

#[test]
fn jb_cache_conflict_cancel_branch_recovers_via_explicit_write_commit_today() {
    // Today's documented recovery path (per the #adoc-jb-cache-cancel-stuck-cycle
    // backlog item) is: run `agent-doc write --commit <FILE>` manually after the
    // cancel-induced wedge. In the simulator, that's a follow-up `Commit` from
    // the WriteApplied phase. This test pins the recovery exit so #jbccc3 can
    // automate it inside preflight without regressing the manual escape hatch.
    let mut world = SimWorld::new(2026_05_25 + 3);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_cancel(&mut world);
    assert_eq!(world.phase, CyclePhase::WriteApplied);

    // Manual recovery — equivalent to `agent-doc write --commit <FILE>`.
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

/// Phase 5 regression (#jbccc5): after the Phase 3 (#jbccc3) binary fix lands,
/// the next `agent-doc preflight` automatically runs `git::commit` for the
/// cancel-induced wedge — no manual `agent-doc write --commit` required. The
/// simulator mirrors that auto-recovery as a follow-up `Commit` driven by the
/// preflight contract rather than the operator. This test pins the
/// post-Phase-3 invariant: from the WriteApplied wedge, the binary-owned
/// recovery alone is sufficient to reach `Committed` with `strict_closeout_invariants`
/// clean.
#[test]
fn jb_cache_conflict_cancel_branch_auto_recovers_via_preflight() {
    let mut world = SimWorld::new(2026_05_25 + 4);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_cancel(&mut world);
    assert_eq!(world.phase, CyclePhase::WriteApplied);
    assert_ne!(world.snapshot, world.doc);

    // Auto-recovery — driven by the next `agent-doc preflight`, not by the
    // operator. In real code this is preflight detecting the cancel pattern
    // (cycle phase WriteApplied + snapshot ≠ HEAD + working tree matches
    // snapshot modulo transient markers) and dispatching `git::commit`.
    world.apply(SimCommand::Commit).unwrap();

    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}

// -------- #jbccacceptdup: deterministic SimWorld scenario for the JB File
// Cache Conflict **late-accept replay** wedge.
//
// See tasks/agent-doc/plan-jb-cache-conflict-accept-duplicates-response.md and
// the repro logged on tasks/agent-doc/agent-doc-bugs2.md [#wkbs].
//
// Real-world incidents repro'd this on 2026-05-26 (`tasks/software/tsift.md`)
// and 2026-05-27 (`boost-client/tasks/astro-listings.md`). Sequence:
//
// 1. The agent-doc cycle reaches `Committed` cleanly (response in HEAD).
// 2. While the cycle was open, IntelliJ surfaced a File Cache Conflict dialog
//    that the user did not resolve. The plugin stashed the IPC patch as a
//    conflict-deferred payload instead of mutating the open document.
// 3. Hours later the user accepts the dialog. The plugin replays the deferred
//    payload against the open document. Because the response body is already
//    in the committed file, the replay appends a second `### Re: …` block
//    to the working tree.
// 4. The next `agent-doc preflight` reaches the drift-recovery branch and
//    auto-commits the duplicated state — the dupe wedges into HEAD.
//
// Today the closeout invariants only check `has_duplicate_response_heading()`
// at phase `WriteApplied`, so once a duplicate lands *after* `Committed`, the
// existing strict invariant is silent. This scenario pins the failing baseline
// (duplicate visible in working tree post-commit) and the documented manual
// recovery (`dedupe_responses` + re-commit) so a future binary-side guard has
// a clear before/after assertion to flip.

#[test]
fn jb_cache_conflict_accept_late_replays_duplicate_response_today() {
    // FAILING BASELINE for #jbccacceptdup.
    //
    // After a committed cycle, the late-accepted conflict replays a stale
    // response payload back into the working tree. `self.doc` ends up with two
    // `### Re: sim closeout` blocks while `self.snapshot` still matches the
    // single-response commit — exactly the shape observed on
    // `boost-client/tasks/astro-listings.md` on 2026-05-27.
    //
    // When the binary-side fix lands (plan steps 2–5 in
    // `plan-jb-cache-conflict-accept-duplicates-response.md`), the plugin /
    // IPC apply path will revalidate against HEAD before mutation and skip
    // the replay. This test should then be replaced (or its assertions
    // inverted) by `jb_cache_conflict_accept_late_replay_rejected_at_apply`.
    let mut world = SimWorld::new(2026_05_27);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_accept(&mut world);
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert_eq!(world.snapshot, world.doc);

    let committed_snapshot = world.snapshot.clone();

    // Hours later: the long-pending File Cache Conflict dialog is accepted and
    // the plugin replays its deferred IPC payload on top of the already-
    // committed response.
    world.apply(SimCommand::DuplicateVisibleResponse).unwrap();

    assert_eq!(
        world.doc.matches("### Re: sim closeout").count(),
        2,
        "late accept must replay the stale response into the working tree"
    );
    assert_eq!(
        world.snapshot, committed_snapshot,
        "snapshot must still match the original committed cycle until the dupe is recommitted"
    );
    assert!(
        world.has_duplicate_response_heading(),
        "duplicate-response detector must observe the replayed block"
    );
}

#[test]
fn jb_cache_conflict_accept_late_replay_manual_repair_recovers_today() {
    // Today's documented manual recovery path for #jbccacceptdup: the operator
    // resolves the duplicate by hand (the replayed block often has a different
    // body than the committed one, so `dedupe_responses` — which only collapses
    // identical-body duplicates — can't always help) and then crosses the
    // binary-owned commit boundary via `agent-doc write --commit <FILE>` or
    // `agent-doc commit <FILE>`. In the simulator that's an explicit `world.doc`
    // edit (modeling the operator's edit) followed by a re-commit so the
    // snapshot catches up. This test pins the recovery exit so a future
    // binary-side replay guard can automate it without regressing the manual
    // escape hatch.
    let mut world = SimWorld::new(2026_05_27 + 1);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    apply_jb_cache_conflict_accept(&mut world);
    world.apply(SimCommand::Commit).unwrap();
    let committed_doc = world.doc.clone();
    world.apply(SimCommand::DuplicateVisibleResponse).unwrap();
    assert_eq!(world.doc.matches("### Re: sim closeout").count(), 2);

    // Manual recovery: operator deletes the replayed block, restoring the
    // committed doc. Then a re-commit updates the snapshot.
    world.doc = committed_doc;
    world.snapshot = world.doc.clone();

    assert_eq!(
        world.doc.matches("### Re: sim closeout").count(),
        1,
        "manual repair must leave a single response block in the working tree"
    );
    assert_eq!(world.snapshot, world.doc);
    world.strict_closeout_invariants().unwrap();
}
