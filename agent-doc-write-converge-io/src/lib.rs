//! Write convergence sidecar adapters.
//!
//! This crate owns file-backed write-convergence decisions that sit between
//! pure realtime/write policy and durable sidecars. It keeps those decision
//! graphs out of the orchestration command crate.

use agent_doc_document::write_normalization::{
    AGENT_RESPONSE_COMPONENT, convergence_recovered_editor_wins_for_payload,
    convergence_recovered_editor_wins_outside_response,
};
use agent_doc_document_realtime::write_policy::{
    AckMismatchRecovery, OperatorReconcileStep, WholeBufferAuthority, WholeBufferAuthorityFacts,
    WholeBufferDelivery, WholeBufferDeliveryAction, classify_ack_mismatch_recovery,
    decide_whole_buffer_delivery, exchange_change_is_safe_historical_reduction,
    live_prompt_drift_recovery_target, normalize_visible_recovery_compare, operator_reconcile_step,
    should_refuse_disk_fallback, snapshot_contains_dropped_prompt, stale_snapshot_reset_drift,
};
use agent_doc_element::element::is_backlog_component;
use agent_doc_ipc_io::editor_target::target_payload_to_live_editor;
use agent_doc_ipc_protocol::{
    IpcRepairDecision, is_socket_ack_timeout_error, is_socket_status_error,
};
use agent_doc_turn::response_replay::response_materialized_in_content;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn log_flow_event(file: &Path, event: agent_doc_flow::types::FlowEvent) {
    let message =
        agent_doc_flow::types::flow_event_log_message(&file.display().to_string(), &event);
    agent_doc_ops_log_io::log_op(file, &message);
}

/// Effects still owned by the orchestration command crate while the write
/// authority queue is being extracted.
pub trait EditorConvergenceEffects {
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;

    fn atomic_write_if_current(
        &self,
        file: &Path,
        content: &str,
        expected_current: &str,
        source: &str,
    ) -> Result<()>;

    fn cycle_already_committed(&self, file: &Path) -> Option<String>;

    fn log_file_ipc_already_committed(&self, file: &Path, cycle_id: &str);

    fn cleanup_fallback_patch_files(&self, file: &Path);

    fn log_file_ipc_proof_failure(
        &self,
        file: &Path,
        patch_id: Option<&str>,
        invariant: &str,
        recovery: &str,
        detail: &str,
    );
}

#[cfg(test)]
const ACK_CONTENT_TYPING_SETTLE_MS: u64 = 75;
#[cfg(not(test))]
const ACK_CONTENT_TYPING_SETTLE_MS: u64 = 500;
#[cfg(test)]
const ACK_CONTENT_TYPING_TIMEOUT_MS: u64 = 1_000;
#[cfg(not(test))]
const ACK_CONTENT_TYPING_TIMEOUT_MS: u64 = 2_000;

fn live_buffer_file_keys(file: &Path) -> Vec<String> {
    let mut keys = Vec::new();
    if let Ok(canonical) = file.canonicalize() {
        keys.push(canonical.to_string_lossy().to_string());
    }
    let raw = file.to_string_lossy().to_string();
    if !keys.iter().any(|key| key == &raw) {
        keys.push(raw);
    }
    keys
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AckContentDiskWriteProof {
    pub authority: WholeBufferAuthority,
    pub source_buffer_matches: bool,
}

impl AckContentDiskWriteProof {
    fn unproven() -> Self {
        Self {
            authority: WholeBufferAuthority::AckContentSidecar,
            source_buffer_matches: false,
        }
    }
}

pub fn ack_content_disk_write_proof(
    file: &Path,
    editor_id: Option<&str>,
    content: &str,
) -> AckContentDiskWriteProof {
    let Some(editor_id) = editor_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return AckContentDiskWriteProof::unproven();
    };
    let content_len = content.len();
    let content_hash = agent_doc_hash::content_hash(content);

    if let Some(typing_key) = live_buffer_file_keys(file).into_iter().find(|file_key| {
        agent_doc_debounce::is_typing_via_file(file_key, ACK_CONTENT_TYPING_SETTLE_MS)
    }) {
        let settled = agent_doc_debounce::await_idle_via_file(
            &typing_key,
            ACK_CONTENT_TYPING_SETTLE_MS,
            ACK_CONTENT_TYPING_TIMEOUT_MS,
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ack_content_disk_write_proof_typing_settle file={} settled={} settle_ms={} timeout_ms={} key={}",
                file.display(),
                settled,
                ACK_CONTENT_TYPING_SETTLE_MS,
                ACK_CONTENT_TYPING_TIMEOUT_MS,
                typing_key
            ),
        );
        if !settled {
            return AckContentDiskWriteProof::unproven();
        }
    }

    let Some(snapshot) = live_buffer_file_keys(file)
        .into_iter()
        .flat_map(|file_key| agent_doc_debounce::live_buffer_snapshots(&file_key))
        .filter(|snapshot| {
            snapshot
                .editor_id
                .as_deref()
                .is_some_and(|candidate| candidate == editor_id)
        })
        .filter(agent_doc_debounce::live_buffer_snapshot_editor_is_live)
        .max_by_key(|snapshot| snapshot.timestamp_ms)
    else {
        return AckContentDiskWriteProof::unproven();
    };

    let source_buffer_matches =
        snapshot.len == content_len && snapshot.hash.eq_ignore_ascii_case(&content_hash);
    let authority =
        if snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY) {
            WholeBufferAuthority::OperatorTextAuthority
        } else {
            WholeBufferAuthority::AckContentSidecar
        };

    AckContentDiskWriteProof {
        authority,
        source_buffer_matches,
    }
}

/// Newest operator-authoritative live-buffer content for `editor_id`, or `None`
/// when no live operator buffer is present.
fn newest_operator_authoritative_buffer(file: &Path, editor_id: &str) -> Option<String> {
    live_buffer_file_keys(file)
        .into_iter()
        .flat_map(|file_key| agent_doc_debounce::live_buffer_snapshots(&file_key))
        .filter(|snapshot| {
            snapshot
                .editor_id
                .as_deref()
                .is_some_and(|candidate| candidate == editor_id)
        })
        .filter(agent_doc_debounce::live_buffer_snapshot_editor_is_live)
        .filter(|snapshot| {
            snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY)
        })
        .max_by_key(|snapshot| snapshot.timestamp_ms)
        .and_then(|snapshot| snapshot.content)
}

/// Settle the editor (wait out active typing) so the next live-buffer read is
/// quiescent. Bounded by the ack-content settle/timeout budget; a no-op when the
/// editor is not typing.
fn settle_editor_typing(file: &Path) {
    for key in live_buffer_file_keys(file) {
        if agent_doc_debounce::is_typing_via_file(&key, ACK_CONTENT_TYPING_SETTLE_MS) {
            agent_doc_debounce::await_idle_via_file(
                &key,
                ACK_CONTENT_TYPING_SETTLE_MS,
                ACK_CONTENT_TYPING_TIMEOUT_MS,
            );
        }
    }
}

/// Max rounds of the bounded reconcile-before-accept loop. Each round settles the
/// editor and re-samples the operator buffer; the loop ends when the buffer is
/// stable across two reads (a fixpoint) or this bound is hit.
const ACK_RECONCILE_MAX_ROUNDS: usize = 4;

