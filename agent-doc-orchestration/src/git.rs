//! # Module: git
//!
//! ## Spec
//! - `commit(file)`: stages and commits a session document with an auto-generated
//!   `agent-doc(<stem>): <timestamp>` message, skipping hooks (`--no-verify`).  Relative paths are
//!   resolved against the git superproject root first, then the toplevel.  When a snapshot exists,
//!   a CLEAN copy of the snapshot (with all ` (HEAD)` heading suffixes stripped via
//!   `strip_head_markers`) is staged via `git hash-object + update-index` so the working tree is
//!   not staged directly. Narrow agent-owned snapshot drift can be absorbed first; plain user
//!   keystrokes typed after the snapshot was taken remain uncommitted (green gutter).
//!   Narrow exception: when the working tree is ahead of the snapshot due to a missed
//!   agent-doc-style status mutation and/or exchange append and/or pending mutation, `commit`
//!   first absorbs that live document state into the snapshot, then stages it. Plain user prompts
//!   are not absorbed.
//!   Refuses untracked paths matched by `.gitignore` before using plumbing that would otherwise
//!   bypass porcelain ignore checks. Falls back to `git add -f` when hash-object fails for
//!   non-ignored paths. If the staged snapshot already matches `HEAD`, `commit` closes the cycle
//!   as an already-committed no-op instead of logging a false `commit_failed`. After a successful
//!   commit, post-commit cleanup collapses boundary churn in the snapshot and fires an IPC
//!   reposition signal (`try_ipc_reposition_boundary`) so the IDE plugin can normalize the working
//!   tree to the same clean shape as the committed blob. Without a live listener, the file is
//!   rewritten locally to that same clean shape. Returns `true` when a git commit was created and
//!   `false` when there was nothing new to commit.
//! - `commit_with_outcome(file)`: same as `commit`, but also reports whether the
//!   VCS refresh signal was available and successfully written after the commit.
//! - `strip_head_markers` (private): strips ` (HEAD)` suffix from markdown headings and bold-text
//!   pseudo-headers in the commit-staging path.  `(HEAD)` is treated as a transient artifact and
//!   must never appear in the committed blob.
//! - `strip_guard_markers` (private): strips `<!-- no-pending-capture -->` and
//!   `<!-- no-pending-done-guard -->` from the commit-staging path.  These are ephemeral
//!   per-cycle signals for `session-check`; the check reads from the capture file, not
//!   the committed document.  Post-commit cleanup also strips them from the snapshot and
//!   working-tree file.
//!
//! ## Agentic Contracts
//! - `commit` never modifies the working tree file directly; all staging is done through the git
//!   index.  The only disk write is to the snapshot file.
//! - `commit` captures all git stdout to stderr so callers that reserve stdout for JSON (e.g.,
//!   `preflight`) are not polluted.
//! - All public functions resolve paths relative to the superproject root when running inside a
//!   submodule, so git commands always run in the correct repo.
//! - `show_head` and `last_commit_mtime` return `Ok(None)` (not `Err`) when the file has no git
//!   history.
//! - Safe out-of-band absorb is narrow: only component-local drift that leaves the redacted
//!   document structure unchanged and looks like an agent-owned `status` change and/or a
//!   `### Re:` response-block insertion and/or a pending-ID superset is absorbed. Free-form user
//!   edits remain uncommitted. Historical response-block insertions that are already committed in
//!   `HEAD` may also repair the snapshot even when they are no longer appended at the tail.
//!
//! ## Evals
//! - strip_head_markers_from_headings: heading lines with ` (HEAD)` suffix → suffix removed; non-heading lines unchanged
//! - strip_head_markers_preserves_non_heading_lines: body text containing "(HEAD)" → preserved verbatim
//! - strip_head_markers_bold_text: bold-text pseudo-header `**Re: Something** (HEAD)` → suffix removed
//! - strip_head_markers_ignores_fenced_code_hash: `(HEAD)` inside fenced code block preserved verbatim
//! - strip_guard_markers_removes_standalone_lines: lines containing only `<!-- no-pending-capture -->` or `<!-- no-pending-done-guard -->` → removed; surrounding content preserved
//! - strip_guard_markers_strips_inline_content: guard markers embedded in content lines → marker text removed, trailing whitespace trimmed
//! - strip_guard_markers_strips_trailing_on_content_line: guard marker appended to end of content line → marker removed, content preserved
//! - commit_staged_blob_has_no_head_markers: regression for #dsng — commit staging strips `(HEAD)` from the blob and post-commit cleanup leaves the working tree/snapshot clean
//! - commit_skips_ignored_untracked_path: untracked session docs matched by `.gitignore` are not staged via hash-object/update-index or `git add -f`
//! - commit_retries_full_transaction_when_stage_hits_index_lock: `update-index` index.lock contention retries the full stage+commit transaction until the lock clears
//! - commit_serializes_closeout_per_git_root: two different docs in the same repo contend on one repo-scoped lock and both close out cleanly
//! - reposition_boundary_to_end_basic: stale boundary before user prompt → boundary repositioned after prompt
//! - reposition_boundary_no_exchange: doc with no exchange component → content returned unchanged
//! - reposition_boundary_preserves_user_edits: user text between response and boundary → all user text preserved, boundary after it
//! - reposition_boundary_cleans_multiple_stale: document with 2 stale boundaries → all removed, exactly 1 fresh boundary at end after user text
//! - commit_repairs_committed_historical_snapshot_drift: historical direct response already committed in `HEAD` repairs the stale snapshot without creating a duplicate commit
//! - commit_repairs_committed_head_before_user_follow_up_noop: snapshot lags behind a committed response in `HEAD`, working tree adds only a new user follow-up, and commit repairs the snapshot up to `HEAD` instead of staging the stale snapshot and rewinding the doc
//! - commit_closes_cycle_when_staged_snapshot_already_matches_head: stale open cycle + later user edit → close as already committed instead of `commit_failed`
//! - commit_skips_terminal_user_follow_up_noop_closeout: terminal committed cycle + later user follow-up → leave the prompt untouched without re-emitting closeout lifecycle bookkeeping
//! - commit_already_current_repairs_transient_working_tree_churn: already-committed no-op closeout repairs boundary / `(HEAD)`-only file drift back to clean `HEAD`
//! - commit_already_current_repairs_transient_working_tree_churn_refreshes_crdt_and_signal: no-op closeout cleanup also refreshes CRDT/editor-facing sidecars so stale transient churn cannot reappear from cached live state
//! - commit_already_current_repairs_stale_agent_response_collapse_preserving_queue_follow_up: already-committed no-op closeout restores a collapsed committed exchange heading while preserving later queue prompt drift outside `agent:exchange`
//! - verify_snapshot_committed_returns_committed_when_matching: snapshot matches HEAD → `Committed`
//! - verify_snapshot_committed_returns_differs_when_snapshot_ahead: snapshot has content not in HEAD → `SnapshotDiffersFromHead`
//! - verify_snapshot_committed_no_snapshot: no snapshot file → `NoSnapshot`
//! - verify_snapshot_committed_no_head: file not tracked → `NoHead`
//! - submodule_noop_commit_updates_stale_parent_pointer: no-op commit in submodule still updates stale parent pointer

