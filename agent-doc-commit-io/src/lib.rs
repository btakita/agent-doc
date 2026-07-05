use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts;
use agent_doc_document::transient_markers::{
    normalize_post_commit_re_heading_drift, normalize_transient_agent_doc_markers,
    strip_head_markers,
};
use agent_doc_document_realtime::write_policy::{
    classify_committed_historical_agent_doc_mutation, classify_safe_out_of_band_agent_doc_mutation,
    detect_reintroduced_reaped_pending_ids, is_empty_template_scaffold_snapshot,
};
use agent_doc_git::{
    agent_doc_commit_message_for_file, classify_post_commit_local_drift, commit_retry_backoff,
    has_blocking_non_exchange_component_drift,
};
use agent_doc_git_io::{
    dirs::{narrow_to_submodule, resolve_to_git_root},
    transaction::{
        CommitLock, CommitTransactionError, stage_and_commit_once, update_parent_submodule_pointer,
    },
};
use agent_doc_queue_io::queue_consume;
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitOutcome {
    pub did_commit: bool,
    pub vcs_refresh_signaled: Option<bool>,
}

pub struct CommitCoordinatorPorts<
    'a,
    PreStageRepair,
    CommitResultReporting,
    CaptureMaterializationGuard,
    GuardMarkerCleanup,
    LiveBufferGuard,
    BoundaryReposition,
    BoundaryInvariant,
    TransientCleanup,
    PostCommitCleanup,
    QueueConsumeWrite,
    WriteConvergence,
> {
    pub pre_stage_repair: &'a PreStageRepair,
    pub commit_result_reporting: &'a CommitResultReporting,
    pub capture_materialization_guard: &'a CaptureMaterializationGuard,
    pub guard_marker_cleanup: &'a GuardMarkerCleanup,
    pub live_buffer_guard: &'a LiveBufferGuard,
    pub boundary_reposition: &'a BoundaryReposition,
    pub boundary_invariant: &'a BoundaryInvariant,
    pub transient_cleanup: &'a TransientCleanup,
    pub post_commit_cleanup: &'a PostCommitCleanup,
    pub queue_consume_write: &'a QueueConsumeWrite,
    pub write_convergence: &'a WriteConvergence,
}

struct RuntimeCommitPreStageRepairEffects;

static COMMIT_PRE_STAGE_REPAIR_EFFECTS: RuntimeCommitPreStageRepairEffects =
    RuntimeCommitPreStageRepairEffects;

struct RuntimeCommitResultReportingEffects;

static COMMIT_RESULT_REPORTING_EFFECTS: RuntimeCommitResultReportingEffects =
    RuntimeCommitResultReportingEffects;

struct RuntimeCaptureMaterializationGuardEffects;

static CAPTURE_MATERIALIZATION_GUARD_EFFECTS: RuntimeCaptureMaterializationGuardEffects =
    RuntimeCaptureMaterializationGuardEffects;

struct RuntimeGuardMarkerCleanupEffects;

static GUARD_MARKER_CLEANUP_EFFECTS: RuntimeGuardMarkerCleanupEffects =
    RuntimeGuardMarkerCleanupEffects;

struct RuntimeLiveBufferGuardEffects;

static LIVE_BUFFER_GUARD_EFFECTS: RuntimeLiveBufferGuardEffects = RuntimeLiveBufferGuardEffects;

#[doc(hidden)]
pub struct RuntimeBoundaryRepositionEffects;

#[doc(hidden)]
pub static BOUNDARY_REPOSITION_EFFECTS: RuntimeBoundaryRepositionEffects =
    RuntimeBoundaryRepositionEffects;

struct RuntimeBoundaryInvariantEffects;

static BOUNDARY_INVARIANT_EFFECTS: RuntimeBoundaryInvariantEffects =
    RuntimeBoundaryInvariantEffects;

struct RuntimeTransientCleanupEffects;

static TRANSIENT_CLEANUP_EFFECTS: RuntimeTransientCleanupEffects = RuntimeTransientCleanupEffects;

#[doc(hidden)]
pub struct RuntimePostCommitCleanupEffects;

#[doc(hidden)]
pub static POST_COMMIT_CLEANUP_EFFECTS: RuntimePostCommitCleanupEffects =
    RuntimePostCommitCleanupEffects;

impl agent_doc_git_io::pre_stage_repair::CommitPreStageRepairEffects
    for RuntimeCommitPreStageRepairEffects
{
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

impl agent_doc_git_io::commit_result_reporting::CommitResultReportingEffects
    for RuntimeCommitResultReportingEffects
{
    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

impl agent_doc_git_io::capture_materialization_guard::CaptureMaterializationGuardEffects
    for RuntimeCaptureMaterializationGuardEffects
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
    for RuntimeGuardMarkerCleanupEffects
{
    fn load_snapshot(&self, file: &Path) -> Result<Option<String>> {
        agent_doc_snapshot_io::load(file)
    }

    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn read_to_string(&self, file: &Path) -> Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "guard_marker_cleanup",
        )
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            current_content,
            target_content,
            reason,
        )
    }
}