/// `#adoc-live-prompt-drift-operator-edit` (Phase 2): the bounded
/// reconcile-before-accept loop. When the operator kept editing past the ack
/// capture (so the ack snapshot is stale relative to the live buffer), settle the
/// editor and re-sample its buffer until it reaches a fixpoint (unchanged across
/// two reads) or the round bound is hit, then adopt that settled
/// operator-authoritative buffer as the snapshot, provided it still presents this
/// cycle's response. This owns only the IO/settling; the decision each round is
/// owned by the realtime model.
pub fn reconcile_ack_snapshot_to_newer_operator_buffer(
    file: &Path,
    editor_id: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    let Some(editor_id) = editor_id.map(str::trim).filter(|id| !id.is_empty()) else {
        return false;
    };
    let mut prev: Option<String> = None;
    for round in 0..ACK_RECONCILE_MAX_ROUNDS {
        settle_editor_typing(file);
        let Some(curr) = newest_operator_authoritative_buffer(file, editor_id) else {
            return false;
        };
        match operator_reconcile_step(&decision.snapshot_content, prev.as_deref(), &curr) {
            OperatorReconcileStep::Accept(content) => {
                if content == decision.snapshot_content {
                    return false;
                }
                if agent_doc_element::element::structural_corruption_reason(&content).is_some() {
                    return false;
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ack_content_snapshot_reconciled_forward file={} editor_id={} reason=operator_buffer_ahead rounds={} stale_len={} stale_hash={} newer_len={} newer_hash={}",
                        file.display(),
                        editor_id,
                        round + 1,
                        decision.snapshot_content.len(),
                        agent_doc_hash::content_hash(&decision.snapshot_content),
                        content.len(),
                        agent_doc_hash::content_hash(&content),
                    ),
                );
                decision.snapshot_content = content;
                return true;
            }
            OperatorReconcileStep::Continue => {
                prev = Some(curr);
            }
            OperatorReconcileStep::FailClosed => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "ack_content_snapshot_reconcile_fail_closed file={} editor_id={} reason=settled_buffer_dropped_response rounds={}",
                        file.display(),
                        editor_id,
                        round + 1,
                    ),
                );
                return false;
            }
        }
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ack_content_snapshot_reconcile_timeout file={} editor_id={} reason=operator_still_editing rounds={}",
            file.display(),
            editor_id,
            ACK_RECONCILE_MAX_ROUNDS,
        ),
    );
    false
}

pub fn mark_ack_content_live_buffer_synced(
    file: &Path,
    patch_id: &str,
    editor_id: Option<&str>,
    content: &str,
) {
    let Some(editor_id) = editor_id.map(str::trim).filter(|id| !id.is_empty()) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ack_content_live_buffer_sync_skipped file={} patch_id={} reason=no_editor_id",
                file.display(),
                patch_id
            ),
        );
        return;
    };
    let path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    match agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
        &path,
        content,
        editor_id,
        "ipc",
        "unknown",
        &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
    ) {
        Ok(()) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ack_content_live_buffer_synced file={} patch_id={} editor_id={} len={} hash={}",
                file.display(),
                patch_id,
                editor_id,
                content.len(),
                agent_doc_hash::content_hash(content)
            ),
        ),
        Err(err) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ack_content_live_buffer_sync_failed file={} patch_id={} editor_id={} error={}",
                file.display(),
                patch_id,
                editor_id,
                err
            ),
        ),
    }
}

pub fn write_ack_content_through_to_disk(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    patch_id: &str,
    content: &str,
    proof: AckContentDiskWriteProof,
) -> Result<bool> {
    let decision = decide_whole_buffer_delivery(WholeBufferAuthorityFacts {
        delivery: WholeBufferDelivery::AckContentDiskWriteThrough,
        authority: proof.authority,
        source_buffer_matches: proof.source_buffer_matches,
        scope_rejection: None,
        enabled: true,
    });
    if decision.action != WholeBufferDeliveryAction::Apply {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ack_content_disk_write_through_blocked file={} patch_id={} authority={} source_buffer_matches={} action={} reason={} len={} hash={}",
                file.display(),
                patch_id,
                proof.authority.as_str(),
                proof.source_buffer_matches,
                decision.action.as_str(),
                decision.reason,
                content.len(),
                agent_doc_hash::content_hash(content)
            ),
        );
        let stale_operator_source = proof.authority == WholeBufferAuthority::OperatorTextAuthority
            && decision.reason == "stale_source_buffer";
        return Ok(!stale_operator_source);
    }

    let before = std::fs::read_to_string(file).ok();
    if before.as_deref() == Some(content) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ack_content_disk_write_through_skipped file={} patch_id={} authority={} reason=already_current len={} hash={}",
                file.display(),
                patch_id,
                proof.authority.as_str(),
                content.len(),
                agent_doc_hash::content_hash(content)
            ),
        );
        return Ok(true);
    }

    effects.atomic_write(file, content).with_context(|| {
        format!(
            "failed to write proven ack-content through to disk for {}",
            file.display()
        )
    })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ack_content_disk_write_through file={} patch_id={} authority={} before_len={} before_hash={} ack_len={} ack_hash={}",
            file.display(),
            patch_id,
            proof.authority.as_str(),
            before.as_deref().map(str::len).unwrap_or(0),
            before
                .as_deref()
                .map(agent_doc_hash::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            content.len(),
            agent_doc_hash::content_hash(content)
        ),
    );
    Ok(true)
}

pub fn mark_ack_content_live_buffer_synced_after_write(
    file: &Path,
    patch_id: &str,
    editor_id: Option<&str>,
    content: &str,
) {
    let proof = ack_content_disk_write_proof(file, editor_id, content);
    if proof.authority == WholeBufferAuthority::OperatorTextAuthority && proof.source_buffer_matches
    {
        mark_ack_content_live_buffer_synced(file, patch_id, editor_id, content);
        return;
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ack_content_live_buffer_sync_skipped file={} patch_id={} reason=post_write_source_unproven authority={} source_buffer_matches={}",
            file.display(),
            patch_id,
            proof.authority.as_str(),
            proof.source_buffer_matches
        ),
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileIpcDeliveryOptions {
    pub guard_committed_cycle: bool,
}

impl FileIpcDeliveryOptions {
    pub fn guard_committed_cycle() -> Self {
        Self {
            guard_committed_cycle: true,
        }
    }
}

/// Write a file-IPC patch and prove the plugin consumed it.
///
/// This owns the durable delivery loop: atomic patch write, committed-cycle
/// fence, negative-ack sidecar handling, and no-ack timeout proof logging. The
/// caller remains responsible for post-consumption snapshot/response validation.
pub fn write_file_ipc_and_poll_delivery(
    effects: &dyn EditorConvergenceEffects,
    patch_file: &Path,
    payload: &serde_json::Value,
    doc_file: &Path,
    patch_count: usize,
    options: FileIpcDeliveryOptions,
) -> Result<bool> {
    let patch_id_for_diagnostics = payload.get("patch_id").and_then(|value| value.as_str());

    effects.atomic_write(patch_file, &serde_json::to_string_pretty(payload)?)?;

    eprintln!(
        "[write] IPC patch written to {} ({} components)",
        patch_file.display(),
        patch_count
    );

    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if options.guard_committed_cycle
            && let Some(ref cycle_id) = effects.cycle_already_committed(doc_file)
        {
            eprintln!(
                "[write] IPC poll skipped: cycle {} already committed for {}",
                cycle_id,
                doc_file.display()
            );
            effects.log_file_ipc_already_committed(doc_file, cycle_id);
            agent_doc_ops_log_io::log_op(
                doc_file,
                &format!(
                    "file_ipc_poll_skip file={} cycle_id={} reason=already_committed",
                    doc_file.display(),
                    cycle_id
                ),
            );
            effects.cleanup_fallback_patch_files(doc_file);
            return Ok(false);
        }

        let nack_file = patch_file.with_extension("nack");
        if nack_file.exists() {
            let detail = std::fs::read_to_string(&nack_file).unwrap_or_default();
            let _ = std::fs::remove_file(&nack_file);
            let _ = std::fs::remove_file(patch_file);
            eprintln!(
                "[write] IPC negative-ack: plugin rejected patch {} — refusing direct document write",
                patch_file.display()
            );
            effects.log_file_ipc_proof_failure(
                doc_file,
                patch_id_for_diagnostics,
                "nack",
                "retry_without_disk_write",
                &format!(
                    "nack_detail={} patch_file={}",
                    detail.trim(),
                    patch_file.display()
                ),
            );
            return Ok(false);
        }

        if !patch_file.exists() {
            return Ok(true);
        }

        std::thread::sleep(poll_interval);
    }

    eprintln!(
        "[write] IPC timeout ({}s) — leaving patch for editor retry; refusing direct document write",
        timeout.as_secs()
    );
    effects.log_file_ipc_proof_failure(
        doc_file,
        patch_id_for_diagnostics,
        "no_ack",
        "retry_without_disk_write",
        &format!(
            "timeout_secs={} patch_file={}",
            timeout.as_secs(),
            patch_file.display()
        ),
    );
    Ok(false)
}