use anyhow::Result;
use std::path::{Path, PathBuf};

pub use agent_doc_commit_io::CommitOutcome;

struct OrchestrationCommitPreStageRepairEffects;

static COMMIT_PRE_STAGE_REPAIR_EFFECTS: OrchestrationCommitPreStageRepairEffects =
    OrchestrationCommitPreStageRepairEffects;

struct OrchestrationCommitResultReportingEffects;

static COMMIT_RESULT_REPORTING_EFFECTS: OrchestrationCommitResultReportingEffects =
    OrchestrationCommitResultReportingEffects;

struct OrchestrationCaptureMaterializationGuardEffects;

static CAPTURE_MATERIALIZATION_GUARD_EFFECTS: OrchestrationCaptureMaterializationGuardEffects =
    OrchestrationCaptureMaterializationGuardEffects;

struct OrchestrationGuardMarkerCleanupEffects;

static GUARD_MARKER_CLEANUP_EFFECTS: OrchestrationGuardMarkerCleanupEffects =
    OrchestrationGuardMarkerCleanupEffects;

struct OrchestrationLiveBufferGuardEffects;

static LIVE_BUFFER_GUARD_EFFECTS: OrchestrationLiveBufferGuardEffects =
    OrchestrationLiveBufferGuardEffects;