impl agent_doc_git_io::live_buffer_guard::LiveBufferGuardEffects for RuntimeLiveBufferGuardEffects {
    fn commit_barrier_ready(&self, file: &Path) -> bool {
        agent_doc_crdt_relay_io::commit_barrier_for_file(file)
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

impl agent_doc_git_io::boundary_reposition::BoundaryRepositionEffects
    for RuntimeBoundaryRepositionEffects
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
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "live_buffer_guard",
        )
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
            agent_doc_write_ipc_io::FileIpcRepositionResult::DeferredExistingPatch => Ok(
                agent_doc_git_io::boundary_reposition::BoundaryRepositionDelivery::DeferredExistingPatch,
            ),
            agent_doc_write_ipc_io::FileIpcRepositionResult::Unavailable => {
                Ok(agent_doc_git_io::boundary_reposition::BoundaryRepositionDelivery::Unavailable)
            }
        }
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }
}

impl agent_doc_git_io::boundary_invariant::BoundaryInvariantEffects
    for RuntimeBoundaryInvariantEffects
{
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

impl agent_doc_git_io::transient_cleanup::TransientCleanupEffects
    for RuntimeTransientCleanupEffects
{
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_snapshot_io::save(file, content, agent_doc_ops_log_io::log_op)
    }

    fn save_document_crdt(&self, file: &Path, legacy_state: &[u8], markdown: &str) -> Result<()> {
        agent_doc_merge_io::save_document_crdt(file, legacy_state, markdown)
    }

    fn editor_attached(&self, file: &Path) -> bool {
        agent_doc_plugin_owner::crdt_authority::authority_for_file(&file.display().to_string())
            .editor_attached()
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
    for RuntimePostCommitCleanupEffects
{
    fn read_to_string(&self, file: &Path) -> Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "post_commit_cleanup",
        )
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
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
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

pub fn commit(file: &Path) -> Result<bool> {
    Ok(commit_with_outcome(file)?.did_commit)
}

pub fn commit_with_outcome(file: &Path) -> Result<CommitOutcome> {
    commit_with_ports_outcome(
        CommitCoordinatorPorts {
            pre_stage_repair: &COMMIT_PRE_STAGE_REPAIR_EFFECTS,
            commit_result_reporting: &COMMIT_RESULT_REPORTING_EFFECTS,
            capture_materialization_guard: &CAPTURE_MATERIALIZATION_GUARD_EFFECTS,
            guard_marker_cleanup: &GUARD_MARKER_CLEANUP_EFFECTS,
            live_buffer_guard: &LIVE_BUFFER_GUARD_EFFECTS,
            boundary_reposition: &BOUNDARY_REPOSITION_EFFECTS,
            boundary_invariant: &BOUNDARY_INVARIANT_EFFECTS,
            transient_cleanup: &TRANSIENT_CLEANUP_EFFECTS,
            post_commit_cleanup: &POST_COMMIT_CLEANUP_EFFECTS,
            queue_consume_write:
                &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
            write_convergence: &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
        },
        file,
    )
}

pub fn commit_with_ports_outcome<
    PreStageRepair,
    CommitResultReporting,
    CaptureMaterializationGuard,
    GuardMarkerCleanup,
    LiveBufferGuard,
    BoundaryReposition,
    BoundaryInvariant,
    TransientCleanup,
    PostCommitCleanup,
    QueueConsumeWrite,
    WriteConvergence,
>(
    ports: CommitCoordinatorPorts<
        '_,
        PreStageRepair,
        CommitResultReporting,
        CaptureMaterializationGuard,
        GuardMarkerCleanup,
        LiveBufferGuard,
        BoundaryReposition,
        BoundaryInvariant,
        TransientCleanup,
        PostCommitCleanup,
        QueueConsumeWrite,
        WriteConvergence,
    >,
    file: &Path,
) -> Result<CommitOutcome>
where
    PreStageRepair: agent_doc_git_io::pre_stage_repair::CommitPreStageRepairEffects,
    CommitResultReporting: agent_doc_git_io::commit_result_reporting::CommitResultReportingEffects,
    CaptureMaterializationGuard:
        agent_doc_git_io::capture_materialization_guard::CaptureMaterializationGuardEffects,
    GuardMarkerCleanup: agent_doc_git_io::guard_marker_cleanup::GuardMarkerCleanupEffects,
    LiveBufferGuard: agent_doc_git_io::live_buffer_guard::LiveBufferGuardEffects,
    BoundaryReposition: agent_doc_git_io::boundary_reposition::BoundaryRepositionEffects,
    BoundaryInvariant: agent_doc_git_io::boundary_invariant::BoundaryInvariantEffects,
    TransientCleanup: agent_doc_git_io::transient_cleanup::TransientCleanupEffects,
    PostCommitCleanup: agent_doc_git_io::post_commit_cleanup::PostCommitCleanupEffects,
    QueueConsumeWrite: agent_doc_queue_io::queue_consume::QueueConsumeWriteEffects,
    WriteConvergence: agent_doc_write_converge_io::EditorConvergenceEffects,
{
    let t_total = std::time::Instant::now();

    let (super_root, resolved) = resolve_to_git_root(file)?;
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    if in_submodule {
        eprintln!(
            "[commit] file is in submodule {} - running git ops there",
            git_root.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "submodule_route file={} submodule={}",
                file.display(),
                git_root.display()
            ),
        );
    }
    let _commit_lock = CommitLock::acquire(&git_root);

    queue_consume::strike_answered_free_text_heads_at_commit_seam(file, ports.queue_consume_write);

    let timestamp = chrono_timestamp();
    let msg = agent_doc_commit_message_for_file(file, &timestamp);

    let mut snapshot_content = agent_doc_snapshot_io::load(file)?;
    let mut file_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "commit_initial_current",
    )
    .unwrap_or_default();
    let head_doc = agent_doc_git_io::revision::show_head(file)?;
    let snapshot_matched_head_before_absorb = snapshot_content
        .as_deref()
        .zip(head_doc.as_deref())
        .is_some_and(|(snapshot, head)| strip_head_markers(snapshot) == head);
    let bypassed_response_write = snapshot_matched_head_before_absorb
        .then(|| agent_doc_session_check_io::detect_bypassed_response_write(file))
        .transpose()?
        .flatten();
    let safe_out_of_band_mutation = snapshot_content
        .as_deref()
        .and_then(|snapshot| classify_safe_out_of_band_agent_doc_mutation(snapshot, &file_content));
    let safe_out_of_band_exchange_only = safe_out_of_band_mutation == Some("exchange");
    let only_heading_attribution_drift = head_doc.as_deref().is_some_and(|head| {
        normalize_post_commit_re_heading_drift(&file_content)
            == normalize_post_commit_re_heading_drift(head)
    });
    if let Some(marker) = bypassed_response_write
        && !safe_out_of_band_exchange_only
        && !only_heading_attribution_drift
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_blocked_bypassed_patchback file={} basis=head marker={}",
                file.display(),
                marker.replace('\n', " ")
            ),
        );
        anyhow::bail!(
            "refusing to treat {} as already committed: found likely direct response patchback without agent-doc cycle: {}",
            file.display(),
            marker
        );
    }

    let turn_scope = agent_doc_turn_scope_io::load(file);
    let committed_historical_patchback =
        snapshot_content
            .as_deref()
            .zip(head_doc.as_deref())
            .map(|(snapshot, head)| {
                let mutation = classify_committed_historical_agent_doc_mutation(snapshot, head);
                (
                    mutation.or_else(|| {
                        has_blocking_non_exchange_component_drift(
                            snapshot,
                            head,
                            turn_scope.as_ref(),
                        )
                        .then_some("typed_component_drift")
                    }),
                    agent_doc_document_realtime::baseline_comparison::detect_bypassed_response_write_between(
                        snapshot, head,
                    ),
                )
            });
    let file_len = file_content.len();
    let snap_len = snapshot_content.as_ref().map(|s| s.len()).unwrap_or(0);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "commit_staging file={} snap_len={} file_len={}",
            file.display(),
            snap_len,
            file_len
        ),
    );
    if let Some(snapshot) = snapshot_content.as_deref()
        && let Some(recovered) = agent_doc_write_converge_io::try_auto_recover_live_prompt_drift(
            ports.write_convergence,
            file,
            snapshot,
            &file_content,
        )?
    {
        file_content = recovered;
        snapshot_content = agent_doc_snapshot_io::load(file)?;
    }
    if agent_doc_write_converge_io::guard_no_stale_snapshot_reset_drift(
        file,
        snapshot_content.as_deref(),
        &file_content,
        "commit",
    )? {
        snapshot_content = agent_doc_snapshot_io::load(file)?;
    }

    let repaired_committed_historical = if let Some(reason) =
        agent_doc_repair_io::repair_committed_historical_snapshot_drift(file)?
    {
        eprintln!(
            "[commit] repaired committed historical {} drift into snapshot for {}",
            reason,
            file.display()
        );
        snapshot_content = agent_doc_snapshot_io::load(file)?;
        true
    } else {
        false
    };

    let cycle_state_for_commit = agent_doc_cycle_state_io::load(file)?;
    let ipc_snapshot_adoption_blocked = cycle_state_for_commit
        .as_ref()
        .is_some_and(|state| state.ipc_snapshot_adoption_blocked);
    let has_dropped_queue_prompt_evidence = cycle_state_for_commit
        .as_ref()
        .is_some_and(|state| !state.dropped_queue_prompts.is_empty());
    let active_response_target = agent_doc_capture_io::load_active(file)
        .ok()
        .flatten()
        .and_then(|capture| {
            agent_doc_turn::response_text::response_prompt_target_from_re_heading(
                &capture.response_body,
            )
        });
    if ipc_snapshot_adoption_blocked
        && let Some(snapshot) = snapshot_content.as_deref()
        && live_prompt_drift_missing_response_with_unanswered_prompt_for_commit(
            snapshot,
            &file_content,
            active_response_target.as_deref(),
        )
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_blocked_live_prompt_drift_user_prompt file={} old_snap_len={} new_snap_len={}",
                file.display(),
                snap_len,
                file_len
            ),
        );
        anyhow::bail!(
            "refusing to close {}: the recovery checkpoint contains a response that is missing from the visible file, and the visible exchange also contains a new unanswered prompt. Re-run preflight after preserving the visible prompt.",
            file.display()
        );
    }
    let reintroduced_reaped_ids = cycle_state_for_commit
        .map(|state| state.reaped_pending_ids.into_iter().collect::<HashSet<_>>())
        .map(|ids| detect_reintroduced_reaped_pending_ids(&file_content, &ids))
        .transpose()?
        .unwrap_or_default();
    if !reintroduced_reaped_ids.is_empty() {
        let refs = reintroduced_reaped_ids
            .iter()
            .map(|id| format!("#{}", id))
            .collect::<Vec<_>>()
            .join(", ");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_blocked_reintroduced_reaped_pending file={} ids={}",
                file.display(),
                refs
            ),
        );
        anyhow::bail!(
            "refusing to close {}: tracked backlog/icebox item(s) reaped earlier in this cycle reappeared in the live file: {}. Re-run preflight after resolving the stale local/editor rewrite",
            file.display(),
            refs
        );
    }

    let snapshot_matches_current_file = snapshot_content.as_deref().is_some_and(|snapshot| {
        snapshot == file_content
            || normalize_transient_agent_doc_markers(snapshot)
                == normalize_transient_agent_doc_markers(&file_content)
    });

    if !repaired_committed_historical
        && !snapshot_matches_current_file
        && let Some((Some(kind), Some(marker))) = committed_historical_patchback.as_ref()
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_blocked_committed_historical_patchback file={} kind={} marker={}",
                file.display(),
                kind,
                marker.replace('\n', " ")
            ),
        );
        anyhow::bail!(
            "refusing to auto-adopt committed historical response patchback for {}: HEAD contains an out-of-band {} mutation with response marker {}",
            file.display(),
            kind,
            marker
        );
    }

    if let Some(ref snapshot) = snapshot_content
        && snapshot != &file_content
        && !(repaired_committed_historical
            && snapshot
                .as_str()
                .eq(head_doc.as_deref().unwrap_or_default()))
        && let Some(reason) = classify_safe_out_of_band_agent_doc_mutation(snapshot, &file_content)
    {
        if reason.contains("exchange")
            && exchange_has_unanswered_disk_only_prompt_target_for_commit(
                snapshot,
                &file_content,
                active_response_target.as_deref(),
            )
        {
            eprintln!(
                "[commit] leaving out-of-band exchange prompt drift unabsorbed for {}",
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_absorb_skipped_prompt_target file={} reason={} old_snap_len={} new_snap_len={}",
                    file.display(),
                    reason,
                    snap_len,
                    file_len
                ),
            );
        } else if ipc_snapshot_adoption_blocked {
            eprintln!(
                "[commit] refusing to absorb out-of-band {} mutation after IPC snapshot adoption was blocked for {}",
                reason,
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_absorb_blocked_after_ipc_snapshot_adoption file={} blocked_by={} old_snap_len={} new_snap_len={}",
                    file.display(),
                    reason,
                    snap_len,
                    file_len
                ),
            );
        } else {
            eprintln!(
                "[commit] absorbing out-of-band {} mutation into snapshot for {}",
                reason,
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_absorb file={} reason={} old_snap_len={} new_snap_len={}",
                    file.display(),
                    reason,
                    snap_len,
                    file_len
                ),
            );
            agent_doc_snapshot_io::save(file, &file_content, agent_doc_ops_log_io::log_op)?;
            snapshot_content = Some(file_content.clone());
        }
    }

    agent_doc_git_io::pre_stage_repair::dedupe_snapshot_and_worktree_before_commit(
        ports.pre_stage_repair,
        file,
        &mut snapshot_content,
        &mut file_content,
    )?;

    let mut snapshot_matches_head = snapshot_content
        .as_deref()
        .zip(head_doc.as_deref())
        .is_some_and(|(snapshot, head)| strip_head_markers(snapshot) == head);
    let mut stale_response_collapse_repaired = false;
    if snapshot_matches_head
        && let Some(head) = head_doc.as_deref()
        && let Some(cleaned) =
            agent_doc_template::deleted_conversation_tail_cleanup(head, &file_content)?
    {
        eprintln!(
            "[commit] committing manual escaped conversation tail cleanup for {}",
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "post_commit_escaped_tail_cleanup file={} basis=head",
                file.display()
            ),
        );
        agent_doc_snapshot_io::save(file, &cleaned, agent_doc_ops_log_io::log_op)?;
        snapshot_content = Some(cleaned);
        snapshot_matches_head = false;
    }
    if snapshot_matches_head
        && let Some(head) = head_doc.as_deref()
        && let Some(repaired) =
            agent_doc_git_io::transient_cleanup::repair_stale_agent_response_collapse_worktree(
                ports.transient_cleanup,
                file,
                head,
                &file_content,
            )?
    {
        file_content = repaired;
        stale_response_collapse_repaired = true;
    }
    let post_commit_local_drift = if snapshot_matches_head {
        head_doc
            .as_deref()
            .and_then(|head| classify_post_commit_local_drift(head, &file_content))
    } else {
        None
    };
    if snapshot_matches_head
        && let Some(drift_kind) = post_commit_local_drift
        && let Some(head) = head_doc.as_deref()
        && response_bearing_exchange_drift_after_committed_head(head, &file_content)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_blocked_response_patchback_uncommitted file={} basis=head drift_kind={}",
                file.display(),
                drift_kind.as_str()
            ),
        );
        agent_doc_flow_io::closeout::log_closeout_guard_event(
            file,
            agent_doc_flow::types::FlowStage::Commit,
            agent_doc_flow::types::FlowOutcome::FailedClosed,
            agent_doc_turn::closeout_guard::CloseoutGuardReason::ResponsePatchbackUncommitted,
        );
        anyhow::bail!(
            "refusing to close {} as already committed: the staged snapshot matches HEAD, but the working tree has response-bearing exchange edits that are not committed. Run `agent-doc write --commit {}` after verifying the visible response body, then run `agent-doc session-check {}`.",
            file.display(),
            file.display(),
            file.display()
        );
    }
    if snapshot_matches_head
        && (ipc_snapshot_adoption_blocked
            || has_dropped_queue_prompt_evidence
            || stale_response_collapse_repaired)
        && let Some(head) = head_doc.as_deref()
        && let Some(added_prompts) =
            agent_doc_queue::queue_replay::preserved_queue_additions_neutralized_by_replay(
                head,
                &file_content,
            )
    {
        eprintln!(
            "[commit] committing preserved queue addition drift for {} ({} prompt(s)); replay normalization neutralized the queue component",
            file.display(),
            added_prompts
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_absorb file={} reason=preserved_queue_addition_replay_neutralized prompts={} old_snap_len={} new_snap_len={}",
                file.display(),
                added_prompts,
                snap_len,
                file_content.len()
            ),
        );
        agent_doc_snapshot_io::save(file, &file_content, agent_doc_ops_log_io::log_op)?;
        snapshot_content = Some(file_content.clone());
        snapshot_matches_head = false;
    }

    let snap_len = snapshot_content.as_ref().map(|s| s.len()).unwrap_or(0);
    let file_len_after_repair = file_content.len();
    if snap_len > 0 && file_len_after_repair > snap_len && post_commit_local_drift.is_none() {
        let drift = file_len_after_repair - snap_len;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "out_of_band_write file={} drift={} snap_len={} file_len={}",
                file.display(),
                drift,
                snap_len,
                file_len_after_repair
            ),
        );
        if drift > 100 && post_commit_local_drift.is_none() {
            eprintln!(
                "[commit] WARNING: file is {} bytes larger than snapshot for {} - possible out-of-band write (snap={}, file={})",
                drift,
                file.display(),
                snap_len,
                file_len_after_repair
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "drift_warning file={} drift={} snap_len={} file_len={}",
                    file.display(),
                    drift,
                    snap_len,
                    file_len_after_repair
                ),
            );

            if file_len_after_repair > snap_len * 5
                && let Some(ref snapshot) = snapshot_content
            {
                let head_exists = head_doc.is_some();
                let scaffold_snapshot = is_empty_template_scaffold_snapshot(snapshot);
                if !head_exists && scaffold_snapshot {
                    eprintln!(
                        "[commit] Extreme drift detected ({}x) - re-syncing bootstrap scaffold snapshot from file content",
                        file_len_after_repair / snap_len.max(1)
                    );
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "snapshot_resync file={} old_snap_len={} new_snap_len={}",
                            file.display(),
                            snap_len,
                            file_len_after_repair
                        ),
                    );
                    agent_doc_snapshot_io::save(file, &file_content, agent_doc_ops_log_io::log_op)?;
                    snapshot_content = Some(file_content.clone());
                } else {
                    eprintln!(
                        "[commit] Extreme drift detected ({}x) - NOT re-syncing tracked/non-scaffold snapshot",
                        file_len_after_repair / snap_len.max(1)
                    );
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "snapshot_resync_blocked file={} head_exists={} scaffold_snapshot={} old_snap_len={} file_len={}",
                            file.display(),
                            head_exists,
                            scaffold_snapshot,
                            snap_len,
                            file_len_after_repair
                        ),
                    );
                }
            }
        }
    }

    if snapshot_content.is_none() && !file_content.is_empty() {
        eprintln!(
            "[commit] WARNING: no snapshot exists for {}. Creating from file content.",
            file.display()
        );
        agent_doc_snapshot_io::save(file, &file_content, agent_doc_ops_log_io::log_op)?;
        snapshot_content = Some(file_content.clone());
    }

    if snapshot_matches_head {
        agent_doc_git_io::capture_materialization_guard::ensure_active_capture_materialized_for_head_current_noop(
            ports.capture_materialization_guard,
            file,
            snapshot_content.as_deref(),
            head_doc.as_deref(),
        )?;
        agent_doc_git_io::live_buffer_guard::ensure_no_live_editor_buffer_ahead_of_disk(
            ports.live_buffer_guard,
            file,
            &file_content,
            "already_current",
            snapshot_content.as_deref().or(head_doc.as_deref()),
        )?;
        let disposition =
            agent_doc_git_io::post_commit_cleanup::log_already_current_local_drift_handoff(
                ports.post_commit_cleanup,
                file,
                post_commit_local_drift,
            );
        if disposition
            == agent_doc_git_io::post_commit_cleanup::AlreadyCurrentLocalDriftDisposition::PromptHandoffNoop
        {
            let elapsed_total = t_total.elapsed().as_millis();
            if elapsed_total > 0 {
                eprintln!("[perf] commit total: {}ms", elapsed_total);
            }
            return Ok(CommitOutcome {
                did_commit: false,
                vcs_refresh_signaled: None,
            });
        }
        eprintln!(
            "[commit] staged snapshot already matches HEAD for {} - closing cycle as already committed",
            file.display()
        );
        let (snapshot_after_noop, file_after_noop) =
            agent_doc_git_io::transient_cleanup::repair_clean_head_if_only_transient_worktree_drift(
                ports.transient_cleanup,
                file,
                &file_content,
            )?
            .unwrap_or((snapshot_content.clone(), file_content.clone()));
        agent_doc_git_io::post_commit_cleanup::finalize_already_committed_noop(
            ports.post_commit_cleanup,
            file,
            "commit_already_current",
            snapshot_after_noop.as_deref(),
            Some(&file_after_noop),
            post_commit_local_drift,
        );

        if in_submodule && agent_doc_git_io::submodule::is_submodule_pointer_stale(file) {
            eprintln!("[commit] submodule pointer stale in parent after no-op commit - updating");
            update_parent_submodule_pointer(&super_root, &git_root, &msg);
        }

        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] commit total: {}ms", elapsed_total);
        }
        return Ok(CommitOutcome {
            did_commit: false,
            vcs_refresh_signaled: None,
        });
    }

    if let (Some(snapshot), Some(head)) = (snapshot_content.as_deref(), head_doc.as_deref()) {
        let dropped = agent_doc_document::commit_integrity::dropped_committed_frontmatter_keys(
            snapshot,
            head,
            &file_content,
        );
        if !dropped.is_empty() {
            let corrected = agent_doc_document::commit_integrity::overlay_live_frontmatter(
                snapshot,
                &file_content,
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "commit_frontmatter_self_heal file={} restored={} basis=live (#fmdrop)",
                    file.display(),
                    dropped.join(",")
                ),
            );
            eprintln!(
                "[commit] snapshot dropped operator frontmatter key(s) [{}] - restoring from the live document and regenerating the snapshot (#fmdrop)",
                dropped.join(", ")
            );
            if let Err(e) =
                agent_doc_snapshot_io::save(file, &corrected, agent_doc_ops_log_io::log_op)
            {
                eprintln!(
                    "[commit] warning: snapshot regenerate after frontmatter self-heal failed: {e} (non-fatal)"
                );
            }
            snapshot_content = Some(corrected);
        }
    }

    let t_reposition = std::time::Instant::now();
    let _snap_changed = agent_doc_git_io::boundary_reposition::reposition_boundary_in_snapshot(
        ports.boundary_reposition,
        file,
    );
    if let Ok(Some(reloaded)) = agent_doc_snapshot_io::load(file) {
        snapshot_content = Some(reloaded);
    }
    file_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "commit_after_boundary_reposition",
    )
    .unwrap_or_default();
    agent_doc_git_io::pre_stage_repair::dedupe_snapshot_and_worktree_before_commit(
        ports.pre_stage_repair,
        file,
        &mut snapshot_content,
        &mut file_content,
    )?;
    agent_doc_git_io::live_buffer_guard::ensure_no_live_editor_buffer_ahead_of_disk(
        ports.live_buffer_guard,
        file,
        &file_content,
        "pre_stage",
        snapshot_content.as_deref(),
    )?;
    agent_doc_git_io::capture_materialization_guard::ensure_active_capture_materialized_for_commit(
        ports.capture_materialization_guard,
        file,
        snapshot_content.as_deref().or(Some(file_content.as_str())),
        "staged",
    )?;
    let elapsed_reposition = t_reposition.elapsed().as_millis();
    if elapsed_reposition > 0 {
        eprintln!("[perf] commit.reposition: {}ms", elapsed_reposition);
    }

    let t_commit = std::time::Instant::now();
    let mut commit_attempts = 0u32;
    let commit_output = loop {
        let t_staging = std::time::Instant::now();
        match stage_and_commit_once(&git_root, &resolved, snapshot_content.as_deref(), &msg) {
            Ok(out) => {
                let elapsed_staging = t_staging.elapsed().as_millis();
                if elapsed_staging > 0 {
                    eprintln!(
                        "[perf] commit.staging (hash_object+update-index): {}ms",
                        elapsed_staging
                    );
                }
                break Ok(out);
            }
            Err(CommitTransactionError::RetryableIndexLock { phase, detail })
                if commit_attempts < 3 =>
            {
                commit_attempts += 1;
                let elapsed_staging = t_staging.elapsed().as_millis();
                if elapsed_staging > 0 {
                    eprintln!(
                        "[perf] commit.staging (hash_object+update-index): {}ms",
                        elapsed_staging
                    );
                }
                eprintln!(
                    "[commit] index.lock contention during {} (retry {}/3): {}",
                    phase, commit_attempts, detail
                );
                std::thread::sleep(commit_retry_backoff(commit_attempts));
                continue;
            }
            Err(CommitTransactionError::RetryableIndexLock { phase, detail }) => {
                break Err(anyhow::anyhow!(
                    "git {} failed after index.lock retries: {}",
                    phase,
                    detail
                ));
            }
            Err(CommitTransactionError::IgnoredPath { path }) => {
                break Err(
                    agent_doc_git_io::commit_result_reporting::ignored_untracked_path_error(
                        ports.commit_result_reporting,
                        file,
                        &path,
                    ),
                );
            }
            Err(CommitTransactionError::Fatal(err)) => break Err(err),
        }
    };
    let elapsed_commit = t_commit.elapsed().as_millis();
    if elapsed_commit > 0 {
        eprintln!("[perf] commit.git_commit: {}ms", elapsed_commit);
    }

    let commit_outcome = agent_doc_git_io::commit_result_reporting::report_commit_output(
        ports.commit_result_reporting,
        file,
        commit_output.as_ref(),
    );

    let mut did_commit = false;
    match commit_outcome {
        agent_doc_git_io::commit_result_reporting::CommitCommandOutcome::Success => {
            did_commit = true;
            agent_doc_git_io::boundary_invariant::enforce_committed_single_boundary_invariant(
                ports.boundary_invariant,
                file,
                &git_root,
                &resolved,
            );
            agent_doc_git_io::post_commit_cleanup::finalize_successful_commit(
                ports.post_commit_cleanup,
                file,
                head_doc.as_deref(),
            );
        }
        agent_doc_git_io::commit_result_reporting::CommitCommandOutcome::Failed
        | agent_doc_git_io::commit_result_reporting::CommitCommandOutcome::Error => {}
    }

    let mut vcs_refresh_signaled = None;
    if commit_outcome == agent_doc_git_io::commit_result_reporting::CommitCommandOutcome::Success {
        if agent_doc_git_io::post_commit_cleanup::should_send_post_commit_ipc_reposition(file) {
            agent_doc_write_ipc_io::try_ipc_reposition_boundary(file);
        } else {
            eprintln!(
                "[commit] skipping IPC boundary reposition: commit changed non-exchange tracked surfaces"
            );
        }

        vcs_refresh_signaled =
            agent_doc_git_io::transient_cleanup::signal_vcs_refresh(ports.transient_cleanup, file);

        agent_doc_git_io::guard_marker_cleanup::strip_guard_markers_from_disk(
            ports.guard_marker_cleanup,
            file,
        );
        if let Ok(cleaned) = agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "commit_transient_cleanup",
        ) {
            match agent_doc_git_io::transient_cleanup::repair_clean_head_if_only_transient_worktree_drift(
                ports.transient_cleanup,
                file,
                &cleaned,
            ) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if let Err(e) =
                        agent_doc_git_io::transient_cleanup::refresh_live_closeout_sidecars(
                            ports.transient_cleanup,
                            file,
                            &cleaned,
                            false,
                        )
                    {
                        eprintln!(
                            "[commit] warning: failed to refresh CRDT sidecars after post-commit cleanup: {}",
                            e
                        );
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[commit] warning: failed to reconcile post-commit transient worktree drift: {}",
                        e
                    );
                }
            }
        }

        agent_doc_git_io::post_commit_cleanup::emit_postcommit_worktree_check(
            ports.post_commit_cleanup,
            file,
        );

        if in_submodule {
            update_parent_submodule_pointer(&super_root, &git_root, &msg);
        }
    }

    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] commit total: {}ms", elapsed_total);
    }

    Ok(CommitOutcome {
        did_commit,
        vcs_refresh_signaled,
    })
}

