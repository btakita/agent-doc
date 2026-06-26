use super::*;

impl SimWorld {
    pub(crate) fn new(seed: u64) -> Self {
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
            recycle_clear: RecycleClearModel {
                // `#supselfheal`: turn-boundary blue/green self-recycle now defaults
                // ON in production (`resolve_supervisor_auto_recycle`), so the model's
                // baseline matches. Opt out via `DisableSupervisorAutoRecycle`.
                auto_recycle: true,
                // `#actorswitchdefer`: agent-change-restart defaults ON in production
                // (`resolve_agent_change_restart`). Opt out via
                // `DisableAgentChangeRestart`.
                agent_change_restart_enabled: true,
                ..RecycleClearModel::default()
            },
            sync: SyncProjection::default(),
            sync_guard: SyncGuardModel::default(),
            ops_log: Vec::new(),
            next_prompt: 1,
            coverage: Coverage::default(),
        }
    }

    pub(crate) fn run_seed(seed: u64, steps: usize) -> Result<Coverage> {
        let mut rng = DeterministicRng::new(seed);
        let mut world = Self::new(seed);
        for _ in 0..steps {
            let command = match rng.next_usize(60) {
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
                47 => SimCommand::SyncVisibleFocusPreserve,
                48 => SimCommand::AdminPauseQueue,
                49 => SimCommand::AdminPauseQueueStale,
                50 => SimCommand::AdminResumeQueue,
                51 => SimCommand::AdminDrainQueue,
                52 => SimCommand::AdminHandoff,
                53 => SimCommand::AdminHandoffStale,
                54 => SimCommand::AdminReap,
                55 => SimCommand::AdminReapStale,
                56 => SimCommand::SupervisorHeartbeatReattach,
                57 => SimCommand::PostCommitIpcRepositionSignal,
                58 => SimCommand::SyncFocusStashedMoveBeforeSelect,
                _ => SimCommand::SupervisorHeartbeatStale,
            };
            world.apply(command)?;
            world.assert_structural_invariants()?;
        }
        if let Err(err) = world.strict_closeout_invariants() {
            world.coverage.record_block(&err.to_string());
        }
        Ok(world.coverage)
    }

    pub(crate) fn run_seed_corpus(seeds: std::ops::Range<u64>, steps: usize) -> Result<CorpusRun> {
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

    pub(crate) fn apply(&mut self, command: SimCommand) -> Result<()> {
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
            SimCommand::PostCommitIpcRepositionSignal => {
                // `#postcommit-ipc-worktree-corruption` regression guard. After a
                // committed closeout (snapshot == HEAD == working tree), the live
                // IPC listener fires a post-commit boundary-reposition signal at
                // the working tree (`IPC reposition boundary signal sent`). The
                // bug class is that signal rewriting the working tree with a
                // stale/spliced buffer so the visible file drifts from HEAD. The
                // production-correct behavior is idempotent: repositioning an
                // already-clean committed boundary must not mutate the tree.
                // `assert_post_commit_reposition_idempotent` re-applies the
                // production reposition to the working tree (`self.doc`) without
                // touching HEAD (`self.snapshot`) and enforces tree == HEAD; a
                // reposition change that mutated an already-clean committed doc
                // would drift the tree and fail closed — exactly the corruption we
                // want to catch offline.
                if self.assert_post_commit_reposition_idempotent()? {
                    self.coverage.post_commit_worktree_checks += 1;
                }
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
            SimCommand::DispatchOperatorPrompt => match self.dispatch_route_prompt_with(true) {
                Ok(()) => {
                    self.recycle_clear.jb_run_recycle_probe_pending = true;
                    self.record_ops_proof(
                        "suprehotreload_jb_cycle action=jb_run_agent_doc result=dispatch_accepted",
                    );
                }
                Err(err) => self.coverage.record_block(&err.to_string()),
            },
            SimCommand::DispatchOperatorPromptWithProtectedDraft => {
                if let Err(err) = self.dispatch_operator_prompt_with_protected_draft() {
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
            SimCommand::DispatchAutoStartRoutePrompt => {
                if let Err(err) = self.dispatch_auto_start_route_prompt() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::DispatchIdleQueueDrainAfterRestart => {
                if let Err(err) = self.dispatch_idle_queue_drain_after_restart() {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::MarkSupervisorRecycleInflight => {
                self.route.recycle_inflight = true;
            }
            SimCommand::SettleSupervisorRecycle => {
                self.route.recycle_inflight = false;
            }
            SimCommand::DispatchDuringSupervisorRecycle => {
                if let Err(err) = self.dispatch_during_supervisor_recycle() {
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
            SimCommand::AdminPauseQueue => {
                if let Err(err) = self
                    .admin_queue_control(QueueControlState::Paused, self.route.durable.generation)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::AdminPauseQueueStale => {
                let stale_generation = self.route.durable.generation.saturating_sub(1);
                if let Err(err) =
                    self.admin_queue_control(QueueControlState::Paused, stale_generation)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::AdminResumeQueue => {
                if let Err(err) = self
                    .admin_queue_control(QueueControlState::Resumed, self.route.durable.generation)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::AdminDrainQueue => {
                if let Err(err) = self
                    .admin_queue_control(QueueControlState::Draining, self.route.durable.generation)
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::AdminHandoff => {
                if let Err(err) = self.admin_handoff(
                    self.route.durable.generation,
                    format!("%handoff{}", self.route.durable.generation + 1),
                ) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::AdminHandoffStale => {
                let stale_generation = self.route.durable.generation.saturating_sub(1);
                if let Err(err) =
                    self.admin_handoff(stale_generation, "%stale-admin-handoff".to_string())
                {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::AdminReap => {
                if let Err(err) = self.admin_reap(self.route.durable.generation) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::AdminReapStale => {
                let stale_generation = self.route.durable.generation.saturating_sub(1);
                if let Err(err) = self.admin_reap(stale_generation) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SupervisorHeartbeatReattach => {
                if let Err(err) = self.supervisor_heartbeat_reattach(
                    self.route.durable.generation,
                    format!(
                        "%heartbeat{}",
                        self.route.durable.generation.saturating_add(1)
                    ),
                ) {
                    self.coverage.record_block(&err.to_string());
                }
            }
            SimCommand::SupervisorHeartbeatStale => {
                let stale_generation = self.route.durable.generation.saturating_sub(1);
                if let Err(err) =
                    self.supervisor_heartbeat_reattach(stale_generation, "%stale-heartbeat")
                {
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
            SimCommand::SyncFocusStashedMoveBeforeSelect => {
                self.sync = SyncProjection::stashed_focus_case();
                self.sync.focus_doc_move_before_select("requested");
                self.coverage.sync_move_before_select_focuses += 1;
            }
            SimCommand::StartBlockingSyncAfterKillPane => {
                self.start_blocking_sync_after_kill_pane();
            }
            SimCommand::AdvanceSyncGuardBeyondBound => {
                self.advance_sync_guard_beyond_bound();
            }
            SimCommand::SyncLayoutClick => {
                self.sync_layout_click();
            }
            SimCommand::FinishSyncLayout => {
                self.finish_sync_layout();
            }
            SimCommand::ActivateGoModeQueueHead => {
                self.recycle_clear.queue_active_head = Some("#govqueuehead".to_string());
            }
            SimCommand::ActivateFocusedCycleQueueHead => {
                self.recycle_clear.queue_active_head = Some("do [#qfocsup-focused]".to_string());
            }
            SimCommand::SessionCheckFocusedCycleDeferredForSupervisorDrain => {
                self.session_check_focused_cycle_deferred_for_supervisor_drain()?;
            }
            SimCommand::FreshAgentDrainActiveHead => {
                self.fresh_agent_drain_active_head()?;
            }
            SimCommand::MarkSupervisorBinaryStale => {
                self.recycle_clear.binary_stale = true;
            }
            SimCommand::SetAgentDocCycleOpen(open) => {
                self.recycle_clear.cycle_open = open;
            }
            SimCommand::MarkWriteWedged => {
                self.recycle_clear.write_wedged = true;
            }
            SimCommand::MarkReexecWillFail => {
                self.recycle_clear.reexec_will_fail = true;
            }
            SimCommand::EnableSupervisorAutoRecycle => {
                self.recycle_clear.auto_recycle = true;
            }
            SimCommand::DisableSupervisorAutoRecycle => {
                self.recycle_clear.auto_recycle = false;
            }
            SimCommand::OperatorRecycleMark => {
                self.recycle_clear.operator_recycle_marked = true;
            }
            SimCommand::DeferOperatorClearPending => {
                self.recycle_clear.deferred_operator_clear_pending = true;
                self.recycle_clear.clear_cooldown_active = true;
                self.recycle_clear.clear_cooldown_idle_ticks = 0;
                self.record_ops_proof(
                    "recycle_session_reclear action=deferred_clear result=queued",
                );
            }
            SimCommand::SupervisorIdleQueueTick => {
                self.supervisor_idle_queue_tick()?;
            }
            SimCommand::SupervisorContextResetClear => {
                self.supervisor_context_reset_clear();
            }
            SimCommand::SetTriggerAlreadyPending(pending) => {
                self.set_trigger_already_pending(pending);
            }
            SimCommand::QueueBetweenTurnFreshContextHandoff => {
                self.queue_between_turn_fresh_context_handoff();
            }
            SimCommand::RequestSupervisorRestart => {
                self.recycle_clear.restart_requested = true;
            }
            SimCommand::SwitchFrontmatterHarness { from, to } => {
                self.switch_frontmatter_harness(from, to);
            }
            SimCommand::DispatchRouteAfterHarnessSwitch => {
                self.dispatch_route_after_harness_switch();
            }
            SimCommand::DisableAgentChangeRestart => {
                self.recycle_clear.agent_change_restart_enabled = false;
            }
            SimCommand::SupervisorHarnessSwitchTick => {
                self.supervisor_harness_switch_tick();
            }
            SimCommand::PerformDeferredHarnessRestart => {
                self.perform_deferred_harness_restart();
            }
            SimCommand::AbandonSupervisorToDeadSocket => {
                self.abandon_supervisor_to_dead_socket();
            }
            SimCommand::RecoverDeadSupervisor {
                caller_is_own_ancestor,
                can_resolve_tmux_target,
            } => {
                self.recover_dead_supervisor(caller_is_own_ancestor, can_resolve_tmux_target);
            }
        }
        Ok(())
    }

    pub(crate) fn record_sync_outcome(&mut self, outcome: SyncOutcome) {
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

    fn record_ops_proof(&mut self, marker: impl Into<String>) {
        self.ops_log.push(marker.into());
    }

    fn queue_between_turn_fresh_context_handoff(&mut self) {
        self.recycle_clear
            .between_turn_enqueue
            .push("/clear".to_string());
        self.recycle_clear
            .between_turn_enqueue
            .push("agent-doc sim.md".to_string());
    }

    fn maybe_deliver_between_turn_enqueue(
        &mut self,
        prompt_visible: bool,
        turn_active: bool,
    ) -> bool {
        if self.recycle_clear.between_turn_enqueue.is_empty() {
            return false;
        }
        if !prompt_visible || turn_active {
            self.coverage.between_turn_enqueue_busy_skips += 1;
            return true;
        }
        let requested = self
            .recycle_clear
            .between_turn_enqueue
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let plan = agent_doc_orchestration::start::decisions::between_turn_enqueue_plan(
            requested,
            "/clear",
            "agent-doc sim.md",
        );
        self.coverage.between_turn_enqueue_deliveries += 1;
        self.coverage.between_turn_enqueue_deduped += plan.deduped;
        self.record_ops_proof(format!(
            "between_turn_enqueue deduped={} kept={} result=delivered",
            plan.deduped,
            plan.kept_labels()
        ));
        self.recycle_clear.between_turn_enqueue.clear();
        true
    }

    pub(crate) fn start_blocking_sync_after_kill_pane(&mut self) {
        self.sync_guard.killed_pane_path = true;
        self.sync_guard.locked = true;
        self.sync_guard.acquired_at_ms = self.sync_guard.now_ms.max(1);
        self.coverage.sync_kill_pane_proofs += 1;
        self.record_ops_proof(format!(
            "sync_kill_pane_path pane_killed=true guard=held acquired_at_ms={}",
            self.sync_guard.acquired_at_ms
        ));
    }

    pub(crate) fn advance_sync_guard_beyond_bound(&mut self) {
        self.sync_guard.now_ms = self
            .sync_guard
            .acquired_at_ms
            .saturating_add(self.sync_guard.stale_bound_ms);
    }

    pub(crate) fn sync_layout_click(&mut self) {
        let decision = agent_doc::ffi::sync_lock_acquire_decision(
            self.sync_guard.locked,
            self.sync_guard.acquired_at_ms,
            self.sync_guard.now_ms,
            self.sync_guard.stale_bound_ms,
        );
        match decision {
            agent_doc::ffi::SyncLockDecision::Acquire => {
                self.sync_guard.locked = true;
                self.sync_guard.acquired_at_ms = self.sync_guard.now_ms;
                self.record_ops_proof(format!(
                    "sync_guard_acquired reason=free_guard at_ms={}",
                    self.sync_guard.now_ms
                ));
            }
            agent_doc::ffi::SyncLockDecision::SupersedeStaleHolder { held_ms } => {
                self.sync_guard.locked = true;
                self.sync_guard.acquired_at_ms = self.sync_guard.now_ms;
                self.coverage.sync_guard_stale_releases += 1;
                self.record_ops_proof(format!(
                    "sync_guard_released reason=stale_holder_superseded held_ms={held_ms} bound_ms={}",
                    self.sync_guard.stale_bound_ms
                ));
            }
            agent_doc::ffi::SyncLockDecision::Defer => {
                self.coverage.sync_guard_defers += 1;
                self.record_ops_proof(format!(
                    "sync_guard_deferred reason=fresh_holder held_ms={}",
                    self.sync_guard
                        .now_ms
                        .saturating_sub(self.sync_guard.acquired_at_ms)
                ));
            }
        }
    }

    pub(crate) fn finish_sync_layout(&mut self) {
        if self.sync_guard.locked {
            self.sync_guard.locked = false;
            self.sync_guard.acquired_at_ms = 0;
            self.coverage.sync_guard_completions += 1;
            self.record_ops_proof("sync_guard_released reason=sync_complete");
        }
    }

    pub(crate) fn apply_sync_protected_growth(&mut self, mode: SyncMode) {
        self.sync = SyncProjection::protected_growth_case();
        let outcome =
            self.sync
                .apply_requested_projection(&["requested", "sibling"], "requested", mode);
        self.record_sync_outcome(outcome);
    }

    pub(crate) fn apply_sync_protected_growth_focus_visible(&mut self) {
        self.sync = SyncProjection::protected_growth_case();
        self.sync.active = Some("protected".to_string());
        let outcome = self.sync.apply_requested_projection(
            &["requested", "sibling"],
            "sibling",
            SyncMode::SafePassive,
        );
        self.record_sync_outcome(outcome);
    }

    pub(crate) fn apply_sync_detachable_replace(&mut self, mode: SyncMode) {
        self.sync = SyncProjection::detachable_replacement_case();
        let outcome = self
            .sync
            .apply_requested_projection(&["requested"], "requested", mode);
        self.record_sync_outcome(outcome);
    }

    pub(crate) fn apply_sync_visible_focus_preserve(&mut self, mode: SyncMode) {
        let _ = mode;
        self.sync = SyncProjection::protected_growth_case();
        self.sync.active = Some("sibling".to_string());
        self.record_sync_outcome(SyncOutcome::PreservedLayoutAndFocused);
    }

    pub(crate) fn apply_sync_rerequest_visible_editor(&mut self, mode: SyncMode) {
        self.sync = SyncProjection::rerequested_visible_editor_case();
        // Re-requesting a document that is already visible must be a no-op for
        // pane cardinality: the editor stays a single pane. Attaching a second
        // pane here is the duplicate-editor-pane regression.
        let outcome = self
            .sync
            .apply_requested_projection(&["editor"], "editor", mode);
        self.record_sync_outcome(outcome);
    }

    pub(crate) fn bind_route_owner(&mut self) {
        let generation = self.route.durable.generation + 1;
        self.route.durable = ActorState {
            generation,
            session_id: format!("session-{generation}"),
            pane_id: Some(format!("%{generation}")),
            lifecycle: SupervisorLifecycle::Starting,
        };
        self.route.projection = self.route.durable.clone();
        self.route.pending_dispatch = None;
        self.route.supervisor_lease_generation = Some(generation);
        // A freshly bound route owner is a live process that binds a live socket.
        self.route.socket = SupervisorSocket::Live;
        self.coverage.route_generation_rebinds += 1;
    }

    /// `#supdead-coldstart-fallback`: the supervisor PROCESS dies abruptly (crash /
    /// OOM / host reboot) WITHOUT an orderly shutdown, so the AF_UNIX socket file is
    /// left behind on disk and a later `connect()` is actively refused
    /// (`ECONNREFUSED`). Distinct from an orderly `Closed` (which reaps its socket):
    /// the actor goes `Dead` and the socket becomes `StaleRefused` ("present but not
    /// accepting"). The lease is dropped (no live heartbeat) and any in-flight
    /// dispatch is abandoned with the process.
    pub(crate) fn abandon_supervisor_to_dead_socket(&mut self) {
        let projection_was_current = self.projection_identity_matches_durable();
        self.route.durable.lifecycle = SupervisorLifecycle::Dead;
        if projection_was_current {
            self.route.projection.lifecycle = SupervisorLifecycle::Dead;
        }
        self.route.socket = SupervisorSocket::StaleRefused;
        self.route.pending_dispatch = None;
        self.route.starting_timeout = None;
        self.route.supervisor_lease_generation = None;
        self.coverage.supervisor_deaths += 1;
        self.record_ops_proof(format!(
            "supervisor_socket_probe generation={} result=stale_refused note=present_but_not_accepting",
            self.route.durable.generation
        ));
    }

    /// `#supdead-coldstart-fallback`: drive the REAL production decision
    /// (`session_actor_cmd::decide_dead_supervisor_recovery`) for a dead supervisor
    /// and apply its outcome to the model. `socket_dead` is derived from the modeled
    /// socket's production [`SocketLiveness`] mapping rather than re-deciding it in
    /// the engine, so the SimWorld scenario exercises the shipped decision code.
    ///
    /// - `ColdStart` → reap the stale socket and cold-start a fresh route-owned
    ///   supervisor (mirrors `cold_start_supervisor_for` + route `auto_start`).
    /// - `Guidance(msg)` → refuse the unsafe cold-start; the supervisor stays `Dead`
    ///   and the actionable message is surfaced (never a raw ECONNREFUSED).
    /// Returns the production decision so callers/tests can assert on it directly.
    pub(crate) fn recover_dead_supervisor(
        &mut self,
        caller_is_own_ancestor: bool,
        can_resolve_tmux_target: bool,
    ) -> crate::session_actor_cmd::DeadSupervisorRecovery {
        use crate::session_actor_cmd::{
            DeadSupervisorRecovery, DeadSupervisorRecoveryInputs, decide_dead_supervisor_recovery,
        };
        let decision = decide_dead_supervisor_recovery(
            Path::new("sim.md"),
            Path::new(".agent-doc/supervisor/sim.sock"),
            &DeadSupervisorRecoveryInputs {
                socket_dead: self.route.socket.is_dead(),
                caller_is_own_ancestor,
                can_resolve_tmux_target,
            },
        );
        match &decision {
            DeadSupervisorRecovery::InPlaceLive => {
                // The socket probed live — not a dead-supervisor scenario; leave the
                // model untouched and let the normal in-place restart/recycle apply.
                self.record_ops_proof(
                    "dead_supervisor_recovery decision=InPlaceLive action=in_place_restart",
                );
            }
            DeadSupervisorRecovery::ColdStart => {
                self.cold_start_dead_supervisor();
            }
            DeadSupervisorRecovery::Guidance(message) => {
                // Refuse the unsafe cold-start: the supervisor stays Dead and the
                // stale socket lingers; surface the actionable guidance message. The
                // raw ECONNREFUSED is deliberately NOT surfaced.
                debug_assert!(
                    !message.contains("Connection refused") && !message.contains("os error 111"),
                    "dead-supervisor guidance must not surface a raw ECONNREFUSED"
                );
                self.coverage.dead_supervisor_guidance_refusals += 1;
                self.record_ops_proof(format!(
                    "dead_supervisor_recovery decision=Guidance action=refuse_unsafe_cold_start message={message:?}"
                ));
            }
        }
        decision
    }

    /// `#supdead-coldstart-fallback`: reap the stale dead socket and cold-start a
    /// fresh route-owned supervisor (mirrors `cold_start_supervisor_for` +
    /// `route::startup::auto_start`). The fresh process binds a new live socket and
    /// comes up `Starting`; a later dispatch-ready prompt promotes it to `Ready`.
    fn cold_start_dead_supervisor(&mut self) {
        // Reap the stale socket so the fresh supervisor can bind cleanly.
        self.route.socket = SupervisorSocket::Absent;
        self.record_ops_proof(
            "supervisor_cold_start_reaped_stale_socket file=sim.md socket=.agent-doc/supervisor/sim.sock",
        );
        // Cold-start a fresh route-owned supervisor at a new generation.
        let generation = self.route.durable.generation + 1;
        self.route.durable = ActorState {
            generation,
            session_id: format!("session-{generation}"),
            pane_id: Some(format!("%{generation}")),
            lifecycle: SupervisorLifecycle::Starting,
        };
        self.route.projection = self.route.durable.clone();
        self.route.pending_dispatch = None;
        self.route.starting_timeout = None;
        self.route.supervisor_lease_generation = Some(generation);
        // The fresh process binds a new live socket as it starts.
        self.route.socket = SupervisorSocket::Live;
        self.coverage.dead_supervisor_cold_starts += 1;
        self.record_ops_proof(format!(
            "supervisor_cold_start decision=ColdStart action=route_auto_start generation={generation}"
        ));
    }

    /// `#actorswitchdefer`: model the operator editing the document frontmatter
    /// `agent:` from `from` to `to` while a live actor of the OLD harness owns the
    /// pane. Records the launch harness (the authoritative actor's stored harness)
    /// and the new frontmatter-resolved harness; the live actor stays Ready (a
    /// healthy old-harness authority that route must NOT replace).
    fn switch_frontmatter_harness(&mut self, from: &str, to: &str) {
        self.recycle_clear.launch_harness = from.to_string();
        self.recycle_clear.frontmatter_harness = to.to_string();
    }

    /// `#actorswitchdefer`: model a `route` dispatch (JB `Run Agent Doc`) landing
    /// while the frontmatter harness no longer matches the live actor's harness.
    ///
    /// Mirrors `route/authoritative_actor.rs::load_authoritative_actor_binding`:
    /// - The replace-or-defer guard `mismatched_authoritative_actor_can_be_replaced`
    ///   returns `false` for a healthy, non-closed old-harness actor — so route
    ///   DEFERS (does not replace the live pane).
    /// - When `agent_change_restart` is DISABLED the defer would never self-heal, so
    ///   route bails EXPLICITLY (`action=bail_restart_disabled`) rather than handing
    ///   back a `restart-supervisor` recovery hint that will not switch harnesses.
    /// - Otherwise it bails with the operator-actionable defer message, carrying the
    ///   paused/stuck `restart-supervisor` recovery suffix when the boundary restart
    ///   cannot fire yet (no silent drop of the switch).
    fn dispatch_route_after_harness_switch(&mut self) {
        let pending_switch = !self.recycle_clear.frontmatter_harness.is_empty()
            && self.recycle_clear.frontmatter_harness != self.recycle_clear.launch_harness;
        if !pending_switch {
            return;
        }
        // The live old-harness actor's health/state. Mirrors the production
        // `mismatched_authoritative_actor_can_be_replaced` rule: replaceable ONLY
        // when the supervisor is unhealthy OR the actor is Closed. A healthy
        // Ready/Busy/WaitingInput old-harness actor is still authoritative → defer.
        let actor_closed =
            matches!(self.route.durable.lifecycle, SupervisorLifecycle::Closed);
        let supervisor_unhealthy = matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Starting
        ) && self.route.supervisor_lease_generation.is_none();
        let can_replace = actor_closed || supervisor_unhealthy;
        if can_replace {
            // Not the deferred path: a stale/closed actor is replaced by the normal
            // create/rebind path. Out of scope for this regression.
            return;
        }

        let old = self.recycle_clear.launch_harness.clone();
        let new = self.recycle_clear.frontmatter_harness.clone();
        // The switch is DEFERRED, never a live-pane replacement.
        self.coverage.actor_switch_route_defers += 1;
        self.coverage.route_dispatch_acceptances += 0; // no dispatch acceptance on a defer

        if !self.recycle_clear.agent_change_restart_enabled {
            // Part B explicit disabled-bail: the defer would never self-heal.
            self.coverage.actor_switch_restart_disabled_bails += 1;
            self.record_ops_proof(format!(
                "route_authoritative_actor_harness_mismatch_deferred stored_harness={old} expected_harness={new} agent_change_restart=disabled action=bail_restart_disabled"
            ));
            return;
        }

        // The pane is paused / not at a dispatch-ready boundary while a turn is in
        // flight or the queue is paused — the boundary restart cannot fire yet, so
        // the bail must carry the `restart-supervisor` recovery suffix and the switch
        // must be held pending (no silent drop).
        let queue_paused =
            matches!(self.route.queue_control, QueueControlState::Paused);
        let turn_active = matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Busy | SupervisorLifecycle::WaitingInput
        );
        if queue_paused || turn_active {
            self.coverage.actor_switch_defer_bail_recoveries += 1;
            self.record_ops_proof(format!(
                "route_authoritative_actor_harness_mismatch_deferred stored_harness={old} expected_harness={new} queue_paused={queue_paused} action=defer_to_boundary_restart recovery=agent-doc_session_restart-supervisor"
            ));
        } else {
            self.record_ops_proof(format!(
                "route_authoritative_actor_harness_mismatch_deferred stored_harness={old} expected_harness={new} action=defer_to_boundary_restart recovery=agent-doc_session_restart-supervisor"
            ));
        }
    }

    /// `#actorswitchdefer`: one supervisor idle-watch tick for a pending harness
    /// switch. Drives the production `agent_change_restart_decision` boundary gate
    /// (`#agentreloadrestart`) and emits the SAME ops-log markers `idle_watch.rs`
    /// does: `harness_change_detected` on any detected change, and
    /// `agent_restart_triggered` only when the decision is `Restart` (knob on + a
    /// quiet dispatch-ready boundary). A paused/stuck pane stays at
    /// `WaitForBoundary` — detection still fires, the trigger does NOT, and the
    /// switch is held pending (no silent drop).
    fn supervisor_harness_switch_tick(&mut self) {
        use agent_doc_orchestration::start::decisions::{
            AgentChangeRestartAction, agent_change_restart_decision,
        };
        let pending_switch = !self.recycle_clear.frontmatter_harness.is_empty()
            && self.recycle_clear.frontmatter_harness != self.recycle_clear.launch_harness;
        if !pending_switch {
            return;
        }
        let prompt_visible =
            matches!(self.route.durable.lifecycle, SupervisorLifecycle::Ready);
        let turn_active = matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Busy | SupervisorLifecycle::WaitingInput
        );
        // A paused queue means the supervisor idle-watch does not tick the drain
        // boundary; model it as "no dispatch-ready boundary reached".
        let queue_paused =
            matches!(self.route.queue_control, QueueControlState::Paused);
        let boundary_prompt_visible = prompt_visible && !queue_paused;

        let decision = agent_change_restart_decision(
            true,
            self.recycle_clear.agent_change_restart_enabled,
            boundary_prompt_visible,
            turn_active,
        );

        let old = self.recycle_clear.launch_harness.clone();
        let new = self.recycle_clear.frontmatter_harness.clone();
        // Detection fires on every observed change (deduped in production per new
        // harness; the model counts each tick that observes the change for the
        // regression assertion that detection is never silently dropped).
        self.coverage.actor_switch_changes_detected += 1;
        self.record_ops_proof(format!(
            "harness_change_detected old={old} new={new} gate={decision:?}"
        ));

        match decision {
            AgentChangeRestartAction::Restart => {
                // Request a FRESH restart (no resume args) at the quiet boundary.
                self.recycle_clear.restart_requested = true;
                self.coverage.actor_switch_restarts_triggered += 1;
                self.record_ops_proof(format!(
                    "agent_restart_triggered old={old} new={new} action=request_fresh_restart"
                ));
            }
            AgentChangeRestartAction::WaitForBoundary => {
                // The switch is HELD pending (a later quiet boundary fires it). No
                // silent drop — `frontmatter_harness` stays set.
            }
            AgentChangeRestartAction::None => {
                // Knob off: the idle-watch never restarts. The route disabled-bail
                // owns the operator guidance in that state.
            }
        }
    }

    /// `#actorswitchdefer`: the supervisor restart loop respawned the new harness
    /// FRESH after the deferred switch was triggered. Re-resolves the harness from
    /// the current frontmatter, rebinds the actor to the new harness, and emits
    /// `agent_restart_performed`, completing the switch.
    fn perform_deferred_harness_restart(&mut self) {
        if !self.recycle_clear.restart_requested
            || self.recycle_clear.frontmatter_harness.is_empty()
        {
            return;
        }
        let old = self.recycle_clear.launch_harness.clone();
        let new = self.recycle_clear.frontmatter_harness.clone();
        self.recycle_clear.restart_requested = false;
        // The supervisor now runs the new harness: launch == frontmatter.
        self.recycle_clear.launch_harness = new.clone();
        // Fresh restart: a new generation/pane comes up Starting then settles Ready.
        self.bind_route_owner();
        self.route.durable.lifecycle = SupervisorLifecycle::Ready;
        self.route.projection = self.route.durable.clone();
        self.coverage.actor_switch_restarts_performed += 1;
        self.record_ops_proof(format!(
            "agent_restart_performed old_harness={old} new_harness={new} action=spawn_fresh_harness"
        ));
    }

    pub(crate) fn clear_session_context(&mut self) -> Result<()> {
        self.current_dispatch_pane()?;
        self.route.pending_dispatch = None;
        if self.recycle_clear.deferred_operator_clear_pending {
            self.coverage.deferred_clear_duplicate_suppressed += 1;
            self.record_ops_proof(
                "recycle_session_reclear action=session_clear_duplicate result=already_deferred",
            );
            return Ok(());
        }
        let turn_active = matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Busy | SupervisorLifecycle::WaitingInput
        );
        if turn_active && self.recycle_clear.queue_active_head.is_some() {
            self.recycle_clear.deferred_operator_clear_pending = true;
            self.recycle_clear.clear_cooldown_active = true;
            self.recycle_clear.clear_cooldown_idle_ticks = 0;
            self.coverage.session_clears += 1;
            self.coverage.recycle_session_reclear_proofs += 1;
            self.record_ops_proof(
                "recycle_session_reclear action=session_clear result=deferred_until_idle",
            );
            return Ok(());
        }
        // `#clearcontresume`: an operator `session clear` / JB `Clear Exchange`
        // writes the manual clear cooldown (`queue_continuation::write_clear_cooldown`).
        // It suppresses passive queue dispatch until the cleared pane settles to a
        // fresh idle prompt — at which point `clear_cooldown_resume_ready` lets an
        // active go-mode drain resume as a continuation step. Reset the idle-tick
        // debounce so the resume counts only polls observed AFTER this clear.
        self.recycle_clear.clear_cooldown_active = true;
        self.recycle_clear.clear_cooldown_idle_ticks = 0;
        self.coverage.session_clears += 1;
        self.coverage.recycle_session_reclear_proofs += 1;
        self.record_ops_proof(
            "recycle_session_reclear action=session_clear result=cooldown_started",
        );
        Ok(())
    }

    /// `#qflood2`: model the idle-queue watch sending its OWN `/clear` between
    /// queue items (an opt-in context reset or a `/clear` queue head). Unlike an
    /// operator clear this never writes the manual cooldown marker, so the next
    /// drain trigger must be held by the in-memory settle gate
    /// (`drain_blocked_awaiting_clear_settle`) until the cleared pane shows a
    /// fresh idle prompt for `CLEAR_COOLDOWN_RESUME_IDLE_TICKS` consecutive polls.
    pub(crate) fn supervisor_context_reset_clear(&mut self) {
        self.recycle_clear.awaiting_clear_settle = true;
        self.recycle_clear.clear_settle_idle_ticks = 0;
        self.recycle_clear.last_dispatched = None;
        if let Some(head) = self.recycle_clear.queue_active_head.clone() {
            self.coverage.focused_cycle_context_resets += 1;
            self.record_ops_proof(format!(
                "idle_queue_watch_context_reset head_bytes={} head_sha256={} reason=\"active queue head is a [focused-cycle] item - clearing to continue in a fresh agent context (#qfocsup)\"",
                head.len(),
                agent_doc_orchestration::ops_log::content_hash(&head)
            ));
        }
    }

    /// `#qflood2`: model that the routed trigger is (or is not) already pending in
    /// the harness composer — the live pane-capture dedup signal.
    pub(crate) fn set_trigger_already_pending(&mut self, pending: bool) {
        self.recycle_clear.trigger_already_pending = pending;
    }

    /// One supervisor idle-queue-watch poll, driving the SAME production decision
    /// predicates the live `start::idle_watch` loop uses (`#clearcontresume`):
    /// the clear-cooldown idle-tick debounce + `clear_cooldown_resume_ready`, the
    /// `supervisor_recycle_action` recycle policy, and `idle_queue_drain_decision`.
    /// No live pane — `prompt_visible` / `turn_active` are derived from the modeled
    /// supervisor lifecycle, exactly the offline simulation the operator asked for.
    pub(crate) fn supervisor_idle_queue_tick(&mut self) -> Result<()> {
        use agent_doc_orchestration::start::decisions::{
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS, IdleQueueDrainDecision, SupervisorRecycleAction,
            SupervisorRestartAction, clear_cooldown_resume_ready,
            drain_blocked_awaiting_clear_settle, drain_dispatch_dedup_skip,
            idle_queue_drain_decision, supervisor_recycle_action, supervisor_restart_action,
        };

        // The supervisor's idle signal: a dispatch-ready harness prompt is visible
        // only when the actor is `Ready`; a turn is active while `Busy`/`WaitingInput`.
        let prompt_visible = matches!(self.route.durable.lifecycle, SupervisorLifecycle::Ready);
        let turn_active = matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Busy | SupervisorLifecycle::WaitingInput
        );
        let has_active_head = self.recycle_clear.queue_active_head.is_some();

        // (1) Clear-cooldown idle-tick accounting — mirrors idle_watch.rs:157-161.
        if self.recycle_clear.clear_cooldown_active
            && has_active_head
            && prompt_visible
            && !turn_active
        {
            self.recycle_clear.clear_cooldown_idle_ticks = self
                .recycle_clear
                .clear_cooldown_idle_ticks
                .saturating_add(1);
        } else {
            self.recycle_clear.clear_cooldown_idle_ticks = 0;
        }

        // (2) Production resume predicate. When it fires the cooldown has served
        // its only job (don't dispatch into an in-flight `/clear`); drop it so the
        // go-mode drain resumes as a continuation step.
        if clear_cooldown_resume_ready(
            self.recycle_clear.clear_cooldown_active,
            has_active_head,
            prompt_visible,
            turn_active,
            self.recycle_clear.deferred_operator_clear_pending,
            self.recycle_clear.clear_cooldown_idle_ticks,
            CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
        ) {
            self.recycle_clear.clear_cooldown_active = false;
            self.recycle_clear.clear_cooldown_idle_ticks = 0;
            self.recycle_clear.last_dispatched = None;
            self.coverage.clear_cooldown_resumes += 1;
            self.record_ops_proof(
                "recycle_session_reclear action=clear_cooldown_resume result=ready",
            );
            // Production `continue`s (idle_watch.rs:204) so the normal drain
            // decision dispatches the head on the NEXT tick with the cooldown
            // already cleared. Return early to model that re-evaluation boundary.
            return Ok(());
        }

        let turn_boundary = prompt_visible && !turn_active;
        let head_pending = has_active_head;

        // (2b) `#supkill-bg` blue/green drain-and-supersede restart. An explicit
        // `restart-supervisor` (IPC `Restart`) drives the production
        // `supervisor_restart_action` policy BEFORE the opt-in auto-recycle path: it
        // DRAINS while a turn is in flight (`AwaitDrain`, restart stays pending) and
        // only at the turn boundary re-execs in place (stale binary → `ReexecInPlace`,
        // the default healthy restart, no dropped turn) or relaunches (fresh binary →
        // `RelaunchChild`). `reexec_intent` is the live staleness probe, exactly as the
        // IPC handler stamps `restart_reexec` from `binary_stale`.
        let restart_action = supervisor_restart_action(
            self.recycle_clear.restart_requested,
            self.recycle_clear.binary_stale,
            turn_boundary,
        );
        match restart_action {
            // `#midturn-recycle-resume`: an open agent-doc cycle defers the in-place
            // reexec so the operator restart cannot sever a live finalize IPC
            // connection mid-cycle. The restart request stays pending for the next
            // tick, exactly as production gates `ReexecInPlace` on `!cycle_open`.
            SupervisorRestartAction::ReexecInPlace
                if !self.recycle_clear.recycle_disabled && !self.recycle_clear.cycle_open =>
            {
                self.recycle_clear.restart_requested = false;
                if self.recycle_clear.reexec_will_fail {
                    // A failed `execve` clears the reexec intent and falls back to a
                    // child relaunch on the current binary (the restart still happens,
                    // pane survives); the binary stays stale.
                    self.recycle_clear.recycle_disabled = true;
                    self.coverage.supervisor_restart_relaunches += 1;
                } else {
                    self.recycle_supervisor_in_place();
                    self.recycle_clear.binary_stale = false;
                    self.coverage.supervisor_restart_drain_reexecs += 1;
                }
            }
            SupervisorRestartAction::RelaunchChild => {
                // Fresh binary: nothing to upgrade, the normal child relaunch serves it.
                self.recycle_clear.restart_requested = false;
                self.coverage.supervisor_restart_relaunches += 1;
            }
            // `AwaitDrain` keeps the request pending (drain the in-flight turn);
            // `None` / a disabled reexec are no-ops.
            _ => {}
        }

        // (3) Recycle policy. The operator `admin recycle` mark forces a recycle at
        // the next idle boundary regardless of staleness; the auto path uses the
        // production `supervisor_recycle_action` predicate (stale binary + opt-in).
        // `explicit_admin=false` here: this model keeps the operator `admin recycle`
        // mark as the separate `operator_recycle_marked` OR below (it forces a recycle
        // regardless of staleness), so the policy predicate runs the auto path only.
        // The `#supselfheal` Phase 1 explicit-admin override is unit-tested in
        // `decisions.rs`; its SimWorld integration lands with the admin→supervisor IPC
        // adapter (`#supselfheal-adminwire`).
        let recycle_action = supervisor_recycle_action(
            self.recycle_clear.binary_stale,
            self.recycle_clear.auto_recycle,
            turn_boundary,
            head_pending,
            false,
            self.recycle_clear.write_wedged,
            self.recycle_clear.reexec_failed,
            // `#midturn-recycle-resume`: an open agent-doc cycle defers the recycle so
            // the `execve` cannot sever the in-flight finalize IPC connection.
            self.recycle_clear.cycle_open,
        );
        // `#supselfheal` Phase 3: a stale supervisor whose in-place execve already
        // failed escalates to a bounded kill+relaunch instead of recycling onto the
        // doomed execve again. The relaunch reclaims the wedged harness child (so the
        // previously-orphaned response can commit) and clears the editor-IPC wedge;
        // past the bound the watch continues on the current binary (operator restart).
        if matches!(
            recycle_action,
            SupervisorRecycleAction::EscalateKillRelaunch
        ) {
            use agent_doc_orchestration::start::decisions::{
                MAX_REEXEC_ESCALATIONS, reexec_escalation_within_bound,
            };
            if turn_boundary
                && reexec_escalation_within_bound(
                    self.recycle_clear.reexec_escalation_attempts,
                    MAX_REEXEC_ESCALATIONS,
                )
            {
                self.recycle_clear.reexec_escalation_attempts += 1;
                // The kill+relaunch reclaims the wedged child; the orphaned response
                // commits and the editor-IPC wedge clears (the relaunched child
                // re-establishes the listener). The supervisor binary stays stale
                // until an operator restart — the relaunch is child-scoped.
                self.recycle_clear.write_wedged = false;
                self.coverage.reexec_kill_relaunch_escalations += 1;
                self.record_ops_proof(format!(
                    "supervisor_reexec_escalate_kill_relaunch attempt={}/{} reason=reexec_failed",
                    self.recycle_clear.reexec_escalation_attempts, MAX_REEXEC_ESCALATIONS,
                ));
            }
            return Ok(());
        }
        // RecycleImmediate fires at once (the next-queue-item boundary bypasses the
        // grace debounce); RecycleDebounced fires after the idle-grace elapses, which
        // we model as satisfied on this tick for determinism. Detect/None never recycle.
        let auto_recycle_now = matches!(
            recycle_action,
            SupervisorRecycleAction::RecycleImmediate | SupervisorRecycleAction::RecycleDebounced
        );
        // `#supselfheal` Phase 2: a wedge-driven RecycleImmediate against an opted-OUT
        // stale supervisor is the wedge trigger overriding the default-OFF surface-only.
        let wedge_triggered = self.recycle_clear.write_wedged
            && self.recycle_clear.binary_stale
            && !self.recycle_clear.auto_recycle
            && matches!(recycle_action, SupervisorRecycleAction::RecycleImmediate);
        // `#suprecyclestall`: once a self-`execve` recycle has failed the watch
        // disables further attempts and runs on the current binary, so a hopeless
        // recycle is not re-tried every idle boundary.
        if turn_boundary
            && !self.recycle_clear.recycle_disabled
            && (self.recycle_clear.operator_recycle_marked || auto_recycle_now)
        {
            self.recycle_clear.operator_recycle_marked = false;
            if self.recycle_clear.reexec_will_fail {
                // `supervisor_perform_reexec` returned Err. The watch logs, surfaces
                // a one-time hint, disables further recycles, and KEEPS RUNNING — it
                // never `process::exit`s, so the pane/generation are untouched and the
                // binary stays stale (the operator restarts to pick up the new build).
                self.recycle_clear.recycle_disabled = true;
                self.coverage.supervisor_recycle_failures += 1;
                // `#supselfheal` Phase 3: the failed in-place execve marks the
                // recycle policy to escalate to a bounded kill+relaunch on the next
                // idle tick (`EscalateKillRelaunch`), instead of sitting on
                // `continue_current_binary` indefinitely.
                self.recycle_clear.reexec_failed = true;
                if self.recycle_clear.jb_run_recycle_probe_pending {
                    self.recycle_clear.jb_run_recycle_probe_pending = false;
                    self.coverage.suprehot_jb_mapped_recycle_failures += 1;
                    self.record_ops_proof(
                        "suprehotreload_jb_cycle result=mapped_operator_verify targets=recyclerestart-verify,aazp,4myd reason=supervisor_reexec_failed",
                    );
                }
            } else {
                self.recycle_supervisor_in_place();
                // The in-place execve promoted the freshly-installed binary.
                self.recycle_clear.binary_stale = false;
                self.coverage.supervisor_recycles += 1;
                if wedge_triggered {
                    // `#supselfheal` Phase 2: the wedge drove this recycle on an
                    // opted-OUT supervisor; promoting the fresh binary resolves the
                    // wedge cause, so clear the latch.
                    self.recycle_clear.write_wedged = false;
                    self.coverage.wedge_triggered_recycles += 1;
                }
                if self.recycle_clear.jb_run_recycle_probe_pending {
                    self.recycle_clear.jb_run_recycle_probe_pending = false;
                    self.coverage.suprehot_jb_observed_promotions += 1;
                    self.record_ops_proof(format!(
                        "suprehotreload_jb_cycle result=observed_fresh_binary generation={} pane_preserved={} mapped=direct",
                        self.route.durable.generation,
                        self.route.durable.pane_id.is_some()
                    ));
                }
            }
        }

        let prompt_visible_now = matches!(self.route.durable.lifecycle, SupervisorLifecycle::Ready);
        let turn_active_now = matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Busy | SupervisorLifecycle::WaitingInput
        );
        if self.maybe_deliver_between_turn_enqueue(prompt_visible_now, turn_active_now) {
            return Ok(());
        }

        // (4) Deferred operator clear delivery. This mirrors the live watch's
        // pause -> clear -> settle -> resume path: after the clear lands, hold the
        // next queue trigger behind the same in-memory settle gate as other
        // supervisor-owned clears.
        if self.recycle_clear.deferred_operator_clear_pending {
            if !prompt_visible_now || turn_active_now {
                return Ok(());
            }
            self.recycle_clear.deferred_operator_clear_pending = false;
            self.recycle_clear.clear_cooldown_active = false;
            self.recycle_clear.clear_cooldown_idle_ticks = 0;
            self.recycle_clear.awaiting_clear_settle = true;
            self.recycle_clear.clear_settle_idle_ticks = 0;
            self.recycle_clear.last_dispatched = None;
            self.coverage.recycle_session_reclear_proofs += 1;
            self.record_ops_proof("recycle_session_reclear action=deferred_clear result=delivered");
            return Ok(());
        }

        // (5) Drain decision. After the cooldown clears, the normal go-mode drain
        // dispatches the waiting head (recompute prompt visibility — a recycle this
        // tick keeps the pane Ready).
        // `#qflood2` (a): advance the post-`/clear` settle debounce for the
        // watch's OWN clears (mirrors idle_watch.rs's `clear_settle_idle_ticks`).
        if self.recycle_clear.awaiting_clear_settle && prompt_visible_now && !turn_active_now {
            self.recycle_clear.clear_settle_idle_ticks =
                self.recycle_clear.clear_settle_idle_ticks.saturating_add(1);
        } else {
            self.recycle_clear.clear_settle_idle_ticks = 0;
        }
        if self.recycle_clear.awaiting_clear_settle
            && self.recycle_clear.clear_settle_idle_ticks >= CLEAR_COOLDOWN_RESUME_IDLE_TICKS
        {
            self.recycle_clear.awaiting_clear_settle = false;
            self.recycle_clear.clear_settle_idle_ticks = 0;
        }

        let head = self.recycle_clear.queue_active_head.clone();
        let drain = idle_queue_drain_decision(
            self.recycle_clear.clear_cooldown_active,
            prompt_visible_now,
            turn_active_now,
            false, // self_driving_loop_active — supervisor owns this drain in the model
            false, // route_submit_in_flight — this model does not inject editor routes
            head.as_deref(),
            self.recycle_clear.last_dispatched.as_deref(),
        );
        if matches!(drain, IdleQueueDrainDecision::Dispatch) {
            // `#qflood2` (a): hold the trigger until the watch's own `/clear`
            // settles, so it is never injected into the in-flight clear.
            if drain_blocked_awaiting_clear_settle(
                self.recycle_clear.awaiting_clear_settle,
                prompt_visible_now,
                turn_active_now,
                self.recycle_clear.clear_settle_idle_ticks,
                CLEAR_COOLDOWN_RESUME_IDLE_TICKS,
            ) {
                self.coverage.drain_settle_skips += 1;
                return Ok(());
            }
            // `#qflood2` (b): never stack a trigger already pending in the composer.
            if drain_dispatch_dedup_skip(Some(self.recycle_clear.trigger_already_pending)) {
                self.recycle_clear.last_dispatched = head;
                self.coverage.drain_dedup_skips += 1;
                return Ok(());
            }
            self.recycle_clear.last_dispatched = head;
            self.coverage.go_drain_dispatches += 1;
            self.record_ops_proof("recycle_session_reclear action=idle_queue_drain result=dispatch proof=go_drain_dispatch");
        }
        Ok(())
    }

    fn session_check_focused_cycle_deferred_for_supervisor_drain(&mut self) -> Result<()> {
        let head = self
            .recycle_clear
            .queue_active_head
            .clone()
            .ok_or_else(|| anyhow!("focused-cycle session-check yield requires an active head"))?;
        self.coverage.focused_cycle_supervisor_yields += 1;
        self.record_ops_proof(format!(
            "session_check_supervisor_drain_handoff ui_outcome=deferred_for_supervisor_drain next_action=yield_to_supervisor_clear_and_continue head_bytes={} head_sha256={} proof_marker=#qfocsup",
            head.len(),
            agent_doc_orchestration::ops_log::content_hash(&head)
        ));
        Ok(())
    }

    fn fresh_agent_drain_active_head(&mut self) -> Result<()> {
        let head = self
            .recycle_clear
            .queue_active_head
            .clone()
            .ok_or_else(|| anyhow!("fresh-agent drain requires an active head"))?;
        if self.recycle_clear.last_dispatched.as_deref() != Some(head.as_str()) {
            bail!(
                "fresh-agent drain requires the active head to be dispatched first; head={head:?} last_dispatched={:?}",
                self.recycle_clear.last_dispatched
            );
        }
        self.recycle_clear.queue_active_head = None;
        self.recycle_clear.last_dispatched = None;
        self.coverage.focused_cycle_fresh_agent_drains += 1;
        self.record_ops_proof(format!(
            "fresh_agent_drain_evidence head_bytes={} head_sha256={} result=consumed response_materialized=true",
            head.len(),
            agent_doc_orchestration::ops_log::content_hash(&head)
        ));
        Ok(())
    }

    /// Recycle the supervisor in place onto a fresh binary (`#ctlrecycle` R3:
    /// `execve_preserve_child`). Unlike `bind_route_owner` (a brand-new pane), an
    /// in-place recycle PRESERVES the live pane and session and only advances the
    /// generation, leaving a fresh dispatch-ready idle prompt. The preserved-pane
    /// invariant is what distinguishes a recycle from a cold rebind.
    fn recycle_supervisor_in_place(&mut self) {
        let preserved_pane = self.route.durable.pane_id.clone();
        let preserved_session = self.route.durable.session_id.clone();
        let generation = self.route.durable.generation + 1;
        self.route.durable = ActorState {
            generation,
            session_id: preserved_session,
            pane_id: preserved_pane,
            lifecycle: SupervisorLifecycle::Ready,
        };
        self.route.projection = self.route.durable.clone();
        self.route.supervisor_lease_generation = Some(generation);
        self.coverage.recycle_binary_promotion_proofs += 1;
        self.record_ops_proof(format!(
            "supervisor_recycle_action result=binary_promoted generation={} pane_preserved={}",
            generation,
            self.route.durable.pane_id.is_some()
        ));
    }

    pub(crate) fn restart_supervisor(
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

    pub(crate) fn restart_live_pane_is_busy(&self) -> bool {
        matches!(
            self.route.durable.lifecycle,
            SupervisorLifecycle::Busy
                | SupervisorLifecycle::WaitingInput
                | SupervisorLifecycle::Blocked
        )
    }

    pub(crate) fn repair_ipc_snapshot_duplicate_prompts(
        &mut self,
        before: &str,
        file: &Path,
    ) -> Result<()> {
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

    pub(crate) fn route_style_duplicate_prompt_cleanup(
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

    pub(crate) fn apply_narrow_normalization_repair(&mut self, normalize_prefix_lines: &[String]) {
        let repaired = agent_doc_orchestration::write::normalize_exchange_prefixes_for_targets(
            &self.doc,
            normalize_prefix_lines,
        );
        if repaired != self.doc {
            self.doc = repaired;
            self.coverage.normalization_repair_patches += 1;
        }
    }

    pub(crate) fn stale_full_content_visible_replacement(
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

    pub(crate) fn apply_sidecar_normalization_fallback(
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

    pub(crate) fn adopt_ipc_snapshot_candidate(
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

    /// Model the `#fintol2` finalize-tolerance decision over the same public gate
    /// primitives the binary's `guard_ipc_snapshot_adoption_against_live_prompt_drift`
    /// uses. When the live buffer drifted after preflight:
    /// - a DISJOINT plain content edit (proven by `response_target_disjoint_from_user_edit`)
    ///   is forward-merged: the committed snapshot is the conflict-free union of the
    ///   response and the user edit, so the response lands AND the edit is preserved;
    /// - a prompt/directive or collision drift keeps today's fail-closed behavior
    ///   (commit `content_ours`, carry the live edit forward in `self.doc`).
    pub(crate) fn finalize_ipc_candidate_with_tolerance(
        &mut self,
        baseline: &str,
        content_ours: &str,
        snapshot_candidate: &str,
    ) {
        self.doc = snapshot_candidate.to_string();
        if !agent_doc_orchestration::write::ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
            baseline,
            snapshot_candidate,
            content_ours,
        ) {
            self.snapshot = snapshot_candidate.to_string();
            return;
        }
        if agent_doc_orchestration::write::response_target_disjoint_from_user_edit(
            baseline,
            content_ours,
            snapshot_candidate,
        ) {
            let union = agent_doc_orchestration::merge::merge_contents(
                baseline,
                content_ours,
                snapshot_candidate,
            )
            .expect("disjoint forward-merge must succeed");
            assert!(
                !union.contains("<<<<<<<"),
                "a disjoint forward-merge must be conflict-free:\n{union}"
            );
            self.snapshot = union.clone();
            self.doc = union;
            self.coverage.live_prompt_forward_merges += 1;
        } else {
            self.snapshot = content_ours.to_string();
            self.coverage.ipc_snapshot_live_prompt_blocks += 1;
        }
    }

    /// `#smsim` (semantic_merge Phase 5): model the `#smconv` node-keyed
    /// convergence over an operator↔agent concurrent edit. Runs the production
    /// `semantic_merge_scoped` (the same `exchange`-active scoping the real
    /// `try_semantic_merge_convergence` applies), adopts the merged doc as both
    /// the snapshot and the live buffer, and records which transition-matrix
    /// outcome fired. `active_exchange` toggles the turn-active scoping so the
    /// same-node ack gating (`#smturnactive`) can be exercised both ways. The
    /// merged document is operator-wins on same-node conflicts, so content is
    /// never lost — only ack emission depends on scope.
    pub(crate) fn converge_semantic_merge(
        &mut self,
        base: &str,
        agent_ours: &str,
        operator_theirs: &str,
        active_component: Option<&str>,
    ) -> agent_doc_markdown_ast::semantic_merge::SemanticMerge {
        use agent_doc_markdown_ast::semantic_merge::{
            AckReason, ActiveNodes, OutcomeKind, semantic_merge_scoped,
        };
        let active = match active_component {
            Some(name) => ActiveNodes::new().active_component(name),
            None => ActiveNodes::new(),
        };
        let sm = semantic_merge_scoped(base, agent_ours, operator_theirs, &active);
        self.snapshot = sm.merged_doc.clone();
        self.doc = sm.merged_doc.clone();

        let has_operator_won = sm
            .outcomes
            .iter()
            .any(|o| o.kind == OutcomeKind::OperatorWonConflict);
        let has_delete_kept = sm
            .outcomes
            .iter()
            .any(|o| o.kind == OutcomeKind::DeletionKept);
        let delete_ack = sm
            .requires_ack
            .iter()
            .any(|a| a.reason == AckReason::OperatorDeletedAgentEditedNode);
        let only_disjoint = sm.requires_ack.is_empty()
            && !has_operator_won
            && !has_delete_kept
            && sm.outcomes.iter().any(|o| {
                matches!(
                    o.kind,
                    OutcomeKind::AppliedAgentEdit
                        | OutcomeKind::AppliedOperatorEdit
                        | OutcomeKind::AppliedAgentAdd
                        | OutcomeKind::AppliedOperatorAdd
                        | OutcomeKind::AppliedBothAdd
                )
            });

        if only_disjoint {
            self.coverage.semantic_merge_node_disjoint += 1;
        }
        if has_operator_won {
            self.coverage.semantic_merge_operator_wins += 1;
        }
        if has_delete_kept && delete_ack {
            self.coverage.semantic_merge_delete_acks += 1;
        }
        // Turn-active gating proof: a same-node conflict exists, but it raised an
        // ack ONLY because the node was inside the active area. The identical
        // conflict with no active area (or a different active component)
        // auto-resolves operator-wins with no ack.
        if has_operator_won && active_component.is_some() && !sm.requires_ack.is_empty() {
            self.coverage.semantic_merge_scope_gated_acks += 1;
        }
        sm
    }

    /// Model the `ipc_socket_already_applied_live_buffer_diverged` recovery
    /// (`#mrhpcdrift2`): the socket reported `already_applied` but the live
    /// buffer diverged with the assistant response fragmented out of `exchange`.
    /// The recovery materializes the response from `content_ours` back into the
    /// visible buffer so it is never silently lost (zero UNRECOVERED drift),
    /// without duplicating an already-present response. `self.doc` is the
    /// divergent live buffer; `self.snapshot` adopts `content_ours`.
    pub(crate) fn recover_already_applied_diverged_response(
        &mut self,
        content_ours: &str,
        expected_response: &str,
    ) {
        if let Some(repaired) =
            agent_doc_orchestration::write::materialize_response_in_current_exchange(
                &self.doc,
                expected_response,
            )
        {
            if repaired != self.doc {
                self.coverage.already_applied_response_recoveries += 1;
            }
            self.doc = repaired;
        }
        self.snapshot = content_ours.to_string();
    }

    pub(crate) fn apply_ack_sidecar_only_repair(&mut self, ack_content: &str) {
        self.snapshot = ack_content.to_string();
        self.coverage.ack_sidecar_only_repairs += 1;
    }

    pub(crate) fn repair_visible_duplicate_response(&mut self) {
        let repaired = agent_doc_orchestration::dedupe::dedupe_responses(&self.doc);
        if repaired != self.doc {
            self.doc = repaired;
            self.coverage.visible_duplicate_repairs += 1;
        }
    }

    pub(crate) fn record_post_commit_follow_up_handoff(&mut self) -> Result<()> {
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

    pub(crate) fn transition_supervisor(
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

    pub(crate) fn dispatch_route_prompt(&mut self) -> Result<()> {
        self.dispatch_route_prompt_with(false)
    }

    /// `#qflood`: drive a controller dispatch. `operator_driven` marks an explicit
    /// operator dispatch (JB `Run Agent Doc`); it is never coalesced, so the
    /// operator can dispatch while auto-drain backpressure holds.
    pub(crate) fn dispatch_route_prompt_with(&mut self, operator_driven: bool) -> Result<()> {
        let pane_id = self.current_dispatch_pane()?;
        if let Some(stage) = self
            .route
            .queue_control
            .as_failed_stage(self.route.durable.lifecycle)
        {
            bail!(
                "dispatch blocked by controller queue control: failed_stage={} generation={} seed={} trace={:?}",
                stage,
                self.route.durable.generation,
                self.seed,
                self.trace
            );
        }
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
        // `#qflood`: coalesce a redundant AUTO re-dispatch while the same cycle's
        // prior dispatch is still in flight (accepted, not yet proven/consumed), so
        // the routed trigger does not pile into the busy pane on each file-change /
        // idle / `/loop` tick. An operator dispatch, or a dispatch once the prior is
        // proven or at a new generation, is not in flight and passes through. Shares
        // the live controller's `dispatch_should_coalesce_in_flight` decision.
        let in_flight_same_cycle = self.route.pending_dispatch.as_ref().is_some_and(|receipt| {
            !receipt.proved
                && receipt.generation == self.route.durable.generation
                && receipt.session_id == self.route.durable.session_id
                && receipt.pane_id == pane_id
        });
        if agent_doc_orchestration::project_controller::dispatch_should_coalesce_in_flight(
            in_flight_same_cycle,
            operator_driven,
        ) {
            self.coverage.route_dispatch_coalesced += 1;
            return Ok(());
        }
        self.route.pending_dispatch = Some(DispatchReceipt {
            generation: self.route.durable.generation,
            session_id: self.route.durable.session_id.clone(),
            pane_id: pane_id.clone(),
            proved: false,
        });
        self.coverage.route_dispatch_acceptances += 1;
        // `#rdypoll` (§D / img_52): a real trigger injection happened — emit the
        // same `dispatch_inject attempt=N` marker the production route logs so a
        // multi-inject regression (N stacked un-submitted triggers after a restart)
        // is provable from ops.log. A correct dispatch only reaches here once it is
        // past the not-ready/Starting gate, so a healthy scenario logs `attempt=1`
        // exactly once and never `attempt=2+`.
        self.coverage.dispatch_injects += 1;
        self.record_ops_proof(format!(
            "dispatch_inject pane={} generation={} attempt={}",
            pane_id, self.route.durable.generation, self.coverage.dispatch_injects
        ));
        Ok(())
    }

    pub(crate) fn dispatch_operator_prompt_with_protected_draft(&mut self) -> Result<()> {
        let pane_id = self.current_dispatch_pane()?;
        let harness = agent_doc_orchestration::harness::HarnessConfig::codex();
        let pane_content = "› Implement {feature}\n";
        let Some(reason) = harness.protected_prompt_input_reason(pane_content) else {
            bail!("protected draft fixture no longer blocks route dispatch");
        };
        let draft_preview = harness
            .last_prompt_candidate(pane_content)
            .map(|candidate| agent_doc_orchestration::prompt::strip_ansi(&candidate))
            .map(|preview| agent_doc_orchestration::secret_redact::redact(preview.trim()))
            .filter(|preview| !preview.is_empty())
            .unwrap_or_else(|| "<none>".to_string());

        self.coverage.protected_prompt_route_blocks += 1;
        self.record_ops_proof(format!(
            "route_dispatch_direct_pane_blocked pane={} harness={} protected_input={} draft_preview={:?}",
            pane_id, harness.binary, reason, draft_preview
        ));
        Ok(())
    }

    pub(crate) fn admin_queue_control(
        &mut self,
        state: QueueControlState,
        observed_generation: u64,
    ) -> Result<()> {
        self.require_current_admin_generation(observed_generation, "queue_control")?;
        self.route.queue_control = state;
        match state {
            QueueControlState::Paused => self.coverage.queue_pauses += 1,
            QueueControlState::Resumed => self.coverage.queue_resumes += 1,
            QueueControlState::Draining => self.coverage.queue_drains += 1,
        }
        Ok(())
    }

    pub(crate) fn admin_handoff(
        &mut self,
        observed_generation: u64,
        to_pane: impl Into<String>,
    ) -> Result<()> {
        self.require_current_admin_generation(observed_generation, "admin_handoff")?;
        let prior_generation = self.route.durable.generation;
        self.route.durable.generation = prior_generation.saturating_add(1);
        self.route.durable.pane_id = Some(to_pane.into());
        self.route.durable.lifecycle = SupervisorLifecycle::Ready;
        self.route.projection = self.route.durable.clone();
        self.route.pending_dispatch = None;
        self.route.starting_timeout = None;
        self.route.supervisor_lease_generation = Some(self.route.durable.generation);
        self.coverage.admin_handoffs += 1;
        Ok(())
    }

    pub(crate) fn admin_reap(&mut self, observed_generation: u64) -> Result<()> {
        self.require_current_admin_generation(observed_generation, "admin_reap")?;
        self.route.durable.lifecycle = SupervisorLifecycle::Closed;
        self.route.durable.pane_id = None;
        self.route.projection = self.route.durable.clone();
        self.route.pending_dispatch = None;
        self.route.starting_timeout = None;
        self.route.supervisor_lease_generation = None;
        self.coverage.admin_reaps += 1;
        Ok(())
    }

    pub(crate) fn supervisor_heartbeat_reattach(
        &mut self,
        generation: u64,
        pane_id: impl Into<String>,
    ) -> Result<()> {
        if generation != self.route.durable.generation {
            bail!(
                "supervisor heartbeat stale generation rejected: observed={} current={}; seed={} trace={:?}",
                generation,
                self.route.durable.generation,
                self.seed,
                self.trace
            );
        }
        self.route.durable.pane_id = Some(pane_id.into());
        self.route.durable.lifecycle = SupervisorLifecycle::Ready;
        self.route.projection = self.route.durable.clone();
        self.route.supervisor_lease_generation = Some(generation);
        self.route.starting_timeout = None;
        self.coverage.supervisor_heartbeat_reattaches += 1;
        Ok(())
    }

    pub(crate) fn require_current_admin_generation(
        &self,
        observed_generation: u64,
        operation: &str,
    ) -> Result<()> {
        if observed_generation != self.route.durable.generation {
            bail!(
                "stale actor generation rejected for {}: observed={} current={}; seed={} trace={:?}",
                operation,
                observed_generation,
                self.route.durable.generation,
                self.seed,
                self.trace
            );
        }
        Ok(())
    }

    pub(crate) fn promote_starting_prompt_ready(&mut self) -> Result<()> {
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

    /// `#jbtsiftnosub`: model the JB `Run Agent Doc` auto-start dispatch path's
    /// cold-start re-verify gate. The auto-start path creates a fresh pane and may
    /// observe a transient dispatch-ready prompt while the harness TUI is still
    /// coming up. Before the send, route re-verifies the harness dispatch-ready
    /// prompt; if the pane is still `Starting` (the composer accepts input but is
    /// not yet submit-ready), the gate fails closed and records
    /// `dispatch_into_starting_pane` rather than typing the trigger into a
    /// not-yet-submit-ready composer. Once the dispatch-ready prompt is observed
    /// (`promote_starting_prompt_ready`), the same auto-start dispatch proceeds
    /// through the normal route dispatch path.
    pub(crate) fn dispatch_auto_start_route_prompt(&mut self) -> Result<()> {
        let pane_id = self.current_dispatch_pane()?;
        if self.route.durable.lifecycle == SupervisorLifecycle::Starting {
            self.coverage.auto_start_starting_pane_blocks += 1;
            self.record_ops_proof(format!(
                "dispatch_into_starting_pane pane={} generation={} reason=harness_not_dispatch_ready_before_auto_start_send",
                pane_id, self.route.durable.generation
            ));
            bail!(
                "auto-start route dispatch refused: harness still starting (no dispatch-ready prompt) for pane {}; seed={} trace={:?}",
                pane_id,
                self.seed,
                self.trace
            );
        }
        // Past the cold-start gate the auto-start send rejoins the normal route
        // dispatch path (the same readiness/coalesce/acceptance invariants apply).
        // `#jbtsiftnosub` / `#j9ja`: record the SUCCESS counterpart of
        // `dispatch_into_starting_pane` so a live operator test of the cleared
        // re-verify gate is provable/disprovable from ops.log.
        self.record_ops_proof(format!(
            "auto_start_dispatch_ready_confirmed pane={} generation={} #jbtsiftnosub",
            pane_id, self.route.durable.generation
        ));
        self.dispatch_route_prompt_with(true)
    }

    /// `#runexitrestart`: model the supervisor idle-watch queue-drain dispatch
    /// AFTER a session restart. The production gate (`idle_queue_prompt_visible`)
    /// dispatches off the `Ready` fast path, but when the actor is NOT yet `Ready`
    /// it must re-verify a fresh-capture dispatch-ready prompt
    /// (`supervisor_pane_dispatch_ready` → `ready_prompt_candidate`) before trusting
    /// the weak pty-buffer prompt-glyph signal. A freshly-restarted pane sits in
    /// `Starting` (composer coming up, not yet submit-ready), so the drain must fail
    /// closed — record `dispatch_into_restarting_pane`, do NOT inject the trigger,
    /// and (crucially) leave nothing in the composer to re-type, so repeated idle
    /// ticks cannot stack duplicate un-submitted triggers. Once the dispatch-ready
    /// prompt is observed (`PromoteStartingPromptReady`) the same drain dispatches
    /// exactly once through the normal drain path.
    pub(crate) fn dispatch_idle_queue_drain_after_restart(&mut self) -> Result<()> {
        let pane_id = self.current_dispatch_pane()?;
        if self.route.durable.lifecycle == SupervisorLifecycle::Starting {
            self.coverage.drain_into_restarting_pane_blocks += 1;
            self.record_ops_proof(format!(
                "dispatch_into_restarting_pane pane={} generation={} reason=harness_not_dispatch_ready_before_restart_drain_send",
                pane_id, self.route.durable.generation
            ));
            bail!(
                "idle-watch restart drain refused: harness still restarting (no dispatch-ready prompt) for pane {}; seed={} trace={:?}",
                pane_id,
                self.seed,
                self.trace
            );
        }
        // Past the restart-dispatch gate the drain rejoins the normal route
        // dispatch path (the same readiness/coalesce/acceptance invariants apply),
        // so a proven-ready restarted pane drains exactly once.
        self.dispatch_route_prompt_with(true)
    }

    /// `#jbdisprecycle` R4: model the `route` dispatch's recycle-inflight gate.
    /// While the project supervisor is mid-`execve` recycle the live dispatch
    /// (`route/dispatch_only.rs` R1, `route/dispatch.rs` R3) must NOT type the
    /// trigger — a keystroke typed across the hot-reload boundary has its submit
    /// Enter dropped, leaving the operator-observed typed-without-submit trigger.
    /// So the gate fails closed, records `dispatch_into_recycling_pane`, and
    /// injects NOTHING (repeated dispatches while recycling cannot stack
    /// duplicates). Once the recycle settles (`SettleSupervisorRecycle`) the same
    /// dispatch rejoins the normal route path and injects exactly once.
    pub(crate) fn dispatch_during_supervisor_recycle(&mut self) -> Result<()> {
        let pane_id = self.current_dispatch_pane()?;
        if self.route.recycle_inflight {
            self.coverage.dispatch_into_recycling_pane_blocks += 1;
            self.record_ops_proof(format!(
                "dispatch_into_recycling_pane pane={} generation={} reason=supervisor_mid_recycle_before_dispatch_send",
                pane_id, self.route.durable.generation
            ));
            bail!(
                "route dispatch refused: project supervisor mid-recycle (trigger typed across the execve boundary would be dropped before submit) for pane {}; seed={} trace={:?}",
                pane_id,
                self.seed,
                self.trace
            );
        }
        // Recycle settled — rejoin the normal route dispatch path (same
        // readiness/coalesce/acceptance invariants), so the dispatch injects once.
        self.record_ops_proof(format!(
            "route_dispatch_submit_recycle_settle pane={} generation={} settled=true action=submit_once_after_settle",
            pane_id, self.route.durable.generation
        ));
        self.dispatch_route_prompt_with(true)
    }

    pub(crate) fn recover_busy_interrupt_to_ready(&mut self) -> Result<()> {
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
    pub(crate) fn repair_busy_projection_with_ready_prompt(&mut self) -> Result<()> {
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

    pub(crate) fn prove_dispatch_accepted(&mut self) -> Result<()> {
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
        let proof_pane = receipt.pane_id.clone();
        let proof_generation = receipt.generation;
        receipt.proved = true;
        self.coverage.route_dispatch_proofs += 1;
        self.record_ops_proof(format!(
            "dispatch_start_proof pane={} generation={} proof=submitted",
            proof_pane, proof_generation
        ));
        Ok(())
    }

    pub(crate) fn current_dispatch_pane(&self) -> Result<String> {
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

    pub(crate) fn projection_identity_matches_durable(&self) -> bool {
        self.route.projection.generation == self.route.durable.generation
            && self.route.projection.session_id == self.route.durable.session_id
            && self.route.projection.pane_id == self.route.durable.pane_id
    }

    pub(crate) fn apply_captured_response(&mut self) -> Result<()> {
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

    pub(crate) fn try_commit(&mut self) -> Result<()> {
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

    pub(crate) fn strict_closeout_invariants(&self) -> Result<()> {
        self.closeout_invariants(true)
    }

    pub(crate) fn pre_commit_invariants(&self) -> Result<()> {
        self.closeout_invariants(false)
    }

    pub(crate) fn closeout_invariants(&self, require_committed: bool) -> Result<()> {
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

    pub(crate) fn recover_after_fault(&mut self) -> Result<()> {
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

    pub(crate) fn take_fault(&mut self, fault: FaultPoint) -> bool {
        if self.pending_fault == Some(fault) {
            self.pending_fault = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn assert_structural_invariants(&self) -> Result<()> {
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
        // Move-before-select ordering invariant (#tmux-switch-lag): the active
        // (selected) pane must never still be parked in the stash window — that is
        // the intermediate stash-frame flash. Focus must promote out of stash
        // before selecting.
        if let Some(active) = &self.sync.active
            && self.sync.stashed.contains(active)
        {
            bail!(
                "sync projection selected a still-stashed pane `{active}` (#tmux-switch-lag move-before-select violation): stashed={:?}; seed={} trace={:?}",
                self.sync.stashed,
                self.seed,
                self.trace
            );
        }
        Ok(())
    }

    /// `#postcommit-ipc-worktree-corruption` invariant: on a *clean* committed
    /// boundary (working tree already byte-equal to HEAD modulo `(HEAD)` /
    /// boundary-id artifacts), the post-commit IPC reposition signal must be
    /// idempotent — it must not drift the visible file away from the committed
    /// blob. Returns `Ok(false)` when the boundary is not clean (a legitimate
    /// post-commit user edit started a new cycle; not the modeled scenario),
    /// `Ok(true)` when the idempotent reposition preserved tree==HEAD, and an
    /// error when the working tree drifted (the corruption regression). Scoped to
    /// the reposition signal rather than asserted globally so ordinary post-commit
    /// edits are not mistaken for corruption.
    pub(crate) fn assert_post_commit_reposition_idempotent(&mut self) -> Result<bool> {
        if !matches!(self.phase, CyclePhase::Committed) {
            return Ok(false);
        }
        let head = normalize_committed_worktree(&self.snapshot);
        if normalize_committed_worktree(&self.doc) != head {
            // Working tree already diverged via a legitimate new edit; the clean
            // committed-boundary reposition scenario does not apply.
            return Ok(false);
        }
        self.doc =
            crate::template::reposition_boundary_to_end_clean_with_id(&self.doc, Some("committed"));
        if normalize_committed_worktree(&self.doc) != head {
            bail!(
                "post-commit working tree drifted from HEAD (#postcommit-ipc-worktree-corruption); seed={} trace={:?}",
                self.seed,
                self.trace
            );
        }
        Ok(true)
    }

    pub(crate) fn has_duplicate_response_heading(&self) -> bool {
        self.doc.matches("### Re: sim closeout").count() > 1
    }

    pub(crate) fn append_to_exchange(&mut self, text: &str) -> Result<()> {
        let body = self.component_content("exchange")?.to_string();
        let boundary = body.find("<!-- agent:boundary:");
        let next = if let Some(pos) = boundary {
            format!("{}{}{}", &body[..pos], text, &body[pos..])
        } else {
            format!("{body}{text}")
        };
        self.replace_component_content("exchange", &next)
    }

    pub(crate) fn component_content(&self, name: &str) -> Result<&str> {
        crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == name)
            .map(|component| component.content(&self.doc))
            .ok_or_else(|| anyhow!("missing component `{name}`"))
    }

    pub(crate) fn replace_component_content(&mut self, name: &str, content: &str) -> Result<()> {
        let component = crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == name)
            .ok_or_else(|| anyhow!("missing component `{name}`"))?;
        self.doc = component.replace_content(&self.doc, content);
        Ok(())
    }

    pub(crate) fn insert_after_exchange(&mut self, content: &str) -> Result<()> {
        let exchange = crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == "exchange")
            .ok_or_else(|| anyhow!("missing component `exchange`"))?;
        self.doc.insert_str(exchange.close_end, content);
        Ok(())
    }
}
