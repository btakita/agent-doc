use agent_doc_flow::{
    closeout::closeout_latency_message,
    types::{FlowOutcome, FlowStage},
};
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_turn::closeout_guard::CloseoutGuardReason;
use agent_doc_turn::closeout_recovery::{
    CloseoutRecoveryCommandInput, CloseoutRecoveryCycleInput, CloseoutRecoveryDecision,
    CloseoutRecoveryDecisionInput, CloseoutRecoveryDrift, CloseoutRecoveryMutationReason,
    CloseoutRecoveryState, CloseoutRecoveryStateInput, MetadataDriftAuthority,
    OpenCycleRecoveryCommandInput, classify_closeout_recovery_state_from_input,
    classify_snapshot_head_drift, classify_snapshot_visible_drift,
    closeout_recovery_command as render_closeout_recovery_command,
    closeout_recovery_decision_from_state, metadata_drift_authority,
};
use anyhow::{Context, Result};
use std::path::Path;

pub trait CloseoutEffects {
    fn commit(&self, file: &Path) -> Result<bool>;

    fn commit_for_authority(&self, file: &Path, _force_disk: bool) -> Result<bool> {
        self.commit(file)
    }

    fn crdt_commit_barrier(&self, file: &Path) -> Result<bool> {
        agent_doc_controller_io::project_controller::commit_barrier_via_controller_model_for_doc(
            file,
        )
    }

    fn run_pending_maintenance(
        &self,
        file: &Path,
        force_disk: bool,
    ) -> Result<agent_doc_preflight_io::PendingMaintenanceReport>;

    fn enforce_clean_closeout(&self, file: &Path) -> Result<()>;

    fn enforce_clean_closeout_for_authority(&self, file: &Path, _force_disk: bool) -> Result<()> {
        self.enforce_clean_closeout(file)
    }

    fn cancel_preflight_cycle(&self, file: &Path) -> Result<()>;

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> Result<bool>;

    fn detect_bypassed_response_write(&self, file: &Path) -> Result<Option<String>>;

    fn resolve_current_document(
        &self,
        file: &Path,
        source: &str,
    ) -> Result<agent_doc_document_realtime::CurrentDocument>;

    fn resolve_current_document_for_authority(
        &self,
        file: &Path,
        source: &str,
        _force_disk: bool,
    ) -> Result<agent_doc_document_realtime::CurrentDocument> {
        self.resolve_current_document(file, source)
    }

    fn write_current_document(
        &self,
        doc: &agent_doc_document_realtime::CurrentDocument,
        content: &str,
        source: &str,
    ) -> Result<()>;

    fn mark_committed_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState>;

    fn mark_abandoned_frontmatter(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<agent_doc_cycle_state_io::CycleState>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompleteRequiredCloseoutOptions {
    pub force_disk: bool,
}

pub fn log_closeout_guard_event(
    file: &Path,
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) {
    crate::log_flow_event(
        file,
        agent_doc_turn::closeout_guard::closeout_guard_event(stage, outcome, reason),
        agent_doc_ops_log_io::log_op,
    );
}

pub fn complete_required_closeout(file: &Path, effects: &dyn CloseoutEffects) -> Result<bool> {
    complete_required_closeout_with_options(
        file,
        effects,
        CompleteRequiredCloseoutOptions::default(),
    )
}

pub fn complete_required_closeout_with_options(
    file: &Path,
    effects: &dyn CloseoutEffects,
    options: CompleteRequiredCloseoutOptions,
) -> Result<bool> {
    let mut timer = CloseoutTimer::start(file);
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());

    // `#crdtauth4` — authority-gated state-vector commit barrier (plan phase 4).
    // Under `CrdtAuthority::MultiReplica` (a live editor is attached) flush every
    // currently-live editor replica into the canonical replica on a consistent cut
    // BEFORE the snapshot is committed, so the committed state provably holds every
    // live editor's last ops — the durable fix for the `no_ack` /
    // `ipc_proof_insufficient` / post-commit-worktree-corruption class. It is a
    // checkpoint, never a global lock: a disconnected editor is excluded from the
    // cut and contributes on reconnect, while an attached editor with unflushed
    // text blocks the commit boundary instead of letting stale disk win.
    // Under `CrdtAuthority::GitAuthoritative` (Detached / headless — most traffic)
    // this is a trivial no-op that touches no hub and leaves the commit path
    // byte-for-byte unchanged.
    let barrier_ready = if options.force_disk {
        true
    } else {
        effects.crdt_commit_barrier(file).with_context(|| {
            format!(
                "controller CRDT commit barrier failed for {}",
                file.display()
            )
        })?
    };
    if !barrier_ready {
        log_closeout_guard_event(
            file,
            FlowStage::PreCommitGuard,
            FlowOutcome::Blocked,
            CloseoutGuardReason::ReplicaDeliveryPending,
        );
        anyhow::bail!(
            "editor is the current authority for {}, but CRDT relay convergence is still pending; disk is a non-authoritative replica and was not used as commit authority",
            file.display()
        );
    }

    let mut did_commit = effects.commit_for_authority(file, options.force_disk)?;
    // `#staleinmem` — record the just-committed on-disk content as the hub baseline
    // so a later out-of-band disk correction is detectable at the next commit
    // barrier (no-op under the Detached / headless path).
    if let Err(err) = agent_doc_controller_io::project_controller::
        record_committed_baseline_via_controller_model_for_doc(file)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "controller_crdt_record_committed_baseline_error file={} error={err}",
                file.display()
            ),
        );
    }
    rc.invalidate_head_content();
    rc.invalidate_snapshot_content();
    timer.mark("git_commit");
    ensure_cycle_committed(file)?;
    timer.mark("cycle_state");

    // `#exit75-done-reap-not-atomic`: reap any `[x]` tracked items the response
    // cycle marked done (`--done`) within the SAME closeout. The retired
    // unproven-IPC fallback used to commit before this reap, so keep the terminal
    // reap idempotent here for all successful closeouts. `run_pending_maintenance`
    // is the same reap preflight runs at cycle start: it writes the reaped/archived document +
    // snapshot (it does not commit), so the snapshot-vs-HEAD retry immediately
    // below stages the reap. It is idempotent — a no-op when nothing is `[x]`
    // (the direct path already reaped), so it only closes the historical reap gap. Errors
    // are non-fatal: the `completed_pending_reap` guard still catches a miss.
    // During realtime cutover, the shared converge write gate fails closed when
    // no editor endpoint proves delivery. Detached closeout has no editor buffer
    // to protect, so use the explicit force-disk maintenance path there; keep
    // editor-attached closeout on the guarded IPC convergence path.
    let editor_attached = agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached();
    let pending_maintenance =
        effects.run_pending_maintenance(file, options.force_disk || !editor_attached);
    match pending_maintenance {
        Ok(_) => {
            rc.invalidate_head_content();
            rc.invalidate_snapshot_content();
            timer.mark("closeout_reap");
        }
        Err(e) => eprintln!("[commit] closeout pending-reap maintenance failed (non-fatal): {e}"),
    }

    retry_snapshot_head_content_hash_drift(
        file,
        effects,
        &rc,
        &mut did_commit,
        &mut timer,
        RetrySnapshotHeadDriftOptions {
            force_disk: options.force_disk,
            commit_mark: "git_commit_retry_snapshot",
            cycle_mark: "cycle_state_retry_snapshot",
        },
    )?;

    if agent_doc_git_io::submodule::submodule_pointer_drift(file)?.is_some() {
        eprintln!("[commit] parent submodule pointer still stale after commit - retrying");
        did_commit |= effects.commit_for_authority(file, options.force_disk)?;
        rc.invalidate_head_content();
        rc.invalidate_snapshot_content();
        timer.mark("git_commit_retry_parent_pointer");
        ensure_cycle_committed(file)?;
        timer.mark("cycle_state_retry_parent_pointer");
    }
    if let Some(drift) = agent_doc_git_io::submodule::submodule_pointer_drift(file)? {
        timer.mark("parent_pointer_verify_failed");
        let parent_head = drift.parent_head.as_deref().unwrap_or("<missing>");
        timer.finish();
        log_closeout_guard_event(
            file,
            FlowStage::TerminalGuard,
            FlowOutcome::FailedClosed,
            CloseoutGuardReason::ParentPointerStale,
        );
        anyhow::bail!(
            "parent submodule pointer is not committed for {} after strict closeout: parent HEAD:{}={} but submodule HEAD={}. Run `agent-doc commit {}` to retry the idempotent parent-pointer closeout.",
            file.display(),
            drift.relative_path,
            parent_head,
            drift.submodule_head,
            file.display()
        );
    }
    if let Err(err) = effects.enforce_clean_closeout_for_authority(file, options.force_disk) {
        log_closeout_guard_event(
            file,
            FlowStage::SessionCheck,
            FlowOutcome::FailedClosed,
            CloseoutGuardReason::SessionCheckInterrupted,
        );
        return Err(err);
    }
    timer.mark("session_check");
    agent_doc_controller_io::project_controller::persist_session_actor_closeout(file)?;
    timer.mark("session_actor_closeout");
    retry_snapshot_head_content_hash_drift(
        file,
        effects,
        &rc,
        &mut did_commit,
        &mut timer,
        RetrySnapshotHeadDriftOptions {
            force_disk: options.force_disk,
            commit_mark: "git_commit_retry_terminal_snapshot",
            cycle_mark: "cycle_state_retry_terminal_snapshot",
        },
    )?;
    record_terminal_closeout_proof(file, did_commit, effects, options)?;
    timer.mark("terminal_proof");
    timer.finish();
    Ok(did_commit)
}

