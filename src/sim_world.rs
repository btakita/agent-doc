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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HarnessKind {
    Codex,
    ClaudeCode,
    OpenCode,
}

impl HarnessKind {
    const ALL: [HarnessKind; 3] = [
        HarnessKind::Codex,
        HarnessKind::ClaudeCode,
        HarnessKind::OpenCode,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HarnessMatrixEdge {
    ResponseCapture,
    PendingBacklogMutation,
    QueueConsumption,
    IpcWrite,
    PtyFallbackWrite,
    HookStop,
    HookContinue,
    FailedCloseoutRecovery,
}

impl HarnessMatrixEdge {
    const ALL: [HarnessMatrixEdge; 8] = [
        HarnessMatrixEdge::ResponseCapture,
        HarnessMatrixEdge::PendingBacklogMutation,
        HarnessMatrixEdge::QueueConsumption,
        HarnessMatrixEdge::IpcWrite,
        HarnessMatrixEdge::PtyFallbackWrite,
        HarnessMatrixEdge::HookStop,
        HarnessMatrixEdge::HookContinue,
        HarnessMatrixEdge::FailedCloseoutRecovery,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookOutcome {
    StopFinalAnswer,
    ContinueInOwnerPane,
    ContinueViaAutoLoop,
    ContinueManually,
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
    PostCommitIpcRepositionSignal,
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
    /// `#qflood`: an explicit operator dispatch (JB `Run Agent Doc`), never
    /// coalesced. Driven only by targeted tests, not the random generator, so the
    /// seed corpus traces are unchanged.
    DispatchOperatorPrompt,
    ProveDispatchAccepted,
    StaleSupervisorUpdate,
    ObserveStalePane,
    ObserveMissingPane,
    DriftProjection,
    RepairProjection,
    PromoteStartingPromptReady,
    BusyInterruptRecoveryReady,
    RepairBusyProjectionWithReadyPrompt,
    AdminPauseQueue,
    AdminPauseQueueStale,
    AdminResumeQueue,
    AdminDrainQueue,
    AdminHandoff,
    AdminHandoffStale,
    AdminReap,
    AdminReapStale,
    SupervisorHeartbeatReattach,
    SupervisorHeartbeatStale,
    SyncProtectedGrowthManual,
    SyncProtectedGrowthPassive,
    SyncProtectedGrowthFocusVisible,
    SyncDetachableReplaceManual,
    SyncDetachableReplacePassive,
    SyncVisibleFocusPreserve,
    SyncRerequestVisibleEditorManual,
    SyncRerequestVisibleEditorPassive,
    SyncFocusStashedMoveBeforeSelect,
    /// `#recyclerestart-agent`: a killed/recycling pane left a manual
    /// `Sync Tmux Layout` subprocess holding the plugin sync guard.
    StartBlockingSyncAfterKillPane,
    /// Advance the simulated clock past the production sync stale-holder bound.
    AdvanceSyncGuardBeyondBound,
    /// Manual `Sync Tmux Layout` click through the production FFI acquire decision.
    SyncLayoutClick,
    /// The sync subprocess exits normally and releases the plugin-local guard.
    FinishSyncLayout,
    /// `#clearcontresume` recycle + clear pipeline. Driven only by targeted
    /// tests, not the random generator, so the seed corpus traces are unchanged.
    ///
    /// A go-mode `queue_active: true` head is waiting to drain.
    ActivateGoModeQueueHead,
    /// A later `cargo install` made the supervisor's launch binary stale.
    MarkSupervisorBinaryStale,
    /// `#suprecyclestall`: the next in-place `execve` recycle will fail to launch
    /// any candidate binary (e.g. an old pre-fix launch path). The watch must fall
    /// back to continuing on the current binary, never `process::exit` (which would
    /// orphan the child and hang the pane).
    MarkReexecWillFail,
    /// `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` opt-in is enabled. (`#supselfheal` made
    /// this the default, so this is now redundant with the baseline; kept for tests
    /// that assert the explicit opt-in path.)
    EnableSupervisorAutoRecycle,
    /// `#supselfheal`: explicit opt OUT of turn-boundary self-recycle (a falsey
    /// `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE` / frontmatter / project knob). A stale
    /// supervisor then only surfaces staleness (`Detect`) instead of self-recycling.
    DisableSupervisorAutoRecycle,
    /// Operator `admin recycle --all-projects`: mark this supervisor to recycle
    /// at the next idle boundary.
    OperatorRecycleMark,
    /// An operator-deferred clear is still pending delivery.
    DeferOperatorClearPending,
    /// One supervisor idle-queue-watch poll. Drives the production cooldown
    /// resume, recycle, and drain decision predicates exactly as `idle_watch.rs`
    /// does, with no live pane.
    SupervisorIdleQueueTick,
    /// `#qflood2`: the idle-queue watch sent its OWN `/clear` between queue items
    /// (an opt-in context reset or a `/clear` head). Engages the in-memory settle
    /// gate so the next drain trigger is not injected into the in-flight clear.
    SupervisorContextResetClear,
    /// `#qflood2`: set whether the routed trigger is already pending in the
    /// modeled composer (the live pane-capture dedup signal).
    SetTriggerAlreadyPending(bool),
    /// `#supkill-bg`: an explicit operator `restart-supervisor` (IPC `Restart`). The
    /// next idle tick drives the `supervisor_restart_action` drain-and-supersede
    /// policy: drain the in-flight turn, then in-place `execve` reexec (stale) or
    /// relaunch (fresh) at the turn boundary.
    RequestSupervisorRestart,
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
enum QueueControlState {
    Resumed,
    Paused,
    Draining,
}

impl QueueControlState {
    const fn as_failed_stage(self, lifecycle: SupervisorLifecycle) -> Option<&'static str> {
        match (self, lifecycle) {
            (Self::Paused, _) => Some("queue_paused"),
            (Self::Draining, SupervisorLifecycle::Ready) => None,
            (Self::Draining, _) => Some("actor_busy_draining"),
            (Self::Resumed, _) => None,
        }
    }
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

    /// Move-before-select focus (`#tmux-switch-lag`). The passive fast-handoff
    /// path must surface a stashed actor pane *out* of the stash window before
    /// selecting it, so the doc-to-doc switch never shows an intermediate stash
    /// frame. Model that ordering: when the focus target is stashed, perform the
    /// move (remove from `stashed`, add to `visible`) FIRST, then the select (set
    /// `active`). Selecting a still-stashed pane is the bug — the
    /// `active`-not-in-`stashed` structural invariant catches it.
    fn focus_doc_move_before_select(&mut self, doc: &str) {
        if self.stashed.remove(doc) {
            // Move: promote the pane into the working layout before selecting.
            if !self.visible.iter().any(|d| d == doc) {
                self.visible.push(doc.to_string());
            }
        }
        // Select only after the move has surfaced the pane.
        self.active = Some(doc.to_string());
    }

    /// The focus target is parked in the stash window. Used by the move-before-
    /// select coverage to prove the switch promotes before selecting.
    fn stashed_focus_case() -> Self {
        Self {
            visible: vec!["agent-doc".to_string()],
            protected_open_cycle: BTreeSet::new(),
            stashed: BTreeSet::from(["requested".to_string()]),
            active: Some("agent-doc".to_string()),
        }
    }
}

impl Default for SyncProjection {
    fn default() -> Self {
        Self::protected_growth_case()
    }
}

#[derive(Debug, Clone)]
struct SyncGuardModel {
    locked: bool,
    acquired_at_ms: u64,
    now_ms: u64,
    stale_bound_ms: u64,
    killed_pane_path: bool,
}

impl Default for SyncGuardModel {
    fn default() -> Self {
        Self {
            locked: false,
            acquired_at_ms: 0,
            now_ms: 1,
            stale_bound_ms: agent_doc::ffi::DEFAULT_SYNC_LOCK_STALE_BOUND_MS,
            killed_pane_path: false,
        }
    }
}

#[derive(Debug, Clone)]
struct RouteModel {
    durable: ActorState,
    projection: ActorState,
    pending_dispatch: Option<DispatchReceipt>,
    starting_timeout: Option<(u64, String)>,
    queue_control: QueueControlState,
    supervisor_lease_generation: Option<u64>,
}

impl RouteModel {
    fn new() -> Self {
        let durable = ActorState::initial();
        Self {
            projection: durable.clone(),
            durable,
            pending_dispatch: None,
            starting_timeout: None,
            queue_control: QueueControlState::Resumed,
            supervisor_lease_generation: Some(1),
        }
    }
}

/// Models the operator **recycle + clear pipeline** (`#clearcontresume`) so the
/// SimWorld engine can drive the SAME production decision predicates the live
/// supervisor idle-queue watch uses — `clear_cooldown_resume_ready` and
/// `supervisor_recycle_action` in `agent_doc_orchestration::start::decisions` —
/// instead of reimplementing the policy in the test harness. The operator's
/// pipeline is `admin recycle --all-projects` (mark recycle at next idle
/// boundary) → `session clear` (write the manual clear cooldown) → the cleared
/// pane settles to a fresh idle prompt → the cooldown auto-expires and the
/// go-mode queue drain resumes as a continuation *step* (not a stall).
#[derive(Debug, Clone, Default)]
struct RecycleClearModel {
    /// Manual clear cooldown is active: an operator `session clear` /
    /// JB `Clear Exchange` / delivered deferred clear wrote the marker
    /// (`queue_continuation::write_clear_cooldown`).
    clear_cooldown_active: bool,
    /// Consecutive idle-prompt polls observed since the clear settled, mirroring
    /// `idle_watch.rs`'s `clear_cooldown_idle_ticks` debounce counter.
    clear_cooldown_idle_ticks: u32,
    /// An operator-deferred clear is still pending delivery. That path owns its
    /// own resume, so the cooldown auto-expiry defers to it.
    deferred_operator_clear_pending: bool,
    /// A go-mode `queue_active: true` head is waiting to drain.
    queue_active_head: Option<String>,
    /// The operator marked this supervisor to recycle at the next idle boundary
    /// (`admin recycle --all-projects`).
    operator_recycle_marked: bool,
    /// The supervisor's launch binary is stale (a later `cargo install`), so the
    /// auto-recycle predicate can fire at a turn boundary.
    binary_stale: bool,
    /// Auto-recycle opt-in (`AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`).
    auto_recycle: bool,
    /// `#suprecyclestall`: the next in-place `execve` recycle will fail (e.g. a
    /// launch path that no longer resolves, an old pre-fix supervisor). Models the
    /// `supervisor_perform_reexec` Err path.
    reexec_will_fail: bool,
    /// `#suprecyclestall`: a self-`execve` recycle already failed, so the watch
    /// disabled further recycle attempts and keeps running on the current binary
    /// (mirrors idle_watch.rs's `reexec_recycle_disabled`).
    recycle_disabled: bool,
    /// The head the idle-queue watch last injected a trigger for (drain dedup).
    last_dispatched: Option<String>,
    /// `#qflood2`: the watch sent its OWN `/clear` (an opt-in context reset or a
    /// `/clear` queue head) and must hold the next drain trigger until the pane
    /// settles, so the trigger is not injected into the in-flight clear.
    awaiting_clear_settle: bool,
    /// Consecutive idle-prompt polls observed since the watch's own `/clear`,
    /// mirroring `idle_watch.rs`'s `clear_settle_idle_ticks` debounce counter.
    clear_settle_idle_ticks: u32,
    /// `#qflood2`: the routed trigger is already pending/visible in the modeled
    /// composer, so a re-send would stack a duplicate (`recent_lines_contain_trigger`).
    trigger_already_pending: bool,
    /// `#supkill-bg`: an explicit `restart-supervisor` (IPC `Restart`) is pending. The
    /// idle tick drives the production `supervisor_restart_action` drain-and-supersede
    /// policy — it DRAINS while a turn is in flight (`turn_active`) and only at the
    /// turn boundary re-execs in place (stale binary) or relaunches (fresh binary).
    restart_requested: bool,
    /// `#suprehotreload-agent`: a JB Run Agent Doc style operator dispatch is
    /// waiting for the next stale-binary recycle boundary to prove whether the
    /// cycle observed the fresh binary or mapped to the existing recycle-failure
    /// operator-verify buckets.
    jb_run_recycle_probe_pending: bool,
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
    route_dispatch_coalesced: usize,
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
    live_prompt_forward_merges: usize,
    already_applied_response_recoveries: usize,
    ack_sidecar_only_repairs: usize,
    visible_duplicate_repairs: usize,
    post_commit_follow_up_handoffs: usize,
    starting_prompt_promotions: usize,
    busy_dispatch_blocks: usize,
    closed_dispatch_blocks: usize,
    busy_interrupt_recoveries: usize,
    busy_projection_ready_repairs: usize,
    queue_pauses: usize,
    queue_resumes: usize,
    queue_drains: usize,
    queue_paused_dispatch_blocks: usize,
    actor_busy_draining_blocks: usize,
    queue_backpressure_events: usize,
    admin_handoffs: usize,
    admin_reaps: usize,
    supervisor_heartbeat_reattaches: usize,
    supervisor_heartbeat_stale_blocks: usize,
    stale_generation_blocks: usize,
    stale_pane_blocks: usize,
    missing_pane_blocks: usize,
    projection_drift_blocks: usize,
    projection_repairs: usize,
    sync_preserve_layout_blocks: usize,
    sync_detachable_replacements: usize,
    sync_protected_expansions: usize,
    sync_focus_handoffs: usize,
    harness_matrix_edges: BTreeSet<(HarnessKind, HarnessMatrixEdge)>,
    commits: usize,
    post_commit_worktree_checks: usize,
    sync_move_before_select_focuses: usize,
    /// `#clearcontresume`: a lingering manual clear cooldown auto-expired and the
    /// active go-mode queue drain resumed via the production
    /// `clear_cooldown_resume_ready` predicate.
    clear_cooldown_resumes: usize,
    /// The supervisor recycled in place onto a fresh binary at an idle boundary
    /// (operator `admin recycle` or the `supervisor_recycle_action` predicate),
    /// preserving the live pane via `execve`.
    supervisor_recycles: usize,
    /// `#suprecyclestall`: a self-`execve` recycle failed and the watch fell back to
    /// continuing on the current binary (never `process::exit`, so the pane survives).
    supervisor_recycle_failures: usize,
    /// `#supkill-bg`: an explicit `restart-supervisor` drained its in-flight turn,
    /// then hot-reloaded in place via `execve` at the turn boundary (stale binary,
    /// `supervisor_restart_action` → `ReexecInPlace`), preserving the live pane.
    supervisor_restart_drain_reexecs: usize,
    /// `#supkill-bg`: an explicit `restart-supervisor` on a fresh binary relaunched
    /// the child at the turn boundary (`supervisor_restart_action` → `RelaunchChild`).
    supervisor_restart_relaunches: usize,
    /// A go-mode queue head was dispatched by the idle-queue drain decision after
    /// the recycle + clear settled.
    go_drain_dispatches: usize,
    /// `#qflood2`: the idle-queue drain was held back because the watch's own
    /// `/clear` had not settled yet (the trigger would have been injected into
    /// the in-flight clear and concatenated as `/clear /agent-doc <FILE>`).
    drain_settle_skips: usize,
    /// `#qflood2`: a drain dispatch was skipped because the routed trigger was
    /// already pending in the composer (de-dup: only one trigger lands).
    drain_dedup_skips: usize,
    /// `#recyclerestart-agent`: a killed-pane sync path was modeled instead of
    /// requiring live JB/manual verification.
    sync_kill_pane_proofs: usize,
    /// `#recyclerestart-agent`: a fresh in-flight sync guard correctly deferred a
    /// later click.
    sync_guard_defers: usize,
    /// `#recyclerestart-agent`: a stale plugin-local sync guard was superseded by
    /// the production FFI stale-holder decision.
    sync_guard_stale_releases: usize,
    /// `#recyclerestart-agent`: a sync holder reached its normal finally/unlock path.
    sync_guard_completions: usize,
    /// `#recyclerestart-agent`: proof that recycle promoted the binary instead of
    /// silently killing the session.
    recycle_binary_promotion_proofs: usize,
    /// `#recyclerestart-agent`: proof that the following clear/drain path re-cleared
    /// or restarted the session before the next queue head.
    recycle_session_reclear_proofs: usize,
    /// `#suprehotreload-agent`: a JB Run Agent Doc style cycle reached the
    /// stale-binary recycle boundary and observed the fresh binary after promotion.
    suprehot_jb_observed_promotions: usize,
    /// `#suprehotreload-agent`: a JB Run Agent Doc style cycle hit the
    /// stale-binary recycle failure path and mapped to the existing
    /// #recyclerestart-verify/#aazp/#4myd operator-verify proof bucket.
    suprehot_jb_mapped_recycle_failures: usize,
}

impl Coverage {
    fn record_harness_matrix_edge(&mut self, harness: HarnessKind, edge: HarnessMatrixEdge) {
        self.harness_matrix_edges.insert((harness, edge));
    }

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
        if message.contains("failed_stage=queue_paused") {
            self.queue_paused_dispatch_blocks += 1;
            self.queue_backpressure_events += 1;
        }
        if message.contains("failed_stage=actor_busy_draining") {
            self.actor_busy_draining_blocks += 1;
            self.queue_backpressure_events += 1;
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
        if message.contains("supervisor heartbeat stale generation") {
            self.supervisor_heartbeat_stale_blocks += 1;
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
        self.route_dispatch_coalesced += other.route_dispatch_coalesced;
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
        self.already_applied_response_recoveries += other.already_applied_response_recoveries;
        self.ack_sidecar_only_repairs += other.ack_sidecar_only_repairs;
        self.visible_duplicate_repairs += other.visible_duplicate_repairs;
        self.post_commit_follow_up_handoffs += other.post_commit_follow_up_handoffs;
        self.starting_prompt_promotions += other.starting_prompt_promotions;
        self.busy_dispatch_blocks += other.busy_dispatch_blocks;
        self.closed_dispatch_blocks += other.closed_dispatch_blocks;
        self.busy_interrupt_recoveries += other.busy_interrupt_recoveries;
        self.busy_projection_ready_repairs += other.busy_projection_ready_repairs;
        self.queue_pauses += other.queue_pauses;
        self.queue_resumes += other.queue_resumes;
        self.queue_drains += other.queue_drains;
        self.queue_paused_dispatch_blocks += other.queue_paused_dispatch_blocks;
        self.actor_busy_draining_blocks += other.actor_busy_draining_blocks;
        self.queue_backpressure_events += other.queue_backpressure_events;
        self.admin_handoffs += other.admin_handoffs;
        self.admin_reaps += other.admin_reaps;
        self.supervisor_heartbeat_reattaches += other.supervisor_heartbeat_reattaches;
        self.supervisor_heartbeat_stale_blocks += other.supervisor_heartbeat_stale_blocks;
        self.stale_generation_blocks += other.stale_generation_blocks;
        self.stale_pane_blocks += other.stale_pane_blocks;
        self.missing_pane_blocks += other.missing_pane_blocks;
        self.projection_drift_blocks += other.projection_drift_blocks;
        self.projection_repairs += other.projection_repairs;
        self.sync_preserve_layout_blocks += other.sync_preserve_layout_blocks;
        self.sync_detachable_replacements += other.sync_detachable_replacements;
        self.sync_protected_expansions += other.sync_protected_expansions;
        self.sync_focus_handoffs += other.sync_focus_handoffs;
        self.harness_matrix_edges.extend(other.harness_matrix_edges);
        self.commits += other.commits;
        self.post_commit_worktree_checks += other.post_commit_worktree_checks;
        self.sync_move_before_select_focuses += other.sync_move_before_select_focuses;
        self.clear_cooldown_resumes += other.clear_cooldown_resumes;
        self.supervisor_recycles += other.supervisor_recycles;
        self.supervisor_recycle_failures += other.supervisor_recycle_failures;
        self.go_drain_dispatches += other.go_drain_dispatches;
        self.drain_settle_skips += other.drain_settle_skips;
        self.drain_dedup_skips += other.drain_dedup_skips;
        self.sync_kill_pane_proofs += other.sync_kill_pane_proofs;
        self.sync_guard_defers += other.sync_guard_defers;
        self.sync_guard_stale_releases += other.sync_guard_stale_releases;
        self.sync_guard_completions += other.sync_guard_completions;
        self.recycle_binary_promotion_proofs += other.recycle_binary_promotion_proofs;
        self.recycle_session_reclear_proofs += other.recycle_session_reclear_proofs;
        self.suprehot_jb_observed_promotions += other.suprehot_jb_observed_promotions;
        self.suprehot_jb_mapped_recycle_failures += other.suprehot_jb_mapped_recycle_failures;
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
    recycle_clear: RecycleClearModel,
    sync: SyncProjection,
    sync_guard: SyncGuardModel,
    ops_log: Vec<String>,
    next_prompt: usize,
    coverage: Coverage,
}

mod engine;

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

/// Normalize a committed document for working-tree==HEAD comparison: collapse the
/// boundary marker id to a stable token and strip ` (HEAD)` heading annotations
/// that the working tree/editor buffer is allowed to carry post-commit while the
/// committed blob does not. Anything left differing is real worktree drift.
fn normalize_committed_worktree(doc: &str) -> String {
    let mut out = String::with_capacity(doc.len());
    let mut rest = doc;
    while let Some(start) = rest.find("<!-- agent:boundary:") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        match after.find("-->") {
            Some(end) => {
                out.push_str("<!-- agent:boundary:NORM -->");
                rest = &after[end + 3..];
            }
            None => {
                out.push_str(after);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.replace(" (HEAD)", "")
}

fn response_patch(topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->\n"
    )
}

fn harness_response_patch(harness: HarnessKind, topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} — gpt-5\n\n{} response captured and verified.\n<!-- /patch:exchange -->\n",
        harness.as_str()
    )
}

fn fallback_response(topic: &str) -> String {
    format!("### Re: {topic} — gpt-5\n\nImplemented and verified through fallback.\n")
}

fn harness_fallback_response(harness: HarnessKind, topic: &str) -> String {
    format!(
        "### Re: {topic} — gpt-5\n\n{} PTY fallback response captured and verified.\n",
        harness.as_str()
    )
}

fn hook_outcome_for(harness: HarnessKind, active_queue: bool) -> HookOutcome {
    if !active_queue {
        return HookOutcome::StopFinalAnswer;
    }

    match harness {
        HarnessKind::Codex => HookOutcome::ContinueInOwnerPane,
        HarnessKind::ClaudeCode => HookOutcome::ContinueViaAutoLoop,
        HarnessKind::OpenCode => HookOutcome::ContinueManually,
    }
}

fn harness_matrix_seed(harness: HarnessKind, edge: HarnessMatrixEdge) -> u64 {
    let harness_offset = match harness {
        HarnessKind::Codex => 0,
        HarnessKind::ClaudeCode => 100,
        HarnessKind::OpenCode => 200,
    };
    let edge_offset = match edge {
        HarnessMatrixEdge::ResponseCapture => 1,
        HarnessMatrixEdge::PendingBacklogMutation => 2,
        HarnessMatrixEdge::QueueConsumption => 3,
        HarnessMatrixEdge::IpcWrite => 4,
        HarnessMatrixEdge::PtyFallbackWrite => 5,
        HarnessMatrixEdge::HookStop => 6,
        HarnessMatrixEdge::HookContinue => 7,
        HarnessMatrixEdge::FailedCloseoutRecovery => 8,
    };
    10_000 + harness_offset + edge_offset
}

fn run_harness_matrix_edge(harness: HarnessKind, edge: HarnessMatrixEdge) -> Result<Coverage> {
    let mut world = SimWorld::new(harness_matrix_seed(harness, edge));

    match edge {
        HarnessMatrixEdge::ResponseCapture => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_response_patch(harness, "matrix response capture"));
            world.apply_captured_response()?;
            world.try_commit()?;
            assert!(
                world.doc.contains(&format!(
                    "{} response captured and verified.",
                    harness.as_str()
                )),
                "response body missing for {harness:?}:\n{}",
                world.doc
            );
        }
        HarnessMatrixEdge::PendingBacklogMutation => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_response_patch(harness, "matrix pending mutation"));
            world.apply_captured_response()?;
            let next_backlog = format!(
                "{}- [ ] [#matrixfollowup] Follow-up from {} pending mutation\n",
                world.component_content("backlog")?,
                harness.as_str()
            );
            world.replace_component_content("backlog", &next_backlog)?;
            world.try_commit()?;
            assert!(
                world
                    .component_content("backlog")?
                    .contains("Follow-up from"),
                "pending/backlog mutation missing for {harness:?}:\n{}",
                world.doc
            );
        }
        HarnessMatrixEdge::QueueConsumption => {
            world.insert_after_exchange(
                "\n## Queue\n\n<!-- agent:queue auto -->\n- do [#matrixhead]\n- do [#matrixnext]\n<!-- /agent:queue -->\n",
            )?;
            world.snapshot = world.doc.clone();
            world.append_to_exchange("❯ do [#matrixhead]\n")?;
            world.replace_component_content(
                "queue",
                "- ~~do [#matrixhead]~~\n- do [#matrixnext]\n",
            )?;
            world.captured_response = Some(harness_response_patch(harness, "do [#matrixhead]"));
            world.apply_captured_response()?;
            world.try_commit()?;
            let queue = world.component_content("queue")?;
            assert!(
                queue.contains("~~do [#matrixhead]~~"),
                "head not consumed:\n{queue}"
            );
            assert!(
                queue.contains("do [#matrixnext]"),
                "next head missing:\n{queue}"
            );
        }
        HarnessMatrixEdge::IpcWrite => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response = Some(harness_response_patch(harness, "matrix ipc write"));
            world.apply_captured_response()?;
            world.try_commit()?;
            assert!(
                world.doc.contains("<!-- agent:boundary:committed -->"),
                "IPC closeout did not cross commit boundary for {harness:?}"
            );
        }
        HarnessMatrixEdge::PtyFallbackWrite => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_fallback_response(harness, "matrix pty fallback"));
            world.apply_captured_response()?;
            world.try_commit()?;
            assert!(
                world.doc.contains("PTY fallback response captured"),
                "PTY fallback body missing for {harness:?}:\n{}",
                world.doc
            );
        }
        HarnessMatrixEdge::HookStop => {
            assert_eq!(
                hook_outcome_for(harness, false),
                HookOutcome::StopFinalAnswer,
                "clean {harness:?} closeout should allow final answer"
            );
        }
        HarnessMatrixEdge::HookContinue => {
            let expected = match harness {
                HarnessKind::Codex => HookOutcome::ContinueInOwnerPane,
                HarnessKind::ClaudeCode => HookOutcome::ContinueViaAutoLoop,
                HarnessKind::OpenCode => HookOutcome::ContinueManually,
            };
            assert_eq!(
                hook_outcome_for(harness, true),
                expected,
                "active queue continuation semantics changed for {harness:?}"
            );
        }
        HarnessMatrixEdge::FailedCloseoutRecovery => {
            world.apply(SimCommand::EditPrompt)?;
            world.captured_response =
                Some(harness_response_patch(harness, "matrix failed closeout"));
            world.apply_captured_response()?;
            world.apply(SimCommand::CrashAt(FaultPoint::GitCommit))?;
            let err = world.try_commit().unwrap_err();
            assert!(
                err.to_string().contains("fault point GitCommit"),
                "unexpected failed closeout error for {harness:?}: {err}"
            );
            assert!(
                matches!(world.phase, CyclePhase::Interrupted(FaultPoint::GitCommit)),
                "failed closeout should fail closed before recovery for {harness:?}"
            );
            world.apply(SimCommand::Recover)?;
            assert_eq!(world.phase, CyclePhase::Committed);
            assert_eq!(world.snapshot, world.doc);
        }
    }

    world.coverage.record_harness_matrix_edge(harness, edge);
    world.strict_closeout_invariants()?;
    Ok(world.coverage)
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
        coverage.queue_pauses > 0
            && coverage.queue_resumes > 0
            && coverage.queue_drains > 0
            && coverage.queue_backpressure_events > 0,
        "seed corpus must exercise controller queue controls and backpressure receipts"
    );
    assert!(
        coverage.actor_busy_draining_blocks > 0 && coverage.queue_paused_dispatch_blocks > 0,
        "seed corpus must block paused queues and busy draining actors before dispatch"
    );
    assert!(
        coverage.admin_handoffs > 0 && coverage.admin_reaps > 0,
        "seed corpus must exercise generation-guarded admin handoff and reap"
    );
    assert!(
        coverage.supervisor_heartbeat_reattaches > 0
            && coverage.supervisor_heartbeat_stale_blocks > 0,
        "seed corpus must exercise supervisor heartbeat reattach and stale heartbeat rejection"
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
    assert!(
        coverage.post_commit_worktree_checks > 0,
        "seed corpus must exercise the post-commit IPC reposition working-tree==HEAD guard (#postcommit-ipc-worktree-corruption)"
    );
    assert!(
        coverage.sync_move_before_select_focuses > 0,
        "seed corpus must exercise move-before-select stash focus (#tmux-switch-lag)"
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
        coverage.queue_backpressure_events > 0
            && coverage.admin_handoffs > 0
            && coverage.admin_reaps > 0
            && coverage.supervisor_heartbeat_reattaches > 0,
        "medium seed corpus must keep queue/admin/heartbeat control-plane coverage"
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
fn post_commit_ipc_reposition_signal_keeps_worktree_equal_to_head() {
    // `#postcommit-ipc-worktree-corruption`: drive a clean committed closeout,
    // then fire the post-commit IPC boundary-reposition signal at the working
    // tree. The production reposition is idempotent on an already-clean committed
    // boundary, so the visible file must stay byte-equal to HEAD and the
    // working-tree==HEAD invariant must hold.
    let mut world = SimWorld::new(515);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.try_commit().unwrap();
    assert!(matches!(world.phase, CyclePhase::Committed));
    assert_eq!(
        world.doc, world.snapshot,
        "committed closeout must leave working tree == HEAD"
    );

    world
        .apply(SimCommand::PostCommitIpcRepositionSignal)
        .unwrap();
    assert_eq!(
        world.coverage.post_commit_worktree_checks, 1,
        "post-commit reposition signal must record a worktree==HEAD check"
    );
    world.assert_structural_invariants().unwrap();
    assert_eq!(
        normalize_committed_worktree(&world.doc),
        normalize_committed_worktree(&world.snapshot),
        "post-commit IPC reposition signal must not drift the working tree from HEAD"
    );
}

#[test]
fn move_before_select_promotes_stashed_pane_before_focus() {
    // `#tmux-switch-lag`: focusing a doc whose pane is parked in stash must move
    // it out of stash (into visible) BEFORE selecting it, so the switch never
    // shows an intermediate stash frame. The active pane must end up visible and
    // not in stash; the structural invariant rejects a still-stashed selection.
    let mut world = SimWorld::new(733);
    world
        .apply(SimCommand::SyncFocusStashedMoveBeforeSelect)
        .unwrap();
    world.assert_structural_invariants().unwrap();
    assert_eq!(world.sync.active.as_deref(), Some("requested"));
    assert!(
        !world.sync.stashed.contains("requested"),
        "focused pane must be promoted out of the stash window"
    );
    assert!(
        world.sync.visible.iter().any(|d| d == "requested"),
        "promoted pane must be visible in the working layout"
    );
    assert_eq!(world.coverage.sync_move_before_select_focuses, 1);

    // Negative control: a select that leaves the pane in stash (the flash bug)
    // must fail the move-before-select ordering invariant.
    let mut bug = SimWorld::new(734);
    bug.sync = SyncProjection::stashed_focus_case();
    bug.sync.active = Some("requested".to_string()); // selected while still stashed
    let err = bug.assert_structural_invariants().unwrap_err();
    assert!(
        err.to_string().contains("#tmux-switch-lag"),
        "unexpected ordering error: {err}"
    );
}

#[test]
fn post_commit_worktree_drift_is_distinguishable_from_head() {
    // Negative control for the working-tree==HEAD guard: the normalized comparison
    // the invariant relies on must distinguish a stale/spliced working-tree buffer
    // (the #postcommit-ipc-worktree-corruption bug) from the committed blob, while
    // still treating `(HEAD)` annotations and boundary-id churn as equal.
    let mut world = SimWorld::new(516);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.try_commit().unwrap();
    assert!(matches!(world.phase, CyclePhase::Committed));

    let head = world.snapshot.clone();
    // Allowed post-commit divergence: editor `(HEAD)` annotations + boundary id.
    let annotated = head
        .replace("### Re: sim closeout", "### Re: sim closeout (HEAD)")
        .replace("agent:boundary:committed", "agent:boundary:live-42");
    assert_eq!(
        normalize_committed_worktree(&annotated),
        normalize_committed_worktree(&head),
        "(HEAD)/boundary-id artifacts must normalize equal to HEAD"
    );

    // Real corruption: the IPC listener splices a duplicate response body into the
    // visible file. This must NOT normalize equal to HEAD.
    let corrupted = format!("{head}### Re: sim closeout — gpt-5\n\nStale spliced buffer.\n");
    assert_ne!(
        normalize_committed_worktree(&corrupted),
        normalize_committed_worktree(&head),
        "a spliced working-tree buffer must be detectable as drift from HEAD"
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
fn closeout_sim_harness_matrix_covers_agent_backends_and_edge_classes() {
    let mut observed = BTreeSet::new();

    for harness in HarnessKind::ALL {
        for edge in HarnessMatrixEdge::ALL {
            let coverage = run_harness_matrix_edge(harness, edge)
                .unwrap_or_else(|err| panic!("{harness:?}/{edge:?} failed: {err}"));
            assert!(
                coverage.harness_matrix_edges.contains(&(harness, edge)),
                "{harness:?}/{edge:?} did not record matrix coverage"
            );
            observed.extend(coverage.harness_matrix_edges);
        }
    }

    for harness in HarnessKind::ALL {
        for edge in HarnessMatrixEdge::ALL {
            assert!(
                observed.contains(&(harness, edge)),
                "missing harness matrix coverage for {harness:?}/{edge:?}"
            );
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
fn control_plane_sim_queue_controls_block_and_resume_route_dispatch() {
    let mut world = SimWorld::new(2_101);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::AdminPauseQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.queue_pauses, 1);
    assert_eq!(world.coverage.queue_paused_dispatch_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::AdminResumeQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(world.coverage.queue_resumes, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);
    assert_eq!(world.coverage.route_dispatch_proofs, 1);

    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::AdminDrainQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.queue_drains, 1);
    assert_eq!(world.coverage.actor_busy_draining_blocks, 1);
    assert_eq!(world.coverage.queue_backpressure_events, 2);
}

#[test]
fn control_plane_sim_admin_handoff_and_reap_require_current_generation() {
    let mut world = SimWorld::new(2_102);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::AdminHandoffStale).unwrap();
    assert_eq!(world.coverage.stale_generation_blocks, 1);
    assert_eq!(world.coverage.admin_handoffs, 0);

    let prior_generation = world.route.durable.generation;
    world.apply(SimCommand::AdminHandoff).unwrap();
    assert_eq!(world.coverage.admin_handoffs, 1);
    assert_eq!(world.route.durable.generation, prior_generation + 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Ready);
    assert_eq!(world.route.projection, world.route.durable);

    world.apply(SimCommand::AdminReapStale).unwrap();
    assert_eq!(world.coverage.stale_generation_blocks, 2);
    assert_eq!(world.coverage.admin_reaps, 0);

    world.apply(SimCommand::AdminReap).unwrap();
    assert_eq!(world.coverage.admin_reaps, 1);
    assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Closed);
    assert_eq!(world.route.durable.pane_id, None);
}

#[test]
fn control_plane_sim_supervisor_heartbeat_repairs_projection_after_drift() {
    let mut world = SimWorld::new(2_103);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DriftProjection).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.projection_drift_blocks, 1);
    assert_eq!(world.coverage.route_dispatch_acceptances, 0);

    world.apply(SimCommand::SupervisorHeartbeatStale).unwrap();
    assert_eq!(world.coverage.supervisor_heartbeat_stale_blocks, 1);

    world
        .apply(SimCommand::SupervisorHeartbeatReattach)
        .unwrap();
    assert_eq!(world.coverage.supervisor_heartbeat_reattaches, 1);
    assert_eq!(world.route.projection, world.route.durable);

    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
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
fn qflood_coalesces_in_flight_auto_redispatch_and_releases_after_proof() {
    // #qflood: while a cycle's first dispatch is in flight (accepted, unproven), an
    // AUTO re-dispatch (route auto-start on a file-change save, idle continuation,
    // `/loop` tick) must be COALESCED — not piled into the busy pane — and the
    // queue must keep running (no pause). This is the deterministic repro of the
    // operator's "triggers accumulate in the harness composer mid-turn" flood.
    let mut world = SimWorld::new(2_205);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);

    // Re-fire twice while the first dispatch is still in flight ⇒ both coalesced.
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "redundant in-flight re-dispatches must not pile up in the pane"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 2);
    assert_eq!(
        world.coverage.queue_pauses, 0,
        "coalescing is backpressure, never a queue stop"
    );

    // Once the in-flight dispatch is proven (consumed), coalescing RELEASES: the
    // next dispatch is admitted normally — no stall.
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 2,
        "a dispatch after the prior is consumed must be admitted, not coalesced"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 2);
}

