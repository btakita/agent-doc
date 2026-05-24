use super::types::{CloseoutState, FlowEvent, FlowName, FlowOutcome, FlowStage};
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CloseoutGuardReason {
    MissingCycleState,
    OpenCycle,
    PendingCaptureTargetMissing,
    PendingCaptureInventoryShortfall,
    PendingCapturePlanShortfall,
    PendingCapturePromisedIdsMissing,
    PendingCaptureRequired,
    PendingCaptureRecommendations,
    PendingDoneMalformedTrackedItem,
    PendingDoneMissing,
    ReviewDoneSourceNotReviewed,
    AlreadyCommitted,
    SnapshotDiffersFromHead,
    ParentPointerStale,
    SessionCheckInterrupted,
    ResponsePatchbackUncommitted,
    CommitBoundaryRecovered,
    StalePreflightLockRepaired,
    StalePreflightCycleAbandoned,
}

impl CloseoutGuardReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::MissingCycleState => "missing_cycle_state",
            Self::OpenCycle => "open_cycle",
            Self::PendingCaptureTargetMissing => "pending_capture_target_missing",
            Self::PendingCaptureInventoryShortfall => "pending_capture_inventory_shortfall",
            Self::PendingCapturePlanShortfall => "pending_capture_plan_shortfall",
            Self::PendingCapturePromisedIdsMissing => "pending_capture_promised_ids_missing",
            Self::PendingCaptureRequired => "pending_capture_required",
            Self::PendingCaptureRecommendations => "pending_capture_recommendations",
            Self::PendingDoneMalformedTrackedItem => "pending_done_malformed_tracked_item",
            Self::PendingDoneMissing => "pending_done_missing",
            Self::ReviewDoneSourceNotReviewed => "review_done_source_not_reviewed",
            Self::AlreadyCommitted => "already_committed",
            Self::SnapshotDiffersFromHead => "snapshot_differs_from_head",
            Self::ParentPointerStale => "parent_pointer_stale",
            Self::SessionCheckInterrupted => "session_check_interrupted",
            Self::ResponsePatchbackUncommitted => "response_patchback_uncommitted",
            Self::CommitBoundaryRecovered => "commit_boundary_recovered",
            Self::StalePreflightLockRepaired => "stale_preflight_lock_repaired",
            Self::StalePreflightCycleAbandoned => "stale_preflight_cycle_abandoned",
        }
    }
}

pub(crate) fn closeout_guard_event(
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) -> FlowEvent {
    FlowEvent::new(FlowName::Closeout, stage, outcome).with_reason(reason.as_str())
}

pub(crate) fn log_closeout_guard_event(
    file: &Path,
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) {
    super::proof::log_flow_event(file, closeout_guard_event(stage, outcome, reason));
}

pub(crate) fn closeout_state_from_cycle_phase(phase: &str) -> Option<CloseoutState> {
    match phase {
        "preflight_started" => Some(CloseoutState::PreflightStarted),
        "response_captured" => Some(CloseoutState::ResponseCaptured),
        "write_applied" => Some(CloseoutState::WriteApplied),
        "committed" => Some(CloseoutState::Committed),
        "abandoned" => Some(CloseoutState::Abandoned),
        _ => None,
    }
}

pub(crate) fn terminal_guard_outcome(state: CloseoutState) -> FlowOutcome {
    match state {
        CloseoutState::Committed => FlowOutcome::Completed,
        CloseoutState::Abandoned => FlowOutcome::FailedClosed,
        CloseoutState::PreflightStarted
        | CloseoutState::ResponseCaptured
        | CloseoutState::WriteApplied => FlowOutcome::Blocked,
    }
}

pub(crate) fn complete_required_closeout(file: &Path) -> Result<bool> {
    let mut timer = CloseoutTimer::start(file);

    let mut did_commit = crate::git::commit(file)?;
    timer.mark("git_commit");
    ensure_cycle_committed(file)?;
    timer.mark("cycle_state");

    if let crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. } =
        crate::git::verify_snapshot_committed(file)?
    {
        eprintln!("[commit] snapshot differs from HEAD after commit - retrying");
        log_closeout_guard_event(
            file,
            FlowStage::SnapshotConvergence,
            FlowOutcome::Blocked,
            CloseoutGuardReason::SnapshotDiffersFromHead,
        );
        did_commit |= crate::git::commit(file)?;
        timer.mark("git_commit_retry_snapshot");
        ensure_cycle_committed(file)?;
        timer.mark("cycle_state_retry_snapshot");
    }

    if crate::git::submodule_pointer_drift(file)?.is_some() {
        eprintln!("[commit] parent submodule pointer still stale after commit - retrying");
        did_commit |= crate::git::commit(file)?;
        timer.mark("git_commit_retry_parent_pointer");
        ensure_cycle_committed(file)?;
        timer.mark("cycle_state_retry_parent_pointer");
    }
    if let Some(drift) = crate::git::submodule_pointer_drift(file)? {
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
    if let Err(err) = crate::session_check::enforce_clean_closeout(file) {
        log_closeout_guard_event(
            file,
            FlowStage::SessionCheck,
            FlowOutcome::FailedClosed,
            CloseoutGuardReason::SessionCheckInterrupted,
        );
        return Err(err);
    }
    timer.mark("session_check");
    cleanup_fallback_patch_files(file);
    timer.mark("fallback_cleanup");
    timer.finish();
    Ok(did_commit)
}

