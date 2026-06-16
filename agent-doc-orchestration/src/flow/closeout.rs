use super::types::{CloseoutState, FlowEvent, FlowName, FlowOutcome, FlowStage};
use anyhow::{Context, Result};
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
    let rc = crate::graph::RunContext::new(file.to_path_buf());

    let mut did_commit = crate::git::commit(file)?;
    rc.invalidate_head_content();
    timer.mark("git_commit");
    ensure_cycle_committed(file)?;
    timer.mark("cycle_state");

    // `#exit75-done-reap-not-atomic`: reap any `[x]` tracked items the response
    // cycle marked done (`--done`) within the SAME closeout. The direct-write
    // finalize path reaps during document mutation, but the IPC-timeout / file-IPC
    // fallback (exit 75) commits the `[x]` and exits before that reap, so the
    // completed item lingers and `enforce_clean_closeout` below fails closed —
    // forcing a manual recovery `preflight` that also strands a fresh
    // `preflight_started` cycle. `run_pending_maintenance` is the same reap
    // preflight runs at cycle start: it writes the reaped/archived document +
    // snapshot (it does not commit), so the snapshot-vs-HEAD retry immediately
    // below stages the reap. It is idempotent — a no-op when nothing is `[x]`
    // (the direct path already reaped), so it only closes the exit-75 gap. Errors
    // are non-fatal: the `completed_pending_reap` guard still catches a miss.
    match crate::preflight::run_pending_maintenance(file) {
        Ok(_) => {
            rc.invalidate_head_content();
            timer.mark("closeout_reap");
        }
        Err(e) => eprintln!("[commit] closeout pending-reap maintenance failed (non-fatal): {e}"),
    }

    if let crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. } =
        rc.snapshot_commit_status()
    {
        eprintln!("[commit] snapshot differs from HEAD after commit - retrying");
        log_closeout_guard_event(
            file,
            FlowStage::SnapshotConvergence,
            FlowOutcome::Blocked,
            CloseoutGuardReason::SnapshotDiffersFromHead,
        );
        did_commit |= crate::git::commit(file)?;
        rc.invalidate_head_content();
        timer.mark("git_commit_retry_snapshot");
        ensure_cycle_committed(file)?;
        timer.mark("cycle_state_retry_snapshot");
    }

    if crate::git::submodule_pointer_drift(file)?.is_some() {
        eprintln!("[commit] parent submodule pointer still stale after commit - retrying");
        did_commit |= crate::git::commit(file)?;
        rc.invalidate_head_content();
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
    crate::project_controller::persist_session_actor_closeout(file)?;
    timer.mark("session_actor_closeout");
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

pub(crate) fn compact_archive_pointers(content: &str) -> Vec<&str> {
    content
        .split("archived to `")
        .skip(1)
        .filter_map(|tail| tail.split_once('`').map(|(path, _)| path.trim()))
        .filter(|path| !path.is_empty())
        .collect()
}

pub(crate) fn read_head_compact_archive(file: &Path, pointer: &str) -> Option<String> {
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
    /// outside the binary write path. Detected via
    /// `session_check::detect_bypassed_response_write` (guarded against the
    /// jb-cache-conflict-cancel `git::commit`-recoverable shape). Safe recovery:
    /// `agent-doc write --commit`. (`#closeout-recovery-state-machine`)
    DirectResponsePatchback,
    /// Raw `<!-- agent:NAME -->` component markers were escaped into the
    /// committed exchange instead of applied as `<!-- patch:* -->` blocks.
    EscapedTemplatePatch,
    /// Snapshot differs from HEAD only by agent-doc-generated exchange artifacts
    /// (boundary / `(HEAD)` markers, answered-prompt-prefix canonicalization).
    /// Safe single recovery: `agent-doc commit`. (`#recursive-repair-state-drift`)
    BoundaryOnlyDrift,
    /// A reaped/closed item left a nested parent submodule pointer uncommitted
    /// while the document itself is clean. Detected via
    /// `git::submodule_pointer_drift`. Safe recovery: `agent-doc commit`.
    /// (`#closeout-recovery-state-machine`)
    NestedParentPointerStale,
    /// An empty `preflight_started` cycle with no capture, response, or pending
    /// mutation — a diagnostic/probe preflight that nothing followed. Safe single
    /// recovery: `agent-doc cancel`. (`#recursive-repair-recovery-states`)
    OpenEmptyPreflight,
    /// Snapshot differs from HEAD only by agent-doc-generated *queue / frontmatter
    /// / status* metadata (e.g. a `queue` sync-attribute regeneration or
    /// `queue_active` flip); the user/response and tracked-item content is
    /// byte-identical. Safe single recovery: `agent-doc commit`.
    /// (`#recursive-repair-recovery-states`)
    QueueMetadataDrift,
    /// The visible/working file is stale relative to its sidecars (or vice versa)
    /// by metadata only, after an accepted metadata change. Safe single recovery:
    /// rebuild sidecars from the visible file via
    /// `agent-doc reset --from-current --preserve-session` then `agent-doc commit`.
    /// (`#recursive-repair-recovery-states`)
    SidecarVisibleDrift,
    /// User-authored prompt/response content drifted vs HEAD. Fail closed: this
    /// must NOT be auto-committed as metadata; the content has to be preserved and
    /// closed through the normal response path. (`#recursive-repair-recovery-states`)
    UnsafeUserContentDrift,
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
            Self::OpenEmptyPreflight => "open_empty_preflight",
            Self::QueueMetadataDrift => "queue_metadata_drift",
            Self::SidecarVisibleDrift => "sidecar_visible_drift",
            Self::UnsafeUserContentDrift => "unsafe_user_content_drift",
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
            Self::NestedParentPointerStale => {
                format!("`agent-doc commit {f}` to update the nested parent submodule pointer")
            }
            Self::OpenEmptyPreflight => format!(
                "`agent-doc cancel {f}` — an empty diagnostic preflight cycle with no captured response; abandoning it leaves no document drift"
            ),
            Self::QueueMetadataDrift => format!(
                "`agent-doc commit {f}` (queue / `queue_active` / status metadata only — user/response content is unchanged, no response body to write)"
            ),
            Self::SidecarVisibleDrift => format!(
                "`agent-doc reset --from-current --preserve-session {f}` then `agent-doc commit {f}` to rebuild stale sidecars from the visible file (metadata-only visible drift)"
            ),
            Self::UnsafeUserContentDrift => format!(
                "preserve the user-authored content and finish through `agent-doc finalize {f}` (or `agent-doc write --commit {f}`) — do NOT `agent-doc commit`, which would commit unreviewed content drift as metadata"
            ),
        })
    }
}