#[test]
fn qflood_operator_dispatch_is_not_coalesced_in_flight() {
    // #qflood: an explicit operator dispatch (JB `Run Agent Doc`) must pass even
    // while an auto dispatch for the same cycle is in flight — operator intent is
    // never blocked by auto-drain backpressure ("type while the queue continues").
    let mut world = SimWorld::new(2_206);
    world.apply(SimCommand::SupervisorReady).unwrap();

    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);

    // An AUTO re-fire here would coalesce; the operator dispatch must NOT.
    world.apply(SimCommand::DispatchOperatorPrompt).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 2,
        "operator dispatch must pass through in-flight backpressure"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 0);
}

#[test]
fn qflood2_in_flight_coalesce_is_deduped_success_not_a_dispatch_failure() {
    // #qflood2: the #qflood in-flight coalesce SUPPRESSES the redundant re-send, but
    // it must report DEDUPED-SUCCESS — not a dispatch failure. This is the
    // deterministic analogue of the live route path translating the controller's
    // `coalesced_in_flight` bail into Ok(deduped pane) instead of an exit-1, while a
    // genuinely blocked dispatch (paused queue) still fails closed (the live
    // `dispatch_error_is_coalesced` classifier must not swallow a real `queue_paused`).
    let mut world = SimWorld::new(2_207);
    world.apply(SimCommand::SupervisorReady).unwrap();

    // First dispatch accepted + in flight.
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(world.coverage.route_dispatch_acceptances, 1);

    // Redundant in-flight AUTO re-dispatch ⇒ coalesced. It returns Ok (deduped
    // success): no second trigger piled into the pane AND no failure/backpressure
    // recorded — a coalesce is success, not a suppressed error.
    let coalesced = world.apply(SimCommand::DispatchRoutePrompt);
    assert!(
        coalesced.is_ok(),
        "a benign in-flight coalesce must report deduped-success, not error: {coalesced:?}"
    );
    assert_eq!(world.coverage.route_dispatch_coalesced, 1);
    assert_eq!(
        world.coverage.route_dispatch_acceptances, 1,
        "coalesce must not pile a second trigger into the pane"
    );
    assert_eq!(
        world.coverage.queue_paused_dispatch_blocks, 0,
        "a coalesce is deduped-success, not a queue block"
    );
    assert_eq!(
        world.coverage.queue_backpressure_events, 0,
        "a coalesce must not record a backpressure failure event"
    );

    // Contrast: a genuinely paused queue is NOT a benign coalesce — it must fail
    // closed and record a block, not be reported as deduped-success.
    world.apply(SimCommand::AdminPauseQueue).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    assert_eq!(
        world.coverage.queue_paused_dispatch_blocks, 1,
        "a real queue_paused dispatch must record a block, not deduped-success"
    );
    assert_eq!(
        world.coverage.route_dispatch_coalesced, 1,
        "the pause is a block, not a coalesce"
    );
}