fn exchange_has_unanswered_disk_only_prompt_target_for_commit(
    snapshot_doc: &str,
    current_doc: &str,
    active_response_target: Option<&str>,
) -> bool {
    let (Ok(snapshot_components), Ok(current_components)) = (
        agent_doc_element::element::parse(snapshot_doc),
        agent_doc_element::element::parse(current_doc),
    ) else {
        return true;
    };
    let (Some(snapshot_exchange), Some(current_exchange)) = (
        snapshot_components
            .iter()
            .find(|component| component.name == "exchange"),
        current_components
            .iter()
            .find(|component| component.name == "exchange"),
    ) else {
        return true;
    };

    let snapshot_counts =
        exchange_prompt_target_counts_for_commit(snapshot_exchange.content(snapshot_doc));
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for prompt in exchange_prompt_target_lines_for_commit(current_exchange.content(current_doc)) {
        let count = seen.entry(prompt.clone()).or_insert(0);
        *count += 1;
        if *count > snapshot_counts.get(&prompt).copied().unwrap_or(0) {
            let answered_by_active_response = active_response_target.is_some_and(|target| {
                prompt_target_matches_active_response_for_commit(&prompt, target)
            });
            if !answered_by_active_response {
                return true;
            }
        }
    }
    false
}

fn live_prompt_drift_missing_response_with_unanswered_prompt_for_commit(
    snapshot_doc: &str,
    current_doc: &str,
    active_response_target: Option<&str>,
) -> bool {
    if agent_doc_document_realtime::baseline_comparison::detect_bypassed_response_write_between(
        current_doc,
        snapshot_doc,
    )
    .is_none()
    {
        return false;
    }
    exchange_has_unanswered_disk_only_prompt_target_for_commit(
        snapshot_doc,
        current_doc,
        active_response_target,
    )
}