/// Input facts that are already known at a closeout recovery call site.
///
/// This is intentionally small for `#smcloseoutdecision`; the follow-on evidence
/// refactor owns gathering these facts from sidecars, IPC, and controller state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CloseoutRecoveryDecisionInput<'a> {
    /// A routed/JB prompt is waiting and should not be typed over an unresolved
    /// closeout.
    pub prompt_context_available: bool,
    /// Low-level blocker text from the caller, retained only as evidence on the
    /// typed decision boundary.
    pub blocker_reason: Option<&'a str>,
    /// Positive proof that the active capture is stale and superseded by visible
    /// exchange content, so retiring it will not drop the user's intended answer.
    pub stale_capture_supersession_proof: Option<&'a str>,
}

/// Typed closeout recovery policy boundary (`#smcloseoutdecision`).
///
/// Route, repair, session-check, and write/commit should converge on this
/// action-shaped decision instead of string-matching individual guard errors.
/// See `tasks/agent-doc/plan-run-agent-doc-closeout-recovery-state-machine.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutRecoveryDecision {
    /// No closeout recovery remains.
    AlreadyCommitted,
    /// The existing response/cycle can be safely replayed or completed by the
    /// binary without choosing between competing user-authored contents.
    ReplaySafe {
        state: CloseoutRecoveryState,
        command: String,
    },
    /// A stale capture can be retired because superseding visible content proves
    /// the captured body should not be replayed.
    RetireStaleCapture {
        state: CloseoutRecoveryState,
        proof: String,
    },
    /// Sidecars are stale relative to the visible markdown and can be rebuilt
    /// from the visible file.
    ResetSidecarsFromVisible {
        state: CloseoutRecoveryState,
        command: String,
    },
    /// A new routed prompt must wait behind the unresolved closeout instead of
    /// being submitted to the pane.
    QueuePromptForAfterCloseout {
        state: CloseoutRecoveryState,
        reason: String,
    },
    /// Recovery is not safe because a required proof is missing.
    Blocked {
        state: CloseoutRecoveryState,
        missing_proof: String,
        recommended: String,
    },
}

impl CloseoutRecoveryDecision {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AlreadyCommitted => "already_committed",
            Self::ReplaySafe { .. } => "replay_safe",
            Self::RetireStaleCapture { .. } => "retire_stale_capture",
            Self::ResetSidecarsFromVisible { .. } => "reset_sidecars_from_visible",
            Self::QueuePromptForAfterCloseout { .. } => "queue_prompt_for_after_closeout",
            Self::Blocked { .. } => "blocked",
        }
    }

    pub const fn state(&self) -> Option<CloseoutRecoveryState> {
        match self {
            Self::AlreadyCommitted => None,
            Self::ReplaySafe { state, .. }
            | Self::RetireStaleCapture { state, .. }
            | Self::ResetSidecarsFromVisible { state, .. }
            | Self::QueuePromptForAfterCloseout { state, .. }
            | Self::Blocked { state, .. } => Some(*state),
        }
    }
}

pub fn decide_closeout_recovery(
    file: &Path,
    input: CloseoutRecoveryDecisionInput<'_>,
) -> CloseoutRecoveryDecision {
    let state = classify_closeout_recovery_state(file);
    closeout_recovery_decision_from_state(file, state, input)
}