#[test]
fn recycle_clear_pipeline_resumes_go_mode_drain_as_a_step() {
    // `#clearcontresume`: the full operator recycle + clear pipeline, driven by the
    // SAME production decision predicates the live supervisor idle-queue watch uses
    // (`supervisor_recycle_action`, `clear_cooldown_resume_ready`,
    // `idle_queue_drain_decision`). The recycle + clear is a continuation *step*: a
    // go-mode drain resumes on its own without an operator route.
    let mut world = SimWorld::new(2_026);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // `admin recycle --all-projects` marks recycle at the next idle boundary; a later
    // `cargo install` made the binary stale with auto-recycle opted in.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::EnableSupervisorAutoRecycle).unwrap();
    world.apply(SimCommand::OperatorRecycleMark).unwrap();

    // First idle tick: the idle boundary recycles in place onto the fresh binary,
    // PRESERVING the live pane (execve) and advancing the generation. No head is
    // waiting yet, so the recycle does not dispatch.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "recycle preserves the live pane via execve (not a cold rebind)"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "recycle promoted the freshly-installed binary"
    );
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "no head was waiting at the recycle boundary"
    );

    // A go-mode head is now waiting; the operator `session clear` writes the manual
    // clear cooldown before any idle poll observes the head.
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    // The cleared pane must settle for CLEAR_COOLDOWN_RESUME_IDLE_TICKS consecutive
    // idle polls before the cooldown auto-expires — earlier ticks must NOT resume.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(
            world.recycle_clear.clear_cooldown_active,
            "cooldown must hold at settle tick {tick}"
        );
        assert_eq!(world.coverage.clear_cooldown_resumes, 0);
        assert_eq!(world.coverage.go_drain_dispatches, 0);
    }

    // 4th settled idle tick: the cooldown auto-expires (resume re-evaluates next tick,
    // so no dispatch yet).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "cooldown auto-expired after the settle debounce"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the resume tick re-evaluates; it does not dispatch into the just-cleared pane"
    );

    // Next tick: the normal go-mode drain dispatches the waiting head — the recycle +
    // clear resumed the drain as a step, with no operator route.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the go-mode drain resumes after the recycle + clear settles"
    );

    // A stuck head is not re-fired every tick (drain dedup).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.go_drain_dispatches, 1);
}