/// `#exch-intermix`: auto-recover the `live_prompt_drift_after_preflight`
/// closeout wedge by rebasing the missing agent response onto the realtime
/// document. Returns the recovered file content on success (the caller must
/// refresh its `file_content` and snapshot), or `None` when no recovery applies.
pub fn try_auto_recover_live_prompt_drift(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    snapshot: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let Some(cycle) = agent_doc_cycle_state_io::load(file)? else {
        return Ok(None);
    };
    if !cycle.ipc_snapshot_adoption_blocked {
        return Ok(None);
    }
    schedule_stale_supervisor_pcp_recycle(file, "live_prompt_drift_after_preflight");

    let dropped_missing_from_snapshot = cycle
        .dropped_exchange_prompts
        .iter()
        .chain(cycle.dropped_queue_prompts.iter())
        .any(|prompt| !snapshot_contains_dropped_prompt(snapshot, prompt));
    if dropped_missing_from_snapshot {
        return Ok(None);
    }
    let Some(recovery_target) = live_prompt_drift_recovery_target(
        snapshot,
        file_content,
        normalize_visible_recovery_compare,
    ) else {
        return Ok(None);
    };

    let ipc_project_root = file
        .canonicalize()
        .ok()
        .map(|c| agent_doc_project_root_io::resolve_ipc_project_root(&c));
    let ipc_listener_active = ipc_project_root
        .as_deref()
        .map(agent_doc_ipc_io::is_listener_active)
        .unwrap_or(false);

    if let Some(project_root) = ipc_project_root.as_deref()
        && ipc_listener_active
    {
        match try_editor_converge_live_prompt_drift(
            file,
            project_root,
            &recovery_target,
            file_content,
        ) {
            Ok(Some(recovered)) => {
                log_live_prompt_drift_auto_recovered(
                    file,
                    &recovery_target,
                    file_content,
                    true,
                    "editor_ipc",
                );
                log_flow_event(
                    file,
                    agent_doc_flow::types::FlowEvent::new(
                        agent_doc_flow::types::FlowName::DocumentMutation,
                        agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
                        agent_doc_flow::types::FlowOutcome::Completed,
                    )
                    .with_reason("live_prompt_drift_auto_recovered"),
                );
                eprintln!(
                    "[commit] auto-recovered live_prompt_drift wedge for {} via editor IPC convergence ({} bytes)",
                    file.display(),
                    recovery_target.len()
                );
                return Ok(Some(recovered));
            }
            Ok(None) => {}
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_error file={} error={}",
                        file.display(),
                        err
                    ),
                );
            }
        }
    }

    if ipc_listener_active {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "[jbstalecache] auto_recovery_disk_write_blocked file={} target_len={} reason=editor_ipc_unconfirmed",
                file.display(),
                recovery_target.len()
            ),
        );
        return Ok(None);
    }

    effects
        .atomic_write(file, &recovery_target)
        .with_context(|| {
            format!(
                "live_prompt_drift auto-recover write for {}",
                file.display()
            )
        })?;
    agent_doc_snapshot_io::save(file, &recovery_target, agent_doc_ops_log_io::log_op)?;
    let crdt_doc = agent_doc_merge::crdt::CrdtDoc::from_text(&recovery_target);
    agent_doc_merge_io::save_document_crdt(file, &crdt_doc.encode_state(), &recovery_target)?;
    log_live_prompt_drift_auto_recovered(
        file,
        &recovery_target,
        file_content,
        ipc_listener_active,
        "disk_fallback",
    );
    log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::DocumentMutation,
            agent_doc_flow::types::FlowStage::IpcSnapshotAdoption,
            agent_doc_flow::types::FlowOutcome::Completed,
        )
        .with_reason("live_prompt_drift_auto_recovered"),
    );
    eprintln!(
        "[commit] auto-recovered live_prompt_drift wedge for {} — merged the missing response into the realtime document ({} bytes) so operator-visible edits stay authoritative",
        file.display(),
        recovery_target.len()
    );
    Ok(Some(recovery_target))
}

pub fn log_live_prompt_drift_auto_recovered(
    file: &Path,
    target: &str,
    file_content: &str,
    ipc_listener_active: bool,
    transport: &str,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "live_prompt_drift_auto_recovered file={} target_len={} file_len={} target_hash={} ipc_listener_active={} transport={}",
            file.display(),
            target.len(),
            file_content.len(),
            agent_doc_hash::content_hash(target),
            ipc_listener_active,
            transport
        ),
    );
}

pub fn try_editor_converge_live_prompt_drift(
    file: &Path,
    project_root: &Path,
    target: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let patches = live_prompt_drift_response_patches(file_content, target)?;
    let frontmatter = None;
    if patches.is_empty() && frontmatter.is_none() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "[jbstalecache] editor_convergence_skipped file={} skip=no_component_or_frontmatter_delta",
                file.display()
            ),
        );
        return Ok(None);
    }

    let canonical = file.canonicalize()?;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical.to_string_lossy(),
        "patches": patches,
        "node_patches": [],
        "unmatched": "",
        "baseline": file_content,
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    if let Ok(Some(ref cycle)) = agent_doc_cycle_state_io::load(file) {
        payload["cycle_id"] = serde_json::Value::String(cycle.cycle_id.clone());
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "[jbstalecache] editor_convergence_attempt file={} patch_id={} patches={} frontmatter={} target_hash={}",
            file.display(),
            payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-"),
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
            agent_doc_hash::content_hash(target)
        ),
    );

    match agent_doc_ipc_io::send_message(project_root, &payload) {
        Ok(Some(_ack)) => {
            let patch_id = payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("-");
            let sidecar = poll_ack_content_sidecar(
                project_root,
                patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = sidecar else {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_no_ack_content file={} patch_id={} action=block_external_disk_write",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(None);
            };
            if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                &recovered,
            ) == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                target,
            ) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                Ok(Some(recovered))
            } else if convergence_recovered_editor_wins_outside_response(&recovered, target) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_succeeded file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc resolution=editor_wins_outside_response #qpcwcmerge",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                Ok(Some(recovered))
            } else {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "[jbstalecache] editor_convergence_ack_mismatch file={} patch_id={} recovered_len={} target_len={} action=block_external_disk_write",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                Ok(None)
            }
        }
        Ok(None) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_no_ack file={} action=block_external_disk_write",
                    file.display()
                ),
            );
            Ok(None)
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "[jbstalecache] editor_convergence_send_failed file={} error={} action=block_external_disk_write",
                    file.display(),
                    err
                ),
            );
            Ok(None)
        }
    }
}

pub fn live_prompt_drift_response_patches(
    file_content: &str,
    snapshot: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut patches =
        agent_doc_document::component_patches::component_replace_patches(file_content, snapshot)?;
    patches.retain(|patch| {
        patch.get("component").and_then(|value| value.as_str()) == Some(AGENT_RESPONSE_COMPONENT)
    });
    Ok(patches)
}

fn refuse_unproven_editor_delivery(
    file: &Path,
    source: &str,
    reason: &str,
    patch_id: Option<&str>,
) -> Result<bool> {
    let sidecar_live = live_editor_sidecar_present(file);
    let owner_holds =
        !agent_doc_plugin_owner::disk_write_permitted_for_file(&file.to_string_lossy());
    let editor_endpoint =
        if should_refuse_disk_fallback(sidecar_live, owner_holds, editor_ipc_listener_active(file))
        {
            "live"
        } else {
            "absent"
        };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=blocked reason={reason} editor_endpoint={} action=editor_convergence_required",
            file.display(),
            editor_endpoint
        ),
    );
    let detail = format!("editor_endpoint={editor_endpoint}");
    if let Err(err) = agent_doc_cycle_state_io::record_editor_convergence_required(
        file,
        source,
        reason,
        patch_id,
        Some(&detail),
    ) {
        eprintln!(
            "[write] WARNING: failed to record editor-convergence blocked closeout for {}: {err}",
            file.display()
        );
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} while editor convergence is unproven (reason={reason}, editor_endpoint={editor_endpoint})",
        file.display()
    );
}