pub fn closeout_recovery_decision_from_state(
    file: &Path,
    state: CloseoutRecoveryState,
    input: CloseoutRecoveryDecisionInput<'_>,
) -> CloseoutRecoveryDecision {
    if input.prompt_context_available {
        return CloseoutRecoveryDecision::QueuePromptForAfterCloseout {
            state,
            reason: input
                .blocker_reason
                .unwrap_or_else(|| state.as_str())
                .to_string(),
        };
    }

    if state == CloseoutRecoveryState::Clean {
        return CloseoutRecoveryDecision::AlreadyCommitted;
    }

    if let Some(proof) = input.stale_capture_supersession_proof
        && matches!(
            state,
            CloseoutRecoveryState::MissingResponseBody
                | CloseoutRecoveryState::UnsafeUserContentDrift
        )
    {
        return CloseoutRecoveryDecision::RetireStaleCapture {
            state,
            proof: proof.to_string(),
        };
    }

    match state {
        CloseoutRecoveryState::Clean => CloseoutRecoveryDecision::AlreadyCommitted,
        CloseoutRecoveryState::DirectResponsePatchback
        | CloseoutRecoveryState::BoundaryOnlyDrift
        | CloseoutRecoveryState::NestedParentPointerStale
        | CloseoutRecoveryState::OpenEmptyPreflight
        | CloseoutRecoveryState::QueueMetadataDrift => CloseoutRecoveryDecision::ReplaySafe {
            state,
            command: state.recovery_command(file).unwrap_or_default(),
        },
        CloseoutRecoveryState::SidecarVisibleDrift => {
            CloseoutRecoveryDecision::ResetSidecarsFromVisible {
                state,
                command: state.recovery_command(file).unwrap_or_default(),
            }
        }
        CloseoutRecoveryState::OpenCycle => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "open cycle must finish, be replayed, or be explicitly queued behind"
                .to_string(),
            recommended: state.recovery_command(file).unwrap_or_default(),
        },
        CloseoutRecoveryState::MissingResponseBody => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "captured response body presence or supersession proof".to_string(),
            recommended: state.recovery_command(file).unwrap_or_default(),
        },
        CloseoutRecoveryState::EscapedTemplatePatch => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "unescaped patchback blocks that can be applied safely".to_string(),
            recommended: state.recovery_command(file).unwrap_or_default(),
        },
        CloseoutRecoveryState::UnsafeUserContentDrift => CloseoutRecoveryDecision::Blocked {
            state,
            missing_proof: "proof that visible user-authored content is metadata-only drift"
                .to_string(),
            recommended: state.recovery_command(file).unwrap_or_default(),
        },
    }
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
/// `QueueMetadataDrift` / `SidecarVisibleDrift` are auto-applied *only when the
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
pub fn apply_closeout_recovery(file: &Path) -> Result<RecoveryApplication> {
    let state = classify_closeout_recovery_state(file);
    match state {
        CloseoutRecoveryState::Clean => Ok(RecoveryApplication::NothingToDo),
        CloseoutRecoveryState::OpenEmptyPreflight => {
            crate::repair::cancel_preflight_cycle(file)?;
            Ok(RecoveryApplication::Applied {
                state,
                action: "abandoned the empty preflight cycle".to_string(),
            })
        }
        CloseoutRecoveryState::BoundaryOnlyDrift => {
            crate::git::commit(file)?;
            Ok(RecoveryApplication::Applied {
                state,
                action: "committed boundary / answered-prompt-prefix artifact drift".to_string(),
            })
        }
        CloseoutRecoveryState::QueueMetadataDrift | CloseoutRecoveryState::SidecarVisibleDrift => {
            apply_metadata_drift_recovery(file, state)
        }
        other => Ok(RecoveryApplication::NotApplied {
            state: other,
            reason: "auto-apply withheld — recovery requires a preserved response body or open-cycle resolution, not a metadata operation".to_string(),
            recommended: other.recovery_command(file).unwrap_or_default(),
        }),
    }
}

/// Which side of a metadata-only drift is authoritative for recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataDriftAuthority {
    /// The local side (snapshot for `QueueMetadataDrift`, the visible file for
    /// `SidecarVisibleDrift`) is authoritative — commit it forward.
    Local,
    /// HEAD (committed) is authoritative — restore the local side from HEAD,
    /// discarding the spurious local metadata drift.
    Head,
    /// Neither side is provably authoritative (both carry distinct live queue
    /// continuation heads with no consuming response) — fail closed.
    Ambiguous,
}

/// Decide the authoritative side of a content-equal metadata-only drift between a
/// `local` document string (the candidate to commit) and the committed `head`.
///
/// The decision turns on the live auto-queue continuation signal
/// (`#recovery-drift-authoritative-side`). Because the caller has already proven
/// the *content* components (exchange / backlog / review / icebox / done) are
/// byte-identical, the only durable state the diff can destroy is an active queue
/// continuation. Legitimate consumption of a queue head always shows up as
/// response/content drift, so a continuation that exists in HEAD but is gone (or
/// re-headed) in a metadata-only local drift cannot have been legitimately
/// consumed — HEAD is authoritative and the local drift is spurious.
pub fn metadata_drift_authority(file: &Path, local: &str, head: &str) -> MetadataDriftAuthority {
    let local_head = crate::queue_continuation::live_continuation_head(file, local);
    let head_head = crate::queue_continuation::live_continuation_head(file, head);
    match (local_head, head_head) {
        // HEAD carries a live continuation that the local side dropped entirely
        // (deactivated / drained / fenced) with no consuming response → HEAD is
        // authoritative; committing the local side would silently lose it. This
        // is the live-observed `queue_active:false` spurious-drift bug.
        (None, Some(_)) => MetadataDriftAuthority::Head,
        // Both sides carry a live continuation but with different ready heads, and
        // (content-equal) no response consumed the old head → the next prompt
        // diverged without proof. Genuinely ambiguous → fail closed.
        (Some(local_id), Some(head_id)) if local_id != head_id => MetadataDriftAuthority::Ambiguous,
        // Same live head, HEAD has no live continuation at risk, or neither side
        // does → committing the local side forward loses no continuation.
        _ => MetadataDriftAuthority::Local,
    }
}

