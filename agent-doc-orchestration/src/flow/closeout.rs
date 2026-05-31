use super::types::{CloseoutState, FlowEvent, FlowName, FlowOutcome, FlowStage};
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CloseoutGuardReason {
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
    pub const fn as_str(self) -> &'static str {
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

pub fn closeout_guard_event(
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) -> FlowEvent {
    FlowEvent::new(FlowName::Closeout, stage, outcome).with_reason(reason.as_str())
}

pub fn log_closeout_guard_event(
    file: &Path,
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: CloseoutGuardReason,
) {
    super::proof::log_flow_event(file, closeout_guard_event(stage, outcome, reason));
}

pub fn closeout_state_from_cycle_phase(phase: &str) -> Option<CloseoutState> {
    match phase {
        "preflight_started" => Some(CloseoutState::PreflightStarted),
        "response_captured" => Some(CloseoutState::ResponseCaptured),
        "write_applied" => Some(CloseoutState::WriteApplied),
        "committed" => Some(CloseoutState::Committed),
        "abandoned" => Some(CloseoutState::Abandoned),
        _ => None,
    }
}

pub fn terminal_guard_outcome(state: CloseoutState) -> FlowOutcome {
    match state {
        CloseoutState::Committed => FlowOutcome::Completed,
        CloseoutState::Abandoned => FlowOutcome::FailedClosed,
        CloseoutState::PreflightStarted
        | CloseoutState::ResponseCaptured
        | CloseoutState::WriteApplied => FlowOutcome::Blocked,
    }
}

pub fn complete_required_closeout(file: &Path) -> Result<bool> {
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

pub fn cycle_already_committed(file: &Path) -> Option<String> {
    match crate::cycle_state::load(file) {
        Ok(Some(state)) if state.phase == crate::cycle_state::CyclePhase::Committed => {
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
    let state = match crate::cycle_state::load(file) {
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
    if state.phase != crate::cycle_state::CyclePhase::Committed {
        return None;
    }
    let capture_id = state.capture_id.as_deref()?;
    let capture = match crate::capture::load_by_id(file, capture_id) {
        Ok(Some(capture)) => capture,
        Ok(None) => return None,
        Err(err) => {
            eprintln!(
                "[preflight] warning: failed to load capture {capture_id} for stuck-cycle detection on {}: {err}",
                file.display()
            );
            return None;
        }
    };
    if capture.cycle_id != state.cycle_id {
        return None;
    }
    if let Some(response_sha256) = state.response_sha256.as_deref()
        && response_sha256 != capture.response_sha256
    {
        return None;
    }
    if capture.response_body.trim().is_empty()
        || matches!(capture.state, crate::capture::CaptureState::Discarded)
    {
        return None;
    }
    let head = match crate::git::show_head(file) {
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
    if crate::write::response_materialized_in_content(&capture.response_body, &head) {
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
/// [`crate::capture::discard_captures_for_archived_responses`]) settles it once
/// so the false positive cannot resurface.
///
/// `mark_discarded` advances the *active* capture, and the active capture is
/// loaded by the cycle's own `capture_id`, so this only ever discards the
/// capture this cycle owns. Returns `true` when a capture was reconciled.
pub fn reconcile_compacted_committed_capture(file: &Path) -> Result<bool> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(false);
    };
    if state.phase != crate::cycle_state::CyclePhase::Committed {
        return Ok(false);
    }
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(false);
    };
    let Some(capture) = crate::capture::load_by_id(file, capture_id)? else {
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
    if capture.response_body.trim().is_empty()
        || matches!(capture.state, crate::capture::CaptureState::Discarded)
    {
        return Ok(false);
    }
    let Some(head) = crate::git::show_head(file)? else {
        return Ok(false);
    };
    // Present in HEAD → a normal committed response; nothing to reconcile.
    if crate::write::response_materialized_in_content(&capture.response_body, &head) {
        return Ok(false);
    }
    // Absent from HEAD but present in a HEAD-referenced compact archive → the
    // response was committed and then intentionally archived. Settle it durably.
    if !response_materialized_in_head_compact_archive(file, &capture.response_body, &head) {
        return Ok(false);
    }
    crate::capture::mark_discarded(file)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "reconcile_compacted_committed_capture file={} capture_id={} cycle_id={}",
            file.display(),
            capture.capture_id,
            state.cycle_id
        ),
    );
    eprintln!(
        "[preflight] reconciled compacted committed capture {} for {} (response archived out of HEAD; marked discarded so the stuck-capture false positive cannot resurface)",
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
    compact_archive_pointers(head).into_iter().any(|pointer| {
        read_head_compact_archive(file, pointer)
            .map(|archive| crate::write::response_materialized_in_content(response_body, &archive))
            .unwrap_or(false)
    })
}

fn compact_archive_pointers(content: &str) -> Vec<&str> {
    content
        .split("archived to `")
        .skip(1)
        .filter_map(|tail| tail.split_once('`').map(|(path, _)| path.trim()))
        .filter(|path| !path.is_empty())
        .collect()
}

fn read_head_compact_archive(file: &Path, pointer: &str) -> Option<String> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = crate::snapshot::find_project_root(&canonical)?;
    let archive_root = project_root
        .join(".agent-doc/archives")
        .canonicalize()
        .ok()?;
    let pointer_path = Path::new(pointer);
    let archive_path = if pointer_path.is_absolute() {
        pointer_path.to_path_buf()
    } else {
        project_root.join(pointer_path)
    };
    let archive_path = archive_path.canonicalize().ok()?;
    if !archive_path.starts_with(&archive_root) {
        return None;
    }
    std::fs::read_to_string(archive_path).ok()
}

fn capture_state_label(state: crate::capture::CaptureState) -> &'static str {
    match state {
        crate::capture::CaptureState::Captured => "captured",
        crate::capture::CaptureState::WriteApplied => "write_applied",
        crate::capture::CaptureState::Replayed => "replayed",
        crate::capture::CaptureState::Committed => "committed",
        crate::capture::CaptureState::Discarded => "discarded",
    }
}

pub fn cleanup_fallback_patch_files(file: &Path) {
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

pub fn write_claimed_patch_sentinel(project_root: &Path, patch_id: &str) {
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

pub fn cycle_phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
}

/// Typed closeout recovery state (`#closeout-repair-churn`). Collapses the
/// scattered "try finalize / write --commit / commit" diagnostic chain into one
/// classified state with a single recovery command. The recovery *mechanisms*
/// already exist (`PatchbackShape::EscapedComponentMarkers` fail-closed,
/// `write --commit` direct-patchback absorption, `complete_required_closeout`
/// parent-pointer retry); this is the unifying classifier + instruction table
/// that `session_check::closeout_recovery_hint` renders for every guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseoutRecoveryState {
    /// No recovery needed.
    Clean,
    /// Cycle still open (preflight_started / response_captured / write_applied).
    OpenCycle,
    /// Committed binary-owned work but the assistant response body is missing
    /// from HEAD (no capture, or a captured body not materialized in HEAD).
    MissingResponseBody,
    /// A visible `### Re:` response was patched directly into the document
    /// outside the binary write path. (Reserved for full detection wiring.)
    DirectResponsePatchback,
    /// Raw `<!-- agent:NAME -->` component markers were escaped into the
    /// committed exchange instead of applied as `<!-- patch:* -->` blocks.
    EscapedTemplatePatch,
    /// Snapshot differs from HEAD only by agent-doc-generated exchange artifacts
    /// (boundary / `(HEAD)` markers, answered-prompt-prefix canonicalization).
    /// Safe single recovery: `agent-doc commit`. (`#recursive-repair-state-drift`)
    BoundaryOnlyDrift,
    /// A reaped/closed item left a nested parent submodule pointer uncommitted.
    /// (Reserved for full detection wiring.)
    NestedParentPointerStale,
}

impl CloseoutRecoveryState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::OpenCycle => "open_cycle",
            Self::MissingResponseBody => "missing_response_body",
            Self::DirectResponsePatchback => "direct_response_patchback",
            Self::EscapedTemplatePatch => "escaped_template_patch",
            Self::BoundaryOnlyDrift => "boundary_only_drift",
            Self::NestedParentPointerStale => "nested_parent_pointer_stale",
        }
    }

    /// The single recovery command for this state, or `None` when `Clean`.
    pub fn recovery_command(self, file: &Path) -> Option<String> {
        let f = file.display();
        Some(match self {
            Self::Clean => return None,
            Self::OpenCycle => format!(
                "finish the response, then `agent-doc finalize {f}` (or `agent-doc write --commit {f}` to absorb an already-visible response)"
            ),
            Self::MissingResponseBody => format!(
                "pipe the final response (with `<!-- patch:exchange -->` blocks) through `agent-doc write --commit {f}`, then re-run `agent-doc session-check {f}`"
            ),
            Self::DirectResponsePatchback => format!(
                "`agent-doc write --commit {f}` to absorb the visible `### Re:` response through the snapshot/commit boundary"
            ),
            Self::EscapedTemplatePatch => format!(
                "rewrite the response with real `<!-- patch:exchange -->` blocks and rerun `agent-doc finalize {f}` — escaped component markers must not reach `agent:exchange`"
            ),
            Self::BoundaryOnlyDrift => format!(
                "`agent-doc commit {f}` (boundary / `(HEAD)` marker or answered-prompt-prefix drift only — no response body to write)"
            ),
            Self::NestedParentPointerStale => format!(
                "`agent-doc commit {f}` to update the nested parent submodule pointer"
            ),
        })
    }
}