pub fn cycle_already_committed(file: &Path) -> Option<String> {
    match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        Ok(Some(state)) if state.phase == agent_doc_turn::CyclePhase::Committed => {
            Some(state.cycle_id)
        }
        _ => None,
    }
}

/// Diagnostic information about a "stuck captured cycle" — a cycle whose
/// `cycle_state` advanced to `Committed` but whose captured response body is
/// not present in HEAD or in a compact archive referenced by HEAD for the
/// document.
///
/// Preflight surfaces this as a non-blocking warning so harnesses can drive
/// recovery via `agent-doc write --commit <FILE>` instead of silently retrying
/// the same finalize.
///
/// Plan: tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StuckCapturedCycleInfo {
    pub cycle_id: String,
    pub response_body_len: usize,
    pub capture_id: String,
    pub capture_state: String,
}

/// Detect a "stuck captured cycle" wedge for `file`. Returns `None` when the
/// document is healthy or when there is not enough durable state to prove the
/// captured response body is absent from both `HEAD` and HEAD-referenced
/// compact archives.
pub fn stuck_captured_cycle(file: &Path) -> Option<StuckCapturedCycleInfo> {
    let state = match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        Ok(Some(state)) => state,
        Ok(None) => return None,
        Err(err) => {
            eprintln!(
                "[preflight] warning: failed to load cycle state for stuck-cycle detection on {}: {err}",
                file.display()
            );
            return None;
        }
    };
    if state.phase != agent_doc_turn::CyclePhase::Committed {
        return None;
    }
    let capture_id = state.capture_id.as_deref()?;
    let capture = match closeout_captured_response_for_state(file, Some(&state)) {
        Ok(Some(capture)) => capture,
        Ok(None) => return None,
        Err(err) => {
            eprintln!(
                "[preflight] warning: failed to load captured response {capture_id} for stuck-cycle detection on {}: {err}",
                file.display()
            );
            return None;
        }
    };
    if capture.response_body.trim().is_empty()
        || matches!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Discarded
        )
    {
        return None;
    }
    let head = match agent_doc_git_io::revision::show_head(file) {
        Ok(Some(head)) => head,
        Ok(None) => return None,
        Err(err) => {
            eprintln!(
                "[preflight] warning: failed to read HEAD for stuck-cycle detection on {}: {err}",
                file.display()
            );
            return None;
        }
    };
    if agent_doc_turn::response_replay::response_materialized_in_content(
        &capture.response_body,
        &head,
    ) {
        return None;
    }
    if response_materialized_in_head_compact_archive(file, &capture.response_body, &head) {
        return None;
    }

    Some(StuckCapturedCycleInfo {
        cycle_id: state.cycle_id,
        response_body_len: capture.response_body.len(),
        capture_id: capture.capture_id,
        capture_state: capture_state_label(capture.state).to_string(),
    })
}

/// `#stuck-capture-compact-false-positive`: durably reconcile a committed-cycle
/// capture whose response body is absent from `HEAD` *only because* `compact`
/// archived it. [`stuck_captured_cycle`] already suppresses the false-positive
/// warning by re-reading the HEAD-referenced compact archive on every preflight
/// pass, but that suppression is not durable — if the archive is later GC'd the
/// same capture would flag stuck again and re-suggest a `write --commit` that
/// would re-inject an already-archived response. Marking the capture `Discarded`
/// (the same terminal state `compact` assigns via
/// [`agent_doc_capture_io::discard_captures_for_archived_responses`]) settles it once
/// so the false positive cannot resurface.
///
/// `mark_discarded` advances the *active* capture, and the active capture is
/// loaded by the cycle's own `capture_id`, so this only ever discards the
/// capture this cycle owns. Returns `true` when a capture was reconciled.
pub fn reconcile_compacted_committed_capture(file: &Path) -> Result<bool> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(false);
    };
    if state.phase != agent_doc_turn::CyclePhase::Committed {
        return Ok(false);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(false);
    };
    let Some(capture) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
    else {
        return Ok(false);
    };
    if capture.cycle_id != state.cycle_id {
        return Ok(false);
    }
    if let Some(response_sha256) = state.response_sha256.as_deref()
        && response_sha256 != capture.response_sha256
    {
        return Ok(false);
    }
    if capture.response_body.trim().is_empty() {
        return Ok(false);
    }
    let Some(head) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(false);
    };
    let response_in_commit_surface =
        agent_doc_turn::response_replay::response_materialized_in_content(
            &capture.response_body,
            &head,
        );
    let response_in_referenced_compact_archive =
        response_materialized_in_head_compact_archive(file, &capture.response_body, &head);
    let decision = agent_doc_workflow::capture::decide_capture_closeout_materialization(
        agent_doc_workflow::capture::CaptureCloseoutMaterializationEvidence {
            active_capture: true,
            capture_terminal: false,
            commit_surface_available: true,
            response_in_commit_surface,
            response_in_referenced_compact_archive,
        },
    );
    // Only archive-backed materialization needs reconciliation here. Inline HEAD
    // materialization is the normal committed response path and all missing
    // evidence remains fail-closed for the response-replay guard.
    if decision
        != agent_doc_workflow::capture::CaptureCloseoutMaterializationDecision::Allow(
            agent_doc_workflow::capture::CaptureCloseoutMaterializationBasis::ReferencedCompactArchive,
        )
    {
        return Ok(false);
    }
    let retired = agent_doc_cycle_state_io::retire_projected_captured_response(
        file,
        &state.cycle_id,
        &capture.capture_id,
        "referenced_compact_archive",
    )?;
    if !retired {
        return Ok(false);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "reconcile_compacted_committed_capture file={} capture_id={} cycle_id={}",
            file.display(),
            capture.capture_id,
            state.cycle_id
        ),
    );
    eprintln!(
        "[preflight] reconciled compacted committed capture {} for {} (response archived out of HEAD; retired the ledger projection so the stuck-capture false positive cannot resurface)",
        capture.capture_id,
        file.display()
    );
    Ok(true)
}

fn response_materialized_in_head_compact_archive(
    file: &Path,
    response_body: &str,
    head: &str,
) -> bool {
    agent_doc_archive_io::read_head_compact_archives(file, head)
        .into_iter()
        .any(|archive| {
            agent_doc_turn::response_replay::response_materialized_in_content(
                response_body,
                &archive,
            )
        })
}

fn capture_state_label(state: agent_doc_workflow::capture::CaptureState) -> &'static str {
    match state {
        agent_doc_workflow::capture::CaptureState::Captured => "captured",
        agent_doc_workflow::capture::CaptureState::WriteApplied => "write_applied",
        agent_doc_workflow::capture::CaptureState::Replayed => "replayed",
        agent_doc_workflow::capture::CaptureState::Committed => "committed",
        agent_doc_workflow::capture::CaptureState::Discarded => "discarded",
    }
}

fn ensure_cycle_committed(file: &Path) -> Result<()> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        log_closeout_guard_event(
            file,
            FlowStage::TerminalGuard,
            FlowOutcome::FailedClosed,
            CloseoutGuardReason::MissingCycleState,
        );
        anyhow::bail!("finalize did not persist cycle state");
    };
    if state.is_open() {
        log_closeout_guard_event(
            file,
            FlowStage::TerminalGuard,
            FlowOutcome::Blocked,
            CloseoutGuardReason::OpenCycle,
        );
        anyhow::bail!(
            "finalize left cycle `{}` open at `{}` ({})",
            state.cycle_id,
            state.phase.as_str(),
            state.last_event
        );
    }
    Ok(())
}

struct RetrySnapshotHeadDriftOptions<'a> {
    force_disk: bool,
    commit_mark: &'a str,
    cycle_mark: &'a str,
}