impl agent_doc_git_io::pre_stage_repair::CommitPreStageRepairEffects
    for OrchestrationCommitPreStageRepairEffects
{
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        crate::write::atomic_write_pub(file, content)
    }

    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

impl agent_doc_git_io::commit_result_reporting::CommitResultReportingEffects
    for OrchestrationCommitResultReportingEffects
{
    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

impl agent_doc_git_io::capture_materialization_guard::CaptureMaterializationGuardEffects
    for OrchestrationCaptureMaterializationGuardEffects
{
    fn load_active_capture(
        &self,
        file: &Path,
    ) -> Result<Option<agent_doc_git_io::capture_materialization_guard::ActiveCaptureMaterialization>>
    {
        Ok(agent_doc_capture_io::load_active(file)?.map(|capture| {
            agent_doc_git_io::capture_materialization_guard::ActiveCaptureMaterialization {
                capture_id: capture.capture_id,
                response_sha256: capture.response_sha256,
                response_body: capture.response_body,
                terminal: matches!(
                    capture.state,
                    agent_doc_workflow::capture::CaptureState::Committed
                        | agent_doc_workflow::capture::CaptureState::Discarded
                ),
            }
        }))
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }

    fn log_missing_capture_guard(&self, file: &Path) {
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::TerminalGuard,
            agent_doc_flow::types::FlowOutcome::FailedClosed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::AlreadyCommitted,
        );
    }
}

impl agent_doc_git_io::guard_marker_cleanup::GuardMarkerCleanupEffects
    for OrchestrationGuardMarkerCleanupEffects
{
    fn load_snapshot(&self, file: &Path) -> Result<Option<String>> {
        agent_doc_snapshot_io::load(file)
    }

    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn read_to_string(&self, file: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(file)?)
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &crate::write::WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            reason,
        )
    }
}

impl agent_doc_git_io::live_buffer_guard::LiveBufferGuardEffects
    for OrchestrationLiveBufferGuardEffects
{
    fn live_buffer_diverges_from_content(
        &self,
        file: &Path,
        file_content: &str,
    ) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
        let file_str = file.display().to_string();
        agent_doc_debounce::live_buffer_diverges_from_content(&file_str, file_content)
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }

    fn log_live_buffer_guard_blocked(&self, file: &Path) {
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::PreCommitGuard,
            agent_doc_flow::types::FlowOutcome::Blocked,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::ReplicaDeliveryPending,
        );
    }
}

#[doc(hidden)]
pub struct OrchestrationBoundaryRepositionEffects;

#[doc(hidden)]
pub static BOUNDARY_REPOSITION_EFFECTS: OrchestrationBoundaryRepositionEffects =
    OrchestrationBoundaryRepositionEffects;

struct OrchestrationBoundaryInvariantEffects;

static BOUNDARY_INVARIANT_EFFECTS: OrchestrationBoundaryInvariantEffects =
    OrchestrationBoundaryInvariantEffects;

struct OrchestrationTransientCleanupEffects;

static TRANSIENT_CLEANUP_EFFECTS: OrchestrationTransientCleanupEffects =
    OrchestrationTransientCleanupEffects;

#[doc(hidden)]
pub struct OrchestrationPostCommitCleanupEffects;