/// Classify the current closeout recovery state for `file`. Reuses the existing
/// detection primitives so the recovery diagnostic is a single typed instruction
/// instead of the historical multi-command chain. Conservatively returns `Clean`
/// when no recovery signal is provable; `DirectResponsePatchback`,
/// `BoundaryOnlyDrift`, and `NestedParentPointerStale` detection wiring is
/// tracked as remaining work in the plan.
pub fn classify_closeout_recovery_state(file: &Path) -> CloseoutRecoveryState {
    let state = match crate::cycle_state::load(file) {
        Ok(Some(state)) => state,
        _ => return CloseoutRecoveryState::Clean,
    };
    use crate::cycle_state::CyclePhase;
    match state.phase {
        CyclePhase::PreflightStarted
        | CyclePhase::ResponseCaptured
        | CyclePhase::WriteApplied => return CloseoutRecoveryState::OpenCycle,
        CyclePhase::Abandoned => return CloseoutRecoveryState::Clean,
        CyclePhase::Committed => {}
    }

    if head_exchange_has_escaped_markers(file) {
        return CloseoutRecoveryState::EscapedTemplatePatch;
    }
    // A captured body that never materialized in HEAD, or a committed
    // response-write turn with no capture at all, are both the missing-body
    // shape recovered by `write --commit`.
    if stuck_captured_cycle(file).is_some() {
        return CloseoutRecoveryState::MissingResponseBody;
    }
    if state.capture_id.is_none()
        && state.response_sha256.is_none()
        && state.had_pending_mutations
    {
        return CloseoutRecoveryState::MissingResponseBody;
    }
    // `#recursive-repair-state-drift`: a committed cycle whose snapshot differs
    // from HEAD *only* by agent-doc-generated artifacts (boundary / `(HEAD)`
    // markers, answered-prompt-prefix canonicalization) is the metadata-only
    // HEAD drift the recursive owner-pane recovery left behind. The single safe
    // recovery is `agent-doc commit` — never `write --commit` (there is no
    // missing response body) — so name it precisely instead of the generic hint.
    if committed_artifact_only_head_drift(file) {
        return CloseoutRecoveryState::BoundaryOnlyDrift;
    }
    CloseoutRecoveryState::Clean
}