#[test]
fn restart_supervisor_drains_then_reexecs_in_place_no_dropped_turn() {
    // `#supkill-bg` (#anw0 parts 1+2): an explicit `restart-supervisor` on a stale
    // supervisor is a blue/green DRAIN-and-supersede, driven by the production
    // `supervisor_restart_action` policy. While a turn is in flight the restart DRAINS
    // (no teardown mid-turn); only at the turn boundary does it hot-reload in place via
    // `execve`, preserving the live pane and advancing the generation with NO dropped
    // turn. This is the default healthy restart that fixes the stale-supervisor
    // `generation closed` / `#fcc0` case the kill-first path could not.
    let mut world = SimWorld::new(4_242);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor's launch binary is stale (a later `cargo install`). The operator
    // runs `restart-supervisor` WHILE a turn is in flight (the pane is Busy).
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();

    // Mid-turn idle ticks must DRAIN, not tear down: `supervisor_restart_action`
    // returns `AwaitDrain` while `turn_active`, so the restart stays pending and the
    // live turn (generation/pane) is untouched. No env opt-in is needed — an explicit
    // restart always supersedes.
    for tick in 1..=2 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.supervisor_restart_drain_reexecs, 0,
            "restart must DRAIN mid-turn (tick {tick}), never reexec into a live turn"
        );
        assert!(
            world.recycle_clear.restart_requested,
            "the restart stays pending while the turn drains (tick {tick})"
        );
        assert_eq!(world.route.durable.generation, gen_before);
        assert!(world.recycle_clear.binary_stale, "binary still stale mid-turn");
    }

    // The in-flight turn finishes (drains) → the pane returns to a dispatch-ready
    // prompt. The NEXT idle tick is the turn boundary: the restart hot-reloads in place
    // via `execve`, preserving the pane and advancing the generation.
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 1,
        "at the drained boundary the stale restart re-execs in place (default healthy restart)"
    );
    assert!(
        !world.recycle_clear.restart_requested,
        "the restart was served at the boundary"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "the in-place reexec advances the generation (supersede)"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "the reexec preserves the live pane via execve (no dropped turn, no cold rebind)"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "the reexec promoted the freshly-installed binary"
    );

    // Idempotent: with no pending restart, later idle ticks do not re-fire.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.supervisor_restart_drain_reexecs, 1);
}

#[test]
fn restart_supervisor_on_fresh_binary_relaunches_not_reexecs() {
    // `#supkill-bg`: an explicit `restart-supervisor` on a FRESH binary has nothing to
    // upgrade, so `supervisor_restart_action` returns `RelaunchChild` at the boundary —
    // the normal kill-child → relaunch path, not an in-place reexec.
    let mut world = SimWorld::new(909);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;

    // Fresh binary (no MarkSupervisorBinaryStale), restart requested at an idle prompt.
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 0,
        "a fresh binary has nothing to hot-reload — no in-place reexec"
    );
    assert_eq!(
        world.coverage.supervisor_restart_relaunches, 1,
        "a fresh-binary restart relaunches the child"
    );
    assert!(!world.recycle_clear.restart_requested);
    assert_eq!(
        world.route.durable.generation, gen_before,
        "a child relaunch does not advance the supervisor generation in place"
    );
}

#[test]
fn restart_supervisor_reexec_failure_falls_back_to_relaunch() {
    // `#supkill-bg` / `#suprecyclestall`: if the in-place `execve` fails, the restart
    // must NOT strand the session — it clears the reexec intent and falls back to a
    // child relaunch on the current binary (pane survives, binary stays stale until a
    // clean restart succeeds).
    let mut world = SimWorld::new(1_337);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::MarkReexecWillFail).unwrap();
    world.apply(SimCommand::RequestSupervisorRestart).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(
        world.coverage.supervisor_restart_drain_reexecs, 0,
        "a failed execve does not count as an in-place reexec"
    );
    assert_eq!(
        world.coverage.supervisor_restart_relaunches, 1,
        "the failed reexec falls back to a current-binary relaunch"
    );
    assert!(
        world.recycle_clear.binary_stale,
        "the failed reexec leaves the binary stale (operator restarts cleanly to upgrade)"
    );
    assert_eq!(world.route.durable.generation, gen_before);
}

#[test]
fn recyclerestart_agent_verifies_kill_pane_sync_guard_and_reclear_proofs() {
    // `#recyclerestart-agent`: replace the remaining live-only operator proof with
    // a deterministic model that covers both reported symptoms:
    // 1. recycle promotes the binary and the next queue item is re-cleared/drained;
    // 2. `Sync Tmux Layout` after a killed pane does not leave the plugin guard held forever.
    let mut world = SimWorld::new(6_150);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world
        .apply(SimCommand::EnableSupervisorAutoRecycle)
        .unwrap();
    world.apply(SimCommand::OperatorRecycleMark).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert_eq!(world.coverage.recycle_binary_promotion_proofs, 1);
    assert_eq!(world.route.durable.generation, gen_before + 1);
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "the hot reload must preserve the live pane"
    );

    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    for _ in 0..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.recycle_session_reclear_proofs, 1);
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(world.coverage.go_drain_dispatches, 1);

    world
        .apply(SimCommand::StartBlockingSyncAfterKillPane)
        .unwrap();
    world.apply(SimCommand::SyncLayoutClick).unwrap();
    assert_eq!(
        world.coverage.sync_guard_defers, 1,
        "a legitimately fresh holder may defer one later click"
    );

    world
        .apply(SimCommand::AdvanceSyncGuardBeyondBound)
        .unwrap();
    world.apply(SimCommand::SyncLayoutClick).unwrap();
    assert_eq!(
        world.coverage.sync_guard_stale_releases, 1,
        "the stale-holder decision must supersede a wedged guard"
    );
    world.apply(SimCommand::FinishSyncLayout).unwrap();
    world.apply(SimCommand::SyncLayoutClick).unwrap();
    assert_eq!(
        world.coverage.sync_guard_completions, 1,
        "the superseded sync reaches the normal unlock path"
    );

    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains("supervisor_recycle_action result=binary_promoted"),
        "ops log must distinguish binary promotion:\n{ops_log}"
    );
    assert!(
        ops_log.contains("recycle_session_reclear action=session_clear")
            && ops_log.contains("recycle_session_reclear action=idle_queue_drain result=dispatch"),
        "ops log must prove the post-recycle session clear/drain:\n{ops_log}"
    );
    assert!(
        ops_log.contains("sync_kill_pane_path pane_killed=true guard=held")
            && ops_log.contains("sync_guard_released reason=stale_holder_superseded")
            && ops_log.contains("sync_guard_released reason=sync_complete"),
        "ops log must prove killed-pane sync guard recovery:\n{ops_log}"
    );
}

#[test]
fn suprehotreload_agent_maps_jb_run_agent_doc_to_fresh_binary_proof() {
    // `#suprehotreload-agent`: a JB `Run Agent Doc` cycle can be verified without
    // live operator inspection when the stale supervisor reaches the recycle
    // boundary: the proof records a preserved-pane promotion onto the fresh binary.
    let mut world = SimWorld::new(6_151);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::DispatchOperatorPrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert!(world.recycle_clear.jb_run_recycle_probe_pending);

    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.supervisor_recycles, 1);
    assert_eq!(world.coverage.suprehot_jb_observed_promotions, 1);
    assert_eq!(world.coverage.suprehot_jb_mapped_recycle_failures, 0);
    assert!(!world.recycle_clear.jb_run_recycle_probe_pending);
    assert!(!world.recycle_clear.binary_stale);
    assert_eq!(world.route.durable.generation, gen_before + 1);
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "hot reload must preserve the JB-owned live pane"
    );

    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log
            .contains("suprehotreload_jb_cycle action=jb_run_agent_doc result=dispatch_accepted")
            && ops_log.contains("suprehotreload_jb_cycle result=observed_fresh_binary")
            && ops_log.contains("mapped=direct")
            && ops_log.contains("supervisor_recycle_action result=binary_promoted"),
        "ops log must prove the JB cycle observed a fresh binary:\n{ops_log}"
    );
}