#[doc(hidden)]
pub static POST_COMMIT_CLEANUP_EFFECTS: OrchestrationPostCommitCleanupEffects =
    OrchestrationPostCommitCleanupEffects;

impl agent_doc_git_io::boundary_reposition::BoundaryRepositionEffects
    for OrchestrationBoundaryRepositionEffects
{
    fn active_run(&self, file: &Path) -> bool {
        file.canonicalize()
            .ok()
            .and_then(|canonical| agent_doc_fs::pending_response_path_for(&canonical).ok())
            .map(|pending_path| pending_path.exists())
            .unwrap_or(false)
    }

    fn load_snapshot(&self, file: &Path) -> Result<Option<String>> {
        agent_doc_snapshot_io::load(file)
    }

    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn ipc_listener_active(&self, file: &Path) -> bool {
        file.canonicalize()
            .map(|canonical| agent_doc_project_root_io::resolve_ipc_project_root(&canonical))
            .map(|root| agent_doc_ipc_io::is_listener_active(&root))
            .unwrap_or(false)
    }

    fn read_to_string(&self, file: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(file)?)
    }

    fn queue_file_ipc_reposition_boundary(
        &self,
        file: &Path,
        committed_boundary_id: Option<&str>,
        normalize_prefix_lines: &[String],
    ) -> Result<agent_doc_git_io::boundary_reposition::BoundaryRepositionDelivery> {
        match agent_doc_write_ipc_io::queue_file_ipc_reposition_boundary(
            file,
            committed_boundary_id,
            normalize_prefix_lines,
        )? {
            agent_doc_write_ipc_io::FileIpcRepositionResult::Queued => {
                Ok(agent_doc_git_io::boundary_reposition::BoundaryRepositionDelivery::Queued)
            }
            agent_doc_write_ipc_io::FileIpcRepositionResult::DeferredExistingPatch => {
                Ok(
                    agent_doc_git_io::boundary_reposition::BoundaryRepositionDelivery::DeferredExistingPatch,
                )
            }
            agent_doc_write_ipc_io::FileIpcRepositionResult::Unavailable => {
                Ok(agent_doc_git_io::boundary_reposition::BoundaryRepositionDelivery::Unavailable)
            }
        }
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        crate::write::atomic_write_pub(file, content)
    }
}

impl agent_doc_git_io::boundary_invariant::BoundaryInvariantEffects
    for OrchestrationBoundaryInvariantEffects
{
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

impl agent_doc_git_io::transient_cleanup::TransientCleanupEffects
    for OrchestrationTransientCleanupEffects
{
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        crate::write::atomic_write_pub(file, content)
    }

    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn save_document_crdt(&self, file: &Path, legacy_state: &[u8], markdown: &str) -> Result<()> {
        agent_doc_merge_io::save_document_crdt(file, legacy_state, markdown)
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }

    fn project_root_containing(&self, file: &Path) -> Option<PathBuf> {
        agent_doc_project_root_io::project_root_containing(file)
    }

    fn ipc_listener_active(&self, project_root: &Path) -> bool {
        agent_doc_ipc_io::is_listener_active(project_root)
    }

    fn send_vcs_refresh(&self, project_root: &Path) -> Result<bool> {
        agent_doc_ipc_io::send_vcs_refresh(project_root)
    }

    fn write_vcs_refresh_signal(&self, signal_file: &Path) -> Result<()> {
        Ok(std::fs::write(signal_file, "")?)
    }
}