fn retry_snapshot_head_content_hash_drift(
    file: &Path,
    effects: &dyn CloseoutEffects,
    rc: &impl AgentDocContextExt,
    did_commit: &mut bool,
    timer: &mut CloseoutTimer<'_>,
    options: RetrySnapshotHeadDriftOptions<'_>,
) -> Result<()> {
    if !matches!(
        agent_doc_snapshot_io::verify_snapshot_head_content_hash(file)?,
        agent_doc_snapshot_io::SnapshotHeadContentHashStatus::SnapshotDiffersFromHead { .. }
    ) {
        return Ok(());
    }

    eprintln!("[commit] snapshot differs from HEAD after commit - retrying");
    log_closeout_guard_event(
        file,
        FlowStage::SnapshotConvergence,
        FlowOutcome::Blocked,
        CloseoutGuardReason::SnapshotDiffersFromHead,
    );
    *did_commit |= effects.commit_for_authority(file, options.force_disk)?;
    rc.invalidate_head_content();
    rc.invalidate_snapshot_content();
    timer.mark(options.commit_mark);
    ensure_cycle_committed(file)?;
    timer.mark(options.cycle_mark);
    Ok(())
}

pub fn record_terminal_closeout_proof(
    file: &Path,
    did_commit: bool,
    effects: &dyn CloseoutEffects,
    options: CompleteRequiredCloseoutOptions,
) -> Result<()> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("terminal proof: failed to canonicalize {}", file.display()))?;
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        eprintln!(
            "[commit] warning: terminal proof ledger unavailable for {}: project root not found",
            file.display()
        );
        return Ok(());
    };
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(&canonical)? else {
        anyhow::bail!(
            "terminal proof cannot record closeout for {}: missing cycle state",
            file.display()
        );
    };
    if state.phase != agent_doc_turn::CyclePhase::Committed {
        anyhow::bail!(
            "terminal proof cannot record closeout for {}: cycle `{}` is `{}`",
            file.display(),
            state.cycle_id,
            state.phase.as_str()
        );
    }
    let current_doc = effects.resolve_current_document_for_authority(
        &canonical,
        "terminal_closeout_proof",
        options.force_disk,
    )?;
    let file_content = current_doc.content();
    let snapshot_content = agent_doc_snapshot_io::load_document_baseline(&canonical)?
        .with_context(|| {
            format!(
                "terminal proof: missing snapshot for {}",
                canonical.display()
            )
        })?;
    let head_content = agent_doc_git_io::revision::show_head(&canonical)?
        .with_context(|| format!("terminal proof: missing HEAD for {}", canonical.display()))?;
    let file_hash = agent_doc_hash::content_hash(file_content);
    let snapshot_hash = agent_doc_hash::content_hash(&snapshot_content);
    let head_hash = agent_doc_hash::content_hash(&head_content);
    let snapshot_head_raw_match = snapshot_hash == head_hash;
    let snapshot_head_transient_match = matches!(
        agent_doc_snapshot_io::snapshot_commit_status_from_contents(
            Some(&snapshot_content),
            Some(&head_content),
        ),
        agent_doc_snapshot_io::SnapshotCommitStatus::Committed
    );
    if !snapshot_head_raw_match && !snapshot_head_transient_match {
        anyhow::bail!(
            "terminal proof mismatch for {}: file_hash={} snapshot_hash={} head_hash={}",
            file.display(),
            file_hash,
            snapshot_hash,
            head_hash
        );
    }
    let agreement = if file_hash == snapshot_hash && snapshot_head_raw_match {
        "file_snapshot_head"
    } else if file_hash == snapshot_hash {
        "file_snapshot_transient_head"
    } else if snapshot_head_raw_match {
        "snapshot_head_visible_drift"
    } else {
        "snapshot_transient_head_visible_drift"
    };
    let state_file_hash_matches = state.file_hash.as_deref() == Some(file_hash.as_str());
    let state_snapshot_hash_matches =
        state.snapshot_hash.as_deref() == Some(snapshot_hash.as_str());
    let content_hash = agent_doc_hash::content_hash(&format!(
        "cycle_id={}\nphase={}\nlast_event={}\nfile_hash={}\nsnapshot_hash={}\nhead_hash={}\ndid_commit={}\nstate_file_hash_matches={}\nstate_snapshot_hash_matches={}\nagreement={}\n",
        state.cycle_id,
        state.phase.as_str(),
        state.last_event,
        file_hash,
        snapshot_hash,
        head_hash,
        did_commit,
        state_file_hash_matches,
        state_snapshot_hash_matches,
        agreement
    ));
    let recorded_at_ms = now_millis();
    let record = agent_doc_workflow_io::proof_ledger::OperationProofRecord::new(
        agent_doc_workflow_io::proof_ledger::OperationProofInput {
            operation_id: format!("terminal_closeout:{}", state.cycle_id),
            operation_kind: agent_doc_workflow_io::proof_ledger::ProofOperationKind::TerminalProof,
            outcome: agent_doc_workflow_io::proof_ledger::ProofOutcome::Recorded,
            subject_id: Some(state.cycle_id.clone()),
            content_hash,
            proof_kind:
                agent_doc_workflow_io::proof_ledger::ProofEvidenceKind::TerminalStateObserved,
            proof: format!(
                "phase={} last_event={} did_commit={} file_hash={} snapshot_hash={} head_hash={} state_file_hash_matches={} state_snapshot_hash_matches={} capture_id={} response_sha256={} session_check=ok actor_closeout=persisted agreement={}",
                state.phase.as_str(),
                state.last_event,
                did_commit,
                file_hash,
                snapshot_hash,
                head_hash,
                state_file_hash_matches,
                state_snapshot_hash_matches,
                state.capture_id.as_deref().unwrap_or("<none>"),
                state.response_sha256.as_deref().unwrap_or("<none>"),
                agreement
            ),
            recorded_at_ms,
        },
    )?;
    let path = agent_doc_workflow_io::proof_ledger::append_operation_proof(
        &project_root,
        &canonical,
        &record,
    )?;
    agent_doc_cycle_state_io::append_terminal_closeout_proof(
        &canonical,
        agent_doc_cycle_state_io::TerminalCloseoutProofInput {
            cycle_id: &state.cycle_id,
            last_event: &state.last_event,
            did_commit,
            file_hash: &file_hash,
            snapshot_hash: &snapshot_hash,
            head_hash: &head_hash,
            state_file_hash_matches,
            state_snapshot_hash_matches,
            agreement,
            capture_id: state.capture_id.as_deref(),
            response_sha256: state.response_sha256.as_deref(),
            recorded_at_ms,
        },
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "terminal_closeout_proof_recorded file={} operation_id={} ledger={}",
            file.display(),
            record.operation_id,
            path.display()
        ),
    );
    Ok(())
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub fn closeout_recovery_command_for_file(
    file: &Path,
    state: CloseoutRecoveryState,
) -> Option<String> {
    render_closeout_recovery_command(CloseoutRecoveryCommandInput {
        document: file.display().to_string(),
        state,
        open_cycle: open_cycle_recovery_command_input(file),
    })
}

fn open_cycle_recovery_command_input(file: &Path) -> Option<OpenCycleRecoveryCommandInput> {
    let Ok(Some(cycle)) = load_closeout_recovery_cycle_view(file) else {
        return None;
    };
    cycle.open_cycle_recovery_command_input()
}

enum CloseoutRecoveryCycleView {
    Checkpoint(Box<agent_doc_cycle_state_io::CycleState>),
    Projection(Box<agent_doc_cycle_state_io::ProjectedCloseoutState>),
}

impl CloseoutRecoveryCycleView {
    fn recovery_cycle_input(&self) -> Option<CloseoutRecoveryCycleInput> {
        match self {
            Self::Checkpoint(state) => Some(CloseoutRecoveryCycleInput {
                phase: state.phase,
                has_capture: state.capture_id.is_some(),
                has_response_hash: state.response_sha256.is_some(),
                had_pending_mutations: state.had_pending_mutations,
            }),
            Self::Projection(projection) => {
                let phase = projection.phase?;
                Some(CloseoutRecoveryCycleInput {
                    phase,
                    has_capture: projection.capture_id.is_some(),
                    has_response_hash: projection.response_sha256.is_some(),
                    // Projection-only preflight cannot prove the old JSON-only
                    // pending-mutation queue was empty. Fail closed by surfacing
                    // an open-cycle recovery instead of suggesting cancel.
                    had_pending_mutations: matches!(
                        phase,
                        agent_doc_turn::CyclePhase::PreflightStarted
                    ),
                })
            }
        }
    }