/// Apply the provably-safe recovery for `QueueMetadataDrift` / `SidecarVisibleDrift`.
fn apply_metadata_drift_recovery(
    file: &Path,
    state: CloseoutRecoveryState,
) -> Result<RecoveryApplication> {
    let head = crate::git::show_head(file)?;
    let snapshot = crate::snapshot::load(file).ok().flatten();
    let working = std::fs::read_to_string(file).ok();
    // For QueueMetadataDrift the local (commit-candidate) side is the snapshot;
    // for SidecarVisibleDrift the snapshot already matches HEAD and the local side
    // is the visible/working file.
    let local = match state {
        CloseoutRecoveryState::QueueMetadataDrift => snapshot.as_deref(),
        _ => working.as_deref(),
    };
    let (Some(local), Some(head)) = (local, head.as_deref()) else {
        return Ok(RecoveryApplication::NotApplied {
            state,
            reason: "auto-apply withheld — could not load both the local and HEAD document sides to prove the authoritative direction".to_string(),
            recommended: state.recovery_command(file).unwrap_or_default(),
        });
    };

    match metadata_drift_authority(file, local, head) {
        MetadataDriftAuthority::Local => {
            // Commit the local side forward. For SidecarVisibleDrift the snapshot
            // is HEAD-equal, so rebuild the sidecars from the visible file first so
            // the selective `git::commit` stages the accepted working metadata.
            if state == CloseoutRecoveryState::SidecarVisibleDrift {
                rebuild_sidecars_from_content(file, local)?;
            }
            crate::git::commit(file)?;
            Ok(RecoveryApplication::Applied {
                state,
                action:
                    "committed local metadata drift (local side authoritative — no live HEAD queue continuation at risk)"
                        .to_string(),
            })
        }
        MetadataDriftAuthority::Head => {
            // HEAD's live queue continuation is authoritative; discard the spurious
            // local metadata drift by restoring the visible file and sidecars from
            // HEAD. No new commit — HEAD already holds the authoritative content.
            std::fs::write(file, head)
                .with_context(|| format!("restore {} from HEAD", file.display()))?;
            rebuild_sidecars_from_content(file, head)?;
            Ok(RecoveryApplication::Applied {
                state,
                action:
                    "restored the document and sidecars from HEAD (committed queue continuation authoritative; discarded spurious local metadata drift)"
                        .to_string(),
            })
        }
        MetadataDriftAuthority::Ambiguous => Ok(RecoveryApplication::NotApplied {
            state,
            reason: "auto-apply withheld — both the local and HEAD sides carry distinct live queue continuation heads with no consuming response; the authoritative direction is ambiguous".to_string(),
            recommended: state.recovery_command(file).unwrap_or_default(),
        }),
    }
}