fn live_editor_sidecar_present(file: &Path) -> bool {
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    agent_doc_debounce::live_buffer_snapshots(&indicator_path)
        .iter()
        .any(agent_doc_debounce::live_buffer_snapshot_editor_is_live)
}

fn editor_ipc_listener_active(file: &Path) -> bool {
    file.canonicalize()
        .ok()
        .map(|c| agent_doc_project_root_io::resolve_ipc_project_root(&c))
        .map(|root| agent_doc_ipc_io::is_listener_active(&root))
        .unwrap_or(false)
}

fn try_detached_disk_write(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
    reason: &str,
) -> Result<bool> {
    let sidecar_live = live_editor_sidecar_present(file);
    let owner_holds =
        !agent_doc_plugin_owner::disk_write_permitted_for_file(&file.to_string_lossy());
    if should_refuse_disk_fallback(sidecar_live, owner_holds, editor_ipc_listener_active(file)) {
        return Ok(false);
    }

    effects
        .atomic_write_if_current(file, target, current, source)
        .with_context(|| {
            format!(
                "{source}: failed detached disk write for {}",
                file.display()
            )
        })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_writeback file={} transport=disk_detached reason={} len={} hash={}",
            file.display(),
            reason,
            target.len(),
            agent_doc_hash::content_hash(target)
        ),
    );
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckMismatchRefreshOutcome {
    NoRecovery,
    RevertedToCurrent,
    ReplayedTarget,
}

fn refresh_editor_after_ack_mismatch(
    file: &Path,
    project_root: &Path,
    canonical: &Path,
    target: &str,
    recovered: &str,
    current_content: &str,
    source: &str,
) -> AckMismatchRefreshOutcome {
    let stale_hash = agent_doc_hash::content_hash(recovered);
    let Some(recovery) = classify_ack_mismatch_recovery(
        target,
        recovered,
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers,
    ) else {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason=untrusted_ack_content_contains_user_drift action=leave_editor_owned_ack_content stale_len={} stale_hash={}",
                file.display(),
                recovered.len(),
                &stale_hash[..stale_hash.len().min(12)]
            ),
        );
        return AckMismatchRefreshOutcome::NoRecovery;
    };
    let (refresh_content, action, success_outcome) = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => (
            current_content,
            "revert_untrusted_ack_content",
            AckMismatchRefreshOutcome::RevertedToCurrent,
        ),
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => (
            target,
            "replay_missing_agent_response",
            AckMismatchRefreshOutcome::ReplayedTarget,
        ),
    };
    let target_hash = agent_doc_hash::content_hash(refresh_content);
    let failure_action = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => {
            "left_untrusted_ack_content_editor_owned"
        }
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "left_missing_agent_response_editor_owned"
        }
    };
    let failure_reason = match recovery {
        AckMismatchRecovery::RevertUntrustedAckToCurrent => "safe_stale_prompt_refresh_failed",
        AckMismatchRecovery::ReplayMissingAgentResponseToTarget => {
            "safe_missing_agent_response_refresh_failed"
        }
    };
    match agent_doc_ipc_io::send_refresh_content(
        project_root,
        &canonical.to_string_lossy(),
        refresh_content,
        &stale_hash,
        recovered.len(),
    ) {
        Ok(true) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=editor_ipc action={} stale_len={} stale_hash={} target_len={} target_hash={}",
                    file.display(),
                    action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)],
                    refresh_content.len(),
                    &target_hash[..target_hash.len().min(12)]
                ),
            );
            success_outcome
        }
        Ok(false) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} no_ack=true action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_ack_mismatch_editor_refresh file={} transport=blocked reason={} send_failed=true error={} action={} stale_len={} stale_hash={}",
                    file.display(),
                    failure_reason,
                    err,
                    failure_action,
                    recovered.len(),
                    &stale_hash[..stale_hash.len().min(12)]
                ),
            );
            AckMismatchRefreshOutcome::NoRecovery
        }
    }
}

pub fn live_buffer_delivery_missing_operator_text_authority_after_refresh(
    file: &Path,
    content: &str,
    source: &str,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    let canonical_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let indicator_path = canonical_file.to_string_lossy().to_string();
    let missing = agent_doc_debounce::live_buffer_delivery_missing_operator_text_authority(
        &indicator_path,
        content,
    )?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical_file);
    if !agent_doc_ipc_io::is_listener_active(&project_root) {
        return match agent_doc_ipc_io::send_publish_live_buffer_file_signal(
            &project_root,
            &indicator_path,
        ) {
            Ok(true) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=file_signal action=publish_live_buffer",
                        file.display()
                    ),
                );
                wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
            }
            Ok(false) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_unavailable action=editor_reload_required",
                        file.display()
                    ),
                );
                Some(missing)
            }
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_editor_authority_refresh file={} transport=blocked outcome=publish_live_buffer_file_signal_failed error={} action=editor_reload_required",
                        file.display(),
                        err
                    ),
                );
                Some(missing)
            }
        };
    }

    match agent_doc_ipc_io::send_publish_live_buffer(&project_root, &indicator_path) {
        Ok(true) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=editor_ipc action=publish_live_buffer",
                    file.display()
                ),
            );
            wait_for_operator_text_authority_refresh(&indicator_path, content, missing)
        }
        Ok(false) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed action=editor_reload_required",
                    file.display()
                ),
            );
            Some(missing)
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_editor_authority_refresh file={} transport=blocked reason=publish_live_buffer_failed error={} action=editor_reload_required",
                    file.display(),
                    err
                ),
            );
            Some(missing)
        }
    }
}