#[test]
fn suprehotreload_agent_maps_reexec_failure_to_existing_operator_verify_buckets() {
    // `#suprehotreload-agent`: if execve fails, the agent-verifiable result is not
    // silent operator inspection. It maps the failed hot-reload attempt to the
    // existing live-proof buckets that already own recycle/restart verification.
    let mut world = SimWorld::new(6_152);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::MarkReexecWillFail).unwrap();
    world.apply(SimCommand::DispatchOperatorPrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();

    assert_eq!(world.coverage.supervisor_recycle_failures, 1);
    assert_eq!(world.coverage.suprehot_jb_observed_promotions, 0);
    assert_eq!(world.coverage.suprehot_jb_mapped_recycle_failures, 1);
    assert!(!world.recycle_clear.jb_run_recycle_probe_pending);
    assert!(world.recycle_clear.recycle_disabled);
    assert!(
        world.recycle_clear.binary_stale,
        "failed execve leaves the supervisor on the stale binary"
    );
    assert_eq!(world.route.durable.generation, gen_before);
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "failed recycle keeps the session alive in the same pane"
    );

    let ops_log = world.ops_log.join("\n");
    assert!(
        ops_log.contains(
            "suprehotreload_jb_cycle result=mapped_operator_verify targets=recyclerestart-verify,aazp,4myd"
        ) && ops_log.contains("reason=supervisor_reexec_failed"),
        "ops log must map failed hot reload to the existing proof buckets:\n{ops_log}"
    );
}

#[test]
fn opted_out_document_clears_and_drains_without_auto_recycle() {
    // `#simworld` / `#supselfheal`: a document that explicitly OPTED OUT of
    // `agent_doc_supervisor_auto_recycle` (falsey env/frontmatter/project knob) must
    // still perform the auto-CLEAR + go-mode drain steps between queue items, even
    // though its stale supervisor does NOT auto-recycle. This proves the recycle/clear
    // pipeline is independent of the recycle decision: with self-recycle now default-ON
    // (`#supselfheal`), an opt-OUT makes `supervisor_recycle_action` return
    // `Detect`/surface-only, but the clear-cooldown resume + drain is universal. Mirrors
    // `recycle_clear_pipeline_resumes_go_mode_drain_as_a_step` with auto-recycle forced
    // OFF to isolate the clear+drain steps from the recycle path.
    let mut world = SimWorld::new(7_311);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    // Explicit opt-out (default is now ON).
    world.apply(SimCommand::DisableSupervisorAutoRecycle).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor binary is stale (a later `cargo install`), but this document opted
    // OUT of auto-recycle. A go-mode head is waiting and the operator `session clear`
    // (or the watch's deferred clear) wrote the manual clear cooldown.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    // Settle debounce: the cooldown must hold for CLEAR_COOLDOWN_RESUME_IDLE_TICKS
    // consecutive idle polls. Throughout, the stale binary must NOT recycle (auto-recycle
    // OFF → `Detect`/surface-only), so the generation/pane are untouched and the binary
    // stays stale — only an operator restart promotes the new build for a non-opted-in doc.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(
            world.recycle_clear.clear_cooldown_active,
            "cooldown must hold at settle tick {tick}"
        );
        assert_eq!(world.coverage.clear_cooldown_resumes, 0);
        assert_eq!(world.coverage.go_drain_dispatches, 0);
        assert_eq!(
            world.coverage.supervisor_recycles, 0,
            "an opted-out stale supervisor never auto-recycles"
        );
    }

    // The cooldown auto-expires after the settle debounce — the auto-CLEAR step ran.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "cooldown auto-expired after the settle debounce — the clear step ran"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the resume tick re-evaluates; it does not dispatch into the just-cleared pane"
    );

    // Next tick: the go-mode drain dispatches the waiting head — the queue drains across
    // the clear with no auto-recycle involved.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the opted-out doc still drains its go-mode queue after the clear settles"
    );

    // The whole time, the stale supervisor stayed on its current binary: no recycle, no
    // generation churn, no cold pane rebind. Staleness is surfaced (Detect), not silently
    // hot-reloaded, for a document that opted out.
    assert_eq!(
        world.coverage.supervisor_recycles, 0,
        "opted out → the doc surfaces staleness instead of recycling"
    );
    assert_eq!(world.coverage.supervisor_recycle_failures, 0);
    assert!(
        world.recycle_clear.binary_stale,
        "the binary stays stale until an operator restart — no auto-recycle for an opted-out doc"
    );
    assert_eq!(
        world.route.durable.generation, gen_before,
        "no recycle → the generation is untouched"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "no recycle → the live pane is preserved (no cold rebind)"
    );
}

#[test]
fn stale_supervisor_self_recycles_at_turn_boundary_by_default() {
    // `#supselfheal`: the headline behavior change. With NO opt-in command and NO
    // operator `admin recycle` mark, a stale supervisor must self-retire at the next
    // turn boundary via the blue/green `execve` hot-reload — because turn-boundary
    // self-recycle now defaults ON (`resolve_supervisor_auto_recycle` → true). This is
    // the hands-off self-heal for the freshly-`cargo install`ed-but-still-stale case
    // that otherwise re-files File Cache Conflict / IPC-drift dialogs forever
    // (`#fcc0`/`#ipcdrift`). The blue/green guarantee: the live pane is preserved (no
    // cold rebind) and only the generation advances.
    let mut world = SimWorld::new(7_313);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor binary goes stale (a later `cargo install`). No opt-in, no
    // operator mark — the default-on policy must drive the recycle on its own.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    assert!(
        world.recycle_clear.auto_recycle,
        "self-recycle defaults ON (`#supselfheal`) with no opt-in command"
    );

    // First idle tick at a turn boundary: stale + default-on → recycle in place. No head
    // is waiting, so the recycle does not dispatch, but the binary is promoted.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "the default-on policy self-recycles a stale supervisor — no opt-in, no operator mark"
    );
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 0,
        "the blue/green execve hot-reload succeeded"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "self-recycle promoted the freshly-installed binary"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "blue/green: the live pane is preserved via execve (not a cold rebind)"
    );
}

#[test]
fn non_dogfood_document_auto_recycles_and_drains_from_opt_in_alone() {
    // `#simworld` / `#suprecyclecfg`: the COMPLEMENT of
    // `opted_out_document_clears_and_drains_without_auto_recycle`. A non-dogfooding
    // document whose project (or frontmatter) DID opt into
    // `agent_doc_supervisor_auto_recycle` must perform BOTH the auto-recycle AND the
    // auto-clear + go-mode drain steps, driven purely by the opt-in — with NO operator
    // `admin recycle` mark. This proves the per-document/per-project opt-in alone (the
    // resolution rung added in `resolve_supervisor_auto_recycle`) is sufficient to drive
    // the full recycle/clear/drain pipeline for any document, not just the dogfooding
    // repo and not requiring a manual `admin recycle --all-projects`. Mirrors
    // `recycle_clear_pipeline_resumes_go_mode_drain_as_a_step` but replaces the
    // `OperatorRecycleMark` with the auto path (stale binary + opt-in → the production
    // `supervisor_recycle_action` predicate fires on its own).
    let mut world = SimWorld::new(7_312);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    // The supervisor binary is stale (a later `cargo install`) and this document opted
    // into auto-recycle — but the operator never ran `admin recycle`. The opt-in alone
    // must drive the recycle.
    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::EnableSupervisorAutoRecycle).unwrap();

    // First idle tick: stale + opt-in + turn boundary → `supervisor_recycle_action`
    // returns Recycle (no operator mark needed). The in-place `execve` promotes the
    // fresh binary, preserves the live pane, and advances the generation. No head is
    // waiting yet, so the recycle does not dispatch.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycles, 1,
        "the opt-in alone drives the auto-recycle — no operator `admin recycle` mark"
    );
    assert_eq!(
        world.route.durable.generation,
        gen_before + 1,
        "recycle advances the generation"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "recycle preserves the live pane via execve (not a cold rebind)"
    );
    assert!(
        !world.recycle_clear.binary_stale,
        "recycle promoted the freshly-installed binary"
    );
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "no head was waiting at the recycle boundary"
    );

    // A go-mode head is now waiting; the operator `session clear` (or the watch's
    // deferred clear) writes the manual clear cooldown before any idle poll observes it.
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    // Settle debounce: the cooldown holds for CLEAR_COOLDOWN_RESUME_IDLE_TICKS
    // consecutive idle polls. The binary is already fresh, so NO further recycle fires.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(
            world.recycle_clear.clear_cooldown_active,
            "cooldown must hold at settle tick {tick}"
        );
        assert_eq!(world.coverage.clear_cooldown_resumes, 0);
        assert_eq!(world.coverage.go_drain_dispatches, 0);
        assert_eq!(
            world.coverage.supervisor_recycles, 1,
            "a freshly-recycled supervisor does not recycle again"
        );
    }

    // 4th settled idle tick: the auto-CLEAR step completes (cooldown auto-expires).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert!(
        !world.recycle_clear.clear_cooldown_active,
        "cooldown auto-expired after the settle debounce — the clear step ran"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 1);
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the resume tick re-evaluates; it does not dispatch into the just-cleared pane"
    );

    // Next tick: the go-mode drain dispatches the waiting head — recycle + clear + drain
    // all resumed as steps, driven by the auto-recycle opt-in with no operator route.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the non-dogfooding doc drains its go-mode queue after the opt-in recycle + clear"
    );
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 0,
        "the in-place recycle succeeded"
    );

    // A stuck head is not re-fired every tick (drain dedup).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.go_drain_dispatches, 1);
}

#[test]
fn failed_reexec_recycle_keeps_session_alive_and_does_not_retry() {
    // `#suprecyclestall`: a stale binary opted into auto-recycle, but the in-place
    // `execve` fails (no candidate binary launches). The watch must NOT
    // `process::exit` — that orphaned the live child and hung the tmux pane.
    // Instead it logs, disables further recycle attempts, and keeps running on the
    // current binary; the operator restarts deliberately.
    let mut world = SimWorld::new(2_026);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();

    let gen_before = world.route.durable.generation;
    let pane_before = world.route.durable.pane_id.clone();

    world.apply(SimCommand::MarkSupervisorBinaryStale).unwrap();
    world.apply(SimCommand::EnableSupervisorAutoRecycle).unwrap();
    world.apply(SimCommand::MarkReexecWillFail).unwrap();

    // Idle boundary: the recycle is attempted and fails.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 1,
        "the failed execve recycle is recorded"
    );
    assert_eq!(
        world.coverage.supervisor_recycles, 0,
        "no successful in-place recycle happened"
    );
    assert_eq!(
        world.route.durable.pane_id, pane_before,
        "the session/pane survives a failed recycle (no process::exit)"
    );
    assert_eq!(
        world.route.durable.generation, gen_before,
        "a failed recycle does not advance the generation"
    );
    assert!(
        matches!(world.route.durable.lifecycle, SupervisorLifecycle::Ready),
        "the supervisor keeps running on its current binary"
    );
    assert!(
        world.recycle_clear.binary_stale,
        "the binary is still stale — only an operator restart promotes the new build"
    );

    // Subsequent idle boundaries do NOT re-attempt the hopeless recycle.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.supervisor_recycle_failures, 1,
        "recycle is disabled after the first failure — no per-tick re-spam"
    );
}

#[test]
fn qflood2_drain_holds_trigger_until_own_clear_settles() {
    // `#qflood2`: after the idle-queue watch sends its OWN `/clear` between queue
    // items, the next drain trigger must NOT dispatch until the cleared pane has
    // settled for CLEAR_COOLDOWN_RESUME_IDLE_TICKS consecutive idle polls.
    // Otherwise it lands in the still-in-flight clear and the harness sees one
    // concatenated line (`/clear /agent-doc <FILE>`). Driven by the production
    // `drain_blocked_awaiting_clear_settle` predicate.
    let mut world = SimWorld::new(4_242);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();

    // The watch sends its own `/clear` (no manual cooldown marker written).
    world.apply(SimCommand::SupervisorContextResetClear).unwrap();

    // The first 3 settle ticks must HOLD the trigger (drain_settle_skip), never
    // dispatching into the in-flight clear.
    for tick in 1..4 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert_eq!(
            world.coverage.go_drain_dispatches, 0,
            "trigger must not dispatch into the in-flight clear at settle tick {tick}"
        );
        assert_eq!(world.coverage.drain_settle_skips, tick);
    }

    // 4th settled idle tick: the gate releases AND the drain dispatches the head
    // exactly once — one `/clear`, then one trigger, with no concatenation.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 1,
        "the single trigger dispatches once the pane settled after the clear"
    );

    // It is not re-fired on subsequent ticks (drain dedup on the same head).
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.go_drain_dispatches, 1);
}