/// Rebuild the snapshot + CRDT sidecars from an explicit content string. Mirrors
/// the binary `reset --from-current` sidecar rebuild (snapshot + CRDT) so a
/// metadata-drift recovery converges the sidecars on the authoritative side. The
/// preflight-owned baseline is intentionally left untouched (it is re-taken at the
/// next stable post-commit point).
fn rebuild_sidecars_from_content(file: &Path, content: &str) -> Result<()> {
    crate::snapshot::save(file, content)?;
    let crdt = crate::crdt::CrdtDoc::from_text(content).encode_state();
    crate::snapshot::save_document_crdt(file, &crdt, content)?;
    Ok(())
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
        // `#recursive-repair-recovery-states`: an empty `preflight_started` cycle
        // (no capture, no captured-response hash, no pending mutation) is a
        // diagnostic/probe preflight that nothing followed — abandonable with a
        // single `agent-doc cancel`, distinct from a real in-progress cycle.
        CyclePhase::PreflightStarted
            if state.capture_id.is_none()
                && state.response_sha256.is_none()
                && !state.had_pending_mutations =>
        {
            return CloseoutRecoveryState::OpenEmptyPreflight;
        }
        CyclePhase::PreflightStarted | CyclePhase::ResponseCaptured | CyclePhase::WriteApplied => {
            return CloseoutRecoveryState::OpenCycle;
        }
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
    if state.capture_id.is_none() && state.response_sha256.is_none() && state.had_pending_mutations
    {
        return CloseoutRecoveryState::MissingResponseBody;
    }
    // `#closeout-recovery-state-machine`: a visible `### Re:` / `## Assistant`
    // response was patched into the working document outside the binary write
    // path. Recover by absorbing it through `write --commit`. Checked before the
    // generic content-drift fallthrough so the response-specific recovery wins
    // over `UnsafeUserContentDrift`. The jb-cache-conflict-cancel shape (the
    // binary write path applied the response but the commit boundary never
    // landed) is `git::commit`-recoverable, so it must NOT be misread as a direct
    // patchback — mirror `session_check::detect_uncommitted_closeout_drift`.
    if !crate::session_check::detect_jb_cache_conflict_cancel_recoverable(file).unwrap_or(false)
        && crate::session_check::detect_bypassed_response_write(file)
            .ok()
            .flatten()
            .is_some()
    {
        return CloseoutRecoveryState::DirectResponsePatchback;
    }
    // `#recursive-repair-state-drift` / `#recursive-repair-recovery-states`:
    // classify committed-cycle drift by *what* differs so the recovery names one
    // safe command. Order matters — narrowest/safest first, content drift last
    // (fail closed).
    let snapshot = crate::snapshot::load(file).ok().flatten();
    let head = crate::git::show_head(file).ok().flatten();
    if let (Some(snapshot), Some(head)) = (snapshot.as_deref(), head.as_deref())
        && snapshot != head
    {
        // Boundary / `(HEAD)` / answered-prompt-prefix artifacts only.
        if crate::git::normalize_committed_exchange_artifacts(snapshot)
            == crate::git::normalize_committed_exchange_artifacts(head)
        {
            return CloseoutRecoveryState::BoundaryOnlyDrift;
        }
        // User/response + tracked-item content is byte-identical → the diff is
        // queue / `queue_active` / status metadata (e.g. a `queue` sync-attribute
        // regeneration). Safe to `agent-doc commit`.
        if content_component_signature(snapshot) == content_component_signature(head) {
            return CloseoutRecoveryState::QueueMetadataDrift;
        }
        // Real user/response content differs from HEAD → never auto-commit.
        return CloseoutRecoveryState::UnsafeUserContentDrift;
    }
    // Snapshot matches HEAD but the visible/working file is stale relative to the
    // sidecars. Metadata-only visible drift → rebuild sidecars from the file;
    // content drift → preserve it through the normal response path.
    if let (Some(snapshot), Ok(working)) = (snapshot.as_deref(), std::fs::read_to_string(file))
        && snapshot != working
    {
        if content_component_signature(snapshot) == content_component_signature(&working) {
            return CloseoutRecoveryState::SidecarVisibleDrift;
        }
        return CloseoutRecoveryState::UnsafeUserContentDrift;
    }
    // `#closeout-recovery-state-machine`: the document itself is clean (snapshot
    // == HEAD == working) but a reaped/closed item left a nested parent submodule
    // pointer uncommitted — single safe recovery is `agent-doc commit`.
    if crate::git::submodule_pointer_drift(file)
        .ok()
        .flatten()
        .is_some()
    {
        return CloseoutRecoveryState::NestedParentPointerStale;
    }
    CloseoutRecoveryState::Clean
}