fn wait_for_operator_text_authority_refresh(
    indicator_path: &str,
    content: &str,
    mut latest_missing: agent_doc_debounce::LiveBufferSnapshot,
) -> Option<agent_doc_debounce::LiveBufferSnapshot> {
    for _ in 0..20 {
        match agent_doc_debounce::live_buffer_delivery_missing_operator_text_authority(
            indicator_path,
            content,
        ) {
            Some(still_missing) => {
                latest_missing = still_missing;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            None => return None,
        }
    }
    Some(latest_missing)
}

pub fn try_editor_converge(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
) -> Result<bool> {
    let canonical_file = file
        .canonicalize()
        .with_context(|| format!("{source}: failed to resolve {}", file.display()))?;
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical_file);
    cleanup_legacy_ipc_degraded(&project_root);
    if current_content == target {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=already_current",
                file.display()
            ),
        );
        return Ok(true);
    }
    if let Some(snapshot) = live_buffer_delivery_missing_operator_text_authority_after_refresh(
        &canonical_file,
        current_content,
        source,
    ) {
        let editor_id = snapshot.editor_id.as_deref().unwrap_or("unknown");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=editor_capability_missing capability={} editor_id={} live_len={} live_hash={} action=editor_reload_required",
                file.display(),
                agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
                editor_id,
                snapshot.len,
                snapshot.hash
            ),
        );
        anyhow::bail!(
            "{source}: refused editor convergence for {} because live editor buffer {} lacks required capability {}",
            file.display(),
            editor_id,
            agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY
        );
    }
    match ipc_direct_disk_degraded(&project_root, file) {
        Ok(true) => {
            log_ipc_dewedge_prefer_file_ipc(file, source);
            let canonical = file.canonicalize()?;
            let patch_id = uuid::Uuid::new_v4().to_string();
            let Some(mut payload) =
                editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
            else {
                if try_detached_disk_write(
                    effects,
                    file,
                    current_content,
                    target,
                    source,
                    "listener_degraded_no_component_delta",
                )? {
                    return Ok(true);
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} transport=blocked degraded_cause=no_component_delta action=refuse_external_disk_write",
                        file.display()
                    ),
                );
                anyhow::bail!(
                    "{source}: refused direct disk write for {} while editor IPC listener is degraded (cause=no_component_delta)",
                    file.display()
                );
            };
            target_payload_to_live_editor(file, &mut payload, "file_ipc_convergence");
            if try_editor_converge_file_ipc(
                effects,
                FileIpcConvergenceRequest {
                    file,
                    project_root: &project_root,
                    payload: &payload,
                    patch_id: &patch_id,
                    source,
                    reason: "listener_degraded",
                },
            )? {
                return Ok(true);
            }
            if try_detached_disk_write(
                effects,
                file,
                current_content,
                target,
                source,
                "listener_degraded_editor_detached",
            )? {
                return Ok(true);
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_writeback file={} transport=blocked degraded_cause=listener_degraded action=refuse_external_disk_write",
                    file.display()
                ),
            );
            anyhow::bail!(
                "{source}: refused direct disk write for {} while editor IPC listener is degraded",
                file.display()
            );
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!(
                "[write] WARNING: {source} converge degradation check failed (non-fatal): {e}"
            );
        }
    }
    if !agent_doc_ipc_io::is_listener_active(&project_root) {
        if try_detached_disk_write(
            effects,
            file,
            current_content,
            target,
            source,
            "no_listener",
        )? {
            return Ok(true);
        }
        return refuse_unproven_editor_delivery(file, source, "no_listener", None);
    }

    let canonical = canonical_file;
    let patch_id = uuid::Uuid::new_v4().to_string();
    let Some(mut payload) =
        editor_convergence_payload(&canonical, target, current_content, source, &patch_id)?
    else {
        if try_detached_disk_write(
            effects,
            file,
            current_content,
            target,
            source,
            "no_component_delta",
        )? {
            return Ok(true);
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked reason=no_component_delta action=refuse_external_disk_write",
                file.display()
            ),
        );
        anyhow::bail!(
            "{source}: refused direct disk write for {} while editor IPC listener is active (reason=no_component_delta)",
            file.display()
        );
    };
    target_payload_to_live_editor(file, &mut payload, "editor_convergence");

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_editor_convergence_attempt file={} patch_id={} patches={} node_patches={} frontmatter={}",
            file.display(),
            patch_id,
            payload
                .get("patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload
                .get("node_patches")
                .and_then(|value| value.as_array())
                .map(Vec::len)
                .unwrap_or(0),
            payload.get("frontmatter").is_some(),
        ),
    );

    match agent_doc_ipc_io::send_message(&project_root, &payload) {
        Ok(Some(_ack)) => {
            let sidecar = poll_ack_content_sidecar(
                &project_root,
                &patch_id,
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(25),
            )?;
            let Some(recovered) = sidecar else {
                return refuse_unproven_editor_delivery(
                    file,
                    source,
                    "no_ack_content",
                    Some(&patch_id),
                );
            };
            if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                &recovered,
            ) == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                target,
            ) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} recovered_len={} transport=editor_ipc",
                        file.display(),
                        patch_id,
                        recovered.len()
                    ),
                );
                if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                    eprintln!(
                        "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                    );
                }
                Ok(true)
            } else if convergence_recovered_editor_wins_for_payload(&recovered, target, &payload) {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc resolution=editor_wins_outside_touched_components",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                    eprintln!(
                        "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                    );
                }
                Ok(true)
            } else {
                let recovery = refresh_editor_after_ack_mismatch(
                    file,
                    &project_root,
                    &canonical,
                    target,
                    &recovered,
                    current_content,
                    source,
                );
                if recovery == AckMismatchRefreshOutcome::ReplayedTarget {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "{source}_writeback file={} patch_id={} recovered_len={} target_len={} transport=editor_ipc recovery=ack_mismatch_replayed_target",
                            file.display(),
                            patch_id,
                            recovered.len(),
                            target.len()
                        ),
                    );
                    if let Err(e) = clear_ipc_socket_ack_timeouts(&project_root, file, source) {
                        eprintln!(
                            "[write] WARNING: {source} converge ack-timeout clear failed (non-fatal): {e}"
                        );
                    }
                    return Ok(true);
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{source}_writeback file={} patch_id={} transport=blocked reason=ack_mismatch recovered_len={} target_len={} action=editor_convergence_required",
                        file.display(),
                        patch_id,
                        recovered.len(),
                        target.len()
                    ),
                );
                refuse_unproven_editor_delivery(file, source, "ack_mismatch", Some(&patch_id))
            }
        }
        Ok(None) => {
            if try_detached_disk_write(effects, file, current_content, target, source, "no_ack")? {
                return Ok(true);
            }
            refuse_unproven_editor_delivery(file, source, "no_ack", Some(&patch_id))
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_writeback file={} reason=send_failed error={} note=converge_send_error",
                    file.display(),
                    err
                ),
            );
            if is_socket_status_error(err.to_string())
                && try_editor_converge_file_ipc(
                    effects,
                    FileIpcConvergenceRequest {
                        file,
                        project_root: &project_root,
                        payload: &payload,
                        patch_id: &patch_id,
                        source,
                        reason: "socket_status_error",
                    },
                )?
            {
                return Ok(true);
            }
            if is_socket_ack_timeout_error(err.to_string()) {
                match record_ipc_socket_ack_timeout(&project_root, file, Some(&patch_id), source) {
                    Ok(true) => {
                        eprintln!(
                            "[write] IPC listener degraded for {} after repeated {source} ack timeouts",
                            file.display()
                        );
                        log_write_wedge_requests_supervisor_recycle(file, source);
                    }
                    Ok(false) => {}
                    Err(e) => eprintln!(
                        "[write] WARNING: {source} converge ack-timeout record failed (non-fatal): {e}"
                    ),
                }
            }
            if try_detached_disk_write(
                effects,
                file,
                current_content,
                target,
                source,
                "send_failed",
            )? {
                return Ok(true);
            }
            refuse_unproven_editor_delivery(file, source, "send_failed", Some(&patch_id))
        }
    }
}

struct FileIpcConvergenceRequest<'a> {
    file: &'a Path,
    project_root: &'a Path,
    payload: &'a serde_json::Value,
    patch_id: &'a str,
    source: &'a str,
    reason: &'a str,
}

fn try_editor_converge_file_ipc(
    effects: &dyn EditorConvergenceEffects,
    request: FileIpcConvergenceRequest<'_>,
) -> Result<bool> {
    let FileIpcConvergenceRequest {
        file,
        project_root,
        payload,
        patch_id,
        source,
        reason,
    } = request;
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=blocked degraded_cause={reason}_no_file_ipc action=refuse_external_disk_write",
                file.display()
            ),
        );
        return Ok(false);
    }
    let patch_file = patches_dir.join(format!("{patch_id}.json"));
    let patch_count = payload
        .get("patches")
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0)
        + payload
            .get("node_patches")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .unwrap_or(0)
        + usize::from(payload.get("frontmatter").is_some());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_file_ipc_convergence_attempt file={} patch_id={} degraded_cause={} patches={}",
            file.display(),
            patch_id,
            reason,
            patch_count
        ),
    );
    if write_file_ipc_and_poll_delivery(
        effects,
        &patch_file,
        payload,
        file,
        patch_count,
        FileIpcDeliveryOptions::guard_committed_cycle(),
    )? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} patch_id={} transport=file_ipc degraded_cause={}",
                file.display(),
                patch_id,
                reason
            ),
        );
        return Ok(true);
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{source}_writeback file={} patch_id={} transport=blocked degraded_cause={reason}_file_ipc_unproven action=refuse_external_disk_write",
            file.display(),
            patch_id
        ),
    );
    Ok(false)
}