    fn open_cycle_recovery_command_input(self) -> Option<OpenCycleRecoveryCommandInput> {
        match self {
            Self::Checkpoint(state) => {
                let state = *state;
                if !state.phase.is_open() {
                    return None;
                }
                let target = state
                    .queue_task_id
                    .clone()
                    .or_else(|| state.prompt_targets.first().cloned());
                let has_pending_mutations = state.had_pending_mutations
                    || !state.pending_done_ids.is_empty()
                    || !state.pending_gated_ids.is_empty()
                    || !state.pending_kept_open_ids.is_empty()
                    || !state.reaped_pending_ids.is_empty();
                Some(OpenCycleRecoveryCommandInput {
                    cycle_id: state.cycle_id,
                    phase: state.phase,
                    target,
                    has_pending_mutations,
                    capture_id: state.capture_id,
                })
            }
            Self::Projection(projection) => {
                let phase = projection.phase?;
                if !phase.is_open() {
                    return None;
                }
                Some(OpenCycleRecoveryCommandInput {
                    cycle_id: projection.cycle_id?,
                    phase,
                    target: None,
                    has_pending_mutations: matches!(
                        phase,
                        agent_doc_turn::CyclePhase::PreflightStarted
                    ),
                    capture_id: projection.capture_id,
                })
            }
        }
    }
}