/// Normalized signature of the user/response + tracked-item *content* components
/// (`exchange`, backlog, review, icebox, done), excluding pure agent-doc metadata
/// (queue, status, frontmatter, boundary markers). Two documents with the same
/// signature differ only in metadata; a differing signature means real
/// user/response or tracked-item content changed. Used to split metadata-only
/// drift (safe to commit) from content drift (fail closed).
fn content_component_signature(doc: &str) -> String {
    let normalized = crate::git::normalize_committed_exchange_artifacts(doc);
    let Ok(components) = crate::component::parse(&normalized) else {
        return normalized;
    };
    let mut sig = String::new();
    for c in &components {
        let is_content = c.name == "exchange"
            || crate::component::is_backlog_component(&c.name)
            || crate::component::is_review_component(&c.name)
            || crate::component::is_icebox_component(&c.name)
            || crate::component::is_backlog_done_component(&c.name);
        if is_content {
            sig.push_str(&c.name);
            sig.push('\u{0}');
            sig.push_str(c.content(&normalized).trim());
            sig.push('\n');
        }
    }
    sig
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
    fn complete_required_closeout_reaps_lingering_completed_item() {
        // #exit75-done-reap-not-atomic: the exit-75 / file-IPC fallback commits a
        // `[x]` item without reaping it, then reaches complete_required_closeout.
        // The closeout must reap the lingering completed item in the same pass so
        // session-check passes without a separate recovery preflight.
        let base = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: close the loop — gpt-5\n\nImplemented and verified.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#donelinger] Close the loop\n",
            "- [ ] [#keep] Keep tracking follow-up\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n<!-- /agent:done -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);

        // Committed response cycle (capture present), with the `[x]` already on
        // disk + in HEAD — exactly the exit-75 residual shape.
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let response = "<!-- patch:exchange -->\n### Re: close the loop — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->";
        crate::capture::capture_response(&doc, response).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();

        complete_required_closeout(&doc).expect("closeout must reap the lingering completed item");

        let content = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !content.contains("- [x] [#donelinger]"),
            "closeout must reap the lingering completed item:\n{content}"
        );
        assert!(
            content.contains("- [ ] [#keep] Keep tracking follow-up"),
            "live follow-up must remain:\n{content}"
        );
        assert!(
            content.contains("<!-- agent:done -->") && content.contains("[#donelinger]"),
            "reaped item must be archived to agent:done:\n{content}"
        );

        // HEAD reflects the reap, and session-check accepts the closeout.
        let head = crate::git::show_head(&doc).unwrap().unwrap();
        assert!(
            !head.contains("- [x] [#donelinger]"),
            "HEAD must not strand the completed item:\n{head}"
        );
        matches!(
            crate::session_check::inspect(&doc).unwrap(),
            crate::session_check::SessionCheckStatus::Ok(_)
        )
        .then_some(())
        .expect("session-check must accept the atomic-reap closeout");
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
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
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
    fn stuck_captured_cycle_ignores_committed_cycle_when_only_guard_marker_stripped() {
        // #8j86: the captured response body carries an ephemeral
        // `<!-- no-pending-done-guard -->` guard marker that `git::strip_guard_markers`
        // removes from the committed blob. The materialization probe must mirror
        // that strip, otherwise stuck_captured_cycle false-alarms on a response
        // that IS in HEAD (seen live 2026-06-10 on agent-doc-bugs2.md capture
        // cycle-1781112407668 — no compact archive involved, body already in HEAD).
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        // Capture stores the raw patch-wrapped body including the guard marker.
        let captured = "<!-- patch:exchange -->\n<!-- no-pending-done-guard -->\n### Re: hello — gpt-5\n\nCommitted response.\n<!-- /patch:exchange -->\n";
        // Committed HEAD has the guard marker stripped (as `git::commit` does).
        let committed_response = "### Re: hello — gpt-5\n\nCommitted response.\n";
        let full_doc = format!("{base}\n{committed_response}");
        let (dir, doc) = setup_git_project_with_doc(base);

        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::capture::capture_response(&doc, captured).unwrap();
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

        assert!(
            stuck_captured_cycle(&doc).is_none(),
            "a committed response whose only HEAD difference is the stripped guard marker must not be flagged stuck"
        );
    }

    #[test]
    fn recovery_command_maps_each_state_to_one_instruction() {
        use CloseoutRecoveryState::*;
        let f = Path::new("tasks/doc.md");
        assert_eq!(Clean.recovery_command(f), None);
        for (state, name, needle) in [
            (OpenCycle, "open_cycle", "agent-doc finalize"),
            (
                MissingResponseBody,
                "missing_response_body",
                "agent-doc write --commit",
            ),
            (
                DirectResponsePatchback,
                "direct_response_patchback",
                "absorb the visible",
            ),
            (
                EscapedTemplatePatch,
                "escaped_template_patch",
                "patch:exchange",
            ),
            (BoundaryOnlyDrift, "boundary_only_drift", "boundary"),
            (
                NestedParentPointerStale,
                "nested_parent_pointer_stale",
                "parent submodule pointer",
            ),
            (
                OpenEmptyPreflight,
                "open_empty_preflight",
                "agent-doc cancel",
            ),
            (
                QueueMetadataDrift,
                "queue_metadata_drift",
                "agent-doc commit",
            ),
            (
                SidecarVisibleDrift,
                "sidecar_visible_drift",
                "reset --from-current",
            ),
            (
                UnsafeUserContentDrift,
                "unsafe_user_content_drift",
                "do NOT `agent-doc commit`",
            ),
        ] {
            assert_eq!(state.as_str(), name);
            let cmd = state
                .recovery_command(f)
                .expect("non-clean states have a command");
            assert!(
                cmd.contains(needle),
                "state {name} command {cmd:?} missing {needle:?}"
            );
            assert!(
                cmd.contains("tasks/doc.md"),
                "command should name the file: {cmd:?}"
            );
        }
    }

    #[test]
    fn recovery_decision_maps_states_to_typed_outcomes() {
        use CloseoutRecoveryDecision::*;
        use CloseoutRecoveryState::*;
        let f = Path::new("tasks/doc.md");

        assert_eq!(
            closeout_recovery_decision_from_state(
                f,
                Clean,
                CloseoutRecoveryDecisionInput::default()
            ),
            AlreadyCommitted
        );

        assert_eq!(
            closeout_recovery_decision_from_state(
                f,
                OpenCycle,
                CloseoutRecoveryDecisionInput {
                    prompt_context_available: true,
                    blocker_reason: Some("active closeout"),
                    stale_capture_supersession_proof: None,
                },
            ),
            QueuePromptForAfterCloseout {
                state: OpenCycle,
                reason: "active closeout".to_string(),
            }
        );

        match closeout_recovery_decision_from_state(
            f,
            DirectResponsePatchback,
            CloseoutRecoveryDecisionInput::default(),
        ) {
            ReplaySafe { state, command } => {
                assert_eq!(state, DirectResponsePatchback);
                assert!(command.contains("write --commit"), "{command}");
            }
            other => panic!("direct patchback should be replay-safe: {other:?}"),
        }

        match closeout_recovery_decision_from_state(
            f,
            SidecarVisibleDrift,
            CloseoutRecoveryDecisionInput::default(),
        ) {
            ResetSidecarsFromVisible { state, command } => {
                assert_eq!(state, SidecarVisibleDrift);
                assert!(command.contains("reset --from-current"), "{command}");
            }
            other => panic!("sidecar drift should reset sidecars: {other:?}"),
        }

        assert_eq!(
            closeout_recovery_decision_from_state(
                f,
                MissingResponseBody,
                CloseoutRecoveryDecisionInput {
                    stale_capture_supersession_proof: Some("heading already answered"),
                    ..CloseoutRecoveryDecisionInput::default()
                },
            ),
            RetireStaleCapture {
                state: MissingResponseBody,
                proof: "heading already answered".to_string(),
            }
        );

        match closeout_recovery_decision_from_state(
            f,
            MissingResponseBody,
            CloseoutRecoveryDecisionInput::default(),
        ) {
            Blocked {
                state,
                missing_proof,
                recommended,
            } => {
                assert_eq!(state, MissingResponseBody);
                assert!(missing_proof.contains("response body"), "{missing_proof}");
                assert!(recommended.contains("write --commit"), "{recommended}");
            }
            other => panic!("missing response body should block: {other:?}"),
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
    fn classify_recovery_open_empty_preflight_when_nothing_followed() {
        // `#recursive-repair-recovery-states`: a bare preflight_started cycle with
        // no capture / response / pending mutation is an abandonable probe.
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::OpenEmptyPreflight
        );
        let cmd = CloseoutRecoveryState::OpenEmptyPreflight
            .recovery_command(&doc)
            .unwrap();
        assert!(cmd.contains("agent-doc cancel"), "{cmd}");
    }

    #[test]
    fn classify_recovery_open_cycle_when_preflight_has_pending_mutations() {
        // A preflight_started cycle that already did work (pending mutation) is a
        // real open cycle to finish, not an abandonable empty probe.
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::OpenCycle
        );
    }

    #[test]
    fn classify_recovery_queue_metadata_drift_when_only_queue_differs() {
        // `#recursive-repair-recovery-states`: snapshot differs from HEAD only by
        // queue lines (a `queue` sync regeneration); exchange content identical.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let (_dir, doc) = setup_git_project_with_doc(head);
        crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::QueueMetadataDrift
        );
    }

    #[test]
    fn classify_recovery_unsafe_user_content_drift_when_exchange_differs() {
        // Real user/response content differs from HEAD → must not auto-commit.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n",
        );
        let snapshot = head.replace("Done.\n", "Done.\n\nReal unreviewed user content.\n");
        let (_dir, doc) = setup_git_project_with_doc(head);
        crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::UnsafeUserContentDrift
        );
    }

    #[test]
    fn classify_recovery_direct_response_patchback_when_visible_response_uncommitted() {
        // `#closeout-recovery-state-machine`: a `### Re:` response was patched
        // directly into the working file outside the binary write path (snapshot
        // and HEAD are clean, the working file gained the response). Classified as
        // DirectResponsePatchback → recover with `write --commit`, NOT the generic
        // UnsafeUserContentDrift / SidecarVisibleDrift.
        let base = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n❯ a question\n<!-- /agent:exchange -->\n",
        );
        let (_dir, doc) = setup_git_project_with_doc(base);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        crate::snapshot::save(&doc, base).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(base), Some(base)).unwrap();
        // Patch a visible response directly into the working file (bypassing write).
        let with_response = base.replace(
            "❯ a question\n",
            "❯ a question\n### Re: a question — gpt-5\n\nDirect answer.\n",
        );
        std::fs::write(&doc, &with_response).unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::DirectResponsePatchback
        );
        let cmd = CloseoutRecoveryState::DirectResponsePatchback
            .recovery_command(&doc)
            .unwrap();
        assert!(cmd.contains("write --commit"), "{cmd}");
    }

    #[test]
    fn classify_recovery_nested_parent_pointer_stale_when_submodule_ahead_of_parent() {
        // `#closeout-recovery-state-machine`: the document is clean (snapshot ==
        // HEAD == working) but its submodule HEAD is ahead of the parent repo's
        // recorded pointer (a reaped item left the parent pointer un-bumped).
        // Classified as NestedParentPointerStale → recover with `agent-doc commit`.
        let root = tempfile::TempDir::new().unwrap();
        let sub_origin = root.path().join("sub_origin");
        let sup = root.path().join("super");
        let init_repo = |p: &Path| {
            std::fs::create_dir_all(p).unwrap();
            run_git(p, &["init"]);
            run_git(p, &["config", "user.email", "test@example.com"]);
            run_git(p, &["config", "user.name", "Test User"]);
        };
        // Submodule origin with an initial commit (S1).
        init_repo(&sub_origin);
        std::fs::write(sub_origin.join("doc.md"), "---\nsession: t\n---\n\nv1\n").unwrap();
        run_git(&sub_origin, &["add", "."]);
        run_git(&sub_origin, &["commit", "-m", "s1", "--no-verify"]);
        // Super repo records the submodule pointer at S1.
        init_repo(&sup);
        run_git(
            &sup,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                sub_origin.to_str().unwrap(),
                "sub",
            ],
        );
        run_git(&sup, &["commit", "-m", "add sub", "--no-verify"]);
        // Advance the submodule HEAD (S2) WITHOUT bumping the parent pointer.
        let subwt = sup.join("sub");
        // The checked-out submodule's git repo (`super/.git/modules/sub`) may not
        // inherit the parent's local identity, and a clean CI sandbox has no global
        // identity, so pass identity inline on this commit command.
        let content = "---\nsession: t\nagent_doc_format: template\n---\n\nv2\n";
        std::fs::write(subwt.join("doc.md"), content).unwrap();
        run_git(&subwt, &["add", "doc.md"]);
        run_git(
            &subwt,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-m",
                "s2",
                "--no-verify",
            ],
        );
        // Document itself is clean: snapshot == HEAD == working.
        let doc = subwt.join("doc.md");
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(content), Some(content))
            .unwrap();
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::NestedParentPointerStale
        );
        let cmd = CloseoutRecoveryState::NestedParentPointerStale
            .recovery_command(&doc)
            .unwrap();
        assert!(cmd.contains("agent-doc commit"), "{cmd}");
    }

    #[test]
    fn apply_recovery_clean_is_nothing_to_do() {
        let (_dir, doc) = setup_git_project_with_doc("---\nsession: test\n---\n\nHi\n");
        assert_eq!(
            apply_closeout_recovery(&doc).unwrap(),
            RecoveryApplication::NothingToDo
        );
    }

    #[test]
    fn apply_recovery_cancels_open_empty_preflight() {
        // `#recursive-repair-apply`: the safe action for an empty probe cycle is to
        // abandon it — exactly the churn the diagnostic-preflight bug produces.
        let base = "---\nsession: test\n---\n\nHi\n";
        let (_dir, doc) = setup_git_project_with_doc(base);
        crate::cycle_state::start_preflight(&doc, Some(base), Some(base)).unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::Applied { state, .. } => {
                assert_eq!(state, CloseoutRecoveryState::OpenEmptyPreflight);
            }
            other => panic!("expected Applied for empty preflight, got {other:?}"),
        }
        // The cycle is now abandoned, so re-classification is Clean.
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn metadata_drift_authority_head_when_local_drops_live_continuation() {
        // `#recovery-drift-authoritative-side`: HEAD carries a live `queue_active`
        // continuation that the local (snapshot) side deactivated → HEAD is
        // authoritative (the spurious `queue_active:false` working-drift bug).
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("queue_active: true", "queue_active: false");
        assert_eq!(
            metadata_drift_authority(&file, &local, head),
            MetadataDriftAuthority::Head
        );
    }

    #[test]
    fn metadata_drift_authority_local_when_no_live_head_continuation() {
        // Neither side has a live continuation → committing the local side forward
        // loses no continuation → local is authoritative.
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        assert_eq!(
            metadata_drift_authority(&file, &local, head),
            MetadataDriftAuthority::Local
        );
    }

    #[test]
    fn metadata_drift_authority_ambiguous_when_live_heads_diverge() {
        // Both sides carry a live continuation but with different ready heads and
        // (content-equal) no consuming response → genuinely ambiguous.
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let local = head.replace("- do [#a]", "- do [#z]");
        assert_eq!(
            metadata_drift_authority(&file, &local, head),
            MetadataDriftAuthority::Ambiguous
        );
    }

    #[test]
    fn apply_recovery_commits_queue_metadata_drift_when_no_live_continuation() {
        // `#recovery-drift-authoritative-side`: with no live HEAD continuation at
        // risk, queue metadata drift is now auto-committed (local authoritative).
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        let (_dir, doc) = setup_git_project_with_doc(head);
        crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
        // The snapshot AND the visible file carry the drift; only HEAD is behind.
        std::fs::write(&doc, &snapshot).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::Applied { state, .. } => {
                assert_eq!(state, CloseoutRecoveryState::QueueMetadataDrift);
            }
            other => panic!("expected Applied for queue metadata drift, got {other:?}"),
        }
        // HEAD now carries the committed queue item, and re-classification is Clean.
        assert!(
            crate::git::show_head(&doc)
                .unwrap()
                .unwrap()
                .contains("- do [#b]")
        );
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn apply_recovery_restores_from_head_when_local_drops_live_continuation() {
        // The live bug: HEAD has an active `queue_active` continuation; a spurious
        // local snapshot drift flipped it to `false`. Auto-apply must restore from
        // HEAD (not commit the snapshot), preserving the live queue.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("queue_active: true", "queue_active: false");
        let (_dir, doc) = setup_git_project_with_doc(head);
        crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::Applied { state, action } => {
                assert_eq!(state, CloseoutRecoveryState::QueueMetadataDrift);
                assert!(action.contains("restored"), "{action}");
            }
            other => panic!("expected Applied (restore) for dropped continuation, got {other:?}"),
        }
        // The visible file + sidecars are restored to HEAD's live queue, so the
        // continuation survives and re-classification is Clean.
        let restored = std::fs::read_to_string(&doc).unwrap();
        assert!(restored.contains("queue_active: true"), "{restored}");
        assert_eq!(crate::snapshot::load(&doc).unwrap().unwrap(), restored);
        assert_eq!(
            classify_closeout_recovery_state(&doc),
            CloseoutRecoveryState::Clean
        );
    }

    #[test]
    fn apply_recovery_withholds_queue_metadata_drift_when_live_heads_diverge() {
        // Both sides carry distinct live continuation heads with no consuming
        // response → the direction is genuinely ambiguous → fail closed.
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n### Re: x — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        let snapshot = head.replace("- do [#a]", "- do [#z]");
        let (_dir, doc) = setup_git_project_with_doc(head);
        crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
        crate::snapshot::save(&doc, &snapshot).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
        .unwrap();
        match apply_closeout_recovery(&doc).unwrap() {
            RecoveryApplication::NotApplied {
                state, recommended, ..
            } => {
                assert_eq!(state, CloseoutRecoveryState::QueueMetadataDrift);
                assert!(recommended.contains("agent-doc commit"), "{recommended}");
            }
            other => panic!("expected NotApplied for ambiguous queue drift, got {other:?}"),
        }
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
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&full_doc),
            Some(&full_doc),
        )
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
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&snapshot),
            Some(&snapshot),
        )
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