pub fn editor_convergence_payload(
    canonical_file: &Path,
    target: &str,
    current_content: &str,
    source: &str,
    patch_id: &str,
) -> Result<Option<serde_json::Value>> {
    let mut patches =
        agent_doc_document::component_patches::component_replace_patches(current_content, target)?;
    let frontmatter = live_prompt_drift_convergence_frontmatter(current_content, target);
    let node_patches = queue_consume_node_patches(current_content, target, source);

    if !node_patches.is_empty() {
        let node_patched_components = node_patches
            .iter()
            .filter_map(|patch| patch.get("component").and_then(|value| value.as_str()))
            .map(str::to_string)
            .collect::<HashSet<_>>();
        patches.retain(|patch| {
            patch
                .get("component")
                .and_then(|value| value.as_str())
                .is_none_or(|component| !node_patched_components.contains(component))
        });
    }

    if patches.is_empty() && node_patches.is_empty() && frontmatter.is_none() {
        return Ok(None);
    }

    let normalized_baseline =
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
            current_content,
        );
    let mut payload = serde_json::json!({
        "type": "patch",
        "file": canonical_file.to_string_lossy(),
        "patches": patches,
        "node_patches": node_patches,
        "unmatched": "",
        "baseline": current_content,
        "baseline_hash": agent_doc_hash::content_hash(current_content),
        "baseline_normalized_hash": agent_doc_hash::content_hash(&normalized_baseline),
        "reposition_boundary": false,
        "patch_id": patch_id,
    });
    if let Some(frontmatter) = frontmatter {
        payload["frontmatter"] = serde_json::Value::String(frontmatter);
    }
    Ok(Some(payload))
}

fn queue_consume_node_patches(
    current_content: &str,
    target: &str,
    source: &str,
) -> Vec<serde_json::Value> {
    if source != "queue_consume" {
        return Vec::new();
    }
    agent_doc_ipc_protocol::build_ipc_node_patches_json(Some(current_content), Some(target))
        .into_iter()
        .filter(|patch| patch.get("component").and_then(|value| value.as_str()) == Some("queue"))
        .collect()
}

pub fn converge_document_or_disk(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    target: &str,
    current: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(effects, file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

pub fn converge_or_disk_write(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
) -> Result<()> {
    if try_editor_converge(effects, file, target, current, source)? {
        return Ok(());
    }
    anyhow::bail!(
        "{source}: refused direct disk write for {} because editor convergence did not complete",
        file.display()
    )
}

pub fn live_prompt_drift_convergence_frontmatter(
    file_content: &str,
    snapshot: &str,
) -> Option<String> {
    let file_frontmatter = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(file_content);
    let snapshot_frontmatter = agent_doc_frontmatter::frontmatter::raw_frontmatter_yaml(snapshot)?;
    if file_frontmatter == Some(snapshot_frontmatter) {
        None
    } else {
        Some(snapshot_frontmatter.to_string())
    }
}

/// Read the ack-content sidecar file written by the plugin after apply.
/// Keyed by `patch_id` (same UUID the binary embedded in the patch payload).
/// Deletes the sidecar on success. Returns None if no sidecar present (old plugin).
pub fn read_ack_content_sidecar(project_root: &Path, patch_id: &str) -> Result<Option<String>> {
    let sidecar = project_root
        .join(".agent-doc/ack-content")
        .join(format!("{patch_id}.md"));
    if !sidecar.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&sidecar)
        .with_context(|| format!("failed to read ack-content sidecar {sidecar:?}"))?;
    let _ = std::fs::remove_file(&sidecar);
    Ok(Some(content))
}

pub fn cleanup_legacy_ipc_degraded(project_root: &Path) {
    let marker = project_root.join(".agent-doc/ipc-degraded");
    if marker.is_file()
        && let Err(e) = std::fs::remove_file(&marker)
    {
        eprintln!(
            "[write] WARNING: failed to remove legacy IPC degraded marker {}: {}",
            marker.display(),
            e
        );
    }
}

pub const IPC_DEWEDGE_TIMEOUT_THRESHOLD: u64 = 2;

pub fn ipc_dewedge_marker_path(project_root: &Path, file: &Path) -> Result<PathBuf> {
    let hash = agent_doc_fs::document_state_hash(file)?;
    Ok(project_root
        .join(".agent-doc/ipc-degraded")
        .join(format!("{hash}.json")))
}

pub fn ipc_dewedge_marker_for_current_session(
    project_root: &Path,
    file: &Path,
) -> Result<Option<serde_json::Value>> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if !marker.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&marker)
        .with_context(|| format!("failed to read IPC degraded marker {}", marker.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse IPC degraded marker {}", marker.display()))?;
    let marker_session = value
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let session_id =
        agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_else(|| "-".to_string());
    if marker_session != session_id {
        return Ok(None);
    }
    Ok(Some(value))
}

pub fn ipc_direct_disk_degraded(project_root: &Path, file: &Path) -> Result<bool> {
    let degraded = ipc_dewedge_marker_for_current_session(project_root, file)?
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if !degraded {
        return Ok(false);
    }
    // `#ipc-degrade-self-heal`: the degrade latch is a circuit breaker, not a
    // permanent session verdict. It may clear only when the plugin proves it can
    // accept AND ack a lightweight message.
    match agent_doc_ipc_io::probe_listener_ack(project_root, ipc_dewedge_probe_timeout()) {
        Ok(true) => {
            remove_ipc_dewedge_marker(project_root, file, "listener_ack_recovered")?;
            return Ok(false);
        }
        Ok(false) => {}
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "ipc_socket_degraded_self_heal_probe_failed file={} reason={}",
                    file.display(),
                    err.to_string().replace(char::is_whitespace, "_")
                ),
            );
        }
    }
    Ok(true)
}

fn ipc_dewedge_probe_timeout() -> std::time::Duration {
    if cfg!(test) {
        std::time::Duration::from_millis(250)
    } else {
        std::time::Duration::from_millis(750)
    }
}

pub fn log_ipc_dewedge_direct_disk_skip(file: &Path, transport: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_listener_degraded_direct_disk file={} transport={} reason=repeated_ack_timeout",
            file.display(),
            transport
        ),
    );
}

/// `#ipc-degraded-prefers-file-ipc`: a latched-degraded socket means only the
/// plugin's *socket* listener is wedged. The file-IPC patch queue uses a
/// separate plugin file watcher that is very likely still alive, so a degraded
/// write routes through it (the plugin applies via the Document API) instead of
/// a raw disk write that manufactures an IDEA "File Cache Conflict". If file IPC
/// also fails to prove delivery, the write fails closed for retry.
pub fn log_ipc_dewedge_prefer_file_ipc(file: &Path, transport: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_degraded_prefer_file_ipc file={} transport={} reason=repeated_ack_timeout disk_write=disabled",
            file.display(),
            transport
        ),
    );
}

pub fn record_ipc_socket_ack_timeout(
    project_root: &Path,
    file: &Path,
    patch_id: Option<&str>,
    transport: &str,
) -> Result<bool> {
    cleanup_legacy_ipc_degraded(project_root);
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create IPC degraded marker directory {}",
                parent.display()
            )
        })?;
    }
    let prior = ipc_dewedge_marker_for_current_session(project_root, file)?;
    let prior_timeouts = prior
        .as_ref()
        .and_then(|value| value.get("consecutive_timeouts").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let consecutive_timeouts = prior_timeouts.saturating_add(1);
    let degraded = agent_doc_supervisor::lifecycle::write_wedged_from_ipc_failures(
        consecutive_timeouts,
        true,
        IPC_DEWEDGE_TIMEOUT_THRESHOLD,
    );
    let value = serde_json::json!({
        "session_id": agent_doc_frontmatter_io::session::read_session_id(file)
            .unwrap_or_else(|| "-".to_string()),
        "consecutive_timeouts": consecutive_timeouts,
        "degraded": degraded,
        "last_patch_id": patch_id.unwrap_or("-"),
        "last_transport": transport,
    });
    atomic_write(&marker, &serde_json::to_string_pretty(&value)?)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_socket_ack_timeout_recorded file={} transport={} patch_id={} consecutive_timeouts={} degraded={}",
            file.display(),
            transport,
            patch_id.unwrap_or("-"),
            consecutive_timeouts,
            degraded
        ),
    );
    Ok(degraded)
}

pub fn remove_ipc_dewedge_marker(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let marker = ipc_dewedge_marker_path(project_root, file)?;
    if marker.exists() {
        std::fs::remove_file(&marker).with_context(|| {
            format!("failed to remove IPC degraded marker {}", marker.display())
        })?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_socket_ack_timeouts_cleared file={} reason={}",
                file.display(),
                reason
            ),
        );
    }
    Ok(())
}

