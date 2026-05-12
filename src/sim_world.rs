//! Test-only deterministic workflow simulator.
//!
//! The simulator deliberately stays small: it models the closeout state that is
//! cheap to exercise in memory, and delegates document semantics to production
//! parsers/classifiers wherever possible.

use anyhow::{Result, anyhow, bail};
use std::collections::BTreeSet;
use std::path::Path;
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
    SyncProtectedGrowthManual,
    SyncProtectedGrowthPassive,
    SyncProtectedGrowthFocusVisible,
    SyncDetachableReplaceManual,
    SyncDetachableReplacePassive,
    SyncVisibleFocusPreserve,
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
}

impl RouteModel {
    fn new() -> Self {
        let durable = ActorState::initial();
        Self {
            projection: durable.clone(),
            durable,
            pending_dispatch: None,
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
    starting_prompt_promotions: usize,
    busy_dispatch_blocks: usize,
    busy_interrupt_recoveries: usize,
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
        self.starting_prompt_promotions += other.starting_prompt_promotions;
        self.busy_dispatch_blocks += other.busy_dispatch_blocks;
        self.busy_interrupt_recoveries += other.busy_interrupt_recoveries;
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
            let command = match rng.next_usize(41) {
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
                20 => SimCommand::BindRouteOwner,
                21 => SimCommand::SupervisorReady,
                22 => SimCommand::SupervisorBusy,
                23 => SimCommand::SupervisorWaitingInput,
                24 => SimCommand::SupervisorBlocked,
                25 => SimCommand::SupervisorClosed,
                26 => SimCommand::DispatchRoutePrompt,
                27 => SimCommand::ProveDispatchAccepted,
                28 => SimCommand::StaleSupervisorUpdate,
                29 => SimCommand::ObserveStalePane,
                30 => SimCommand::ObserveMissingPane,
                31 => SimCommand::DriftProjection,
                32 => SimCommand::RepairProjection,
                33 => SimCommand::PromoteStartingPromptReady,
                34 => SimCommand::BusyInterruptRecoveryReady,
                35 => SimCommand::SyncProtectedGrowthManual,
                36 => SimCommand::SyncProtectedGrowthPassive,
                37 => SimCommand::SyncProtectedGrowthFocusVisible,
                38 => SimCommand::SyncDetachableReplaceManual,
                39 => SimCommand::SyncDetachableReplacePassive,
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
        self.coverage.supervisor_lifecycle_updates += 1;
        Ok(())
    }

    fn dispatch_route_prompt(&mut self) -> Result<()> {
        let pane_id = self.current_dispatch_pane()?;
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
                crate::pending::detect_malformed_item_lines(component.content(&self.doc))
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

        if let Some(diff_text) = crate::diff::unified_diff_from_contents(&self.snapshot, &self.doc)
        {
            let prompt_targets = crate::diff::classify_prompt_bearing_changes(&diff_text)
                .into_iter()
                .filter(|change| {
                    matches!(
                        change.kind,
                        crate::diff::PromptBearingChangeKind::PromptTarget
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