/// True when the committed snapshot differs from HEAD only by agent-doc-generated
/// exchange artifacts (transient boundary / `(HEAD)` markers and answered-prompt
/// prefix canonicalization). `verify_snapshot_committed` normalizes only the
/// transient markers, so prompt-prefix drift on already-answered prompts trips
/// its snapshot-vs-HEAD guard even though `agent-doc commit` is the safe fix; the
/// fuller `normalize_committed_exchange_artifacts` equality proves no real
/// user/response content drift remains. Conservatively `false` on any read error
/// so a genuine content difference never masquerades as metadata-only.
fn committed_artifact_only_head_drift(file: &Path) -> bool {
    let snapshot = match crate::snapshot::load(file) {
        Ok(Some(snapshot)) => snapshot,
        _ => return false,
    };
    let head = match crate::git::show_head(file) {
        Ok(Some(head)) => head,
        _ => return false,
    };
    if snapshot == head {
        return false;
    }
    crate::git::normalize_committed_exchange_artifacts(&snapshot)
        == crate::git::normalize_committed_exchange_artifacts(&head)
}

fn head_exchange_has_escaped_markers(file: &Path) -> bool {
    let Ok(Some(head)) = crate::git::show_head(file) else {
        return false;
    };
    let Ok(components) = crate::component::parse(&head) else {
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
    use std::process::Command;

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

    fn setup_git_project_with_doc(base: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, base).unwrap();
        crate::snapshot::save(&doc, base).unwrap();
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "initial", "--no-verify"]);
        (dir, doc)
    }

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed with {status}");
    }

    #[test]
    fn stuck_captured_cycle_detects_committed_cycle_missing_response_in_head() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCaptured but never committed.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);

        let state = crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = crate::capture::capture_response(&doc, response).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();

        let info = stuck_captured_cycle(&doc).expect("missing HEAD response should be detected");
        assert_eq!(info.cycle_id, state.cycle_id);
        assert_eq!(info.capture_id, capture.capture_id);
        assert_eq!(info.response_body_len, response.len());
        assert_eq!(info.capture_state, "captured");
    }

    #[test]
    fn stuck_captured_cycle_ignores_queue_prompt_echo_inserted_in_head() {
        // #stuck-capture-queue-echo-false-positive: when a queue head is consumed,
        // the binary inserts a `> **Queue prompt:**` echo blockquote between the
        // response heading and body. The captured response is the raw heading+body,
        // so the materialized HEAD differs by that echo. stuck_captured_cycle must
        // still treat the response as present in HEAD (no false-positive warning).
        let base = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\nOlder response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = "### Re: do [#thing] — gpt-5\n\nShipped the fix.\n";
        let (dir, doc) = setup_git_project_with_doc(base);

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();

        // Materialize the response into HEAD with the queue-prompt echo inserted
        // between the heading and body, exactly as queue consumption writes it.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\nOlder response.\n",
            "### Re: do [#thing] — gpt-5\n\n",
            "> **Queue prompt:**\n>\n> do [#thing]\n\n",
            "Shipped the fix.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
            .unwrap();

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "queue-prompt echo inserted between heading and body must not flag the cycle stuck"
        );
    }

    #[test]
    fn stuck_captured_cycle_ignores_committed_cycle_when_response_is_in_head() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCommitted response.\n";
        let full_doc = format!("{base}\n{response}");
        let (dir, doc) = setup_git_project_with_doc(base);

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &full_doc).unwrap();
        crate::snapshot::save(&doc, &full_doc).unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&full_doc),
            Some(&full_doc),
        )
        .unwrap();

        assert!(stuck_captured_cycle(&doc).is_none());
    }

    #[test]
    fn recovery_command_maps_each_state_to_one_instruction() {
        use CloseoutRecoveryState::*;
        let f = Path::new("tasks/doc.md");
        assert_eq!(Clean.recovery_command(f), None);
        for (state, name, needle) in [
            (OpenCycle, "open_cycle", "agent-doc finalize"),
            (MissingResponseBody, "missing_response_body", "agent-doc write --commit"),
            (DirectResponsePatchback, "direct_response_patchback", "absorb the visible"),
            (EscapedTemplatePatch, "escaped_template_patch", "patch:exchange"),
            (BoundaryOnlyDrift, "boundary_only_drift", "boundary"),
            (
                NestedParentPointerStale,
                "nested_parent_pointer_stale",
                "parent submodule pointer",
            ),
        ] {
            assert_eq!(state.as_str(), name);
            let cmd = state
                .recovery_command(f)
                .expect("non-clean states have a command");
            assert!(cmd.contains(needle), "state {name} command {cmd:?} missing {needle:?}");
            assert!(cmd.contains("tasks/doc.md"), "command should name the file: {cmd:?}");
        }
    }

    #[test]
    fn classify_recovery_clean_without_cycle_state() {
        let (_dir, doc) = setup_git_project_with_doc("---\nsession: test\n---\n\nHi\n");
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn classify_recovery_open_cycle_when_preflight_started() {
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::OpenCycle
        );
    }

    #[test]
    fn classify_recovery_missing_response_body_for_stuck_cycle() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCaptured but never committed.\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::MissingResponseBody
        );
    }

    #[test]
    fn classify_recovery_clean_when_response_committed_in_head() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nCommitted response.\n";
        let full_doc = format!("{base}\n{response}");
        let (dir, doc) = setup_git_project_with_doc(base);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &full_doc).unwrap();
        crate::snapshot::save(&doc, &full_doc).unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&full_doc), Some(&full_doc))
            .unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn classify_recovery_boundary_only_drift_for_answered_prompt_prefix() {
        // `#recursive-repair-state-drift`: snapshot differs from HEAD only by an
        // answered-prompt-prefix (`❯ do …` vs bare `do …` above a real `### Re:`).
        // `verify_snapshot_committed` normalizes only transient markers, so this
        // still trips the snapshot-vs-HEAD guard, but the fuller artifact
        // normalization proves it is safe metadata-only drift → BoundaryOnlyDrift
        // → single `agent-doc commit` recovery (never `write --commit`).
        let head = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please rerun the deploy check.\n",
            "### Re: deploy check — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n",
        );
        let snapshot = head.replace("Please rerun the", "❯ Please rerun the");
        // `setup_git_project_with_doc` already commits `head` to HEAD.
        let (_dir, doc) = setup_git_project_with_doc(head);
        crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
        // Snapshot carries the un-canonicalized prompt prefix; HEAD has the bare
        // form. Artifact normalization makes them equal; transient does not.
        crate::snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&snapshot), Some(&snapshot))
            .unwrap();
        assert_ne!(
            crate::git::normalize_transient_agent_doc_markers(&snapshot),
            crate::git::normalize_transient_agent_doc_markers(head),
            "test precondition: transient normalization must still differ"
        );
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::BoundaryOnlyDrift
        );
        let cmd = CloseoutRecoveryState::BoundaryOnlyDrift
            .recovery_command(&doc)
            .unwrap();
        assert!(cmd.contains("agent-doc commit"), "{cmd}");
        assert!(!cmd.contains("write --commit"), "{cmd}");
    }

    #[test]
    fn stuck_captured_cycle_ignores_committed_template_patch_body_in_head() {
        let base = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: hello — gpt-5\n\n",
            "Committed through template patching.\n",
            "<!-- /patch:exchange -->\n",
        );
        let full_doc = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "### Re: hello — gpt-5\n\n",
            "Committed through template patching.\n",
            "<!-- /agent:exchange -->\n",
        );
        let (dir, doc) = setup_git_project_with_doc(base);

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, full_doc).unwrap();
        crate::snapshot::save(&doc, full_doc).unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "response", "--no-verify"]);
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(full_doc), Some(full_doc))
            .unwrap();

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "template patch wrappers are not expected in HEAD after materialization"
        );
    }

    #[test]
    fn stuck_captured_cycle_ignores_committed_cycle_when_response_is_in_compact_archive() {
        let base = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Older response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = "### Re: compacted — gpt-5\n\nArchived response body.\n";
        let (dir, doc) = setup_git_project_with_doc(base);
        let archive_dir = dir.path().join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archive_path = archive_dir.join("doc-20260527-000000.md");
        std::fs::write(
            &archive_path,
            format!(
                "---\narchived_from: compact\ncomponent: exchange\ndocument: doc.md\n---\n\n{base}\n{response}"
            ),
        )
        .unwrap();
        let compacted = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n*Compacted. Content archived to `{}`*\n<!-- /agent:exchange -->\n",
            archive_path.display()
        );

        let state = crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &compacted).unwrap();
        crate::snapshot::save(&doc, &compacted).unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "compact", "--no-verify"]);
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&compacted),
            Some(&compacted),
        )
        .unwrap();

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "cycle {} should not warn when the captured response is materialized in the compact archive",
            state.cycle_id
        );
    }

    #[test]
    fn reconcile_compacted_committed_capture_discards_and_survives_archive_gc() {
        // #stuck-capture-compact-false-positive: reconciliation marks the capture
        // Discarded once the response is proven in the compact archive, so the
        // false-positive stuck warning cannot resurface even if the archive is
        // later garbage-collected.
        let base = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Older response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = "### Re: compacted — gpt-5\n\nArchived response body.\n";
        let (dir, doc) = setup_git_project_with_doc(base);
        let archive_dir = dir.path().join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archive_path = archive_dir.join("doc-20260527-000000.md");
        std::fs::write(
            &archive_path,
            format!(
                "---\narchived_from: compact\ncomponent: exchange\ndocument: doc.md\n---\n\n{base}\n{response}"
            ),
        )
        .unwrap();
        let compacted = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n*Compacted. Content archived to `{}`*\n<!-- /agent:exchange -->\n",
            archive_path.display()
        );

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        std::fs::write(&doc, &compacted).unwrap();
        crate::snapshot::save(&doc, &compacted).unwrap();
        run_git(dir.path(), &["add", "doc.md"]);
        run_git(dir.path(), &["commit", "-m", "compact", "--no-verify"]);
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&compacted),
            Some(&compacted),
        )
        .unwrap();

        // Reconcile once: the capture is durably marked Discarded.
        assert!(
            reconcile_compacted_committed_capture(&doc).unwrap(),
            "expected reconciliation to settle the compacted committed capture"
        );
        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        assert!(
            matches!(capture.state, crate::capture::CaptureState::Discarded),
            "capture should be terminally Discarded after reconciliation, got {:?}",
            capture.state
        );

        // A second pass is a no-op (already discarded).
        assert!(
            !reconcile_compacted_committed_capture(&doc).unwrap(),
            "reconciliation should be idempotent once the capture is discarded"
        );

        // Durability: even after the archive is GC'd, the discarded capture must
        // not resurface as a stuck-capture false positive.
        std::fs::remove_file(&archive_path).unwrap();
        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "a reconciled (discarded) capture must not flag stuck after the archive is removed"
        );
    }
}