pub fn clear_ipc_socket_ack_timeouts(project_root: &Path, file: &Path, reason: &str) -> Result<()> {
    let Some(value) = ipc_dewedge_marker_for_current_session(project_root, file)? else {
        return Ok(());
    };
    // A routine successful write clears accrued timeout votes, but it must NOT
    // clear a *degraded* latch on its own. The degraded latch is cleared only by
    // a proven-live listener re-probe (`#ipc-degrade-self-heal`).
    if value
        .get("degraded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(());
    }
    remove_ipc_dewedge_marker(project_root, file, reason)
}

/// Poll for the ack-content sidecar with timeout.
pub fn poll_ack_content_sidecar(
    project_root: &Path,
    patch_id: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
) -> Result<Option<String>> {
    let start = std::time::Instant::now();
    loop {
        match read_ack_content_sidecar(project_root, patch_id)? {
            Some(content) => return Ok(Some(content)),
            None if start.elapsed() >= timeout => return Ok(None),
            None => std::thread::sleep(poll_interval),
        }
    }
}

/// `#supselfheal` Phase 2 — read the persisted editor-IPC wedge fact for `file`
/// so the route-owned supervisor idle watch can feed `write_wedged` into
/// `supervisor_recycle_action`. Best effort: a missing/unreadable marker is
/// "not wedged".
pub fn editor_ipc_write_wedged(project_root: &Path, file: &Path) -> bool {
    ipc_dewedge_marker_for_current_session(project_root, file)
        .ok()
        .flatten()
        .and_then(|value| value.get("degraded").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// `#supselfheal` Phase 2 — log that a wedged editor-IPC write is now requesting
/// a supervisor recycle through the policy owner.
pub fn log_write_wedge_requests_supervisor_recycle(file: &Path, source: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "write_wedged_supervisor_recycle_requested file={} source={} action=request_recycle_through_owner reason=repeated_ack_timeout_active_listener",
            file.display(),
            source
        ),
    );
}

/// `#turnsaferecycle` Goal 2 — pure: given stale-supervisor evidence at a proven
/// IPC drift, does the workflow kernel say to schedule an immediate forced PCP
/// recycle (`RecycleNow`) rather than only surface advisory guidance?
pub fn stale_ipc_drift_forces_pcp_recycle(stale: bool, auto_recycle: bool) -> bool {
    matches!(
        agent_doc_workflow::decide_stale_supervisor(agent_doc_workflow::StaleSupervisorEvidence {
            stale,
            auto_recycle,
            turn_boundary: true,
            queue_head_pending: true,
        })
        .decision,
        agent_doc_workflow::WorkflowDecision::Supervisor(
            agent_doc_workflow::SupervisorWorkflowDecision::RecycleNow
        )
    )
}

/// `#turnsaferecycle` Goal 2 — schedule a forced PCP recycle for a proven stale
/// supervisor IPC drift. Fail-open: missing root, fresh supervisor, opted-out
/// auto-recycle, or scheduling failure leaves existing retry/advisory behavior.
pub fn schedule_stale_supervisor_pcp_recycle(file: &Path, source: &str) -> bool {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return false;
    };
    if agent_doc_controller_io::project_controller::stale_supervisor_warning_for_doc(file).is_none()
    {
        return false;
    }
    let auto_recycle = agent_doc_supervisor_io::config::supervisor_auto_recycle_enabled(file);
    if !stale_ipc_drift_forces_pcp_recycle(true, auto_recycle) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "stale_supervisor_ipc_drift_surfaced file={} source={} action=advisory_only reason=auto_recycle_opted_out",
                file.display(),
                source
            ),
        );
        return false;
    }
    match agent_doc_controller_io::project_controller::recycle_controller_force(&project_root, true)
    {
        Ok(scheduled) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "stale_supervisor_ipc_drift_forced_recycle file={} source={} scheduled={} action=recycle_controller_force reason=stale_supervisor_ipc",
                    file.display(),
                    source,
                    scheduled
                ),
            );
            eprintln!(
                "[write] stale-supervisor IPC drift for {} ({source}); scheduling an immediate forced PCP recycle instead of thrashing the doomed write",
                file.display()
            );
            scheduled
        }
        Err(err) => {
            eprintln!(
                "[write] warning: failed to schedule forced PCP recycle on stale-supervisor IPC drift for {}: {err:#}",
                file.display()
            );
            false
        }
    }
}

/// `#turnsaferecycle` Goal 3 — the shared stale-supervisor write-entry
/// short-circuit for IPC write entry points.
pub fn stale_supervisor_write_short_circuit(
    file: &Path,
    source: &str,
) -> Option<agent_doc_flow::outcome::UserFacingOutcome> {
    let base = file
        .canonicalize()
        .ok()
        .map(|canonical| agent_doc_project_root_io::resolve_ipc_project_root(&canonical))?;
    if !agent_doc_turn_status_io::supervisor_stale(&base) {
        return None;
    }
    schedule_stale_supervisor_pcp_recycle(file, source);
    if let Err(err) = agent_doc_supervisor_io::recycle_request::request_recycle_for_doc(
        file,
        agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT,
    ) {
        eprintln!(
            "[write] warning: failed to mark supervisor recycle-request for {}: {err:#}",
            file.display()
        );
    }
    let binary = agent_doc_flow::outcome::supervisor_stale_self_recycled_outcome();
    let ui = agent_doc_flow::outcome::deferred_for_recycle_outcome();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "stale_supervisor_write_short_circuit file={} source={} {} {}",
            file.display(),
            source,
            binary.log_fields(),
            ui.log_fields()
        ),
    );
    eprintln!(
        "[write] stale supervisor hosting {} ({source}); deferring the IPC write for a recycle instead of thrashing the doomed buffer (deferred_for_recycle)",
        file.display()
    );
    Some(ui)
}

pub fn guard_no_stale_snapshot_reset_drift(
    file: &Path,
    snapshot_doc: Option<&str>,
    current_doc: &str,
    phase: &str,
) -> Result<bool> {
    let Some(snapshot_doc) = snapshot_doc else {
        return Ok(false);
    };
    if let Ok(Some(cleaned)) =
        agent_doc_template::deleted_conversation_tail_cleanup(snapshot_doc, current_doc)
        && cleaned == current_doc
    {
        return Ok(false);
    }
    let Some(drift) = stale_snapshot_reset_drift(snapshot_doc, current_doc) else {
        return Ok(false);
    };
    let snapshot_len = drift.snapshot_len;
    let current_len = drift.current_len;
    if active_capture_response_removed(file, snapshot_doc, current_doc) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "stale_snapshot_rebase_skipped_active_capture file={} phase={} old_snap_len={} new_snap_len={}",
                file.display(),
                phase,
                snapshot_len,
                current_len
            ),
        );
        return Ok(false);
    }
    if let Some(reason) = classify_stale_snapshot_visible_rebase(file, snapshot_doc, current_doc) {
        agent_doc_snapshot_io::save(file, current_doc, agent_doc_ops_log_io::log_op)?;
        let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(current_doc).encode_state();
        agent_doc_merge_io::save_document_crdt(file, &crdt, current_doc)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "stale_snapshot_visible_rebased file={} phase={} reason={} old_snap_len={} new_snap_len={}",
                file.display(),
                phase,
                reason,
                snapshot_len,
                current_len
            ),
        );
        return Ok(true);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "stale_snapshot_reset_drift_blocked file={} phase={} snap_len={} file_len={}",
            file.display(),
            phase,
            snapshot_len,
            current_len
        ),
    );
    anyhow::bail!(
        "refusing {phase} for {}: snapshot is {} bytes but the visible file is {} bytes, which looks like a manual cleanup with stale snapshot/CRDT state. Reset the sidecars from the current file before writing: `agent-doc reset --from-current {}`",
        file.display(),
        snapshot_len,
        current_len,
        file.display()
    );
}