impl agent_doc_git_io::post_commit_cleanup::PostCommitCleanupEffects
    for OrchestrationPostCommitCleanupEffects
{
    fn read_to_string(&self, file: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(file)?)
    }

    fn load_snapshot(&self, file: &Path) -> Option<String> {
        agent_doc_snapshot_io::load(file).ok().flatten()
    }

    fn cycle_is_terminal(&self, file: &Path) -> bool {
        agent_doc_cycle_state_io::load(file)
            .ok()
            .flatten()
            .is_some_and(|state| !state.is_open())
    }

    fn log_cycle(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) {
        agent_doc_ops_log_io::log_cycle(file, event, snapshot_content, file_content);
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }

    fn log_closeout_commit_completed(&self, file: &Path, reason: &str) {
        agent_doc_flow_io::log_flow_event(
            file,
            agent_doc_flow::types::FlowEvent::new(
                agent_doc_flow::types::FlowName::Closeout,
                agent_doc_flow::types::FlowStage::Commit,
                agent_doc_flow::types::FlowOutcome::Completed,
            )
            .with_reason(reason),
            agent_doc_ops_log_io::log_op,
        );
    }

    fn mark_pipeline_committed(
        &self,
        file: &Path,
        event: &str,
        snapshot_content: Option<&str>,
        file_content: Option<&str>,
    ) -> Result<()> {
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            file,
            event,
            snapshot_content,
            file_content,
        )?;
        Ok(())
    }

    fn mark_capture_committed(&self, file: &Path) -> Result<()> {
        agent_doc_capture_io::mark_committed(file)
    }

    fn clear_queue_journal(&self, file: &Path) {
        agent_doc_queue_io::queue_journal::clear(file);
    }

    fn reconcile_queue_continuation(
        &self,
        file: &Path,
        phase: &str,
    ) -> Option<agent_doc_git_io::post_commit_cleanup::QueueContinuationProof> {
        agent_doc_queue_io::queue_continuation::reconcile_marker(file, phase).map(|continuation| {
            agent_doc_git_io::post_commit_cleanup::QueueContinuationProof {
                head_prompt: continuation.head_prompt,
                head_id: continuation.head_id,
            }
        })
    }

    fn read_session_id(&self, file: &Path) -> String {
        agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_default()
    }

    fn fire_post_commit(&self, file: &Path, session_id: &str) {
        agent_doc_hooks_io::fire_post_commit(file, session_id, None);
    }

    fn fire_doc_event(&self, file: &Path, event: &str) {
        agent_doc_hooks_io::fire_doc_event(file, event);
    }
}

/// Commit a file with an auto-generated message. Skips hooks.
/// Relative paths are resolved against the git root (superproject if in a submodule).
/// Git commands run from the resolved git root, so this works even when CWD is a submodule.
pub fn commit(file: &Path) -> Result<bool> {
    Ok(commit_with_outcome(file)?.did_commit)
}

/// Commit a file and report whether the VCS refresh signal was written.
///
/// `vcs_refresh_signaled` is:
/// - `Some(true)` when the commit path wrote `.agent-doc/patches/vcs-refresh.signal`
/// - `Some(false)` when a refresh target was available but writing it failed
/// - `None` when no refresh target was available or no new git commit was created
pub fn commit_with_outcome(file: &Path) -> Result<CommitOutcome> {
    agent_doc_commit_io::commit_with_outcome(
        agent_doc_commit_io::CommitCoordinatorPorts {
            pre_stage_repair: &COMMIT_PRE_STAGE_REPAIR_EFFECTS,
            commit_result_reporting: &COMMIT_RESULT_REPORTING_EFFECTS,
            capture_materialization_guard: &CAPTURE_MATERIALIZATION_GUARD_EFFECTS,
            guard_marker_cleanup: &GUARD_MARKER_CLEANUP_EFFECTS,
            live_buffer_guard: &LIVE_BUFFER_GUARD_EFFECTS,
            boundary_reposition: &BOUNDARY_REPOSITION_EFFECTS,
            boundary_invariant: &BOUNDARY_INVARIANT_EFFECTS,
            transient_cleanup: &TRANSIENT_CLEANUP_EFFECTS,
            post_commit_cleanup: &POST_COMMIT_CLEANUP_EFFECTS,
            queue_consume_write: &crate::write::QUEUE_CONSUME_WRITEBACK_EFFECTS,
            write_convergence: &crate::write::WRITE_CONVERGENCE_EFFECTS,
        },
        file,
    )
}