pub(crate) fn cycle_already_committed(file: &Path) -> Option<String> {
    match crate::cycle_state::load(file) {
        Ok(Some(state)) if state.phase == crate::cycle_state::CyclePhase::Committed => {
            Some(state.cycle_id)
        }
        _ => None,
    }
}

pub(crate) fn cleanup_fallback_patch_files(file: &Path) {
    let Ok(canonical) = file.canonicalize() else {
        return;
    };
    let project_root = crate::write::resolve_ipc_project_root_pub(&canonical);
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        return;
    }
    let Ok(hash) = crate::snapshot::doc_hash(file) else {
        return;
    };
    let patch_file = patches_dir.join(format!("{hash}.json"));
    if patch_file.exists() {
        if let Ok(stale_content) = std::fs::read_to_string(&patch_file)
            && let Ok(stale_json) = serde_json::from_str::<serde_json::Value>(&stale_content)
            && let Some(patch_id) = stale_json.get("patch_id").and_then(|v| v.as_str())
        {
            write_claimed_patch_sentinel(&project_root, patch_id);
        }
        match std::fs::remove_file(&patch_file) {
            Ok(()) => eprintln!(
                "[write] cleaned up fallback patch file after closeout: {}",
                patch_file.display()
            ),
            Err(e) => eprintln!(
                "[write] WARNING: failed to clean up fallback patch file after closeout: {e}"
            ),
        }
    }
}

pub(crate) fn write_claimed_patch_sentinel(project_root: &Path, patch_id: &str) {
    let claimed_dir = project_root.join(".agent-doc/claimed-patches");
    match std::fs::create_dir_all(&claimed_dir) {
        Err(e) => {
            eprintln!("[write] WARNING: failed to create claimed-patches dir: {e}");
        }
        Ok(_) => {
            let sentinel = claimed_dir.join(patch_id);
            if let Err(e) = std::fs::write(&sentinel, "") {
                eprintln!("[write] WARNING: failed to write patch sentinel: {e}");
            } else {
                eprintln!(
                    "[write] patch_id {} claimed (sentinel written)",
                    &patch_id[..patch_id.len().min(8)]
                );
            }
        }
    }
}

fn ensure_cycle_committed(file: &Path) -> Result<()> {
    let Some(state) = crate::cycle_state::load(file)? else {
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
            cycle_phase_name(state.phase),
            state.last_event
        );
    }
    Ok(())
}

pub(crate) fn cycle_phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
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
        crate::ops_log::log_op(self.file, &message);
    }
}

fn closeout_latency_message(file: &Path, total_ms: u128, phases: &[(String, u128)]) -> String {
    let phase_text = phases
        .iter()
        .map(|(phase, elapsed)| format!("{phase}:{elapsed}ms"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "closeout_latency file={} total_ms={} phases={}",
        file.display(),
        total_ms,
        phase_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_is_terminal_completed() {
        assert_eq!(
            terminal_guard_outcome(CloseoutState::Committed),
            FlowOutcome::Completed
        );
    }

    #[test]
    fn closeout_guard_event_is_typed() {
        let event = closeout_guard_event(
            FlowStage::PreWriteGuard,
            FlowOutcome::Blocked,
            CloseoutGuardReason::PendingCaptureRecommendations,
        );

        assert_eq!(event.flow, FlowName::Closeout);
        assert_eq!(event.stage, FlowStage::PreWriteGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("pending_capture_recommendations")
        );
    }

    #[test]
    fn closeout_guard_event_carries_review_done_reason() {
        let event = closeout_guard_event(
            FlowStage::PreWriteGuard,
            FlowOutcome::Blocked,
            CloseoutGuardReason::ReviewDoneSourceNotReviewed,
        );

        assert_eq!(event.flow, FlowName::Closeout);
        assert_eq!(event.stage, FlowStage::PreWriteGuard);
        assert_eq!(event.outcome, FlowOutcome::Blocked);
        assert_eq!(
            event.reason.as_deref(),
            Some("review_done_source_not_reviewed")
        );
    }

    #[test]
    fn cycle_phase_name_matches_persisted_phase_strings() {
        assert_eq!(
            cycle_phase_name(crate::cycle_state::CyclePhase::ResponseCaptured),
            "response_captured"
        );
    }

    #[test]
    fn closeout_latency_message_lists_phase_timings() {
        let message = closeout_latency_message(
            Path::new("tasks/doc.md"),
            300,
            &[
                ("git_commit".to_string(), 12),
                ("session_check".to_string(), 4),
            ],
        );

        assert!(message.contains("closeout_latency file=tasks/doc.md total_ms=300"));
        assert!(message.contains("git_commit:12ms,session_check:4ms"));
    }
}