fn classify_stale_snapshot_visible_rebase(
    file: &Path,
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<&'static str> {
    let scope = agent_doc_turn_scope_io::load(file);
    let recent_binary_compaction =
        agent_doc_session_accretion_io::recent_exchange_compaction_timestamp(file)
            .ok()
            .flatten()
            .is_some();
    if active_capture_response_removed(file, snapshot_doc, current_doc) {
        return None;
    }

    let (snapshot_frontmatter, snapshot_body) =
        agent_doc_frontmatter::frontmatter::parse(snapshot_doc).ok()?;
    let (current_frontmatter, current_body) =
        agent_doc_frontmatter::frontmatter::parse(current_doc).ok()?;
    if !agent_doc_frontmatter::frontmatter::frontmatter_agent_only_equivalent(
        &snapshot_frontmatter,
        &current_frontmatter,
    ) {
        return None;
    }

    let snap_components = agent_doc_element::element::parse(snapshot_body).ok()?;
    let current_components = agent_doc_element::element::parse(current_body).ok()?;
    if snap_components.is_empty() || snap_components.len() != current_components.len() {
        return None;
    }

    let mut saw_exchange_trim = false;
    let mut saw_independent_component = false;
    for (snap_comp, current_comp) in snap_components.iter().zip(current_components.iter()) {
        if snap_comp.name != current_comp.name {
            return None;
        }
        if !is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != current_comp.patch_mode()
        {
            return None;
        }

        let snap_content =
            agent_doc_document::commit_normalization::normalize_component_content_for_absorb(
                snap_comp.content(snapshot_body),
            );
        let current_content =
            agent_doc_document::commit_normalization::normalize_component_content_for_absorb(
                current_comp.content(current_body),
            );
        if snap_content == current_content {
            continue;
        }

        if snap_comp.name == "exchange" {
            if exchange_change_is_safe_historical_reduction(
                snap_comp.content(snapshot_body),
                current_comp.content(current_body),
            ) {
                saw_exchange_trim = true;
                continue;
            }
            return None;
        }

        match scope.as_ref() {
            Some(scope)
                if component_change_is_turn_independent(
                    snapshot_body,
                    current_body,
                    &snap_comp.name,
                    scope,
                ) =>
            {
                saw_independent_component = true;
                continue;
            }
            _ => return None,
        }
    }

    match (saw_exchange_trim, saw_independent_component) {
        (true, true) => Some("historical_exchange_trim_unrelated_drift"),
        (true, false) => {
            if scope.is_some() || recent_binary_compaction {
                Some("historical_exchange_trim")
            } else {
                None
            }
        }
        (false, true) => Some("unrelated_component_drift"),
        (false, false) => None,
    }
}

fn active_capture_response_removed(file: &Path, snapshot_doc: &str, current_doc: &str) -> bool {
    let Ok(Some(state)) = agent_doc_cycle_state_io::load(file) else {
        return false;
    };
    if !state.is_open() {
        return false;
    }
    let Ok(Some(capture)) = agent_doc_capture_io::load_active(file) else {
        return false;
    };
    !capture.response_body.trim().is_empty()
        && response_materialized_in_content(&capture.response_body, snapshot_doc)
        && !response_materialized_in_content(&capture.response_body, current_doc)
}

fn component_change_is_turn_independent(
    snap_body: &str,
    current_body: &str,
    component_name: &str,
    scope: &agent_doc_turn::turn_scope::TurnScope,
) -> bool {
    use agent_doc_turn::op_log::OpActor;
    use agent_doc_turn::turn_scope::{Address, classify_op};

    let events: Vec<_> = agent_doc_markdown_ast::events::diff_node_events(snap_body, current_body)
        .into_iter()
        .filter(|event| event.component == component_name)
        .collect();
    if events.is_empty() {
        return false;
    }

    events.iter().all(|event| {
        let address = Address::from_component_node_key(&event.component, &event.node_key);
        let node_index = event.after_index.or(event.before_index);
        !classify_op(
            OpActor::User,
            event.kind.as_str(),
            &address,
            node_index,
            scope,
        )
        .affects_turn()
    })
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ipc_ack_timeouts_degrade_current_session_to_file_ipc_retry() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test-session\n---\n\ncontent").unwrap();

        assert!(
            !record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap(),
            "first timeout should only record health state"
        );
        assert!(
            record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap(),
            "second consecutive timeout should mark the listener degraded"
        );
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "current session should now bypass IPC"
        );

        fs::write(&doc, "---\nsession: next-session\n---\n\ncontent").unwrap();
        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a new session id must not inherit the old session's degraded marker"
        );
    }

    #[test]
    fn degraded_latch_self_heals_when_listener_recovers() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: heal-session\n---\n\ncontent").unwrap();

        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p1"), "socket_ipc").unwrap();
        record_ipc_socket_ack_timeout(dir.path(), &doc, Some("p2"), "socket_ipc").unwrap();
        assert!(
            ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "two timeouts with no live listener should stay degraded"
        );

        let root_clone = dir.path().to_path_buf();
        let server = std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root_clone, |_msg| {
                Some(r#"{"type":"ack","id":"x"}"#.to_string())
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

        assert!(
            !ipc_direct_disk_degraded(dir.path(), &doc).unwrap(),
            "a recovered live listener must self-heal the degrade latch"
        );
        let marker = ipc_dewedge_marker_path(dir.path(), &doc).unwrap();
        assert!(
            !marker.exists(),
            "self-heal must remove the degraded marker"
        );

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(dir.path()));
        drop(server);
    }

    #[test]
    fn stale_supervisor_write_short_circuit_passes_through_when_fresh() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        assert!(stale_supervisor_write_short_circuit(&file, "unit_test").is_none());
    }

    #[test]
    fn stale_supervisor_write_short_circuit_defers_when_marker_present() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let canonical = file.canonicalize().unwrap();
        let base = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
        agent_doc_turn_status_io::set_supervisor_stale_marker(&base, true).unwrap();

        let outcome = stale_supervisor_write_short_circuit(&file, "unit_test")
            .expect("stale marker must short-circuit the write");
        assert_eq!(
            outcome.outcome,
            agent_doc_flow::outcome::UserFacingOutcomeKind::DeferredForRecycle
        );

        agent_doc_turn_status_io::set_supervisor_stale_marker(&base, false).unwrap();
    }

    #[test]
    fn stale_ipc_drift_forces_pcp_recycle_only_when_stale_and_auto_recycle_on() {
        assert!(
            stale_ipc_drift_forces_pcp_recycle(true, true),
            "stale + auto-recycle must force RecycleNow"
        );
        assert!(
            !stale_ipc_drift_forces_pcp_recycle(true, false),
            "auto-recycle opted out must stay advisory"
        );
        assert!(
            !stale_ipc_drift_forces_pcp_recycle(false, true),
            "a fresh supervisor is never a recycle candidate"
        );
    }

    #[test]
    fn editor_ipc_write_wedged_reads_latched_degraded_marker() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let file = project_root.join("plan.md");
        fs::write(&file, "# plan\n").unwrap();
        assert!(!editor_ipc_write_wedged(project_root, &file));
        for _ in 0..IPC_DEWEDGE_TIMEOUT_THRESHOLD {
            record_ipc_socket_ack_timeout(project_root, &file, Some("p1"), "finalize").unwrap();
        }
        assert!(
            editor_ipc_write_wedged(project_root, &file),
            "a latched degraded marker should read as a write wedge"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_blocks_large_snapshot_only_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let stale_exchange = "duplicated response\n".repeat(20);
        let snapshot = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{}<!-- /agent:exchange -->\n",
            stale_exchange
        );
        let current = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\nclean\n<!-- /agent:exchange -->\n";

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write");

        let message = result
            .expect_err("stale larger snapshot must fail closed")
            .to_string();
        assert!(
            message.contains("agent-doc reset --from-current"),
            "recovery guidance should name deterministic sidecar reset: {message}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_rebases_compact_summary_after_clear_via_binary_origin_marker() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n- Prior summary/context: compacted prior responses\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "preflight")
                .expect("binary-origin compaction marker should rebase the stale snapshot");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap(),
            Some(current.to_string())
        );
    }
}