#[test]
fn qflood2_drain_dedups_trigger_already_pending_in_composer() {
    // `#qflood2`: even at a settled idle boundary, if the routed trigger is
    // already pending in the composer (a prior dispatch / another owner put it
    // there), the watch must skip the re-send so duplicates never stack
    // (`/agent-doc <FILE>/agent-doc <FILE>`). Driven by `drain_dispatch_dedup_skip`.
    let mut world = SimWorld::new(4_243);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SetTriggerAlreadyPending(true)).unwrap();

    // The pane is idle and a head is waiting, but the trigger is already pending:
    // the drain must dedup-skip, not stack a second copy.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "a trigger already in the composer must not be re-sent"
    );
    assert_eq!(world.coverage.drain_dedup_skips, 1);

    // Once the composer clears (operator submitted / consumed the pending trigger),
    // the drain dispatches normally — the dedup never permanently suppresses it.
    world.apply(SimCommand::SetTriggerAlreadyPending(false)).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.go_drain_dispatches, 0,
        "the same head was already recorded as dispatched by the dedup-skip"
    );
    // The head was marked last_dispatched by the dedup path, so the normal drain
    // dedup (`SkipAlreadyDispatched`) now owns it — no duplicate, no hot loop.
    assert_eq!(world.coverage.drain_dedup_skips, 1);
}

#[test]
fn clear_cooldown_without_active_head_stays_authoritative() {
    // A plain operator clear with NO active go-mode head keeps the cooldown
    // authoritative-until-operator-route: the non-go behavior must be preserved.
    let mut world = SimWorld::new(2_027);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();
    assert!(world.recycle_clear.clear_cooldown_active);

    for _ in 0..8 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    assert!(
        world.recycle_clear.clear_cooldown_active,
        "no active head → cooldown stays authoritative (non-go behavior preserved)"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 0);
    assert_eq!(world.coverage.go_drain_dispatches, 0);
}

#[test]
fn clear_cooldown_deferred_operator_clear_blocks_resume() {
    // When an operator-deferred clear is still pending delivery, that path owns its
    // own resume — the cooldown auto-expiry must defer to it even with a live head.
    let mut world = SimWorld::new(2_028);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::DeferOperatorClearPending).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();

    for _ in 0..8 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    }
    assert!(
        world.recycle_clear.clear_cooldown_active,
        "a pending deferred operator clear blocks the auto-resume"
    );
    assert_eq!(world.coverage.clear_cooldown_resumes, 0);
}

#[test]
fn clear_cooldown_resume_debounce_resets_when_a_turn_interrupts_idle() {
    // The settle debounce is consecutive: a turn becoming active mid-settle resets the
    // idle-tick count, so a resumed drain never injects into an in-flight turn.
    let mut world = SimWorld::new(2_029);
    world.apply(SimCommand::BindRouteOwner).unwrap();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::ActivateGoModeQueueHead).unwrap();
    world.apply(SimCommand::SessionClear).unwrap();

    // Two settle ticks, then a turn interrupts (Busy), resetting the debounce.
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    world.apply(SimCommand::SupervisorBusy).unwrap();
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(world.coverage.clear_cooldown_resumes, 0);
    assert!(world.recycle_clear.clear_cooldown_active);

    // Back to idle: must settle for the full threshold again from zero.
    world.apply(SimCommand::SupervisorReady).unwrap();
    for _ in 0..3 {
        world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
        assert!(world.recycle_clear.clear_cooldown_active);
    }
    world.apply(SimCommand::SupervisorIdleQueueTick).unwrap();
    assert_eq!(
        world.coverage.clear_cooldown_resumes, 1,
        "the debounce restarts from zero after the interrupting turn"
    );
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

// -------- #fintol3: finalize tolerance for independent concurrent edits --------

#[test]
fn finalize_forward_merges_plain_concurrent_edit_outside_response() {
    // #fintol3: the operator edits a plain parked comment-note OUTSIDE the
    // response target while the agent finalizes. The edit carries no
    // prompt/directive, so finalize forward-merges instead of rejecting — the
    // response commits AND the user's edit is preserved in the same commit.
    let mut world = SimWorld::new(2_044);
    let baseline = concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "old parked note\n",
        "<!-- /agent:exchange -->\n\n",
        "<!--\nold parked note body\n-->\n",
    );
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: reply — gpt-5\n\nAnswered with a sufficiently long response body.\n<!-- /agent:exchange -->",
    );
    // The user edits the parked note prose (outside exchange) mid-finalize.
    let candidate = content_ours.replace("old parked note body", "edited parked note body");

    world.finalize_ipc_candidate_with_tolerance(baseline, &content_ours, &candidate);

    assert_eq!(
        world.coverage.live_prompt_forward_merges, 1,
        "a disjoint plain edit outside the response must forward-merge"
    );
    assert_eq!(
        world.coverage.ipc_snapshot_live_prompt_blocks, 0,
        "a disjoint plain edit must NOT be rejected"
    );
    assert!(
        world.snapshot.contains("### Re: reply — gpt-5"),
        "the agent response must be committed:\n{}",
        world.snapshot
    );
    assert!(
        world.snapshot.contains("edited parked note body"),
        "the user's concurrent plain edit must survive in the commit:\n{}",
        world.snapshot
    );
    assert!(
        !world.snapshot.contains("<<<<<<<"),
        "the committed union must be conflict-free:\n{}",
        world.snapshot
    );
}

#[test]
fn finalize_carries_directive_edit_forward_instead_of_merging() {
    // #fintol3 (complement): a concurrent edit that adds a prompt/directive (a
    // `dispatch #…` scratch directive here) is NOT forward-merged — it stays on
    // today's fail-closed path so the directive is a next-cycle diff, not a
    // premature commit. The safety property the forward-merge must never weaken.
    let mut world = SimWorld::new(2_045);
    let baseline = concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "<!-- /agent:exchange -->\n\n",
        "<!--\n-->\n",
    );
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: Please reply — gpt-5\n\nThe agent's answer body, long enough to matter.\n<!-- /agent:exchange -->",
    );
    // The user typed a scratch dispatch directive into the comment block.
    let candidate =
        content_ours.replace("<!--\n-->", "<!--\ndispatch #spec-test-build-install\n-->");

    world.finalize_ipc_candidate_with_tolerance(baseline, &content_ours, &candidate);

    assert_eq!(
        world.coverage.live_prompt_forward_merges, 0,
        "a directive-bearing edit must not forward-merge"
    );
    assert_eq!(
        world.coverage.ipc_snapshot_live_prompt_blocks, 1,
        "a directive-bearing edit keeps today's fail-closed carry-forward behavior"
    );
    assert_eq!(
        world.snapshot, content_ours,
        "the committed snapshot stays the agent response (the directive is not absorbed)"
    );
    assert!(
        world.doc.contains("dispatch #spec-test-build-install"),
        "the directive is carried forward in the live buffer for the next cycle:\n{}",
        world.doc
    );
}

// #mrhpcdrift2: the recurring `ipc_socket_already_applied_live_buffer_diverged`
// drift must always be RECOVERED, never silently lost. When the socket reports
// `already_applied` but the live buffer diverged with the assistant response
// fragmented out of `exchange` (plus a fresh user keystroke), the recovery
// materializes the response back from `content_ours` so the committed snapshot
// carries the full response and the live user edit survives. The target is zero
// UNRECOVERED drift — clean detect+recover is acceptable, silent loss is not.
#[test]
fn already_applied_diverged_sim_recovers_dropped_response_without_loss() {
    let mut world = SimWorld::new(2_037);
    world.append_to_exchange("❯ Please reply\n").unwrap();
    let baseline = world.doc.clone();
    let expected_response = "### Re: Please reply — gpt-5\n\nAnswered.";
    let content_ours = baseline.replace(
        "<!-- /agent:exchange -->",
        "### Re: Please reply — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
    );
    // The already_applied socket ack came back, but the live buffer diverged:
    // the response fragmented out of exchange (only the prompt survives) and the
    // operator typed the next prompt mid-finalize. The recovery must not lose
    // the response.
    world.doc = baseline.replace(
        "<!-- /agent:exchange -->",
        "❯ typing the next prompt mid-finalize\n<!-- /agent:exchange -->",
    );

    world.recover_already_applied_diverged_response(&content_ours, expected_response);

    assert_eq!(
        world.coverage.already_applied_response_recoveries, 1,
        "the dropped response must trigger exactly one recovery"
    );
    assert_eq!(
        world.doc.matches("### Re: Please reply — gpt-5").count(),
        1,
        "the dropped response must be materialized back exactly once (zero UNRECOVERED drift)"
    );
    assert!(
        world.doc.contains("❯ typing the next prompt mid-finalize"),
        "the live user keystroke must be preserved alongside the recovered response"
    );
    assert_eq!(
        world.snapshot, content_ours,
        "the committed snapshot adopts content_ours (baseline + full response)"
    );

    // Idempotent: re-running recovery once the response is materialized is a
    // no-op — no duplicate heading, no extra recovery count.
    world.recover_already_applied_diverged_response(&content_ours, expected_response);
    assert_eq!(
        world.coverage.already_applied_response_recoveries, 1,
        "recovery must not re-fire once the response is already materialized"
    );
    assert_eq!(
        world.doc.matches("### Re: Please reply — gpt-5").count(),
        1,
        "no duplicate response heading after idempotent recovery"
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
    assert!(
        after.contains("- ~~do [#alpha]~~"),
        "alpha struck:\n{after}"
    );
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

// ============================================================================
// #swint — SimWorld editor + tmux integration harness
//
// `SimEditor` is a deterministic in-harness actor that speaks the same durable
// live-buffer protocol the JetBrains / VS Code plugins speak over socket IPC /
// FFI: it records the editor-visible buffer via
// `debounce::record_live_buffer_digest_content` (the `#pcp6` full-content
// digest the plugin writes on every change) and reads "current document" back
// through the *production* `realtime_model::resolve_current_doc` seam (rung 3b,
// `#rtwatch`). This lets a SimWorld scenario exercise the editor-buffer-vs-disk
// read-authority reconcile, multi-editor CRDT broadcast (`#rtwbcast`), and the
// tmux dispatch/drain integrated system (`#kp5z`) *without a live IDE* — turning
// the File-Cache-Conflict / IPC-drift / queue-flood live-verify-only classes
// into deterministic regressions.
//
// See tasks/agent-doc/plan-simworld-editor-integration.md.
// ============================================================================

use agent_doc_orchestration::realtime_model::{BufferState, DocAuthority, Reconciliation};

/// Which editor's live-buffer protocol a [`SimEditor`] emulates. The read
/// authority contract is identical across kinds (a dirty buffer is always
/// authoritative over stale disk); the kind only changes how an *external* disk
/// write that lands while the buffer is dirty is surfaced to the user — the
/// File-Cache-Conflict semantics that actually bite (Slice 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorKind {
    /// No editor-specific conflict modeling — the pure realtime-model seam.
    Generic,
    /// IntelliJ / JetBrains: a read-only VFS WatchService notices the external
    /// disk change and raises a modal "File Cache Conflict" dialog. The buffer
    /// stays authoritative until the user resolves the dialog.
    JetBrains,
    /// VS Code: buffer events keep the dirty buffer; an external disk change is
    /// flagged non-modally and the in-memory buffer stays authoritative.
    VsCode,
}

impl EditorKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::JetBrains => "jetbrains",
            Self::VsCode => "vscode",
        }
    }
}

/// How a [`SimEditor`] of a given [`EditorKind`] reconciles an external disk
/// write (e.g. agent-doc's own patchback) that lands while the buffer holds
/// unsaved edits. Both editors keep the buffer authoritative (no clobber) until
/// the user resolves; only the surfaced signal differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheConflict {
    /// Buffer was clean (in sync with disk): the external write is adopted
    /// silently — there is nothing to conflict with.
    NoneAdopted,
    /// JetBrains modal File-Cache-Conflict dialog raised; buffer stays dirty.
    JetBrainsDialog,
    /// VS Code non-modal "file changed on disk" badge; buffer stays dirty.
    VsCodeKeepBuffer,
}