fn exchange_prompt_target_counts_for_commit(
    exchange_body: &str,
) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::new();
    for prompt in exchange_prompt_target_lines_for_commit(exchange_body) {
        *counts.entry(prompt).or_insert(0) += 1;
    }
    counts
}

fn exchange_prompt_target_lines_for_commit(exchange_body: &str) -> Vec<String> {
    exchange_body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('❯')
                || agent_doc_prompt_lines::text_line_looks_like_prompt_target(trimmed)
            {
                Some(
                    trimmed
                        .strip_prefix('❯')
                        .unwrap_or(trimmed)
                        .trim()
                        .to_string(),
                )
            } else {
                None
            }
        })
        .collect()
}

fn prompt_target_matches_active_response_for_commit(
    prompt: &str,
    active_response_target: &str,
) -> bool {
    let prompt = prompt_match_key_for_commit(prompt);
    let target = prompt_match_key_for_commit(active_response_target);
    if prompt.is_empty() || target.is_empty() {
        return false;
    }
    if prompt == target || prompt.contains(&target) || target.contains(&prompt) {
        return true;
    }
    let target_tokens = target.split_whitespace().collect::<Vec<_>>();
    target_tokens.len() >= 2 && target_tokens.iter().all(|token| prompt.contains(token))
}

fn prompt_match_key_for_commit(text: &str) -> String {
    text.trim()
        .trim_start_matches('❯')
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn response_bearing_exchange_drift_after_committed_head(head_doc: &str, current_doc: &str) -> bool {
    let normalized_head = normalize_committed_exchange_artifacts(head_doc);
    let normalized_current = normalize_committed_exchange_artifacts(current_doc);
    if normalized_head == normalized_current {
        return false;
    }
    if agent_doc_document_realtime::baseline_comparison::detect_bypassed_response_write_between(
        &normalized_head,
        &normalized_current,
    )
    .is_some()
    {
        return true;
    }
    agent_doc_document_realtime::baseline_comparison::exchange_has_new_appended_content(
        &normalized_head,
        &normalized_current,
    ) && !exchange_append_is_prompt_target_only(&normalized_head, &normalized_current)
}

fn exchange_append_is_prompt_target_only(snapshot_doc: &str, current_doc: &str) -> bool {
    let Some(diff) = agent_doc_diff::unified_diff_from_contents(snapshot_doc, current_doc) else {
        return false;
    };
    let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff);
    !changes.is_empty()
        && changes
            .iter()
            .all(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
}

fn chrono_timestamp() -> String {
    let output = Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".to_string(),
    }
}