fn load_closeout_recovery_cycle_view(file: &Path) -> Result<Option<CloseoutRecoveryCycleView>> {
    if let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? {
        return Ok(Some(CloseoutRecoveryCycleView::Checkpoint(Box::new(state))));
    }
    Ok(agent_doc_cycle_state_io::load_closeout_projection(file)?
        .filter(|projection| projection.cycle_id.is_some() && projection.phase.is_some())
        .map(Box::new)
        .map(CloseoutRecoveryCycleView::Projection))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseoutRecoveryEvidence {
    pub visible_markdown_hash: String,
    pub snapshot_hash: Option<String>,
    pub active_cycle: Option<CloseoutCycleEvidence>,
    pub active_capture: Option<CloseoutCaptureEvidence>,
    pub response_body: CloseoutResponseBodyEvidence,
    pub queue_only_drift: Option<CloseoutQueueOnlyDriftEvidence>,
    pub snapshot_head_drift: Option<CloseoutRecoveryDrift>,
    pub snapshot_visible_drift: Option<CloseoutRecoveryDrift>,
    pub editor_ipc: CloseoutEditorIpcEvidence,
    pub binary_freshness: CloseoutBinaryFreshnessEvidence,
}

impl CloseoutRecoveryEvidence {
    pub fn stale_capture_supersession_proof(&self) -> Option<&str> {
        match &self.response_body {
            CloseoutResponseBodyEvidence::SupersededByVisibleExchange { proof, .. } => {
                Some(proof.as_str())
            }
            _ => None,
        }
    }

    fn reports_missing_response_body(&self) -> bool {
        matches!(
            self.response_body,
            CloseoutResponseBodyEvidence::EmptyCapture { .. }
                | CloseoutResponseBodyEvidence::SupersededByVisibleExchange { .. }
                | CloseoutResponseBodyEvidence::MissingFromVisible { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseoutCycleEvidence {
    pub cycle_id: String,
    pub phase: agent_doc_turn::CyclePhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseoutCaptureEvidence {
    pub capture_id: String,
    pub cycle_id: String,
    pub state: agent_doc_workflow::capture::CaptureState,
    pub response_sha256: String,
}

#[derive(Debug, Clone)]
struct CloseoutCapturedResponse {
    capture_id: String,
    cycle_id: String,
    state: agent_doc_workflow::capture::CaptureState,
    response_sha256: String,
    response_body: String,
    file_hash: Option<String>,
    baseline_content: Option<String>,
    has_baseline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutResponseBodyEvidence {
    NoActiveCapture,
    EmptyCapture { capture_id: String },
    PresentInVisible { capture_id: String },
    SupersededByVisibleExchange { capture_id: String, proof: String },
    MissingFromVisible { capture_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseoutQueueOnlyDriftEvidence {
    pub file_hash_mismatch: bool,
    pub snapshot_hash_mismatch: bool,
    pub proven_queue_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutEditorIpcEvidence {
    NoLiveBuffer {
        socket_degraded: bool,
    },
    FreshLiveBuffer {
        live_buffer_count: usize,
        socket_degraded: bool,
    },
    DivergedLiveBuffer {
        live_buffer_count: usize,
        editor_id: Option<String>,
        live_len: usize,
        live_hash: String,
        socket_degraded: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutBinaryFreshnessEvidence {
    NoStaleWarning,
    Stale { warning: String },
}

pub fn gather_closeout_recovery_evidence(
    file: &Path,
    effects: &dyn CloseoutEffects,
) -> Result<CloseoutRecoveryEvidence> {
    let visible_doc = effects.resolve_current_document(file, "closeout_recovery_evidence")?;
    let visible = visible_doc.content();
    let visible_markdown_hash = agent_doc_capture_io::replay_file_hash(visible);
    let snapshot = agent_doc_snapshot_io::load_document_baseline(file)?;
    let snapshot_hash = snapshot.as_deref().map(agent_doc_hash::content_hash);
    let head = agent_doc_git_io::revision::show_head(file)?;
    let snapshot_head_drift = match (snapshot.as_deref(), head.as_deref()) {
        (Some(snapshot), Some(head)) if snapshot != head => {
            Some(classify_snapshot_head_drift(snapshot, head))
        }
        _ => None,
    };
    let snapshot_visible_drift = match snapshot.as_deref() {
        Some(snapshot) if snapshot != visible => {
            Some(classify_snapshot_visible_drift(snapshot, visible))
        }
        _ => None,
    };
    let cycle = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let active_cycle = cycle.as_ref().map(|state| CloseoutCycleEvidence {
        cycle_id: state.cycle_id.clone(),
        phase: state.phase,
    });
    let capture = closeout_captured_response_for_state(file, cycle.as_ref())?;
    let active_capture = capture.as_ref().map(|capture| CloseoutCaptureEvidence {
        capture_id: capture.capture_id.clone(),
        cycle_id: capture.cycle_id.clone(),
        state: capture.state,
        response_sha256: capture.response_sha256.clone(),
    });
    let response_body = closeout_response_body_evidence(visible, capture.as_ref());
    let queue_only_drift = closeout_queue_only_drift_evidence(
        visible,
        visible_markdown_hash.as_str(),
        capture.as_ref(),
    )?;
    let editor_ipc = closeout_editor_ipc_evidence(&visible_doc);
    let binary_freshness =
        match agent_doc_controller_io::project_controller::recycle_stale_supervisor_for_turn_stage(
            file,
            "closeout_evidence",
        ) {
            Some(warning) => CloseoutBinaryFreshnessEvidence::Stale { warning },
            None => CloseoutBinaryFreshnessEvidence::NoStaleWarning,
        };

    Ok(CloseoutRecoveryEvidence {
        visible_markdown_hash,
        snapshot_hash,
        active_cycle,
        active_capture,
        response_body,
        queue_only_drift,
        snapshot_head_drift,
        snapshot_visible_drift,
        editor_ipc,
        binary_freshness,
    })
}

pub fn observe_closeout_recovery_evidence(
    file: &Path,
    effects: &dyn CloseoutEffects,
) -> Result<CloseoutRecoveryEvidence> {
    let evidence = gather_closeout_recovery_evidence(file, effects)?;
    record_closeout_recovery_evidence(file, &evidence)?;
    Ok(evidence)
}

pub fn load_current_observed_closeout_recovery_evidence(
    file: &Path,
    effects: &dyn CloseoutEffects,
) -> Result<Option<CloseoutRecoveryEvidence>> {
    let visible_doc =
        effects.resolve_current_document(file, "observed_closeout_recovery_evidence")?;
    let visible_markdown_hash = agent_doc_capture_io::replay_file_hash(visible_doc.content());
    let current_snapshot_hash = agent_doc_snapshot_io::load_document_baseline(file)?
        .as_deref()
        .map(agent_doc_hash::content_hash);
    let Some(projection) = agent_doc_cycle_state_io::load_latest_closeout_recovery_evidence(file)?
    else {
        return Ok(None);
    };
    if projection.visible_markdown_hash != visible_markdown_hash {
        return Ok(None);
    }
    if current_snapshot_hash.is_some() && projection.snapshot_hash != current_snapshot_hash {
        return Ok(None);
    }
    Ok(projected_closeout_recovery_evidence(projection))
}

fn projected_closeout_recovery_evidence(
    projection: agent_doc_state_backbone::CloseoutRecoveryEvidenceProjection,
) -> Option<CloseoutRecoveryEvidence> {
    let active_cycle = match (projection.active_cycle_id, projection.active_cycle_phase) {
        (Some(cycle_id), Some(phase)) => Some(CloseoutCycleEvidence { cycle_id, phase }),
        _ => None,
    };
    let active_capture = match (
        projection.active_capture_id,
        projection.active_capture_cycle_id,
        projection.active_capture_state,
        projection.active_capture_response_sha256,
    ) {
        (Some(capture_id), Some(cycle_id), Some(state), Some(response_sha256)) => {
            Some(CloseoutCaptureEvidence {
                capture_id,
                cycle_id,
                state: capture_state_from_label(&state)?,
                response_sha256,
            })
        }
        _ => None,
    };
    Some(CloseoutRecoveryEvidence {
        visible_markdown_hash: projection.visible_markdown_hash,
        snapshot_hash: projection.snapshot_hash,
        active_cycle,
        active_capture,
        response_body: flow_response_body_evidence(projection.response_body),
        queue_only_drift: projection
            .queue_only_drift
            .map(flow_queue_only_drift_evidence),
        snapshot_head_drift: projection.snapshot_head_drift.map(flow_recovery_drift),
        snapshot_visible_drift: projection.snapshot_visible_drift.map(flow_recovery_drift),
        editor_ipc: flow_editor_ipc_evidence(projection.editor_ipc),
        binary_freshness: flow_binary_freshness_evidence(projection.binary_freshness),
    })
}

fn record_closeout_recovery_evidence(
    file: &Path,
    evidence: &CloseoutRecoveryEvidence,
) -> Result<bool> {
    let active_cycle = evidence.active_cycle.as_ref();
    let active_capture = evidence.active_capture.as_ref();
    agent_doc_cycle_state_io::append_closeout_recovery_evidence(
        file,
        agent_doc_cycle_state_io::CloseoutRecoveryEvidenceInput {
            visible_markdown_hash: evidence.visible_markdown_hash.as_str(),
            snapshot_hash: evidence.snapshot_hash.as_deref(),
            active_cycle_id: active_cycle.map(|cycle| cycle.cycle_id.as_str()),
            active_cycle_phase: active_cycle.map(|cycle| cycle.phase),
            active_capture_id: active_capture.map(|capture| capture.capture_id.as_str()),
            active_capture_cycle_id: active_capture.map(|capture| capture.cycle_id.as_str()),
            active_capture_state: active_capture.map(|capture| capture_state_label(capture.state)),
            active_capture_response_sha256: active_capture
                .map(|capture| capture.response_sha256.as_str()),
            response_body: backbone_response_body_evidence(&evidence.response_body),
            queue_only_drift: evidence
                .queue_only_drift
                .as_ref()
                .map(backbone_queue_only_drift_evidence),
            snapshot_head_drift: evidence.snapshot_head_drift.map(backbone_recovery_drift),
            snapshot_visible_drift: evidence.snapshot_visible_drift.map(backbone_recovery_drift),
            editor_ipc: backbone_editor_ipc_evidence(&evidence.editor_ipc),
            binary_freshness: backbone_binary_freshness_evidence(&evidence.binary_freshness),
            recorded_at_ms: now_millis(),
        },
    )
}

fn backbone_response_body_evidence(
    evidence: &CloseoutResponseBodyEvidence,
) -> agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence {
    match evidence {
        CloseoutResponseBodyEvidence::NoActiveCapture => {
            agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::NoActiveCapture
        }
        CloseoutResponseBodyEvidence::EmptyCapture { capture_id } => {
            agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::EmptyCapture {
                capture_id: capture_id.clone(),
            }
        }
        CloseoutResponseBodyEvidence::PresentInVisible { capture_id } => {
            agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::PresentInVisible {
                capture_id: capture_id.clone(),
            }
        }
        CloseoutResponseBodyEvidence::SupersededByVisibleExchange { capture_id, proof } => {
            agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::SupersededByVisibleExchange {
                capture_id: capture_id.clone(),
                proof: proof.clone(),
            }
        }
        CloseoutResponseBodyEvidence::MissingFromVisible { capture_id } => {
            agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::MissingFromVisible {
                capture_id: capture_id.clone(),
            }
        }
    }
}

fn backbone_queue_only_drift_evidence(
    evidence: &CloseoutQueueOnlyDriftEvidence,
) -> agent_doc_state_backbone::CloseoutRecoveryQueueOnlyDriftEvidence {
    agent_doc_state_backbone::CloseoutRecoveryQueueOnlyDriftEvidence {
        file_hash_mismatch: evidence.file_hash_mismatch,
        snapshot_hash_mismatch: evidence.snapshot_hash_mismatch,
        proven_queue_only: evidence.proven_queue_only,
    }
}

fn backbone_recovery_drift(
    drift: CloseoutRecoveryDrift,
) -> agent_doc_state_backbone::CloseoutRecoveryDriftEvidence {
    match drift {
        CloseoutRecoveryDrift::BoundaryOnly => {
            agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::BoundaryOnly
        }
        CloseoutRecoveryDrift::MetadataOnly => {
            agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::MetadataOnly
        }
        CloseoutRecoveryDrift::Content => {
            agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::Content
        }
    }
}

fn backbone_editor_ipc_evidence(
    evidence: &CloseoutEditorIpcEvidence,
) -> agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence {
    match evidence {
        CloseoutEditorIpcEvidence::NoLiveBuffer { socket_degraded } => {
            agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence::NoLiveBuffer {
                socket_degraded: *socket_degraded,
            }
        }
        CloseoutEditorIpcEvidence::FreshLiveBuffer {
            live_buffer_count,
            socket_degraded,
        } => agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence::FreshLiveBuffer {
            live_buffer_count: *live_buffer_count,
            socket_degraded: *socket_degraded,
        },
        CloseoutEditorIpcEvidence::DivergedLiveBuffer {
            live_buffer_count,
            editor_id,
            live_len,
            live_hash,
            socket_degraded,
        } => agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence::DivergedLiveBuffer {
            live_buffer_count: *live_buffer_count,
            editor_id: editor_id.clone(),
            live_len: *live_len,
            live_hash: live_hash.clone(),
            socket_degraded: *socket_degraded,
        },
    }
}

fn backbone_binary_freshness_evidence(
    evidence: &CloseoutBinaryFreshnessEvidence,
) -> agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence {
    match evidence {
        CloseoutBinaryFreshnessEvidence::NoStaleWarning => {
            agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence::NoStaleWarning
        }
        CloseoutBinaryFreshnessEvidence::Stale { warning } => {
            agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence::Stale {
                warning: warning.clone(),
            }
        }
    }
}

fn flow_response_body_evidence(
    evidence: agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence,
) -> CloseoutResponseBodyEvidence {
    match evidence {
        agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::NoActiveCapture => {
            CloseoutResponseBodyEvidence::NoActiveCapture
        }
        agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::EmptyCapture {
            capture_id,
        } => CloseoutResponseBodyEvidence::EmptyCapture { capture_id },
        agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::PresentInVisible {
            capture_id,
        } => CloseoutResponseBodyEvidence::PresentInVisible { capture_id },
        agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::SupersededByVisibleExchange {
            capture_id,
            proof,
        } => CloseoutResponseBodyEvidence::SupersededByVisibleExchange { capture_id, proof },
        agent_doc_state_backbone::CloseoutRecoveryResponseBodyEvidence::MissingFromVisible {
            capture_id,
        } => CloseoutResponseBodyEvidence::MissingFromVisible { capture_id },
    }
}

fn flow_queue_only_drift_evidence(
    evidence: agent_doc_state_backbone::CloseoutRecoveryQueueOnlyDriftEvidence,
) -> CloseoutQueueOnlyDriftEvidence {
    CloseoutQueueOnlyDriftEvidence {
        file_hash_mismatch: evidence.file_hash_mismatch,
        snapshot_hash_mismatch: evidence.snapshot_hash_mismatch,
        proven_queue_only: evidence.proven_queue_only,
    }
}

fn flow_recovery_drift(
    drift: agent_doc_state_backbone::CloseoutRecoveryDriftEvidence,
) -> CloseoutRecoveryDrift {
    match drift {
        agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::BoundaryOnly => {
            CloseoutRecoveryDrift::BoundaryOnly
        }
        agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::MetadataOnly => {
            CloseoutRecoveryDrift::MetadataOnly
        }
        agent_doc_state_backbone::CloseoutRecoveryDriftEvidence::Content => {
            CloseoutRecoveryDrift::Content
        }
    }
}

fn flow_editor_ipc_evidence(
    evidence: agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence,
) -> CloseoutEditorIpcEvidence {
    match evidence {
        agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence::NoLiveBuffer {
            socket_degraded,
        } => CloseoutEditorIpcEvidence::NoLiveBuffer { socket_degraded },
        agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence::FreshLiveBuffer {
            live_buffer_count,
            socket_degraded,
        } => CloseoutEditorIpcEvidence::FreshLiveBuffer {
            live_buffer_count,
            socket_degraded,
        },
        agent_doc_state_backbone::CloseoutRecoveryEditorIpcEvidence::DivergedLiveBuffer {
            live_buffer_count,
            editor_id,
            live_len,
            live_hash,
            socket_degraded,
        } => CloseoutEditorIpcEvidence::DivergedLiveBuffer {
            live_buffer_count,
            editor_id,
            live_len,
            live_hash,
            socket_degraded,
        },
    }
}

fn flow_binary_freshness_evidence(
    evidence: agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence,
) -> CloseoutBinaryFreshnessEvidence {
    match evidence {
        agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence::NoStaleWarning => {
            CloseoutBinaryFreshnessEvidence::NoStaleWarning
        }
        agent_doc_state_backbone::CloseoutRecoveryBinaryFreshnessEvidence::Stale { warning } => {
            CloseoutBinaryFreshnessEvidence::Stale { warning }
        }
    }
}

fn capture_state_from_label(label: &str) -> Option<agent_doc_workflow::capture::CaptureState> {
    match label {
        "captured" => Some(agent_doc_workflow::capture::CaptureState::Captured),
        "write_applied" => Some(agent_doc_workflow::capture::CaptureState::WriteApplied),
        "replayed" => Some(agent_doc_workflow::capture::CaptureState::Replayed),
        "committed" => Some(agent_doc_workflow::capture::CaptureState::Committed),
        "discarded" => Some(agent_doc_workflow::capture::CaptureState::Discarded),
        _ => None,
    }
}

fn capture_state_for_cycle_phase(
    phase: agent_doc_turn::CyclePhase,
) -> agent_doc_workflow::capture::CaptureState {
    match phase {
        agent_doc_turn::CyclePhase::PreflightStarted
        | agent_doc_turn::CyclePhase::ResponseCaptured => {
            agent_doc_workflow::capture::CaptureState::Captured
        }
        agent_doc_turn::CyclePhase::WriteApplied => {
            agent_doc_workflow::capture::CaptureState::WriteApplied
        }
        agent_doc_turn::CyclePhase::Committed => {
            agent_doc_workflow::capture::CaptureState::Committed
        }
        agent_doc_turn::CyclePhase::Abandoned => {
            agent_doc_workflow::capture::CaptureState::Discarded
        }
    }
}

fn closeout_captured_response_for_state(
    file: &Path,
    state: Option<&agent_doc_cycle_state_io::CycleState>,
) -> Result<Option<CloseoutCapturedResponse>> {
    let Some(state) = state else {
        return Ok(None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(None);
    };
    let Some(projection) = agent_doc_cycle_state_io::load_closeout_projection(file)? else {
        return Ok(None);
    };
    if projection.captured_response_retired_reason.is_some() {
        return Ok(None);
    }
    let Some(projected) = projection.captured_response else {
        return Ok(None);
    };
    if projected.capture_id != capture_id {
        return Ok(None);
    }
    if projected.cycle_id != state.cycle_id
        || state
            .response_sha256
            .as_deref()
            .is_some_and(|sha| sha != projected.response_sha256)
    {
        return Ok(None);
    }
    let has_baseline = projected.file_hash.is_some() && projected.baseline_content.is_some();
    Ok(Some(CloseoutCapturedResponse {
        capture_id: projected.capture_id,
        cycle_id: projected.cycle_id,
        state: capture_state_for_cycle_phase(state.phase),
        response_sha256: projected.response_sha256,
        response_body: projected.response_body,
        file_hash: projected.file_hash,
        baseline_content: projected.baseline_content,
        has_baseline,
    }))
}

fn closeout_response_body_evidence(
    visible: &str,
    capture: Option<&CloseoutCapturedResponse>,
) -> CloseoutResponseBodyEvidence {
    let Some(capture) = capture else {
        return CloseoutResponseBodyEvidence::NoActiveCapture;
    };
    if capture.response_body.trim().is_empty() {
        return CloseoutResponseBodyEvidence::EmptyCapture {
            capture_id: capture.capture_id.clone(),
        };
    }
    if agent_doc_turn::response_replay::response_already_applied(visible, &capture.response_body)
        || agent_doc_turn::response_replay::response_already_applied_after_prefix_strip(
            visible,
            &capture.response_body,
        )
    {
        return CloseoutResponseBodyEvidence::PresentInVisible {
            capture_id: capture.capture_id.clone(),
        };
    }
    if let Some(heading) =
        agent_doc_turn::response_replay::first_response_heading_line(&capture.response_body)
        && agent_doc_turn::response_replay::live_exchange_answers_heading(visible, heading)
    {
        return CloseoutResponseBodyEvidence::SupersededByVisibleExchange {
            capture_id: capture.capture_id.clone(),
            proof: format!("response heading {heading:?} is already answered in live exchange"),
        };
    }
    CloseoutResponseBodyEvidence::MissingFromVisible {
        capture_id: capture.capture_id.clone(),
    }
}

fn closeout_queue_only_drift_evidence(
    visible: &str,
    visible_hash: &str,
    capture: Option<&CloseoutCapturedResponse>,
) -> Result<Option<CloseoutQueueOnlyDriftEvidence>> {
    let Some(capture) = capture else {
        return Ok(None);
    };
    if !capture.has_baseline {
        return Ok(None);
    }
    let file_hash_mismatch = capture.file_hash.as_deref() != Some(visible_hash);
    if !file_hash_mismatch {
        return Ok(None);
    }
    let proven_queue_only = file_hash_mismatch
        && agent_doc_capture_io::live_drift_is_queue_only_against_baseline(
            visible,
            capture.baseline_content.as_deref(),
        )?;
    Ok(Some(CloseoutQueueOnlyDriftEvidence {
        file_hash_mismatch,
        snapshot_hash_mismatch: false,
        proven_queue_only,
    }))
}

fn closeout_editor_ipc_evidence(
    visible_doc: &agent_doc_document_realtime::CurrentDocument,
) -> CloseoutEditorIpcEvidence {
    // Transport health may trigger endpoint re-registration or one supervisor
    // recycle, but it never changes the document authority or elects disk.
    let socket_degraded = false;
    if visible_doc.authority() == agent_doc_document_realtime::DocAuthority::EditorBuffer {
        CloseoutEditorIpcEvidence::FreshLiveBuffer {
            live_buffer_count: 1,
            socket_degraded,
        }
    } else {
        CloseoutEditorIpcEvidence::NoLiveBuffer { socket_degraded }
    }
}

pub fn decide_closeout_recovery(
    file: &Path,
    input: CloseoutRecoveryDecisionInput<'_>,
    effects: &dyn CloseoutEffects,
) -> CloseoutRecoveryDecision {
    let state = classify_closeout_recovery_state_for_file(file, effects);
    let evidence = load_current_observed_closeout_recovery_evidence(file, effects)
        .ok()
        .flatten()
        .or_else(|| {
            let evidence = gather_closeout_recovery_evidence(file, effects).ok();
            if let Some(evidence) = evidence.as_ref() {
                let _ = record_closeout_recovery_evidence(file, evidence);
            }
            evidence
        });
    let stale_capture_supersession_proof = input.stale_capture_supersession_proof.or_else(|| {
        evidence
            .as_ref()
            .and_then(CloseoutRecoveryEvidence::stale_capture_supersession_proof)
    });
    let recovery_command = closeout_recovery_command_for_file(file, state);
    closeout_recovery_decision_from_state(
        state,
        CloseoutRecoveryDecisionInput {
            stale_capture_supersession_proof,
            ..input
        },
        recovery_command.as_deref(),
    )
}

/// Outcome of [`apply_closeout_recovery`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryApplication {
    /// `Clean` — no recovery needed.
    NothingToDo,
    /// A provably-safe recovery action was performed.
    Applied {
        state: CloseoutRecoveryState,
        action: String,
    },
    /// The state is recoverable but not *unambiguously* safe to auto-apply (the
    /// authoritative side is ambiguous, or a response body must be preserved), so
    /// nothing was mutated. Carries the recommended manual command.
    NotApplied {
        state: CloseoutRecoveryState,
        reason: String,
        recommended: String,
    },
}

/// `#recursive-repair-apply`: apply the *unambiguously safe* closeout recovery for
/// `file` in one call, replacing the manual `cancel` → `reset --from-current` →
/// `commit` expert sequence for the common cases.
///
/// Only states whose recovery cannot lose or revert real content are
/// auto-applied:
/// - `OpenEmptyPreflight` → abandon the empty preflight cycle
///   ([`crate::repair::cancel_preflight_cycle`]).
/// - `BoundaryOnlyDrift` → `git::commit` (agent-doc-generated boundary /
///   prompt-prefix artifacts only; the commit path normalizes them and cannot
///   revert user/response content).
///
/// `QueueMetadataDrift` / `RecoveryProjectionVisibleDrift` are auto-applied *only when the
/// authoritative side is provable* (`#recovery-drift-authoritative-side`). The
/// safe direction (commit the local side vs restore from HEAD) is decided by
/// [`metadata_drift_authority`]: a live auto-queue continuation present in HEAD
/// but dropped by the local metadata drift means HEAD is authoritative
/// (restore-from-HEAD), because legitimate queue-head consumption always surfaces
/// as response/content drift, never as metadata-only drift. This closes the
/// live-observed gap where a spurious `queue_active:false` working-drift would be
/// wrongly committed. A genuinely ambiguous direction (both sides carry distinct
/// live continuation heads with no consuming response) still returns `NotApplied`.
/// `UnsafeUserContentDrift`, `OpenCycle`, `MissingResponseBody`, and the reserved
/// states also fail closed because they need a preserved response, not a metadata
/// operation.
pub fn apply_closeout_recovery(
    file: &Path,
    effects: &dyn CloseoutEffects,
) -> Result<RecoveryApplication> {
    let state = classify_closeout_recovery_state_for_file(file, effects);
    if matches!(
        state,
        CloseoutRecoveryState::OpenCycle
            | CloseoutRecoveryState::MissingResponseBody
            | CloseoutRecoveryState::UnsafeUserContentDrift
    ) {
        let decision =
            decide_closeout_recovery(file, CloseoutRecoveryDecisionInput::default(), effects);
        if let CloseoutRecoveryDecision::RetireStaleCapture { proof, .. } = decision {
            let visible_doc =
                effects.resolve_current_document(file, "closeout_recovery_retire_stale_capture")?;
            apply_closeout_recovery_mutation(
                file,
                CloseoutRecoveryMutation::RetireStaleCapture {
                    content: Some(visible_doc.content()),
                    clear_pending_response: true,
                    clear_undo_content: true,
                    mark_cycle_committed_event: None,
                    mark_cycle_abandoned_event: Some("closeout_recovery_retire_stale_capture"),
                    reason: CloseoutRecoveryMutationReason::RetireSupersededCapturedOnlyOrphan,
                },
                effects,
            )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "closeout_recovery_retire_stale_capture file={} proof={}",
                    file.display(),
                    proof.replace('\n', " ")
                ),
            );
            return Ok(RecoveryApplication::Applied {
                state,
                action: format!("retired stale captured response recovery ({proof})"),
            });
        }
    }
    match state {
        CloseoutRecoveryState::Clean => Ok(RecoveryApplication::NothingToDo),
        CloseoutRecoveryState::OpenEmptyPreflight => {
            effects.cancel_preflight_cycle(file)?;
            Ok(RecoveryApplication::Applied {
                state,
                action: "abandoned the empty preflight cycle".to_string(),
            })
        }
        CloseoutRecoveryState::BoundaryOnlyDrift => {
            effects.commit(file)?;
            Ok(RecoveryApplication::Applied {
                state,
                action: "committed boundary / answered-prompt-prefix artifact drift".to_string(),
            })
        }
        CloseoutRecoveryState::QueueMetadataDrift
        | CloseoutRecoveryState::RecoveryProjectionVisibleDrift => {
            apply_metadata_drift_recovery(file, state, effects)
        }
        other => Ok(RecoveryApplication::NotApplied {
            state: other,
            reason: "auto-apply withheld — recovery requires a preserved response body or open-cycle resolution, not a metadata operation".to_string(),
            recommended: closeout_recovery_command_for_file(file, other).unwrap_or_default(),
        }),
    }
}

/// Durable mutation primitive for closeout recovery (`#smrecoverymutate`).
///
/// Policy decides *which* recovery is allowed before this point. This primitive
/// owns the shared mutation mechanics so replay-baseline refresh, stale-capture
/// retirement, reset-from-visible, and restore-from-HEAD cannot each rebuild
/// snapshots / CRDT / capture state slightly differently.
pub enum CloseoutRecoveryMutation<'a> {
    RefreshReplayBaseline {
        capture: &'a agent_doc_capture_io::CaptureRecord,
        current_file_hash: &'a str,
        current_snapshot_hash: Option<&'a str>,
        reason: CloseoutRecoveryMutationReason,
    },
    RefreshRecoveryProjectionFromContent {
        content: &'a str,
        write_visible_file: bool,
        reason: CloseoutRecoveryMutationReason,
    },
    RetireStaleCapture {
        content: Option<&'a str>,
        clear_pending_response: bool,
        clear_undo_content: bool,
        mark_cycle_committed_event: Option<&'a str>,
        mark_cycle_abandoned_event: Option<&'a str>,
        reason: CloseoutRecoveryMutationReason,
    },
}

pub fn apply_closeout_recovery_mutation(
    file: &Path,
    mutation: CloseoutRecoveryMutation<'_>,
    effects: &dyn CloseoutEffects,
) -> Result<()> {
    match mutation {
        CloseoutRecoveryMutation::RefreshReplayBaseline {
            capture,
            current_file_hash,
            current_snapshot_hash,
            reason,
        } => {
            let changed = agent_doc_capture_io::refresh_replay_baseline_for_recovery(
                file,
                capture,
                current_file_hash,
                current_snapshot_hash,
                None,
                reason.capture_refresh_event(),
                reason.capture_refresh_message(),
            )?;
            if changed {
                log_closeout_recovery_mutation(file, "refresh_replay_baseline", reason);
            }
        }
        CloseoutRecoveryMutation::RefreshRecoveryProjectionFromContent {
            content,
            write_visible_file,
            reason,
        } => {
            if write_visible_file {
                let doc =
                    effects.resolve_current_document(file, "closeout_recovery_restore_visible")?;
                effects.write_current_document(
                    &doc,
                    content,
                    "closeout_recovery_restore_visible",
                )?;
            }
            refresh_recovery_projection_from_content(file, content)?;
            log_closeout_recovery_mutation(
                file,
                "refresh_recovery_projection_from_content",
                reason,
            );
        }
        CloseoutRecoveryMutation::RetireStaleCapture {
            content,
            clear_pending_response,
            clear_undo_content,
            mark_cycle_committed_event,
            mark_cycle_abandoned_event,
            reason,
        } => {
            let _ = clear_pending_response;
            if clear_undo_content && let Err(e) = agent_doc_snapshot_io::clear_undo_content(file) {
                eprintln!("[repair] warning: failed to clear undo checkpoint: {}", e);
            }
            agent_doc_capture_io::mark_discarded(file)?;
            if let Some(content) = content {
                refresh_recovery_projection_from_content(file, content)?;
            }
            if let Some(event) = mark_cycle_committed_event {
                effects.mark_committed_frontmatter(file, event, content, content)?;
            } else if let Some(event) = mark_cycle_abandoned_event {
                let resolved_content;
                let content = match content {
                    Some(content) => Some(content),
                    None => {
                        resolved_content = effects
                            .resolve_current_document(
                                file,
                                "closeout_recovery_retire_stale_capture",
                            )
                            .ok()
                            .map(|doc| doc.content().to_string());
                        resolved_content.as_deref()
                    }
                };
                effects.mark_abandoned_frontmatter(file, event, content, content)?;
            }
            log_closeout_recovery_mutation(file, "retire_stale_capture", reason);
        }
    }
    Ok(())
}

fn log_closeout_recovery_mutation(
    file: &Path,
    action: &str,
    reason: CloseoutRecoveryMutationReason,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "closeout_recovery_mutation file={} action={} reason={}",
            file.display(),
            action,
            reason.as_str()
        ),
    );
}

/// Apply the provably-safe recovery for `QueueMetadataDrift` /
/// `RecoveryProjectionVisibleDrift`.
fn apply_metadata_drift_recovery(
    file: &Path,
    state: CloseoutRecoveryState,
    effects: &dyn CloseoutEffects,
) -> Result<RecoveryApplication> {
    let head = agent_doc_git_io::revision::show_head(file)?;
    let snapshot = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let visible_doc = effects
        .resolve_current_document(file, "closeout_metadata_drift_recovery")
        .ok();
    // For QueueMetadataDrift the local (commit-candidate) side is the snapshot;
    // for RecoveryProjectionVisibleDrift the snapshot already matches HEAD and the local side
    // is the visible/working file.
    let local = match state {
        CloseoutRecoveryState::QueueMetadataDrift => snapshot.as_deref(),
        _ => visible_doc.as_ref().map(|doc| doc.content()),
    };
    let (Some(local), Some(head)) = (local, head.as_deref()) else {
        return Ok(RecoveryApplication::NotApplied {
            state,
            reason: "auto-apply withheld — could not load both the local and HEAD document sides to prove the authoritative direction".to_string(),
            recommended: closeout_recovery_command_for_file(file, state).unwrap_or_default(),
        });
    };

    match metadata_drift_authority(local, head) {
        MetadataDriftAuthority::Local => {
            // Commit the local side forward. For RecoveryProjectionVisibleDrift the snapshot
            // is HEAD-equal, so rebuild recovery projections from the visible file first so
            // the selective `git::commit` stages the accepted working metadata.
            if state == CloseoutRecoveryState::RecoveryProjectionVisibleDrift {
                apply_closeout_recovery_mutation(
                    file,
                    CloseoutRecoveryMutation::RefreshRecoveryProjectionFromContent {
                        content: local,
                        write_visible_file: false,
                        reason: CloseoutRecoveryMutationReason::ResetFromVisible,
                    },
                    effects,
                )?;
            } else {
                log_closeout_recovery_mutation(
                    file,
                    "commit_metadata_drift",
                    CloseoutRecoveryMutationReason::CommitQueueMetadataDrift,
                );
            }
            effects.commit(file)?;
            Ok(RecoveryApplication::Applied {
                state,
                action:
                    "committed local metadata drift (local side authoritative — no live HEAD queue continuation at risk)"
                        .to_string(),
            })
        }
        MetadataDriftAuthority::Head => {
            // HEAD's live queue continuation is authoritative; discard the spurious
            // local metadata drift by restoring the visible file and state projections from
            // HEAD. No new commit — HEAD already holds the authoritative content.
            apply_closeout_recovery_mutation(
                file,
                CloseoutRecoveryMutation::RefreshRecoveryProjectionFromContent {
                    content: head,
                    write_visible_file: true,
                    reason: CloseoutRecoveryMutationReason::RestoreHeadMetadata,
                },
                effects,
            )?;
            Ok(RecoveryApplication::Applied {
                state,
                action:
                    "restored the document and state projections from HEAD (committed queue continuation authoritative; discarded spurious local metadata drift)"
                        .to_string(),
            })
        }
        MetadataDriftAuthority::Ambiguous => Ok(RecoveryApplication::NotApplied {
            state,
            reason: "auto-apply withheld — both the local and HEAD sides carry distinct live queue continuation heads with no consuming response; the authoritative direction is ambiguous".to_string(),
            recommended: closeout_recovery_command_for_file(file, state).unwrap_or_default(),
        }),
    }
}

/// Refresh the durable baseline and cold CRDT restart projection from explicit content. Mirrors
/// `reset --from-current` without creating per-document compatibility files. The
/// preflight-owned baseline is intentionally left untouched (it is re-taken at the
/// next stable post-commit point).
fn refresh_recovery_projection_from_content(file: &Path, content: &str) -> Result<()> {
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        content,
        agent_doc_ops_log_io::log_op,
    )?;
    let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(content).encode_state();
    let lineage = format!(
        "closeout-recovery:{}",
        agent_doc_hash::content_hash(content)
    );
    agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(file, &crdt, &lineage)?;
    Ok(())
}

/// Classify the current closeout recovery state for `file`. Reuses the existing
/// detection primitives so the recovery diagnostic is a single typed instruction
/// instead of the historical multi-command chain. Conservatively returns `Clean`
/// when no recovery signal is provable; `DirectResponsePatchback`,
/// `BoundaryOnlyDrift`, and `NestedParentPointerStale` detection wiring is
/// tracked as remaining work in the plan.
pub fn classify_closeout_recovery_state_for_file(
    file: &Path,
    effects: &dyn CloseoutEffects,
) -> CloseoutRecoveryState {
    let cycle = match load_closeout_recovery_cycle_view(file) {
        Ok(Some(cycle)) => cycle,
        _ => {
            return classify_closeout_recovery_state_from_input(
                CloseoutRecoveryStateInput::default(),
            );
        }
    };
    let Some(cycle) = cycle.recovery_cycle_input() else {
        return classify_closeout_recovery_state_from_input(CloseoutRecoveryStateInput::default());
    };
    let mut input = CloseoutRecoveryStateInput {
        cycle: Some(cycle),
        ..CloseoutRecoveryStateInput::default()
    };
    if !cycle.needs_file_recovery_evidence() {
        return classify_closeout_recovery_state_from_input(input);
    }

    let observed_evidence = load_current_observed_closeout_recovery_evidence(file, effects)
        .ok()
        .flatten();
    input.head_has_escaped_template_patch = head_exchange_has_escaped_markers(file);
    // A captured body that never materialized in HEAD, or a committed
    // response-write turn with no capture at all, are both the missing-body
    // shape recovered by `write --commit`.
    input.missing_captured_response_body = observed_evidence
        .as_ref()
        .map(CloseoutRecoveryEvidence::reports_missing_response_body)
        .unwrap_or_else(|| stuck_captured_cycle(file).is_some());
    // `#closeout-recovery-state-machine`: a visible `### Re:` / `## Assistant`
    // response was patched into the working document outside the binary write
    // path. Recover by absorbing it through `write --commit`. Checked before the
    // generic content-drift fallthrough so the response-specific recovery wins
    // over `UnsafeUserContentDrift`. The jb-cache-conflict-cancel shape (the
    // binary write path applied the response but the commit boundary never
    // landed) is `git::commit`-recoverable, so it must NOT be misread as a direct
    // patchback — mirror `session_check::detect_uncommitted_closeout_drift`.
    input.direct_response_patchback = !effects
        .detect_jb_cache_conflict_cancel_recoverable(file)
        .unwrap_or(false)
        && effects
            .detect_bypassed_response_write(file)
            .ok()
            .flatten()
            .is_some();
    // `#recursive-repair-state-drift` / `#recursive-repair-recovery-states`:
    // classify committed-cycle drift by *what* differs so the recovery names one
    // safe command. Order matters — narrowest/safest first, content drift last
    // (fail closed).
    if let Some(snapshot_head_drift) = observed_evidence
        .as_ref()
        .and_then(|evidence| evidence.snapshot_head_drift)
    {
        input.snapshot_head_drift = Some(snapshot_head_drift);
        return classify_closeout_recovery_state_from_input(input);
    }
    let snapshot = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let head = agent_doc_git_io::revision::show_head(file).ok().flatten();
    if let (Some(snapshot), Some(head)) = (snapshot.as_deref(), head.as_deref())
        && snapshot != head
    {
        input.snapshot_head_drift = Some(classify_snapshot_head_drift(snapshot, head));
        return classify_closeout_recovery_state_from_input(input);
    }
    // Snapshot matches HEAD but the visible/working file is stale relative to the
    // state projections. Metadata-only visible drift → rebuild projections from the file;
    // content drift → preserve it through the normal response path.
    if let Some(snapshot_visible_drift) = observed_evidence
        .as_ref()
        .and_then(|evidence| evidence.snapshot_visible_drift)
    {
        input.snapshot_visible_drift = Some(snapshot_visible_drift);
        return classify_closeout_recovery_state_from_input(input);
    }
    if let (Some(snapshot), Ok(visible)) = (
        snapshot.as_deref(),
        effects.resolve_current_document(file, "classify_closeout_recovery_snapshot_visible"),
    ) && snapshot != visible.content()
    {
        input.snapshot_visible_drift =
            Some(classify_snapshot_visible_drift(snapshot, visible.content()));
        return classify_closeout_recovery_state_from_input(input);
    }
    // `#closeout-recovery-state-machine`: the document itself is clean (snapshot
    // == HEAD == working) but a reaped/closed item left a nested parent submodule
    // pointer uncommitted — single safe recovery is `agent-doc commit`.
    input.nested_parent_pointer_stale = agent_doc_git_io::submodule::submodule_pointer_drift(file)
        .ok()
        .flatten()
        .is_some();
    classify_closeout_recovery_state_from_input(input)
}

fn head_exchange_has_escaped_markers(file: &Path) -> bool {
    let Ok(Some(head)) = agent_doc_git_io::revision::show_head(file) else {
        return false;
    };
    let Ok(components) = agent_doc_element::element::parse(&head) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    let body = exchange.content(&head);
    body.contains("&lt;!-- agent:") || body.contains("&lt;!-- /agent:")
}

#[derive(Debug)]
struct CloseoutTimer<'a> {
    file: &'a Path,
    started: std::time::Instant,
    last_mark: std::time::Instant,
    phases: Vec<(String, u128)>,
}

impl<'a> CloseoutTimer<'a> {
    const REPORT_THRESHOLD_MS: u128 = 250;

    fn start(file: &'a Path) -> Self {
        let now = std::time::Instant::now();
        Self {
            file,
            started: now,
            last_mark: now,
            phases: Vec::new(),
        }
    }

    fn mark(&mut self, phase: &str) {
        let now = std::time::Instant::now();
        self.phases.push((
            phase.to_string(),
            now.duration_since(self.last_mark).as_millis(),
        ));
        self.last_mark = now;
    }

    fn finish(&self) {
        let total_ms = self.started.elapsed().as_millis();
        if total_ms < Self::REPORT_THRESHOLD_MS {
            return;
        }
        let message = closeout_latency_message(self.file, total_ms, &self.phases);
        eprintln!("[perf] {message}");
        agent_doc_ops_log_io::log_op(self.file, &message);
    }
}