/// A deterministic editor-buffer actor that speaks the durable live-buffer
/// protocol against a real on-disk document, so SimWorld scenarios can drive the
/// production read-authority reconcile without a live IDE.
#[derive(Debug)]
struct SimEditor {
    kind: EditorKind,
    editor_id: String,
    path: PathBuf,
    /// Canonical path string used as the live-buffer sidecar key. Mirrors the key
    /// `realtime_model::resolve_current_doc` canonicalizes the file to, so a
    /// relative-vs-absolute mismatch cannot silently miss the sidecar.
    key: String,
    buffer: String,
    dirty: bool,
    generation: u64,
}

impl SimEditor {
    /// Attach an editor of `kind` to an existing on-disk document. The buffer
    /// starts equal to disk (clean, in sync).
    fn attach(kind: EditorKind, path: &Path) -> Result<Self> {
        Self::attach_with_id(kind, path, kind.as_str())
    }

    fn attach_with_id(kind: EditorKind, path: &Path, editor_id: &str) -> Result<Self> {
        let buffer = std::fs::read_to_string(path)
            .map_err(|err| anyhow!("SimEditor attach read {}: {err}", path.display()))?;
        Ok(Self {
            kind,
            editor_id: editor_id.to_string(),
            path: path.to_path_buf(),
            key: editor_buffer_key(path),
            buffer,
            dirty: false,
            generation: 0,
        })
    }

    fn generic(path: &Path) -> Result<Self> {
        Self::attach(EditorKind::Generic, path)
    }

    fn jetbrains(path: &Path) -> Result<Self> {
        Self::attach(EditorKind::JetBrains, path)
    }

    fn vscode(path: &Path) -> Result<Self> {
        Self::attach(EditorKind::VsCode, path)
    }

    /// Type an unsaved edit: the buffer now holds `content` ahead of disk. Records
    /// the durable live-buffer sidecar so the production realtime feed surfaces it
    /// as a genuine unsaved edit (the `#queue-user-edit-overwrite` no-clobber
    /// hazard this whole plan exists to defend).
    fn type_unsaved(&mut self, content: &str) -> Result<()> {
        self.buffer = content.to_string();
        self.dirty = true;
        self.generation += 1;
        self.record_buffer()
    }

    /// Flush the buffer to disk (Ctrl-S): buffer == disk, clean. Re-records the
    /// sidecar so the realtime feed classifies the editor as in sync with disk.
    fn save(&mut self) -> Result<()> {
        std::fs::write(&self.path, &self.buffer)
            .map_err(|err| anyhow!("SimEditor save write {}: {err}", self.path.display()))?;
        self.dirty = false;
        self.generation += 1;
        self.record_buffer()
    }

    /// Close the document in the editor: clear the live-buffer sidecar so the
    /// cycle falls back to disk (`editor_absent`). Uses the production
    /// `debounce::clear_live_buffer` editor-close primitive.
    fn close(self) -> Result<()> {
        agent_doc_orchestration::debounce::clear_live_buffer_for_editor(
            &self.key,
            Some(&self.editor_id),
        )
        .map_err(|err| anyhow!("SimEditor close clear sidecar: {err}"))
    }

    /// Adopt a CRDT-merged document broadcast back from a peer editor (Slice 3,
    /// `#rtwbcast`): the realtime model merged a peer's change and pushed the
    /// conflict-free result back into this buffer. The buffer stays unsaved (the
    /// merge lives in-buffer) but converged.
    fn adopt_broadcast(&mut self, merged: &str) -> Result<()> {
        self.buffer = merged.to_string();
        self.dirty = true;
        self.generation += 1;
        self.record_buffer()
    }

    fn apply_targeted_patch_file(&mut self, patch_file: &Path) -> Result<bool> {
        let payload: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(patch_file).map_err(|err| {
                anyhow!(
                    "SimEditor read targeted patch {}: {err}",
                    patch_file.display()
                )
            })?)
            .map_err(|err| anyhow!("SimEditor parse targeted patch JSON: {err}"))?;
        let target = payload
            .get("editor_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if target != self.editor_id {
            return Ok(false);
        }
        let origin = payload
            .get("origin_editor_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if origin == self.editor_id {
            bail!("targeted broadcast patch echoed back to originator {origin}");
        }
        let mut next = self.buffer.clone();
        for patch in payload
            .get("patches")
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow!("targeted patch payload missing patches array"))?
        {
            let component_name = patch
                .get("component")
                .and_then(|value| value.as_str())
                .ok_or_else(|| anyhow!("targeted patch missing component"))?;
            let op = patch
                .get("op")
                .and_then(|value| value.as_str())
                .unwrap_or("replace");
            let content = patch
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let component = agent_doc_core::component::parse(&next)?
                .into_iter()
                .find(|component| component.name == component_name)
                .ok_or_else(|| anyhow!("component {component_name} not found in editor buffer"))?;
            let existing = component.content(&next);
            let replacement = match op {
                "replace" => content.to_string(),
                "append" => format!("{existing}{content}"),
                "prepend" => format!("{content}{existing}"),
                other => bail!("unsupported targeted component patch op {other}"),
            };
            next = component.replace_content(&next, &replacement);
            agent_doc_core::component::parse(&next).map_err(|err| {
                anyhow!("targeted patch made component {component_name} invalid: {err}\n{next}")
            })?;
        }
        self.adopt_broadcast(&next)?;
        std::fs::remove_file(patch_file).map_err(|err| {
            anyhow!(
                "SimEditor remove targeted patch {} after ACK: {err}",
                patch_file.display()
            )
        })?;
        Ok(true)
    }

    /// Reload from disk after the controller wrote+committed the document model
    /// (Slice 4 broadcast-back): buffer == disk, clean.
    fn reload_from_disk(&mut self) -> Result<()> {
        let disk = std::fs::read_to_string(&self.path)
            .map_err(|err| anyhow!("SimEditor reload read {}: {err}", self.path.display()))?;
        self.buffer = disk;
        self.dirty = false;
        self.generation += 1;
        self.record_buffer()
    }

    /// Model an external disk write (agent-doc patchback) landing while the editor
    /// is open. Returns the kind-specific [`CacheConflict`] surfaced to the user.
    /// A clean buffer silently adopts the new disk content; a dirty buffer ahead
    /// of disk stays authoritative (no clobber) and the editor re-reports its
    /// still-unsaved buffer so the realtime feed keeps preferring it.
    fn external_disk_write(&mut self, content: &str) -> Result<CacheConflict> {
        std::fs::write(&self.path, content).map_err(|err| {
            anyhow!(
                "SimEditor external_disk_write {}: {err}",
                self.path.display()
            )
        })?;
        if !self.dirty {
            self.buffer = content.to_string();
            self.generation += 1;
            self.record_buffer()?;
            return Ok(CacheConflict::NoneAdopted);
        }
        // The plugin re-reports the still-dirty buffer when its VFS watch fires on
        // the external change, refreshing the sidecar timestamp so the buffer
        // stays provably ahead of the disk write.
        self.generation += 1;
        self.record_buffer()?;
        Ok(match self.kind {
            EditorKind::JetBrains => CacheConflict::JetBrainsDialog,
            // VS Code and the generic seam both keep the dirty buffer non-modally.
            EditorKind::VsCode | EditorKind::Generic => CacheConflict::VsCodeKeepBuffer,
        })
    }

    /// Resolve "current document" through the *production* realtime model
    /// (`resolve_current_doc`), the exact seam `preflight` / `write` /
    /// `session-check` source the current doc through.
    fn resolve(&self) -> Result<Reconciliation> {
        let disk = std::fs::read_to_string(&self.path)
            .map_err(|err| anyhow!("SimEditor resolve read {}: {err}", self.path.display()))?;
        Ok(agent_doc_orchestration::realtime_model::resolve_current_doc(&self.path, &disk))
    }

    fn record_buffer(&self) -> Result<()> {
        agent_doc_orchestration::debounce::record_live_buffer_digest_content_for_editor(
            &self.key,
            &self.buffer,
            Some(&self.editor_id),
        )
        .map_err(|err| anyhow!("SimEditor record live buffer: {err}"))
    }

    /// The pure [`BufferState`] this editor currently holds — what the plugin
    /// reports over IPC. Feeds the seam-isolated `reconcile_current_doc` primitive
    /// directly (vs the durable-feed `resolve`, which suppresses an in-sync buffer
    /// to `None` and so reports `editor_absent` rather than `in_sync`).
    fn buffer_state(&self) -> BufferState {
        BufferState::new(self.buffer.clone(), self.dirty, self.generation)
    }
}

/// Canonical live-buffer sidecar key for a document path. Matches
/// `realtime_model::indicator_path`: canonicalize, falling back to the raw path.
fn editor_buffer_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// A template document with one exchange prompt — the on-disk baseline a
/// `SimEditor` attaches to.
fn editor_baseline_doc() -> String {
    template_doc("❯ do #sim-existing. spec-test-build-install-commit-push\n")
}

/// [`editor_baseline_doc`] with an extra well-formed backlog line spliced in —
/// the shape of a queue/backlog item a user types in the editor without saving.
fn editor_doc_with_backlog(extra_line: &str) -> String {
    editor_baseline_doc().replace(
        "- [ ] [#tigersim] Implement the simulator MVP\n",
        &format!("- [ ] [#tigersim] Implement the simulator MVP\n{extra_line}"),
    )
}

/// Build a temp project with `.agent-doc/{snapshots,logs}` and the document on
/// disk. Returns the live `TempDir` (keep it in scope) and the document path.
fn editor_project(disk: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
    let doc = dir.path().join("doc.md");
    std::fs::write(&doc, disk).unwrap();
    (dir, doc)
}

/// Drive a finalize cycle on `baseline` (the resolved current document): capture
/// the agent response, apply it, and commit. Returns the committed `SimWorld` so
/// callers can assert the committed snapshot.
fn finalize_on_resolved(seed: u64, baseline: &str) -> SimWorld {
    let mut world = SimWorld::new(seed);
    world.doc = baseline.to_string();
    world.snapshot = baseline.to_string();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    world
}

// -------- Slice 1: SimEditor buffer actor (foundation) --------

#[test]
fn simeditor_unsaved_buffer_edit_resolves_to_editor_buffer_and_survives_commit() {
    // #swint Slice 1 acceptance — the deterministic form of the live-only
    // `#rtwverify` proof: an unsaved buffer edit is authoritative over stale disk
    // and survives the agent's commit, with the grep-able ops.log marker.
    let disk = editor_baseline_doc();
    let buffer = editor_doc_with_backlog(
        "- [ ] [#buffer-only-edit] user typed this in IDEA without saving\n",
    );
    let (dir, doc) = editor_project(&disk);

    let mut editor = SimEditor::generic(&doc).unwrap();
    // Clean buffer in sync with disk → disk authority.
    assert_eq!(editor.resolve().unwrap().authority, DocAuthority::Disk);

    editor.type_unsaved(&buffer).unwrap();
    let reconciliation = editor.resolve().unwrap();
    assert_eq!(
        reconciliation.authority,
        DocAuthority::EditorBuffer,
        "the production realtime model must read the unsaved buffer, not stale disk"
    );
    assert!(
        reconciliation.content.contains("#buffer-only-edit"),
        "resolve must surface the unsaved buffer edit"
    );
    assert_eq!(reconciliation.content, buffer);

    let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("realtime_doc_resolve authority=editor_buffer"),
        "ops.log must record the editor-buffer authority decision:\n{ops_log}"
    );

    // The agent's response commits on top of the buffer the cycle read, so the
    // unsaved edit crosses the commit boundary instead of being clobbered.
    let world = finalize_on_resolved(2026_06_11, &reconciliation.content);
    assert_eq!(world.phase, CyclePhase::Committed);
    assert!(
        world.snapshot.contains("#buffer-only-edit"),
        "the unsaved buffer edit must survive the agent's commit (no clobber)"
    );
    assert!(
        world.snapshot.contains("### Re: sim closeout"),
        "the agent response committed on top of the buffer content"
    );
    world.strict_closeout_invariants().unwrap();
    drop(dir);
}

#[test]
fn simeditor_save_then_close_falls_back_to_disk_authority() {
    // The "falling back to the file on disk" half of the authority model: once the
    // editor saves (buffer == disk) disk is canonical, and once it closes (sidecar
    // cleared) the realtime model reports `editor_absent`.
    let disk = editor_baseline_doc();
    let buffer = editor_doc_with_backlog("- [ ] [#unsaved-then-saved] typed then saved\n");
    let (dir, doc) = editor_project(&disk);

    let mut editor = SimEditor::generic(&doc).unwrap();
    editor.type_unsaved(&buffer).unwrap();
    assert_eq!(
        editor.resolve().unwrap().authority,
        DocAuthority::EditorBuffer
    );

    editor.save().unwrap();
    let disk_now = std::fs::read_to_string(&doc).unwrap();
    assert!(
        disk_now.contains("#unsaved-then-saved"),
        "save flushed to disk"
    );
    // Pure seam: a present, in-sync buffer is disk-canonical with reason `in_sync`.
    let in_sync = agent_doc_orchestration::realtime_model::reconcile_current_doc(
        &disk_now,
        Some(&editor.buffer_state()),
    );
    assert_eq!(in_sync.authority, DocAuthority::Disk);
    assert_eq!(in_sync.reason, "in_sync");
    // Durable seam: the in-sync buffer is suppressed to no-feed, so disk wins.
    assert_eq!(editor.resolve().unwrap().authority, DocAuthority::Disk);

    editor.close().unwrap();
    let closed = agent_doc_orchestration::realtime_model::resolve_current_doc(&doc, &disk_now);
    assert_eq!(closed.authority, DocAuthority::Disk);
    assert_eq!(
        closed.reason, "editor_absent",
        "closing the document must clear the live-buffer sidecar"
    );
    drop(dir);
}

// -------- Slice 2: JB + VS Code protocol parity fixtures --------

#[test]
fn simeditor_jb_and_vscode_buffer_authority_parity_with_kind_specific_conflict() {
    // #swint Slice 2 / File-Cache-Conflict class (#w42v): JetBrains and VS Code
    // must agree on read authority (a dirty buffer always wins) while differing
    // only on the surfaced cache-conflict signal for an external disk write.
    for kind in [EditorKind::JetBrains, EditorKind::VsCode] {
        let disk = editor_baseline_doc();
        let marker = format!("#{}-unsaved", kind.as_str());
        let buffer = editor_doc_with_backlog(&format!(
            "- [ ] [{marker}] typed in {} without saving\n",
            kind.as_str()
        ));
        let (dir, doc) = editor_project(&disk);
        let mut editor = SimEditor::attach(kind, &doc).unwrap();

        // Parity 1: a clean buffer defers to disk regardless of kind.
        assert_eq!(
            editor.resolve().unwrap().authority,
            DocAuthority::Disk,
            "{kind:?}: clean buffer defers to disk"
        );

        // Parity 2: an unsaved edit is authoritative regardless of kind.
        editor.type_unsaved(&buffer).unwrap();
        let r = editor.resolve().unwrap();
        assert_eq!(
            r.authority,
            DocAuthority::EditorBuffer,
            "{kind:?}: dirty buffer must win over disk"
        );
        assert!(r.content.contains(&marker));

        // Divergence: the cache-conflict signal for an external disk write while
        // the buffer is dirty is kind-specific (modal dialog vs non-modal badge).
        let conflict = editor.external_disk_write(&disk).unwrap();
        let expected = match kind {
            EditorKind::JetBrains => CacheConflict::JetBrainsDialog,
            EditorKind::VsCode => CacheConflict::VsCodeKeepBuffer,
            EditorKind::Generic => unreachable!("loop only covers JB + VS Code"),
        };
        assert_eq!(conflict, expected, "{kind:?}: cache-conflict signal");

        // Parity 3: despite the external write, the unsaved buffer is still
        // authoritative — the conflict never silently clobbers the user's edit.
        let after = editor.resolve().unwrap();
        assert_eq!(
            after.authority,
            DocAuthority::EditorBuffer,
            "{kind:?}: external write must not clobber the unsaved buffer"
        );
        assert!(after.content.contains(&marker));

        // A clean buffer, by contrast, silently adopts the external write.
        editor.save().unwrap();
        assert_eq!(
            editor.external_disk_write(&disk).unwrap(),
            CacheConflict::NoneAdopted,
            "{kind:?}: a clean buffer adopts the external write with no conflict"
        );
        assert_eq!(editor.resolve().unwrap().authority, DocAuthority::Disk);
        drop(dir);
    }
}

// -------- Slice 3: multi-editor sync (#rtwbcast harness) --------

#[test]
fn multi_editor_crdt_broadcast_converges_without_file_cache_conflict() {
    // #swint Slice 3 / #rtwbcast: two editors open the same document; an edit in A
    // and an edit in B merge conflict-free via the production CRDT path and
    // targeted broadcast patch delivery so both buffers converge. Only testable
    // with two emulated editors — there is no live two-IDE harness.
    let disk = editor_baseline_doc();
    let (dir, doc) = editor_project(&disk);
    std::fs::create_dir_all(dir.path().join(".agent-doc/patches")).unwrap();
    let mut editor_a = SimEditor::attach_with_id(EditorKind::JetBrains, &doc, "editor-A").unwrap();
    let mut editor_b = SimEditor::attach_with_id(EditorKind::VsCode, &doc, "editor-B").unwrap();

    let buffer_a = editor_doc_with_backlog("- [ ] [#edit-A] queued in editor A\n");
    let buffer_b = editor_doc_with_backlog("- [ ] [#edit-B] queued in editor B\n");
    editor_a.type_unsaved(&buffer_a).unwrap();
    editor_b.type_unsaved(&buffer_b).unwrap();

    // A's edit queues a targeted patch for B through the production broadcast
    // writer. A ignores the peer-targeted file; B applies and ACK-deletes it.
    let deliveries = agent_doc_orchestration::realtime_model::broadcast_editor_change(
        &doc, "editor-A", &buffer_a,
    )
    .unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].editor_id, "editor-B");
    assert!(
        !editor_a
            .apply_targeted_patch_file(&deliveries[0].patch_file)
            .unwrap()
    );
    assert_eq!(editor_a.buffer, buffer_a);
    assert!(
        editor_b
            .apply_targeted_patch_file(&deliveries[0].patch_file)
            .unwrap()
    );
    assert!(
        !deliveries[0].patch_file.exists(),
        "targeted patch file should be deleted after peer ACK"
    );
    let merged = editor_b.buffer.clone();

    assert!(
        merged.contains("#edit-A") && merged.contains("#edit-B"),
        "CRDT merge must union both editors' edits:\n{merged}"
    );
    for marker in ["<<<<<<<", "=======", ">>>>>>>"] {
        assert!(
            !merged.contains(marker),
            "CRDT broadcast must be conflict-free; found `{marker}`:\n{merged}"
        );
    }

    // B now rebroadcasts the converged buffer to A. Echo suppression again skips
    // the originator and targets only the stale peer buffer.
    let rebroadcast =
        agent_doc_orchestration::realtime_model::broadcast_editor_change(&doc, "editor-B", &merged)
            .unwrap();
    assert_eq!(rebroadcast.len(), 1);
    assert_eq!(rebroadcast[0].editor_id, "editor-A");
    assert!(
        !editor_b
            .apply_targeted_patch_file(&rebroadcast[0].patch_file)
            .unwrap()
    );
    assert!(
        editor_a
            .apply_targeted_patch_file(&rebroadcast[0].patch_file)
            .unwrap()
    );
    assert!(
        !rebroadcast[0].patch_file.exists(),
        "rebroadcast patch file should be deleted after peer ACK"
    );

    assert_eq!(editor_a.buffer, merged);
    assert_eq!(editor_b.buffer, merged);

    let ra = editor_a.resolve().unwrap();
    let rb = editor_b.resolve().unwrap();
    assert_eq!(ra.authority, DocAuthority::EditorBuffer);
    assert_eq!(rb.authority, DocAuthority::EditorBuffer);
    assert_eq!(
        ra.content, rb.content,
        "both editors converge on the same merged document after broadcast"
    );
    assert!(ra.content.contains("#edit-A") && ra.content.contains("#edit-B"));
    let disk_after = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        disk_after, disk,
        "broadcast convergence must not write disk"
    );
    let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("realtime_broadcast_queued")
            && ops_log.contains("origin_editor_id=editor-A target_editor_id=editor-B")
            && ops_log.contains("origin_editor_id=editor-B target_editor_id=editor-A"),
        "ops.log must prove targeted two-way broadcast delivery:\n{ops_log}"
    );
    drop(dir);
}

// -------- Slice 4: tmux + integrated system --------

#[test]
fn integrated_editor_edit_routes_drains_under_drain_owner_gate_and_broadcasts_back() {
    // #swint Slice 4: editor edit → queue trigger → route dispatch → drain-owner
    // gate (#kp5z) → controller drain → document update → broadcast back to
    // editors, with the stuck-handoff reaper gating ownership under multi-owner
    // contention. Connects the SimEditor seam (Slices 1–3) to the existing
    // route/controller actor model and the public `drain_owner` lease.
    let disk = editor_baseline_doc();
    let (dir, doc) = editor_project(&disk);
    let doc_key = doc.to_string_lossy().to_string();

    let mut owner_editor = SimEditor::jetbrains(&doc).unwrap();
    let mut observer_editor = SimEditor::vscode(&doc).unwrap();

    // 1. The user queues a follow-up by typing an unsaved queue item in the editor.
    let queued = editor_doc_with_backlog("- [ ] [#queued-followup] do the next step\n");
    owner_editor.type_unsaved(&queued).unwrap();
    let reconciliation = owner_editor.resolve().unwrap();
    assert_eq!(
        reconciliation.authority,
        DocAuthority::EditorBuffer,
        "the queued edit is authoritative over stale disk"
    );
    assert!(reconciliation.content.contains("#queued-followup"));

    // 2. The queue trigger routes to the owner pane and is accepted + proven.
    let mut world = SimWorld::new(2026_06_11 + 1);
    world.doc = reconciliation.content.clone();
    world.snapshot = reconciliation.content.clone();
    world.apply(SimCommand::SupervisorReady).unwrap();
    world.apply(SimCommand::DispatchRoutePrompt).unwrap();
    world.apply(SimCommand::ProveDispatchAccepted).unwrap();
    assert_eq!(
        world.coverage.route_dispatch_proofs, 1,
        "the queued trigger dispatched to the owner pane and proved acceptance"
    );

    // 3. Drain-owner gate (#kp5z): a self-driving loop owns the drain (fresh
    //    lease), so the supervisor must NOT double-inject (SkipSelfDrivingLoopOwner).
    agent_doc_orchestration::drain_owner::refresh_drain_owner_lease(
        &doc_key,
        agent_doc_orchestration::drain_owner::DRAIN_OWNER_CLAUDE_LOOP,
    )
    .unwrap();
    let lease = agent_doc_orchestration::drain_owner::read_drain_owner_lease(&doc_key)
        .expect("drain-owner lease present after refresh");
    assert!(
        agent_doc_orchestration::drain_owner::fresh_drain_owner_lease(
            &doc_key,
            lease.heartbeat_secs
        )
        .is_some(),
        "a fresh drain-owner lease must gate the supervisor drain to the loop owner"
    );

    // 4. The owning loop drains: apply the response and commit the document model.
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::Commit).unwrap();
    assert_eq!(world.phase, CyclePhase::Committed);
    assert!(
        world.snapshot.contains("#queued-followup"),
        "the editor-queued edit survived the route + drain"
    );

    // 5. Stuck-handoff reaper under multi-owner contention: a stale generation can
    //    neither hand off nor reap while the current owner holds the drain.
    world.apply(SimCommand::AdminHandoffStale).unwrap();
    assert_eq!(
        world.coverage.admin_handoffs, 0,
        "a stale-generation handoff must be rejected under contention"
    );
    world.apply(SimCommand::AdminReapStale).unwrap();
    assert_eq!(
        world.coverage.admin_reaps, 0,
        "a stale-generation reap must be rejected under contention"
    );
    assert!(world.coverage.stale_generation_blocks >= 2);

    // 6. The controller saved the committed document; broadcast back to both
    //    editors — they reload and converge on the committed state, clean.
    std::fs::write(&doc, &world.snapshot).unwrap();
    owner_editor.reload_from_disk().unwrap();
    observer_editor.reload_from_disk().unwrap();
    assert_eq!(owner_editor.buffer, world.snapshot);
    assert_eq!(observer_editor.buffer, world.snapshot);
    assert_eq!(
        owner_editor.resolve().unwrap().authority,
        DocAuthority::Disk,
        "after broadcast-back the owner editor is in sync with committed disk"
    );
    assert_eq!(
        observer_editor.resolve().unwrap().authority,
        DocAuthority::Disk,
        "the observer editor also converges on committed disk"
    );

    // The loop terminates: release the drain-owner lease back to the supervisor.
    agent_doc_orchestration::drain_owner::clear_drain_owner_lease(&doc_key);
    assert!(
        agent_doc_orchestration::drain_owner::read_drain_owner_lease(&doc_key).is_none(),
        "clearing the lease hands the drain back to the supervisor"
    );
    drop(dir);
}
