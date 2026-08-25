//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_document::write_normalization::strip_boundary_for_dedup;
use agent_doc_document_realtime::write_policy::{
    SnapshotPersistMode, committed_snapshot_union_excluding_carry_forward,
    snapshot_content_to_persist, snapshot_persist_mode, snapshot_persist_mode_with_current,
};
use agent_doc_element_exchange::extract_normalization_targets;
use agent_doc_frontmatter::frontmatter::content_uses_crdt_write;
use agent_doc_queue_io::queue_consume;
use agent_doc_queue_io::queue_consumption_proof::QueueConsumptionProofStage;
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_template::response_materialization::sanitize_template_patchback_response;
use agent_doc_template::todo_patch_guard::enforce_no_destructive_todo_patch;
use agent_doc_template_io as template_io;
use agent_doc_template_io::{
    enforce_imperative_response_contract_with_mutation_evidence, lift_pending_from_exchange_safe,
    normalize_template_structure_or_fail_preserving, normalize_user_prompts_in_exchange_safe,
    template_mode_overrides_for_current_doc,
};
use agent_doc_turn::op_log::OpsLogEvent;

// Deeper root cause A superseded the interim `#qftlossdelta` recovery-sidecar
// safety net (`preserve_dropped_operator_buffer_if_needed`): the committed
// snapshot is now the full union minus carry-forward prompts
// (`committed_snapshot_union_excluding_carry_forward`), so operator edits are
// retained in-place and never need after-the-fact recovery. The pure detector
// `content_ours_drops_operator_text` and `agent_doc_fs::preserve_dropped_operator_buffer`
// remain available for diagnostics.

fn resolve_current_document_content(file: &Path, source: &str) -> Result<String> {
    agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
}

fn resolve_disk_document_content(file: &Path, source: &str) -> Result<String> {
    agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
}

fn resolve_document_content_for_write_mode(
    file: &Path,
    force_disk: bool,
    current_source: &str,
    disk_source: &str,
) -> Result<String> {
    if force_disk {
        resolve_disk_document_content(file, disk_source)
    } else {
        resolve_current_document_content(file, current_source)
    }
}

fn atomic_write_for_write_mode(
    file: &Path,
    expected_current: &str,
    content: &str,
    force_disk: bool,
    source: &str,
) -> Result<()> {
    if force_disk {
        agent_doc_document_realtime_io::atomic_write_force_disk_through_authority(file, content)
    } else {
        agent_doc_document_realtime_io::atomic_write_rebased_through_authority(
            file,
            expected_current,
            content,
            source,
        )
    }
}

#[derive(Debug)]
struct RecoveryMergeContent {
    content: String,
    replayed_pending_editor_ops: bool,
}

#[derive(Debug)]
struct StreamFinalPayload {
    content: String,
    crdt_state: Vec<u8>,
    cleaned_resolved_backlog_prompts: bool,
    operator_current: String,
    replayed_pending_editor_ops: bool,
}

/// Opening-cycle proof that the visible document at preflight, rather than the
/// older durable snapshot, is the response's application base.
///
/// A prompt may already be visible when preflight opens the cycle. That prompt
/// belongs to the response being written and must be committed with it. A
/// prompt that appears after preflight is concurrent carry-forward and must
/// remain outside the response snapshot. Capture this witness before pending
/// response persistence advances the cycle to `response_captured`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PreflightApplicationBaseWitness {
    file_hash: String,
}

fn capture_preflight_application_base_witness(
    file: &Path,
    baseline: Option<&str>,
) -> Result<Option<PreflightApplicationBaseWitness>> {
    let Some(baseline) = baseline else {
        return Ok(None);
    };
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if state.phase != agent_doc_turn::CyclePhase::PreflightStarted
        || state.snapshot_hash.as_deref() != Some(agent_doc_hash::content_hash(baseline).as_str())
    {
        return Ok(None);
    }
    Ok(state
        .file_hash
        .map(|file_hash| PreflightApplicationBaseWitness { file_hash }))
}

fn application_baseline_for_preflight_witness<'a>(
    witness: Option<&PreflightApplicationBaseWitness>,
    baseline: Option<&'a str>,
    current_content: &'a str,
) -> Option<&'a str> {
    if witness
        .is_some_and(|witness| witness.file_hash == agent_doc_hash::content_hash(current_content))
    {
        Some(current_content)
    } else {
        baseline
    }
}

fn clear_replayed_editor_ops_after_write(file: &Path, replayed: bool, source: &str) {
    if !replayed {
        return;
    }
    match agent_doc_op_capture_io::clear_op_capture(file) {
        Ok(()) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_pending_editor_cut_consumed file={} outcome=cleared_after_successful_write",
                file.display(),
            ),
        ),
        Err(err) => {
            eprintln!(
                "[write] warning: failed to clear replayed editor-op capture for {} after successful write: {err}",
                file.display(),
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{source}_pending_editor_cut_clear_failed file={} error={}",
                    file.display(),
                    err,
                ),
            );
        }
    }
}

fn recover_empty_response_if_configured(file: &Path, flags: &WriteFlags) -> Result<bool> {
    if let Some(recover) = flags.empty_response_recovery {
        recover(
            file,
            flags.strict_closeout,
            flags.has_pending_mutation,
            flags.force_disk,
        )
    } else {
        Ok(false)
    }
}

const NO_PENDING_CAPTURE_MARKER: &str = "<!-- no-pending-capture -->";

/// Persist the caller's explicit no-followup closeout intent as response
/// evidence. The marker is stripped by the ordinary transient-marker cleanup,
/// so it never becomes document content, while both pre-write and pre-commit
/// guards observe the same durable capture on retries.
fn encode_no_pending_capture_intent(file: &Path, response: &mut String, enabled: bool) {
    if !enabled || response.contains(NO_PENDING_CAPTURE_MARKER) {
        return;
    }
    if !response.ends_with('\n') {
        response.push('\n');
    }
    response.push_str(NO_PENDING_CAPTURE_MARKER);
    response.push('\n');
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "pending_capture_intent file={} outcome=declared_none source=cli_no_followups evidence=transient_response_marker",
            file.display()
        ),
    );
}

fn response_cell_from_patchback(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> Option<String> {
    if !unmatched.trim().is_empty() || patches.len() != 1 || patches[0].name != "exchange" {
        return None;
    }
    let response = patches[0].content.trim_matches(['\n', '\r']);
    (!response.is_empty()
        && agent_doc_template::response_materialization::response_text_has_heading(response)
        && agent_doc_merge::response_cell::is_assistant_only_response_cell(response))
    .then(|| response.to_string())
}

fn enforce_strict_template_closeout_contract(
    current_content: &str,
    parsed_marker_count: usize,
    patches: &[template::PatchBlock],
    unmatched: &str,
    strict_closeout: bool,
) -> Result<()> {
    if !strict_closeout {
        return Ok(());
    }
    let template_mode = frontmatter::parse(current_content)
        .map(|(fm, _)| fm.resolve_mode().is_template())
        .unwrap_or(false);
    agent_doc_template::response_materialization::ensure_strict_template_patch_markers(
        template_mode,
        parsed_marker_count,
        patches,
        unmatched,
    )?;
    agent_doc_template::response_materialization::ensure_strict_template_response_heading_for_current_doc(
        current_content,
        patches,
        unmatched,
    )
}

/// Apply the response-only part of finalize as one idempotent semantic CRDT op.
///
/// The operation carries no caller baseline or whole-document candidate.  The
/// controller evaluates it against the apply-time canonical, persists the CRDT
/// projection, and records `ResponseCellAdded` in the state backbone. Before the
/// fast path returns, the canonical projection is routed through the ordinary
/// write authority so live replicas ACK it and the acknowledged cut is
/// materialized to disk. Composite patchbacks continue through the legacy write
/// path until their individual semantic operations are decomposed as well.
fn materialize_response_cell_projection(file: &Path, content: &str) -> Result<String> {
    agent_doc_document_realtime_io::atomic_write_through_authority(file, content)?;
    let materialized =
        resolve_disk_document_content(file, "response_cell_projection_materialized")?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "response_cell_projection_materialized file={} len={} hash={} transport=crdt_then_disk_projection",
            file.display(),
            materialized.len(),
            agent_doc_hash::content_hash(&materialized),
        ),
    );
    Ok(materialized)
}

fn response_cell_materialized_after_projection(response: &str, content: &str) -> bool {
    agent_doc_turn::response_replay::response_materialized_in_content(response, content)
}

/// Persist a cumulative response checkpoint without sealing or committing the
/// active cycle. Checkpoints contain only complete assistant response nodes; the
/// response-cell merge supersedes an older uncommitted checkpoint while
/// preserving every operator prompt in the apply-time canonical document.
pub fn checkpoint_response(file: &Path, response: &str) -> Result<()> {
    let response = agent_doc_turn::response_text::strip_assistant_heading(response);
    anyhow::ensure!(!response.trim().is_empty(), "empty response checkpoint");
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
        .ok_or_else(|| anyhow::anyhow!("response checkpoint requires an active preflight cycle"))?;
    anyhow::ensure!(
        state.is_open(),
        "response checkpoint requires an open cycle; current phase is {:?}",
        state.phase
    );

    let response_sha256 = agent_doc_hash::content_hash(&response);
    let operation_id = format!("response-checkpoint:{}:{response_sha256}", state.cycle_id);
    let committed_content = agent_doc_git_io::revision::show_head(file)?;
    let Some(write) = agent_doc_controller_io::project_controller::
        checkpoint_response_cell_via_controller_model_for_doc(
            file,
            &state.cycle_id,
            &operation_id,
            &response_sha256,
            &response,
            committed_content.as_deref(),
            "response_checkpoint",
        )?
    else {
        anyhow::bail!(
            "response checkpoint requires the live controller/Lazily document model; use finalize for a headless document"
        );
    };

    let materialized = materialize_response_cell_projection(file, &write.content)?;
    anyhow::ensure!(
        response_cell_materialized_after_projection(&response, &materialized),
        "response checkpoint was not present after acknowledged materialization for {}",
        file.display(),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "response_checkpoint_materialized file={} cycle_id={} operation_id={} cell_id={} applied={} response_sha256={} content_hash={}",
            file.display(),
            state.cycle_id,
            operation_id,
            write.cell_id,
            write.applied,
            response_sha256,
            agent_doc_hash::content_hash(&materialized),
        ),
    );
    eprintln!(
        "[write] response checkpoint {} ({})",
        write.cell_id,
        if write.applied {
            "advanced"
        } else {
            "unchanged"
        }
    );
    Ok(())
}

/// Publish one structurally complete, explicitly salient non-final result into
/// the live exchange. Repeated calls replace the node owned by the active cycle;
/// finalize removes it as part of final response-cell insertion.
pub fn checkpoint_salient_response(file: &Path, response: &str) -> Result<()> {
    let body = match agent_doc_turn::salient_response::decide_salient_checkpoint(response) {
        agent_doc_turn::salient_response::SalientCheckpointDecision::Apply { body } => body,
        agent_doc_turn::salient_response::SalientCheckpointDecision::Reject(rejection) => {
            anyhow::bail!(rejection.message())
        }
    };
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
        .ok_or_else(|| anyhow::anyhow!("salient response requires an active preflight cycle"))?;
    anyhow::ensure!(
        state.is_open(),
        "salient response requires an open cycle; current phase is {:?}",
        state.phase,
    );

    let response_sha256 = agent_doc_hash::content_hash(&body);
    let operation_id = format!("salient-response:{}:{response_sha256}", state.cycle_id);
    let Some(write) =
        agent_doc_controller_io::project_controller::salient_response_via_controller_model_for_doc(
            file,
            &state.cycle_id,
            &operation_id,
            &response_sha256,
            &body,
            "salient_response_checkpoint",
        )?
    else {
        anyhow::bail!(
            "salient response requires the live controller/Lazily document model; use console commentary for a headless document"
        );
    };

    let materialized = materialize_response_cell_projection(file, &write.content)?;
    anyhow::ensure!(
        agent_doc_merge::salient_response::salient_response_materialized(
            &materialized,
            &state.cycle_id,
            &body,
        )?,
        "salient response was not present after acknowledged materialization for {}",
        file.display(),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "salient_response_checkpoint_materialized file={} cycle_id={} operation_id={} cell_id={} applied={} response_sha256={} content_hash={}",
            file.display(),
            state.cycle_id,
            operation_id,
            write.cell_id,
            write.applied,
            response_sha256,
            agent_doc_hash::content_hash(&materialized),
        ),
    );
    eprintln!(
        "[write] salient response {} ({})",
        write.cell_id,
        if write.applied {
            "advanced"
        } else {
            "unchanged"
        },
    );
    Ok(())
}

fn try_add_response_cell_via_realtime_backbone(
    file: &Path,
    patches: &[template::PatchBlock],
    unmatched: &str,
    force_disk: bool,
    source: &str,
) -> Result<bool> {
    if force_disk {
        return Ok(false);
    }
    let Some(response_cell) = response_cell_from_patchback(patches, unmatched) else {
        return Ok(false);
    };
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(false);
    };
    let response_sha256 = state
        .response_sha256
        .unwrap_or_else(|| agent_doc_hash::content_hash(&response_cell));
    let operation_id = format!("response-cell:{}:{response_sha256}", state.cycle_id);
    let committed_content = agent_doc_git_io::revision::show_head(file)?;
    let Some(write) = agent_doc_controller_io::project_controller::
        add_response_cell_via_controller_model_for_doc(
            file,
            &state.cycle_id,
            &operation_id,
            &response_sha256,
            &response_cell,
            committed_content.as_deref(),
            source,
        )?
    else {
        return Ok(false);
    };

    let materialized = materialize_response_cell_projection(file, &write.content)?;
    anyhow::ensure!(
        response_cell_materialized_after_projection(&response_cell, &materialized),
        "response cell {} was not present after acknowledged disk materialization for {}",
        write.cell_id,
        file.display(),
    );
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &materialized,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_repair_io::pending::clear_pending(file)?;
    agent_doc_ops_log_io::log_cycle(
        file,
        "write_response_cell",
        Some(&response_cell),
        Some(&materialized),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "response_cell_write_done file={} source={} cycle_id={} operation_id={} cell_id={} applied={} content_hash={} materialized_hash={} update_bytes={} targets={} live_editors={} delivery_converged={}",
            file.display(),
            source,
            state.cycle_id,
            operation_id,
            write.cell_id,
            write.applied,
            write.content_hash,
            agent_doc_hash::content_hash(&materialized),
            write.update_bytes,
            write.targets,
            write.live_editors,
            write.delivery_converged,
        ),
    );
    eprintln!(
        "[write] response cell {} via realtime backbone ({})",
        if write.applied {
            "added"
        } else {
            "already present"
        },
        write.cell_id,
    );
    Ok(true)
}

/// Run the write command: append assistant response to document.
///
/// `baseline` is the document content at the time the response was generated.
/// If omitted, the current document content is used (no merge needed).
pub(crate) fn run(file: &Path, baseline: Option<&str>, flags: WriteFlags) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;

    let response = read_response_input_for_closeout(flags.strict_closeout)?;

    if response.trim().is_empty() {
        if recover_empty_response_if_configured(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!(EMPTY_RESPONSE_ERROR);
    }

    let preflight_application_base = capture_preflight_application_base_witness(file, baseline)?;

    let current_content = resolve_document_content_for_write_mode(
        file,
        flags.force_disk,
        "write_append_current_content",
        "write_append_force_disk_current_content",
    )
    .with_context(|| format!("failed to read {}", file.display()))?;
    let baseline = application_baseline_for_preflight_witness(
        preflight_application_base.as_ref(),
        baseline,
        &current_content,
    );
    enforce_imperative_response_contract_with_mutation_evidence(
        file,
        baseline,
        &current_content,
        &response,
        flags.has_metadata_only_mutation,
    )?;

    // Strip leading "## Assistant" heading if present — the write command adds its own
    let mut response = agent_doc_turn::response_text::strip_assistant_heading(&response);
    encode_no_pending_capture_intent(file, &mut response, flags.no_pending_capture);
    let pending_flags = super::pending_write_flags(&flags);
    agent_doc_session_check_io::prewrite_pending_capture_check(file, &response, &pending_flags)?;
    agent_doc_session_check_io::prewrite_pending_done_check(file, &response, &pending_flags)?;

    // Save response to pending store (survives context compaction)
    agent_doc_repair_io::pending::save_pending_with_current_content_and_plan(
        file,
        &response,
        &current_content,
        flags.mutation_plan_json.as_deref(),
    )?;

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let content_at_start = capture_undo_checkpoint(file)?;

    let base = baseline.unwrap_or(&content_at_start);

    // Build "ours": baseline + response appended
    let mut content_ours = base.to_string();
    // Ensure trailing newline before appending
    if !content_ours.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("## Assistant\n\n");
    content_ours.push_str(&response);
    if !response.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("\n## User\n\n");

    // Resolve the authoritative current document through the realtime model. If
    // an editor is active, the CRDT relay owns the current text and disk is not
    // used as a substitute.
    let force_disk_editor_attached = flags.force_disk && editor_crdt_authority_attached(file);
    if force_disk_editor_attached {
        ensure_force_disk_editor_authority_ready(file)?;
    }
    let content_current = resolve_document_content_for_write_mode(
        file,
        flags.force_disk,
        "write_append_merge_current_content",
        "write_append_force_disk_merge_current_content",
    )
    .with_context(|| format!("failed to read current file {}", file.display()))?;

    let final_content = if content_current == base {
        // No edits — use our version directly
        content_ours.clone()
    } else {
        eprintln!("[write] Current document changed since response baseline. Merging...");
        agent_doc_merge_io::merge_contents(base, &content_ours, &content_current)?
    };

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        let _ = agent_doc_cycle_state_io::mark_write_applied(
            file,
            "write_inline_dedup",
            Some(&content_current),
            Some(&content_current),
        );
        agent_doc_repair_io::pending::clear_pending(file)?;
        return Ok(());
    }

    let snapshot_mode = snapshot_persist_mode_with_current(
        baseline,
        base,
        &content_current,
        &content_ours,
        &final_content,
    );
    let snapshot_content =
        snapshot_content_to_persist(snapshot_mode, &content_ours, &final_content);
    // Deeper root cause A: when the merge would commit `content_ours` (to carry a
    // concurrently-typed prompt forward), commit the full union instead — the
    // on-disk `final_content` (agent response + operator edits) minus only the
    // carry-forward prompt lines. Operator edits are never lost (#qftlossdelta) and
    // the new prompt still stays uncommitted on disk for the next cycle (#fintol/#pcwc).
    let snapshot_content = if snapshot_mode == SnapshotPersistMode::ContentOurs {
        committed_snapshot_union_excluding_carry_forward(
            base,
            &content_ours,
            &content_current,
            &final_content,
        )
    } else {
        snapshot_content.to_string()
    };

    // Save snapshot BEFORE document write (#wcf5): external watchers (IDE
    // file-change listeners, git hooks) trigger on the document rename and may
    // compute diffs immediately. Writing the snapshot first guarantees they
    // always read a current baseline instead of racing against a stale one.
    log_exchange_write_diagnostic(
        file,
        "write_inline",
        "inline_disk",
        None,
        baseline,
        &content_current,
        &final_content,
        &[],
        "",
    );
    if !force_disk_editor_attached {
        guard_visible_write_expected_current_or_target(
            file,
            "write_inline",
            &content_current,
            Some(&final_content),
        )?;
    }
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &snapshot_content,
        agent_doc_ops_log_io::log_op,
    )?;

    atomic_write_for_write_mode(
        file,
        &content_current,
        &final_content,
        flags.force_disk,
        "write_inline",
    )?;

    agent_doc_ops_log_io::log_cycle(
        file,
        "write_inline",
        Some(&content_ours),
        Some(&final_content),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "write_inline_done file={} snap_len={}",
            file.display(),
            final_content.len()
        ),
    );
    if let Err(e) = agent_doc_cycle_state_io::mark_write_applied(
        file,
        "write_inline",
        Some(&final_content),
        Some(&final_content),
    ) {
        eprintln!("[write] cycle-state update failed: {} (non-fatal)", e);
    }
    // #22a8: mirror the live pipeline phase into the document frontmatter now the
    // response is fully on disk (doc lock still held, so no writer races).
    if let Ok(Some(st)) = agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        agent_doc_cycle_state_io::pipeline_frontmatter::mirror_pipeline_frontmatter(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            file,
            &st,
        );
    }

    // Clear pending response after successful write
    agent_doc_repair_io::pending::clear_pending(file)?;

    eprintln!("[write] Response appended to {}", file.display());
    Ok(())
}

/// Run the template write command: parse patch blocks and apply to components.
///
/// `baseline` is the document content at the time the response was generated.
pub(crate) fn run_template(
    file: &Path,
    baseline: Option<&str>,
    origin: Option<&str>,
    flags: WriteFlags,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());

    let mut response = read_response_input_for_closeout(flags.strict_closeout)?;

    if response.trim().is_empty() {
        if recover_empty_response_if_configured(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!(EMPTY_RESPONSE_ERROR);
    }

    let preflight_application_base = capture_preflight_application_base_witness(file, baseline)?;

    let current_content = resolve_document_content_for_write_mode(
        file,
        flags.force_disk,
        "write_template_current_content",
        "write_template_force_disk_current_content",
    )
    .with_context(|| format!("failed to read {}", file.display()))?;
    let baseline = application_baseline_for_preflight_witness(
        preflight_application_base.as_ref(),
        baseline,
        &current_content,
    );
    let snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    guard_stale_snapshot_recovery_only(
        file,
        snapshot_doc.as_deref(),
        &current_content,
        "template write",
    );
    sanitize_template_patchback_response(&mut response)?;
    enforce_imperative_response_contract_with_mutation_evidence(
        file,
        baseline,
        &current_content,
        &response,
        flags.has_metadata_only_mutation,
    )?;
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &current_content);

    // Parse and validate patchback shape before any visible document mutation.
    let parsed = agent_doc_template_io::parse_template_patchback(
        file,
        &response,
        "run_template",
        agent_doc_ops_log_io::log_op,
    )?;
    let parsed_marker_count = parsed.marker_count;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    template::sanitize::sanitize_patches(&mut patches);
    template::sanitize::sanitize_unmatched(&mut unmatched);

    let normalized = template_io::normalize_backlog_patch_response(
        file,
        &current_content,
        patches,
        unmatched,
        flags.allow_replace_pending,
    )?;
    if let Some(response_override) = normalized.response_for_capture {
        response = response_override;
    }
    encode_no_pending_capture_intent(file, &mut response, flags.no_pending_capture);
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject tracked-work full-replacement blocks unless allowed.
    template_io::enforce_no_replace_pending(&patches, flags.allow_replace_pending)?;
    enforce_no_destructive_todo_patch(&current_content, &patches)?;
    if let Err(reason) = agent_doc_template::patchback::enforce_orchestrate_patchback_contract(
        origin, &patches, &unmatched,
    ) {
        anyhow::bail!("{}", reason.message());
    }

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    if !flags.allow_replace_pending && !template_io::pending_replace_escape_hatch_enabled() {
        agent_doc_template::response_materialization::ensure_template_response_write_proof(
            &patches, &unmatched,
        )?;
    }
    enforce_strict_template_closeout_contract(
        &current_content,
        parsed_marker_count,
        &patches,
        &unmatched,
        flags.strict_closeout,
    )?;
    let pending_flags = super::pending_write_flags(&flags);
    agent_doc_session_check_io::prewrite_pending_capture_check(file, &response, &pending_flags)?;
    agent_doc_session_check_io::prewrite_pending_done_check(file, &response, &pending_flags)?;

    // Save response to pending store (survives context compaction)
    agent_doc_repair_io::pending::save_pending_with_current_content_and_plan(
        file,
        &response,
        &current_content,
        flags.mutation_plan_json.as_deref(),
    )?;

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let content_at_start = capture_undo_checkpoint(file)?;

    let base_cow = template_patch_application_base(TemplatePatchApplicationBase {
        file,
        baseline,
        content_at_start: &content_at_start,
        patches: &patches,
        unmatched: &unmatched,
        mode_overrides: &mode_overrides,
        source: "run_template",
        strict_closeout: flags.strict_closeout,
    })?;
    let base = base_cow.as_ref();
    let snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();

    // Apply patches to baseline
    let content_ours = template_io::apply_patches_with_overrides_with_project_config(
        base,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
        Some(rc.project_config()),
    )
    .context("failed to apply template patches")?;
    let content_ours =
        normalize_template_structure_or_fail_preserving(&content_ours, file, Some(base))?;

    let force_disk_editor_attached = flags.force_disk && editor_crdt_authority_attached(file);
    // Resolve the authoritative document to check for user edits since lock
    // acquisition. Active editors resolve through the CRDT relay; detached docs
    // use disk as the fallback replica. An explicit force-disk write against an
    // attached editor must pass the relay barrier before disk is read or written.
    if force_disk_editor_attached {
        ensure_force_disk_editor_authority_ready(file)?;
    }
    let content_current = resolve_document_content_for_write_mode(
        file,
        flags.force_disk,
        "write_template_merge_current_content",
        "write_template_force_disk_merge_current_content",
    )
    .with_context(|| format!("failed to read current file {}", file.display()))?;

    // Recompute the merged + normalized content for a given on-disk `current`.
    // Factored so the reconcile loop below can re-merge against a fresh disk
    // state when a foreign agent-doc writer appends mid-generation.
    let recompute_final = |content_current: &str| -> Result<(String, bool)> {
        let final_content = if let Some(repaired_current) =
            adopt_current_response_without_duplication(
                file,
                base,
                &content_ours,
                content_current,
                snapshot_doc.as_deref(),
                &response,
            )? {
            eprintln!(
                "[write] response already present in current file; adopting normalized current content"
            );
            repaired_current
        } else if content_current == base {
            content_ours.clone()
        } else {
            eprintln!("[write] Current document changed since response baseline. Merging...");
            agent_doc_merge_io::merge_contents(base, &content_ours, content_current)?
        };
        // Prompt cleanup is scoped to the operator cut observed before this
        // turn started mutating the document. Granular agent-authored backlog
        // changes may already be visible in `content_current`; treating those
        // as operator prompts resurrects/deletes the wrong queue intent.
        let final_closeout = finalize_template_closeout_content(FinalTemplateCloseoutRequest {
            file,
            base,
            snapshot: snapshot_doc.as_deref(),
            before_current: content_current,
            current_at_response_capture: &current_content,
            content: &final_content,
            response: &response,
            mode: FinalTemplateNormalizationMode::Required,
        })?;
        Ok((
            final_closeout.content,
            final_closeout.cleaned_resolved_backlog_prompts,
        ))
    };

    let initial_payload = recompute_final(&content_current)?;

    // Reconcile the visible-write guard with the CRDT merge: if a foreign
    // agent-doc writer appended to the document after the merge was computed
    // (disk drift, not a pending user edit), re-merge the captured response
    // against the fresh disk state and retry instead of failing closed and
    // stranding the response outside HEAD (#ipc-drift-visbuf-reconcile).
    let (content_current, (final_content, cleaned_resolved_backlog_prompts_applied)) =
        if force_disk_editor_attached {
            (content_current, initial_payload)
        } else {
            reconcile_visible_write(
                file,
                content_current,
                initial_payload,
                VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS,
                |f, expected, payload| {
                    guard_visible_write_reconcile_with_target(
                        f,
                        "run_template",
                        expected,
                        Some(&payload.0),
                    )
                },
                recompute_final,
                |f, current, payload| {
                    guard_visible_write_expected_current_or_target(
                        f,
                        "run_template",
                        current,
                        Some(&payload.0),
                    )
                },
            )?
        };

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        let _ = agent_doc_cycle_state_io::mark_write_applied(
            file,
            "write_template_dedup",
            Some(&content_current),
            Some(&content_current),
        );
        agent_doc_repair_io::pending::clear_pending(file)?;
        return Ok(());
    }

    let snapshot_mode = if cleaned_resolved_backlog_prompts_applied {
        snapshot_persist_mode(baseline, &content_ours, &final_content)
    } else {
        snapshot_persist_mode_with_current(
            baseline,
            base,
            &content_current,
            &content_ours,
            &final_content,
        )
    };
    let snapshot_content =
        snapshot_content_to_persist(snapshot_mode, &content_ours, &final_content);
    // Deeper root cause A: when the merge would commit `content_ours` (to carry a
    // concurrently-typed prompt forward), commit the full union instead — the
    // on-disk `final_content` (agent response + operator edits) minus only the
    // carry-forward prompt lines. Operator edits are never lost (#qftlossdelta) and
    // the new prompt still stays uncommitted on disk for the next cycle (#fintol/#pcwc).
    let snapshot_content = if snapshot_mode == SnapshotPersistMode::ContentOurs {
        committed_snapshot_union_excluding_carry_forward(
            base,
            &content_ours,
            &content_current,
            &final_content,
        )
    } else {
        snapshot_content.to_string()
    };

    // Save snapshot BEFORE document write (#wcf5): external watchers (IDE
    // file-change listeners, git hooks) trigger on the document rename and may
    // compute diffs immediately. Writing the snapshot first guarantees they
    // always read a current baseline instead of racing against a stale one.
    log_exchange_write_diagnostic(
        file,
        "run_template",
        "template_disk",
        None,
        baseline,
        &content_current,
        &final_content,
        &patches,
        &unmatched,
    );
    // Visible-write guard already reconciled above (see #ipc-drift-visbuf-reconcile).
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &snapshot_content,
        agent_doc_ops_log_io::log_op,
    )?;

    // `#fcc0`: template (non-CRDT) mode must converge through the editor path;
    // if editor convergence is unavailable or unproven, fail closed instead of
    // writing the merged document straight to disk.
    if flags.force_disk {
        agent_doc_document_realtime_io::atomic_write_force_disk_through_authority(
            file,
            &final_content,
        )?;
    } else {
        agent_doc_write_converge_io::try_editor_converge(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            &final_content,
            &content_current,
            "write_template",
        )?;
    }

    agent_doc_ops_log_io::log_cycle(
        file,
        "write_template",
        Some(&content_ours),
        Some(&final_content),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "write_template_done file={} snap_len={} patches={}",
            file.display(),
            final_content.len(),
            patches.len()
        ),
    );
    if let Err(e) = agent_doc_cycle_state_io::mark_write_applied(
        file,
        "write_template",
        Some(&final_content),
        Some(&final_content),
    ) {
        eprintln!("[write] cycle-state update failed: {} (non-fatal)", e);
    }
    // #22a8: mirror the live pipeline phase into the document frontmatter now the
    // response is fully on disk (doc lock still held, so no writer races).
    if let Ok(Some(st)) = agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        agent_doc_cycle_state_io::pipeline_frontmatter::mirror_pipeline_frontmatter(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            file,
            &st,
        );
    }

    // Clear pending response after successful write
    agent_doc_repair_io::pending::clear_pending(file)?;

    eprintln!(
        "[write] Template patches applied to {} ({} components patched)",
        file.display(),
        patches.len()
    );
    Ok(())
}

/// Run the stream write command: template patches with CRDT merge (conflict-free).
///
/// Like `run_template`, but uses CRDT merge instead of git merge-file.
/// `baseline` is the document content at the time the response was generated.
///
/// When `force_disk` is false, writes target the registered Lazily replica over
/// its PID-scoped endpoint. There is no filesystem transport fallback.
pub(crate) fn run_stream(
    file: &Path,
    baseline: Option<&str>,
    force_disk: bool,
    origin: Option<&str>,
    flags: WriteFlags,
) -> Result<()> {
    let t_total = std::time::Instant::now();

    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    // #jb-tsift-pane-sync diagnostic: capture a streamed write/commit to `file`
    // executing inside a tmux pane that owns a different document.
    agent_doc_sync_io::sync::log_cross_document_execution_context(file, "stream");

    let mut response = read_response_input_for_closeout(flags.strict_closeout)?;

    if response.trim().is_empty() {
        if recover_empty_response_if_configured(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!(EMPTY_RESPONSE_ERROR);
    }
    sanitize_template_patchback_response(&mut response)?;

    let preflight_application_base = capture_preflight_application_base_witness(file, baseline)?;

    let pre_capture_current_content =
        capture_validated_stream_closeout_before_authority_resolution(
            file, baseline, force_disk, &response, &flags,
        )?;

    let current_content = match pre_capture_current_content {
        Some(content) => content,
        None => resolve_document_content_for_write_mode(
            file,
            force_disk,
            "write_stream_current_content",
            "write_stream_force_disk_current_content",
        )
        .with_context(|| format!("failed to read {}", file.display()))?,
    };
    let baseline = application_baseline_for_preflight_witness(
        preflight_application_base.as_ref(),
        baseline,
        &current_content,
    );
    let mut snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    if guard_stale_snapshot_recovery_only(
        file,
        snapshot_doc.as_deref(),
        &current_content,
        "stream write",
    ) {
        snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
            .ok()
            .flatten();
    }
    enforce_imperative_response_contract_with_mutation_evidence(
        file,
        baseline,
        &current_content,
        &response,
        flags.has_metadata_only_mutation,
    )?;
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &current_content);

    if let Some(signal) =
        agent_doc_turn::heuristics::future_work_signal(&response, flags.has_pending_add)
    {
        eprintln!(
            "[write] WARN: response contains future-work signal {:?} but no --pending-add was provided",
            signal
        );
    }

    // Parse and validate patchback shape before any visible document mutation.
    let parsed = agent_doc_template_io::parse_template_patchback(
        file,
        &response,
        "run_stream",
        agent_doc_ops_log_io::log_op,
    )?;
    let parsed_marker_count = parsed.marker_count;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    template::sanitize::sanitize_patches(&mut patches);
    template::sanitize::sanitize_unmatched(&mut unmatched);

    let normalized = template_io::normalize_backlog_patch_response(
        file,
        &current_content,
        patches,
        unmatched,
        flags.allow_replace_pending,
    )?;
    if let Some(response_override) = normalized.response_for_capture {
        response = response_override;
    }
    encode_no_pending_capture_intent(file, &mut response, flags.no_pending_capture);
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject tracked-work full-replacement blocks unless allowed.
    template_io::enforce_no_replace_pending(&patches, flags.allow_replace_pending)?;
    enforce_no_destructive_todo_patch(&current_content, &patches)?;
    if let Err(reason) = agent_doc_template::patchback::enforce_orchestrate_patchback_contract(
        origin, &patches, &unmatched,
    ) {
        anyhow::bail!("{}", reason.message());
    }

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    if !flags.allow_replace_pending && !template_io::pending_replace_escape_hatch_enabled() {
        agent_doc_template::response_materialization::ensure_template_response_write_proof(
            &patches, &unmatched,
        )?;
    }
    enforce_strict_template_closeout_contract(
        &current_content,
        parsed_marker_count,
        &patches,
        &unmatched,
        flags.strict_closeout,
    )?;
    let pending_flags = super::pending_write_flags(&flags);
    if let Err(err) =
        agent_doc_session_check_io::prewrite_pending_capture_check(file, &response, &pending_flags)
    {
        retain_ipc_patch_for_retry_error(
            file,
            baseline,
            &response,
            &err,
            "prewrite_pending_capture_check",
        )?;
        return Err(err);
    }
    if let Err(err) =
        agent_doc_session_check_io::prewrite_pending_done_check(file, &response, &pending_flags)
    {
        retain_ipc_patch_for_retry_error(
            file,
            baseline,
            &response,
            &err,
            "prewrite_pending_done_check",
        )?;
        return Err(err);
    }

    agent_doc_template::response_materialization::reject_marker_response_with_zero_patches(
        parsed_marker_count,
        patches.len(),
    )?;

    if patches.is_empty() {
        eprintln!(
            "[write] WARNING: 0 template patches found for {} — response may be missing or malformed. \
             Only normalization/boundary changes will be applied.",
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "zero_patches_warning file={} source=run_stream markers=0 response may be empty or malformed",
                file.display()
            ),
        );
    }

    // Save response to pending store (survives context compaction)
    agent_doc_repair_io::pending::save_pending_with_current_content_and_plan(
        file,
        &response,
        &current_content,
        flags.mutation_plan_json.as_deref(),
    )?;

    if try_add_response_cell_via_realtime_backbone(
        file,
        &patches,
        &unmatched,
        force_disk,
        "run_stream",
    )? {
        return Ok(());
    }

    // Warn when patches target a file with no template components
    if patches.is_empty() && !unmatched.trim().is_empty() {
        let current = resolve_document_content_for_write_mode(
            file,
            force_disk,
            "write_stream_empty_patch_component_check",
            "write_stream_force_disk_empty_patch_component_check",
        )
        .with_context(|| format!("failed to read {}", file.display()))?;
        let comps = agent_doc_element::element::parse(&current).unwrap_or_default();
        if comps.is_empty() {
            eprintln!(
                "[write] WARNING: {} bytes of content but file has no template components — \
                 content may not be applied correctly. Consider running `agent-doc init` \
                 with --mode template first.",
                unmatched.trim().len()
            );
        }
    }

    let content_at_start = capture_undo_checkpoint(file)?;

    // Try IPC when plugin is installed and --force-disk is not set
    if !force_disk {
        // `#docop-plane` P4: relay membership is transport state, not editor
        // liveness. Only an authoritative reliable-sync Close permits the disk
        // path to skip redundant CP delivery attempts.
        let editor_absent = write_path_editor_absent(file);
        if editor_absent {
            eprintln!(
                "[write] reliable-sync reports the editor absent — disk is document authority, skipping IPC cascade"
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "write_ipc_editor_absent_disk_authority file={} reason=reliable_sync_editor_absent recovery=direct_disk_write",
                    file.display(),
                ),
            );
        }
        if !editor_absent {
            // Compute content_ours (baseline + patches) for snapshot saving.
            // The IPC path sends patches to the plugin but we need a clean snapshot
            // that represents baseline+response WITHOUT user's concurrent edits.
            let base_cow = template_patch_application_base(TemplatePatchApplicationBase {
                file,
                baseline,
                content_at_start: &content_at_start,
                patches: &patches,
                unmatched: &unmatched,
                mode_overrides: &mode_overrides,
                source: "run_stream_ipc",
                strict_closeout: flags.strict_closeout,
            })?;
            let base = base_cow.as_ref();
            let ipc_baseline = baseline.map(|_| base);
            let t_apply = std::time::Instant::now();
            let mut content_ours = template_io::apply_patches_with_overrides_with_project_config(
                base,
                &patches,
                &unmatched,
                file,
                &mode_overrides,
                Some(rc.project_config()),
            )
            .context("failed to apply patches for snapshot")?;
            let elapsed_apply = t_apply.elapsed().as_millis();
            if elapsed_apply > 0 {
                eprintln!("[perf] apply_patches_with_overrides: {}ms", elapsed_apply);
            }

            // Guard: detect stale baseline by structural component comparison.
            // A baseline is stale when it's MISSING committed content from the snapshot
            // (e.g., a previous response was committed but the baseline predates it).
            // A baseline with EXTRA content beyond the snapshot is normal (user edits).
            //
            // IMPORTANT: Skip this check when an explicit baseline was provided via
            // the state.db baseline. Streaming checkpoints intentionally use the original
            // document (before any response) as baseline so cumulative patch blocks
            // apply cleanly on each checkpoint. The snapshot will have content from
            // earlier checkpoints, causing is_stale_baseline to incorrectly fire and
            // apply patches on top of content_at_start (which already has earlier
            // checkpoint content) → duplicate response content.
            //
            // Compare component-by-component: for each component in the snapshot, check
            // that the baseline's corresponding component contains the snapshot content.
            // This handles user edits anywhere in the document (not just appended at end).
            if baseline.is_none()
                && let Ok(Some(current_snap)) = agent_doc_snapshot_io::load_document_baseline(file)
                && is_stale_baseline(base, &current_snap)
            {
                eprintln!(
                    "[write] WARNING: baseline missing snapshot content — stale baseline detected, using current file as baseline"
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "stale_baseline_detected file={} base_len={} snap_len={} file_len={}",
                        file.display(),
                        base.len(),
                        current_snap.len(),
                        content_at_start.len()
                    ),
                );
                // Re-apply patches to the current file content instead of the stale baseline
                content_ours = template_io::apply_patches_with_overrides_with_project_config(
                    &content_at_start,
                    &patches,
                    &unmatched,
                    file,
                    &mode_overrides,
                    Some(rc.project_config()),
                )
                .context("failed to apply patches with fresh baseline")?;
            }

            // Normalize user input in exchange: add ❯  prefix to user-added lines.
            // Uses the snapshot (loaded above) to identify new lines.
            // Compute normalization targets for the IPC plugin so the editor also shows
            // the prefix immediately (not just the snapshot).
            let normalize_prefix_lines: Vec<String> = if let Some(ref snap) = snapshot_doc {
                let before = content_ours.clone();
                content_ours =
                    normalize_user_prompts_in_exchange_safe(&content_ours, base, snap, file);
                extract_normalization_targets(&before, &content_ours)
            } else {
                vec![]
            };

            // Lift pending out of exchange if nested (structural repair)
            content_ours = lift_pending_from_exchange_safe(&content_ours, file);
            content_ours =
                normalize_template_structure_or_fail_preserving(&content_ours, file, Some(base))?;

            // Shrink guard: refuse if new exchange content is dramatically shorter
            agent_doc_element_exchange_io::check_exchange_shrink_guard_with_log(
                &content_at_start,
                &content_ours,
                file,
                SHRINK_GUARD_MIN_BYTES,
                SHRINK_GUARD_MAX_RATIO,
                agent_doc_ops_log_io::log_op,
            )?;

            // Dedup: skip IPC if patches produce no changes (strip boundary markers)
            if strip_boundary_for_dedup(&content_ours)
                == strip_boundary_for_dedup(&content_at_start)
            {
                log_dedup(file, "no changes after merge, skipping write");
                agent_doc_repair_io::pending::clear_pending(file)?;
                return Ok(());
            }

            // Plugin is installed — try IPC
            let t_ipc = std::time::Instant::now();
            let norm_lines_opt = if normalize_prefix_lines.is_empty() {
                None
            } else {
                Some(normalize_prefix_lines.as_slice())
            };
            let ipc_result = agent_doc_write_ipc_io::try_ipc_with_effects(
                &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
                file,
                &patches,
                &unmatched,
                None,
                ipc_baseline,
                Some(&content_ours),
                norm_lines_opt,
                None,
            )?;
            if ipc_result.skipped_committed_cycle {
                let elapsed_total = t_total.elapsed().as_millis();
                if elapsed_total > 0 {
                    eprintln!("[perf] run_stream total: {}ms", elapsed_total);
                }
                agent_doc_repair_io::pending::clear_pending(file)?;
                return Ok(());
            }
            if ipc_result.success {
                let elapsed_ipc = t_ipc.elapsed().as_millis();
                if elapsed_ipc > 0 {
                    eprintln!("[perf] try_ipc: {}ms", elapsed_ipc);
                }
                let elapsed_total = t_total.elapsed().as_millis();
                if elapsed_total > 0 {
                    eprintln!("[perf] run_stream total: {}ms", elapsed_total);
                }
                // IPC succeeded — plugin applied patches
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{} file={} patches={}",
                        OpsLogEvent::IpcWriteConsumed,
                        file.display(),
                        patches.len()
                    ),
                );
                // Fire post_write hook for cross-session coordination
                let session_id =
                    agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_default();
                let hook_effects = agent_doc_hooks_io::default_post_response_hook_effects();
                agent_doc_hooks_io::fire_post_write_with_effects(
                    &hook_effects,
                    file,
                    &session_id,
                    patches.len(),
                );
                agent_doc_hooks_io::fire_doc_event(file, "post_write");
                agent_doc_repair_io::pending::clear_pending(file)?;
                return Ok(());
            }
            let recovery = if editor_crdt_authority_attached(file) {
                "cp_crdt_relay"
            } else {
                "detached_disk_authority"
            };
            eprintln!(
                "[write] editor IPC did not prove the write — recovering through document authority ({recovery})"
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "run_stream_ipc_recover_via_document_authority file={} patch_id={} patches={} recovery={}",
                    file.display(),
                    ipc_result.patch_id,
                    patches.len(),
                    recovery,
                ),
            );
        }
    }

    // No live editor or explicit --force-disk: disk is the only authority.
    let t_disk = std::time::Instant::now();
    let force_disk_editor_attached = force_disk && editor_crdt_authority_attached(file);
    if force_disk_editor_attached {
        ensure_force_disk_editor_authority_ready(file)?;
    }

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let base_cow = template_patch_application_base(TemplatePatchApplicationBase {
        file,
        baseline,
        content_at_start: &content_at_start,
        patches: &patches,
        unmatched: &unmatched,
        mode_overrides: &mode_overrides,
        source: "run_stream_disk",
        strict_closeout: flags.strict_closeout,
    })?;
    let base = base_cow.as_ref();

    let build_patched_content = |patch_base: &str,
                                 normalize_base: &str,
                                 normalize_user_prompts: bool,
                                 normalize_structure: bool|
     -> Result<String> {
        // Apply patches using the mode resolution chain:
        // inline attr (patch=append on tag) > config.toml ([components] section) > built-in default.
        // The skill sends delta content for append-mode components.
        let mut content = template_io::apply_patches_with_overrides_with_project_config(
            patch_base,
            &patches,
            &unmatched,
            file,
            &mode_overrides,
            Some(rc.project_config()),
        )
        .context("failed to apply template patches")?;

        // Apply frontmatter patch if present (fixes #16 — disk write path was missing this)
        if let Some(fm_patch) = patches.iter().find(|p| p.name == "frontmatter") {
            content = agent_doc_frontmatter::frontmatter::merge_fields(&content, &fm_patch.content)
                .context("failed to merge frontmatter patch")?;
        }

        // Normalize user input in exchange: add ❯  prefix to user-added lines.
        // Load snapshot to identify which lines are new (user-typed this cycle).
        if normalize_user_prompts && let Some(ref snap) = snapshot_doc {
            content = normalize_user_prompts_in_exchange_safe(&content, normalize_base, snap, file);
        }

        if normalize_structure {
            // Lift pending out of exchange if nested (structural repair)
            content = lift_pending_from_exchange_safe(&content, file);
            normalize_template_structure_or_fail_preserving(&content, file, Some(normalize_base))
        } else {
            Ok(content)
        }
    };

    let t_apply2 = std::time::Instant::now();
    let content_ours = build_patched_content(base, base, true, true)?;
    let elapsed_apply2 = t_apply2.elapsed().as_millis();
    if elapsed_apply2 > 0 {
        eprintln!(
            "[perf] apply_patches_with_overrides (disk): {}ms",
            elapsed_apply2
        );
    }

    // Shrink guard: refuse if new exchange content is dramatically shorter
    agent_doc_element_exchange_io::check_exchange_shrink_guard_with_log(
        &content_at_start,
        &content_ours,
        file,
        SHRINK_GUARD_MIN_BYTES,
        SHRINK_GUARD_MAX_RATIO,
        agent_doc_ops_log_io::log_op,
    )?;

    // Resolve the authoritative document to check for user edits since lock
    // acquisition. Active editors resolve through the CRDT relay; detached docs
    // use disk as the fallback replica. An explicit force-disk write against an
    // attached editor must pass the relay barrier before disk is read or written.
    let content_current = resolve_document_content_for_write_mode(
        file,
        force_disk,
        "write_stream_merge_current_content",
        "write_stream_force_disk_merge_current_content",
    )
    .with_context(|| format!("failed to read current file {}", file.display()))?;

    // Recompute the CRDT-merged + normalized content (and its encoded state)
    // for a given on-disk `current`. Factored so the reconcile loop below can
    // re-merge against a fresh disk state when a foreign agent-doc writer
    // appends mid-generation (#ipc-drift-visbuf-reconcile).
    let recompute_final = |content_current: &str| -> Result<StreamFinalPayload> {
        let reconciled_current = if force_disk {
            agent_doc_document_realtime_io::PendingEditorCutReconciliation {
                content: content_current.to_string(),
                replayed_editor_ops: false,
            }
        } else {
            agent_doc_document_realtime_io::reconcile_pending_editor_cut(
                file,
                base,
                content_current,
                "run_stream",
            )?
        };
        let operator_current = reconciled_current.content;
        let merge_current = operator_current.as_str();
        let (final_content, mut crdt_state, skip_final_normalize) = if let Some(repaired_current) =
            adopt_current_response_without_duplication(
                file,
                base,
                &content_ours,
                merge_current,
                snapshot_doc.as_deref(),
                &response,
            )? {
            eprintln!(
                "[write] response already present in current file; adopting normalized current content"
            );
            let doc = agent_doc_merge::crdt::CrdtDoc::from_text(&repaired_current);
            (
                repaired_current,
                doc.encode_state(),
                force_disk_editor_attached,
            )
        } else if merge_current == base {
            // No edits — build CRDT state from result
            let doc = agent_doc_merge::crdt::CrdtDoc::from_text(&content_ours);
            (content_ours.clone(), doc.encode_state(), false)
        } else if force_disk {
            eprintln!(
                "[write] File was modified during force-disk response generation. Applying patches to current document..."
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "force_disk_patch_current file={} source=run_stream current_len={} base_len={}",
                    file.display(),
                    merge_current.len(),
                    base.len()
                ),
            );
            let patched_current = match apply_simple_exchange_patch_to_current(
                file,
                merge_current,
                &patches,
                &unmatched,
            ) {
                Some(result) => result?,
                None => build_patched_content(merge_current, merge_current, false, false)?,
            };
            agent_doc_element_exchange_io::check_exchange_shrink_guard_with_log(
                merge_current,
                &patched_current,
                file,
                SHRINK_GUARD_MIN_BYTES,
                SHRINK_GUARD_MAX_RATIO,
                agent_doc_ops_log_io::log_op,
            )?;
            let doc = agent_doc_merge::crdt::CrdtDoc::from_text(&patched_current);
            (
                patched_current,
                doc.encode_state(),
                force_disk_editor_attached,
            )
        } else {
            eprintln!(
                "[write] Current document changed since response baseline. Rebasing the captured intent onto current authority..."
            );
            let rebased = match apply_simple_exchange_patch_to_current(
                file,
                merge_current,
                &patches,
                &unmatched,
            ) {
                Some(result) => result?,
                None => merge_template_document_model(file, base, &content_ours, merge_current)?,
            };
            let doc = agent_doc_merge::crdt::CrdtDoc::from_text(&rebased);
            (rebased, doc.encode_state(), false)
        };
        let final_closeout = finalize_template_closeout_content(FinalTemplateCloseoutRequest {
            file,
            base,
            snapshot: snapshot_doc.as_deref(),
            before_current: merge_current,
            current_at_response_capture: &current_content,
            content: &final_content,
            response: &response,
            mode: if skip_final_normalize {
                FinalTemplateNormalizationMode::PreserveAdoptedAuthority
            } else {
                FinalTemplateNormalizationMode::Required
            },
        })?;
        let final_content = final_closeout.content;
        if final_closeout.cleaned_resolved_backlog_prompts {
            crdt_state = agent_doc_merge::crdt::CrdtDoc::from_text(&final_content).encode_state();
        }
        Ok(StreamFinalPayload {
            content: final_content,
            crdt_state,
            cleaned_resolved_backlog_prompts: final_closeout.cleaned_resolved_backlog_prompts,
            operator_current,
            replayed_pending_editor_ops: reconciled_current.replayed_editor_ops,
        })
    };

    let initial_payload = recompute_final(&content_current)?;

    // Reconcile the visible-write guard with the CRDT merge: re-merge the
    // captured response against a foreign disk append landed after the merge
    // was computed instead of failing closed and stranding the response
    // outside HEAD (#ipc-drift-visbuf-reconcile).
    let (content_current, final_payload) = if force_disk_editor_attached {
        (content_current, initial_payload)
    } else {
        reconcile_visible_write(
            file,
            content_current,
            initial_payload,
            VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS,
            |f, expected, payload| {
                guard_visible_write_reconcile_with_target(
                    f,
                    "run_stream",
                    expected,
                    Some(&payload.content),
                )
            },
            recompute_final,
            |f, current, payload| {
                guard_visible_write_expected_current_or_target(
                    f,
                    "run_stream",
                    current,
                    Some(&payload.content),
                )
            },
        )?
    };
    let final_content = final_payload.content;
    let cleaned_resolved_backlog_prompts_applied = final_payload.cleaned_resolved_backlog_prompts;
    let operator_current = final_payload.operator_current;
    let replayed_pending_editor_ops = final_payload.replayed_pending_editor_ops;
    let _crdt_state = final_payload.crdt_state;

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        let _ = agent_doc_cycle_state_io::mark_write_applied(
            file,
            "write_stream_dedup",
            Some(&content_current),
            Some(&content_current),
        );
        agent_doc_repair_io::pending::clear_pending(file)?;
        clear_replayed_editor_ops_after_write(file, replayed_pending_editor_ops, "run_stream");
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] run_stream total: {}ms", elapsed_total);
        }
        return Ok(());
    }

    let snapshot_mode = if cleaned_resolved_backlog_prompts_applied {
        snapshot_persist_mode(baseline, &content_ours, &final_content)
    } else {
        snapshot_persist_mode_with_current(
            baseline,
            base,
            &operator_current,
            &content_ours,
            &final_content,
        )
    };
    let mut final_content = final_content;
    // Deeper root cause A (see run_template/run_inline): commit the union minus
    // carry-forward prompts rather than bare content_ours, so operator edits are
    // never lost while the concurrently-typed prompt still carries forward.
    let mut snapshot_content = if snapshot_mode == SnapshotPersistMode::ContentOurs {
        committed_snapshot_union_excluding_carry_forward(
            base,
            &content_ours,
            &operator_current,
            &final_content,
        )
    } else {
        snapshot_content_to_persist(snapshot_mode, &content_ours, &final_content).to_string()
    };
    let integrated_queue_plan = if flags.queue_completion_ids.is_empty() {
        None
    } else {
        queue_consume::plan_queue_prompt_consumption_with_snapshot(
            file,
            &final_content,
            Some(&snapshot_content),
            &flags.queue_completion_ids,
        )?
    };
    if let Some(plan) = integrated_queue_plan.as_ref() {
        queue_consume::record_queue_consumption_proofs(
            file,
            plan,
            QueueConsumptionProofStage::BeforeMutation,
        )?;
        final_content = plan.new_document.clone();
        snapshot_content = plan.new_snapshot.clone();
    }
    // Save snapshot BEFORE document write (#wcf5): external watchers (IDE
    // file-change listeners, git hooks) trigger on the document rename and may
    // compute diffs immediately. Writing the snapshot first guarantees they
    // always read a current baseline instead of racing against a stale one.
    log_exchange_write_diagnostic(
        file,
        "run_stream",
        "stream_disk",
        None,
        baseline,
        &operator_current,
        &final_content,
        &patches,
        &unmatched,
    );
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &snapshot_content,
        agent_doc_ops_log_io::log_op,
    )?;
    atomic_write_for_write_mode(
        file,
        &content_current,
        &final_content,
        force_disk,
        "run_stream",
    )?;
    clear_replayed_editor_ops_after_write(file, replayed_pending_editor_ops, "run_stream");
    if force_disk && snapshot_content != final_content {
        match
            agent_doc_controller_io::project_controller::record_visible_write_materialized_carry_forward_for_file(
                file,
                &content_current,
                &final_content,
                &snapshot_content,
                "run_stream_force_disk",
            ) {
            Ok(_) => {}
            Err(err) => agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "force_disk_visible_write_materialized_carry_forward_proof_failed file={} error={}",
                    file.display(),
                    err
                ),
            ),
        }
    }
    if let Some(plan) = integrated_queue_plan.as_ref() {
        queue_consume::record_queue_consumption_proofs(
            file,
            plan,
            QueueConsumptionProofStage::AfterMutation,
        )?;
        if plan.consumed_texts.len() == 1 {
            eprintln!(
                "[queue] consumed: {:?} (remaining: {})",
                plan.consumed_text, plan.remaining
            );
        } else {
            eprintln!(
                "[queue] consumed {} item(s): {:?} (remaining: {})",
                plan.consumed_texts.len(),
                plan.consumed_texts,
                plan.remaining
            );
        }
        if plan.drained {
            eprintln!("[queue] drained — cleared queue_active");
        } else if plan.auto {
            eprintln!(
                "[queue] auto queue has {} prompt(s) remaining after this closeout",
                plan.remaining
            );
        }
        if let Err(err) = agent_doc_owner_pane_io::clear(file) {
            eprintln!(
                "[recguard-wedge] WARNING: failed to clear wedge counter for {}: {}",
                file.display(),
                err
            );
        }
    }
    agent_doc_ops_log_io::log_cycle(
        file,
        "write_stream",
        Some(&content_ours),
        Some(&final_content),
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "write_stream_done file={} snap_len={}",
            file.display(),
            final_content.len()
        ),
    );
    if let Err(e) = agent_doc_cycle_state_io::mark_write_applied(
        file,
        "write_stream",
        Some(&final_content),
        Some(&final_content),
    ) {
        eprintln!("[write] cycle-state update failed: {} (non-fatal)", e);
    }

    // Clear pending response after successful write
    agent_doc_repair_io::pending::clear_pending(file)?;

    let elapsed_disk = t_disk.elapsed().as_millis();
    if elapsed_disk > 0 {
        eprintln!("[perf] disk_write_path: {}ms", elapsed_disk);
    }
    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] run_stream total: {}ms", elapsed_total);
    }

    eprintln!(
        "[write] Stream patches applied to {} ({} components patched, document model)",
        file.display(),
        patches.len()
    );
    Ok(())
}

/// Capture the semantic mutation envelope before the first live-authority read.
///
/// Resolving an editor-owned document can stop at the pre-write delivery barrier.
/// The response body is still recoverable from the capture ledger, so its
/// backlog/status mutation plan must already be attached to the same capture or
/// binary-owned recovery could later commit only the response.
fn capture_validated_stream_closeout_before_authority_resolution(
    file: &Path,
    baseline: Option<&str>,
    force_disk: bool,
    response: &str,
    flags: &WriteFlags,
) -> Result<Option<String>> {
    if !flags.has_pending_mutation {
        return Ok(None);
    }
    // Harness-native closeout supplies the preflight-owned baseline, so the
    // normal path captures its validated mutation plan before editor authority.
    // Standalone `finalize --done/--pending-add` remains compatible without
    // weakening validation: resolve through the binary-owned write adapter,
    // validate before capture, and return that content for reuse by run_stream.
    // This exceptional no-preflight lane never performs a direct disk read.
    let pre_capture_current_content = if flags.strict_closeout && baseline.is_none() {
        Some(
            resolve_document_content_for_write_mode(
                file,
                force_disk,
                "write_stream_pre_capture_current_content",
                "write_stream_force_disk_pre_capture_current_content",
            )
            .with_context(|| format!("failed to read {}", file.display()))?,
        )
    } else {
        None
    };
    if flags.strict_closeout {
        let pre_capture_content = baseline
            .or(pre_capture_current_content.as_deref())
            .ok_or_else(|| {
            anyhow::anyhow!(
                "strict stream closeout for {} cannot durably capture tracked-work mutations without a binary-owned cycle baseline",
                file.display()
            )
        })?;
        let parsed = agent_doc_template_io::parse_template_patchback(
            file,
            response,
            "run_stream_pre_capture",
            agent_doc_ops_log_io::log_op,
        )?;
        if !flags.allow_replace_pending && !template_io::pending_replace_escape_hatch_enabled() {
            agent_doc_template::response_materialization::ensure_template_response_write_proof(
                &parsed.patches,
                &parsed.unmatched,
            )?;
        }
        enforce_strict_template_closeout_contract(
            pre_capture_content,
            parsed.marker_count,
            &parsed.patches,
            &parsed.unmatched,
            true,
        )?;
    }

    capture_closeout_mutation_plan_before_authority_resolution(file, response, flags)?;
    Ok(pre_capture_current_content)
}

fn capture_closeout_mutation_plan_before_authority_resolution(
    file: &Path,
    response: &str,
    flags: &WriteFlags,
) -> Result<()> {
    if !flags.has_pending_mutation {
        return Ok(());
    }
    let mutation_plan_json = flags.mutation_plan_json.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "closeout for {} has tracked-work mutations but no durable mutation plan",
            file.display()
        )
    })?;
    agent_doc_repair_io::pending::save_pending_with_plan(file, response, Some(mutation_plan_json))?;
    agent_doc_cycle_state_io::mark_pending_mutations(file)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "closeout_mutation_plan_captured_before_authority_resolution file={} response_hash={} plan_hash={} recovery=replay_same_closeout_envelope",
            file.display(),
            agent_doc_hash::content_hash(response),
            agent_doc_hash::content_hash(mutation_plan_json),
        ),
    );
    Ok(())
}

/// Explicit editor mode: deliver through the registered Lazily replica.
/// Fails closed when no PID-scoped endpoint accepts the retained intent.
pub(crate) fn run_ipc(file: &Path, baseline: Option<&str>, flags: WriteFlags) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    let mut response = read_response_input_for_closeout(flags.strict_closeout)?;

    if response.trim().is_empty() {
        if recover_empty_response_if_configured(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!(EMPTY_RESPONSE_ERROR);
    }

    // #rtwwire rung 3b: the IPC write path normalizes/parses patches against the
    // authoritative current document. If an editor is active, source it from the
    // CRDT relay; if no editor is active, use disk as the fallback replica.
    let current_content = match resolve_current_document_content(file, "write_ipc_current_content")
    {
        Ok(current) => current,
        Err(resolve_err) => {
            if let Err(retain_err) =
                retain_ipc_patch_for_editor_authority_retry(file, baseline, &response)
            {
                eprintln!(
                    "[write] warning: failed to retain IPC retry patch for {}: {}",
                    file.display(),
                    retain_err
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "run_ipc_authority_retry_patch_retention_failed file={} error={} recovery=retry_without_disk_write",
                        file.display(),
                        retain_err
                    ),
                );
            }
            return Err(resolve_err);
        }
    };
    let snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    guard_stale_snapshot_recovery_only(
        file,
        snapshot_doc.as_deref(),
        &current_content,
        "IPC write",
    );
    sanitize_template_patchback_response(&mut response)?;
    enforce_imperative_response_contract_with_mutation_evidence(
        file,
        baseline,
        &current_content,
        &response,
        flags.has_metadata_only_mutation,
    )?;

    // Parse and validate patchback shape before any visible document mutation.
    let parsed = agent_doc_template_io::parse_template_patchback(
        file,
        &response,
        "run_ipc",
        agent_doc_ops_log_io::log_op,
    )?;
    let parsed_marker_count = parsed.marker_count;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    template::sanitize::sanitize_patches(&mut patches);
    template::sanitize::sanitize_unmatched(&mut unmatched);

    let normalized = template_io::normalize_backlog_patch_response(
        file,
        &current_content,
        patches,
        unmatched,
        flags.allow_replace_pending,
    )?;
    if let Some(response_override) = normalized.response_for_capture {
        response = response_override;
    }
    encode_no_pending_capture_intent(file, &mut response, flags.no_pending_capture);
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject tracked-work full-replacement blocks unless allowed.
    template_io::enforce_no_replace_pending(&patches, flags.allow_replace_pending)?;
    enforce_no_destructive_todo_patch(&current_content, &patches)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    if !flags.allow_replace_pending && !template_io::pending_replace_escape_hatch_enabled() {
        agent_doc_template::response_materialization::ensure_template_response_write_proof(
            &patches, &unmatched,
        )?;
    }
    enforce_strict_template_closeout_contract(
        &current_content,
        parsed_marker_count,
        &patches,
        &unmatched,
        flags.strict_closeout,
    )?;
    let pending_flags = super::pending_write_flags(&flags);
    if let Err(err) =
        agent_doc_session_check_io::prewrite_pending_capture_check(file, &response, &pending_flags)
    {
        retain_ipc_patch_for_retry_error(file, baseline, &response, &err, "pending_capture")?;
        return Err(err);
    }
    if let Err(err) =
        agent_doc_session_check_io::prewrite_pending_done_check(file, &response, &pending_flags)
    {
        retain_ipc_patch_for_retry_error(file, baseline, &response, &err, "pending_done")?;
        return Err(err);
    }

    // Capture only after all response-shape and pending-work guards pass. A
    // malformed IPC closeout must not become a durable retry loop.
    agent_doc_repair_io::pending::save_pending_with_current_content_and_plan(
        file,
        &response,
        &current_content,
        flags.mutation_plan_json.as_deref(),
    )?;

    if try_add_response_cell_via_realtime_backbone(file, &patches, &unmatched, false, "run_ipc")? {
        return Ok(());
    }

    anyhow::bail!(
        "no registered Lazily editor endpoint accepted the write for {}; response intent remains retained in state.db",
        file.display()
    );
}

fn retain_ipc_patch_for_retry_error(
    file: &Path,
    _baseline: Option<&str>,
    response: &str,
    err: &anyhow::Error,
    source: &str,
) -> Result<()> {
    let retry_without_disk = error_requests_retry_without_disk(err);
    if !retry_without_disk {
        return Ok(());
    }
    retain_ipc_patch_for_editor_authority_retry(file, None, response).with_context(|| {
        format!(
            "failed to retain IPC retry patch after {source} authority failure for {}",
            file.display()
        )
    })
}

pub(crate) fn error_requests_retry_without_disk(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let message = cause.to_string();
        message.contains("retry_without_disk_write")
            || message.contains("disk is not consulted until the editor is detached")
                || message.contains("disk is not consulted as a fallback")
                || message.contains("disk remained non-authoritative")
                // 0.35.100's zero-member barrier retained the canonical
                // editor target but omitted the common retry marker from its
                // typed error text. Classify that exact fail-closed invariant
                // as retained as well so mixed-generation callers preserve
                // the downstream commit continuation.
                || (message.contains("retained canonical target")
                    && message.contains("zero-member delivery convergence is not visible-write proof")
                    && message.contains("disk was not written"))
            // `#fzmutloss`: any failure that retains its change for retry must
            // also retain the same closeout's backlog/status mutations. A CRDT
            // convergence timeout says exactly that and used to fall through
            // here, so the response landed on retry while its `--done` was
            // silently dropped and the item stayed queued.
            || message.contains(agent_doc_document_realtime_io::RETAINED_FOR_RETRY_MARKER)
    })
}

pub(crate) fn retain_ipc_patch_for_editor_authority_retry(
    file: &Path,
    _baseline: Option<&str>,
    response: &str,
) -> Result<()> {
    let mut retained_response = response.to_string();
    sanitize_template_patchback_response(&mut retained_response)?;
    let parsed = agent_doc_template_io::parse_template_patchback(
        file,
        &retained_response,
        "run_ipc_authority_retry",
        agent_doc_ops_log_io::log_op,
    )?;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;
    template::sanitize::sanitize_patches(&mut patches);
    template::sanitize::sanitize_unmatched(&mut unmatched);
    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    if let Err(err) = agent_doc_repair_io::pending::save_pending(file, &retained_response) {
        eprintln!(
            "[write] warning: failed to save pending response for editor-authority retry: {err}"
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "ipc_retry_pending_save_failed file={} error={} recovery=retain_patch",
                file.display(),
                err
            ),
        );
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "run_ipc_authority_retry_intent_retained file={} patches={} authority=state_db recovery=retry_registered_lazily_endpoint",
            file.display(),
            patches.len()
        ),
    );
    Ok(())
}

fn merge_template_document_model(
    file: &Path,
    base: &str,
    content_ours: &str,
    content_current: &str,
) -> Result<String> {
    let cell_plan = agent_doc_merge::merge(agent_doc_merge::MergeRequest::cell(
        base,
        content_ours,
        content_current,
    ));
    if !cell_plan.fell_back {
        let merged_doc = agent_doc_template::canonicalize_boundary_after_document_merge(
            &cell_plan.merged_doc,
            content_ours,
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "template_document_model_merge file={} engine=cell conflicts={} boundary=canonical_response_branch",
                file.display(),
                cell_plan.conflicts.len()
            ),
        );
        return Ok(merged_doc);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "template_document_model_merge_cell_fallback file={} conflicts={}",
            file.display(),
            cell_plan.conflicts.len()
        ),
    );
    let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
    let (merged_doc, _merged_state) = agent_doc_merge_io::merge_contents_crdt_with_ops(
        file,
        Some(&base_state),
        content_ours,
        content_current,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "template_document_model_merge file={} engine=crdt_op_capture base_len={} ours_len={} current_len={}",
            file.display(),
            base.len(),
            content_ours.len(),
            content_current.len()
        ),
    );
    Ok(agent_doc_template::canonicalize_boundary_after_document_merge(&merged_doc, content_ours))
}

fn editor_crdt_authority_attached(file: &Path) -> bool {
    agent_doc_controller_io::project_controller::reliable_sync_editor_live_for_file(file)
        || agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached()
}

/// Whether the shared CRDT authority says no editor holds `file`.
///
/// Durable reliable-sync liveness covers cold start, while a routed relay model
/// covers the interval after a process-scoped replica registration and before its
/// separately scheduled `Open` frame arrives. Neither signal alone is sufficient
/// to authorize a disk write.
fn write_path_editor_absent(file: &Path) -> bool {
    !editor_crdt_authority_attached(file)
}

fn ensure_force_disk_editor_authority_ready(file: &Path) -> Result<()> {
    // `--force-disk` is the explicit operator escape hatch. The atomic writer
    // retains the pre-write disk bytes plus the target as a Lazily deferred
    // intent before touching disk; a reappearing editor then restores the
    // target or component-merges later unsaved edits. Do not stall the turn on
    // a missing relay member once that durable handoff is available.
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "force_disk_editor_authority_ready file={} status=durable_reconnect_handoff phase=pre_write",
            file.display()
        ),
    );
    Ok(())
}

fn merge_recovery_content(
    file: &Path,
    base: &str,
    content_ours: &str,
    content_current: &str,
    source: &str,
    force_disk: bool,
) -> Result<RecoveryMergeContent> {
    let reconciled_current = if force_disk {
        agent_doc_document_realtime_io::PendingEditorCutReconciliation {
            content: content_current.to_string(),
            replayed_editor_ops: false,
        }
    } else {
        agent_doc_document_realtime_io::reconcile_pending_editor_cut(
            file,
            base,
            content_current,
            source,
        )?
    };
    let content_current = reconciled_current.content.as_str();
    if content_current == base {
        return Ok(RecoveryMergeContent {
            content: content_ours.to_string(),
            replayed_pending_editor_ops: false,
        });
    }

    let content = if content_uses_crdt_write(base) {
        eprintln!(
            "[write] Current document changed since response recovery baseline. Document-model merging..."
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "recovery_document_model_merge file={} source={} recovery=reconcile_document_model",
                file.display(),
                source
            ),
        );
        merge_template_document_model(file, base, content_ours, content_current)
            .with_context(|| format!("document-model merge failed during {source}"))
    } else {
        agent_doc_merge_io::merge_contents(base, content_ours, content_current)
    }?;
    Ok(RecoveryMergeContent {
        content,
        replayed_pending_editor_ops: reconciled_current.replayed_editor_ops,
    })
}

fn save_recovery_snapshot(file: &Path, content: &str, _use_crdt: bool) -> Result<()> {
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        content,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(())
}

fn apply_simple_exchange_patch_to_current(
    file: &Path,
    current: &str,
    patches: &[agent_doc_template::PatchBlock],
    unmatched: &str,
) -> Option<Result<String>> {
    if !unmatched.trim().is_empty() {
        return None;
    }

    let mut exchange_patch: Option<&str> = None;
    let mut frontmatter_patch: Option<&str> = None;
    for patch in patches {
        match patch.name.as_str() {
            "exchange" => {
                if exchange_patch.replace(patch.content.as_str()).is_some() {
                    return None;
                }
            }
            "frontmatter" => {
                if frontmatter_patch.replace(patch.content.as_str()).is_some() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    let exchange_patch = exchange_patch?;

    Some((|| {
        let components =
            agent_doc_element::element::parse(current).context("failed to parse components")?;
        let exchange = components
            .iter()
            .find(|component| component.name == "exchange")
            .ok_or_else(|| anyhow::anyhow!("missing exchange component"))?;
        let exchange_content = exchange.content(current);
        let (exchange_before_boundary, exchange_after_boundary) =
            split_exchange_content_at_boundary(exchange_content).unwrap_or_else(|| {
                (
                    strip_exchange_boundary_lines(exchange_content),
                    String::new(),
                )
            });
        let summary = file.file_stem().and_then(|stem| stem.to_str());
        let boundary_id = agent_doc_element::id::new_boundary_id_with_summary(summary);
        let boundary_marker = agent_doc_element::id::format_boundary_marker(&boundary_id);

        let mut new_exchange = exchange_before_boundary.trim_end().to_string();
        if !new_exchange.is_empty() {
            new_exchange.push_str("\n\n");
        }
        new_exchange.push_str(exchange_patch.trim());
        if !new_exchange.ends_with('\n') {
            new_exchange.push('\n');
        }
        new_exchange.push_str(&boundary_marker);
        new_exchange.push('\n');
        new_exchange.push_str(&exchange_after_boundary);

        eprintln!(
            "[template] force-disk current exchange fast path inserted boundary {}",
            boundary_id
        );

        let mut result = exchange.replace_content(current, &new_exchange);
        if let Some(frontmatter_patch) = frontmatter_patch {
            result = agent_doc_frontmatter::frontmatter::merge_fields(&result, frontmatter_patch)
                .context("failed to merge frontmatter patch")?;
        }
        Ok(result)
    })())
}

fn split_exchange_content_at_boundary(content: &str) -> Option<(String, String)> {
    let mut before = String::with_capacity(content.len());
    let mut after = String::new();
    let mut found_boundary = false;
    for segment in content.split_inclusive('\n') {
        if segment.trim().starts_with("<!-- agent:boundary:") {
            found_boundary = true;
            continue;
        }
        if found_boundary {
            after.push_str(segment);
        } else {
            before.push_str(segment);
        }
    }
    found_boundary.then_some((before, after))
}

fn strip_exchange_boundary_lines(content: &str) -> String {
    let mut stripped = String::with_capacity(content.len());
    for segment in content.split_inclusive('\n') {
        if !segment.trim().starts_with("<!-- agent:boundary:") {
            stripped.push_str(segment);
        }
    }
    if !content.ends_with('\n')
        && let Some(tail) = content.rsplit('\n').next()
        && !tail.trim().starts_with("<!-- agent:boundary:")
        && !stripped.ends_with(tail)
    {
        stripped.push_str(tail);
    }
    stripped
}

/// Apply an append-mode response from a string (not stdin).
/// Used by `repair` to apply orphaned responses.
pub fn apply_append_from_string(file: &Path, response: &str) -> Result<()> {
    let response = agent_doc_turn::response_text::strip_assistant_heading(response);
    let content = resolve_current_document_content(file, "apply_append_from_string")
        .with_context(|| format!("failed to read {}", file.display()))?;
    let use_crdt = content_uses_crdt_write(&content);

    let mut content_ours = content.clone();
    if !content_ours.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("## Assistant\n\n");
    content_ours.push_str(&response);
    if !response.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("\n## User\n\n");

    let content_current =
        resolve_current_document_content(file, "apply_append_from_string_current_content")?;

    let merged = merge_recovery_content(
        file,
        &content,
        &content_ours,
        &content_current,
        "apply_append_from_string",
        false,
    )?;
    let final_content = merged.content;

    guard_visible_write_expected_current_or_target(
        file,
        "apply_append_from_string",
        &content_current,
        Some(&final_content),
    )?;
    agent_doc_document_realtime_io::atomic_write_rebased_through_authority(
        file,
        &content_current,
        &final_content,
        "apply_append_from_string",
    )?;
    // Save snapshot as content_ours, not final_content
    save_recovery_snapshot(file, &content_ours, use_crdt)?;
    clear_replayed_editor_ops_after_write(
        file,
        merged.replayed_pending_editor_ops,
        "apply_append_from_string",
    );
    eprintln!("[write] Response appended to {}", file.display());
    Ok(())
}

/// Apply template-mode patches from a string (not stdin).
/// Used by `repair` to apply orphaned template responses.
#[cfg(test)]
pub(crate) fn apply_template_from_string(file: &Path, response: &str) -> Result<()> {
    apply_template_from_string_with_options(file, response, TemplateApplyOptions::default())
}

pub fn apply_template_from_string_with_options(
    file: &Path,
    response: &str,
    options: TemplateApplyOptions,
) -> Result<()> {
    let content = resolve_document_content_for_write_mode(
        file,
        options.force_disk,
        "apply_template_from_string",
        "apply_template_from_string_force_disk_initial",
    )
    .with_context(|| format!("failed to read {}", file.display()))?;
    let use_crdt = content_uses_crdt_write(&content);
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    let mut response = response.to_string();
    sanitize_template_patchback_response(&mut response)?;

    let parsed = agent_doc_template_io::parse_template_patchback(
        file,
        &response,
        "apply_template_from_string",
        agent_doc_ops_log_io::log_op,
    )?;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    template::sanitize::sanitize_patches(&mut patches);
    template::sanitize::sanitize_unmatched(&mut unmatched);

    let normalized =
        template_io::normalize_backlog_patch_response(file, &content, patches, unmatched, false)?;
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject tracked-work full-replacement blocks unless allowed.
    template_io::enforce_no_replace_pending(&patches, false)?;
    enforce_no_destructive_todo_patch(&content, &patches)?;

    let mode_overrides = template_mode_overrides_for_current_doc(file, None, &content);
    let snapshot_doc = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let content_ours = template_io::apply_patches_with_overrides_with_project_config(
        &content,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
        Some(rc.project_config()),
    )
    .context("failed to apply template patches")?;
    let content_ours =
        normalize_template_structure_or_fail_preserving(&content_ours, file, Some(&content))?;

    let content_current = resolve_document_content_for_write_mode(
        file,
        options.force_disk,
        "apply_template_from_string_current_content",
        "apply_template_from_string_force_disk_current_content",
    )?;

    let (final_content, replayed_pending_editor_ops) = if let Some(repaired_current) =
        adopt_current_response_without_duplication(
            file,
            &content,
            &content_ours,
            &content_current,
            snapshot_doc.as_deref(),
            &response,
        )? {
        eprintln!(
            "[write] response already present in current file; adopting normalized current content"
        );
        (repaired_current, false)
    } else {
        let merged = merge_recovery_content(
            file,
            &content,
            &content_ours,
            &content_current,
            "apply_template_from_string",
            options.force_disk,
        )?;
        (merged.content, merged.replayed_pending_editor_ops)
    };
    let final_content = normalize_final_template_content(
        file,
        &content,
        snapshot_doc.as_deref(),
        Some(&content_current),
        &final_content,
        Some(response.as_str()),
    )?;

    if options.force_disk {
        agent_doc_document_realtime_io::atomic_write_force_disk_through_authority(
            file,
            &final_content,
        )?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "apply_template_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                final_content.len(),
                agent_doc_hash::content_hash(&final_content)
            ),
        );
    } else {
        guard_visible_write_expected_current_or_target(
            file,
            "apply_template_from_string",
            &content_current,
            Some(&final_content),
        )?;
        // `#fcc0`: repair recovery applies template (component) patches through
        // the editor path; if editor convergence is unavailable or unproven,
        // fail closed instead of writing the repaired document straight to disk.
        agent_doc_write_converge_io::try_editor_converge(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            &final_content,
            &content_current,
            "apply_template",
        )?;
    }
    // Save snapshot as the repaired/merged final content.
    save_recovery_snapshot(file, &final_content, use_crdt)?;
    clear_replayed_editor_ops_after_write(
        file,
        replayed_pending_editor_ops,
        "apply_template_from_string",
    );
    eprintln!("[write] Template patches applied to {}", file.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn preflight_visible_prompt_is_the_response_application_base() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("preflight-owned-prompt.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- agent:boundary:base -->\n<!-- /agent:exchange -->\n";
        let current = "<!-- agent:exchange patch=append -->\n❯ fix the retained prompt\n<!-- agent:boundary:base -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, current).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(current)).unwrap();

        let witness = capture_preflight_application_base_witness(&doc, Some(snapshot)).unwrap();
        assert_eq!(
            application_baseline_for_preflight_witness(witness.as_ref(), Some(snapshot), current,),
            Some(current),
            "the prompt visible when preflight opened belongs to this response cycle",
        );

        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "test_capture",
            Some(snapshot),
            Some(current),
            "response-sha",
            None,
        )
        .unwrap();
        assert!(
            capture_preflight_application_base_witness(&doc, Some(snapshot))
                .unwrap()
                .is_none(),
            "a replayed response_captured cycle must not adopt a later visible prompt",
        );
    }

    #[test]
    fn prompt_typed_after_preflight_remains_carry_forward() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("post-preflight-prompt.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- agent:boundary:base -->\n<!-- /agent:exchange -->\n";
        let opening = "<!-- agent:exchange patch=append -->\n❯ answer this cycle\n<!-- agent:boundary:base -->\n<!-- /agent:exchange -->\n";
        let later = "<!-- agent:exchange patch=append -->\n❯ answer this cycle\n❯ next cycle prompt\n<!-- agent:boundary:base -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, opening).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(opening)).unwrap();

        let witness = capture_preflight_application_base_witness(&doc, Some(snapshot)).unwrap();
        assert_eq!(
            application_baseline_for_preflight_witness(witness.as_ref(), Some(snapshot), later,),
            Some(snapshot),
            "text that diverges after preflight must stay outside the response snapshot",
        );
    }

    /// `#fzmutloss`: a CRDT convergence timeout retains its change for retry,
    /// so it must also retain the SAME closeout's backlog/status mutations.
    ///
    /// Operator-reported 2026-07-19 (brookebrodack-dev.md): finalize timed out
    /// at 60s with repeated `compare_and_swap_raced`, the binary resumed and
    /// committed the response — but the `--done` was gone, so the item stayed
    /// `[ ]` and queued despite the response having landed. The timeout error
    /// says "pending change retained for retry" and matched none of the four
    /// disk-authority substrings this classifier keyed off, so
    /// `write_outcome_retains_closeout_mutations` returned false and
    /// `apply_pending_and_status_mutations` was skipped.
    #[test]
    fn crdt_convergence_timeout_retains_closeout_mutations() {
        let timeout = anyhow::anyhow!(
            "finalize: CRDT convergence for /tmp/session.md did not settle within 60000ms \
             (reason=TypingQuiescence); {}",
            agent_doc_document_realtime_io::RETAINED_FOR_RETRY_MARKER,
        );
        assert!(
            error_requests_retry_without_disk(&timeout),
            "a failure that retains its change for retry must retain its mutations too"
        );

        // The marker is the contract, so a wrapped cause still classifies.
        let wrapped = timeout.context("write --commit failed");
        assert!(error_requests_retry_without_disk(&wrapped));

        let zero_replica = anyhow::anyhow!(
            "write: recovery=await_editor_replica_no_disk_write_then_session_check; {}",
            agent_doc_document_realtime_io::RETAINED_FOR_RETRY_MARKER,
        );
        assert!(
            error_requests_retry_without_disk(&zero_replica),
            "a zero-replica retained write must retain its pending-only commit identity",
        );

        let zero_member_035100 = anyhow::anyhow!(
            "pending_write: retained canonical target for /tmp/session.md after its editor replica \
             disappeared (content_hash=abc): zero-member delivery convergence is not \
             visible-write proof; disk was not written; recycle_status=request_skipped"
        );
        assert!(
            error_requests_retry_without_disk(&zero_member_035100),
            "the 0.35.100 zero-member barrier must preserve its requested commit across build skew",
        );

        // A genuine hard failure retains nothing and must stay unclassified,
        // or a real error would silently apply mutations it never earned.
        let hard = anyhow::anyhow!("finalize: refusing to mutate a non-git document");
        assert!(!error_requests_retry_without_disk(&hard));
    }

    /// `#bugbacklogdropped`: an already-retained editor target can block the
    /// first authority read. The mutation plan must be durable before that read
    /// so capture recovery cannot replay only the response body.
    #[test]
    fn tracked_work_mutation_plan_is_captured_before_authority_resolution() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("pre-barrier-plan.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: pre-barrier-plan\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### User Prompt\n\n",
            "Capture the whole closeout.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#existing] Existing item\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, baseline).unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: Capture the whole closeout — gpt-5\n\n",
            "Done.\n",
            "<!-- /patch:exchange -->\n",
        );
        let plan = agent_doc_write_command_io::CapturedCloseoutMutationPlan {
            is_template: true,
            is_stream: true,
            pending_add: vec!["[#new] New retained work".to_string()],
            pending_edit: vec!["existing=Edited retained work".to_string()],
            ..Default::default()
        };
        let plan_json = serde_json::to_string(&plan).unwrap();
        let flags = WriteFlags {
            allow_replace_pending: false,
            has_pending_add: true,
            has_pending_done: false,
            has_pending_mutation: true,
            has_metadata_only_mutation: true,
            pending_done_ids: Vec::new(),
            queue_completion_ids: Vec::new(),
            pending_kept_open_ids: Vec::new(),
            strict_closeout: true,
            force_disk: false,
            no_pending_capture: false,
            mutation_plan_json: Some(plan_json.clone()),
            empty_response_recovery: None,
            rerun_command_base: None,
        };

        capture_validated_stream_closeout_before_authority_resolution(
            &doc,
            Some(baseline),
            false,
            response,
            &flags,
        )
        .unwrap();

        let state = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
            .unwrap()
            .expect("eager capture should create a durable cycle projection");
        let capture_id = state
            .capture_id
            .as_deref()
            .expect("eager capture should identify its response");
        let capture = agent_doc_cycle_state_io::load_projected_captured_response(&doc, capture_id)
            .unwrap()
            .expect("eager capture should retain the response and mutation plan");
        assert_eq!(
            capture.mutation_plan_json.as_deref(),
            Some(plan_json.as_str())
        );
        let recovered: agent_doc_write_command_io::CapturedCloseoutMutationPlan =
            serde_json::from_str(capture.mutation_plan_json.as_deref().unwrap()).unwrap();
        assert_eq!(recovered.pending_add, plan.pending_add);
        assert_eq!(recovered.pending_edit, plan.pending_edit);
        assert!(
            state.had_pending_mutations,
            "recovery must fail closed until the retained mutation envelope lands"
        );
    }

    #[test]
    fn invalid_strict_stream_closeout_does_not_create_captured_cycle() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("invalid-pre-barrier-response.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: invalid-pre-barrier-response\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### User Prompt\n\n",
            "Keep malformed closeout input retryable.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, baseline).unwrap();
        let plan = agent_doc_write_command_io::CapturedCloseoutMutationPlan {
            is_template: true,
            is_stream: true,
            pending_gate: vec!["operatorverifylive".to_string()],
            ..Default::default()
        };
        let flags = WriteFlags {
            allow_replace_pending: false,
            has_pending_add: false,
            has_pending_done: false,
            has_pending_mutation: true,
            has_metadata_only_mutation: true,
            pending_done_ids: Vec::new(),
            queue_completion_ids: Vec::new(),
            pending_kept_open_ids: Vec::new(),
            strict_closeout: true,
            force_disk: false,
            no_pending_capture: false,
            mutation_plan_json: Some(serde_json::to_string(&plan).unwrap()),
            empty_response_recovery: None,
            rerun_command_base: None,
        };

        let err = capture_validated_stream_closeout_before_authority_resolution(
            &doc,
            Some(baseline),
            false,
            "### Re: malformed — gpt-5\n\nNo patch markers.\n",
            &flags,
        )
        .unwrap_err();

        assert!(err.to_string().contains("<!-- patch:exchange -->"));
        assert!(
            agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
                .unwrap()
                .is_none(),
            "shape validation must fail before a durable response_captured cycle exists"
        );
    }

    #[test]
    fn strict_stream_and_ipc_closeout_reject_legacy_markers_before_capture() {
        let current = "---\nagent_doc_mode: template\n---\n";
        let legacy = concat!(
            "<!-- agent:patch:exchange -->\n",
            "### Re: retained — gpt-5\n\nRecovered.\n",
            "<!-- /agent:patch:exchange -->\n",
        );
        let err =
            enforce_strict_template_closeout_contract(current, 0, &[], legacy, true).unwrap_err();
        assert!(
            err.to_string().contains("<!-- patch:exchange -->"),
            "legacy/unmarked payload must fail the common strict contract: {err:#}"
        );

        let canonical =
            template::PatchBlock::new("exchange", "\n### Re: retained — gpt-5\n\nRecovered.\n");
        enforce_strict_template_closeout_contract(current, 2, &[canonical], "", true).unwrap();
    }

    #[test]
    fn response_cell_fast_path_accepts_only_one_exchange_operation() {
        let response =
            template::PatchBlock::new("exchange", "\n### Re: response cell — gpt-5\n\nApplied.\n");
        assert_eq!(
            response_cell_from_patchback(std::slice::from_ref(&response), "").as_deref(),
            Some("### Re: response cell — gpt-5\n\nApplied.")
        );
        assert!(response_cell_from_patchback(std::slice::from_ref(&response), "extra").is_none());
        assert!(
            response_cell_from_patchback(
                &[response, template::PatchBlock::new("status", "done")],
                "",
            )
            .is_none()
        );
        assert!(response_cell_from_patchback(&[], "").is_none());
        let legacy_headingless = template::PatchBlock::new(
            "exchange",
            "Implemented the requested recovery.\n\n- Verification passed.\n",
        );
        assert!(
            response_cell_from_patchback(std::slice::from_ref(&legacy_headingless), "").is_none(),
            "legacy headingless patchbacks must use the semantic component patch path instead of an invalid response cell",
        );
        let legacy_heading_depth =
            template::PatchBlock::new("exchange", "\n## Re: retained — gpt-5\n\nRecovered.\n");
        assert!(
            response_cell_from_patchback(std::slice::from_ref(&legacy_heading_depth), "").is_none(),
            "legacy response heading depths must use compatibility patchback instead of repeatedly failing the canonical response-cell parser",
        );
        let embedded_prompt = template::PatchBlock::new(
            "exchange",
            "\n### Re: retained — gpt-5\n\nRecovered.\n\n❯ next operator prompt\n",
        );
        assert!(
            response_cell_from_patchback(std::slice::from_ref(&embedded_prompt), "").is_none(),
            "a mixed response/prompt patch must not be selected as an assistant-only response cell",
        );
    }

    #[test]
    fn response_cell_projection_proof_tolerates_terminal_head_annotation() {
        let response = "### Re: response cell — gpt-5\n\nApplied.";
        let materialized = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: response cell — gpt-5 (HEAD)\n\n",
            "Applied.\n",
            "<!-- agent:boundary:next -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_cell_materialized_after_projection(
            response,
            materialized
        ));
    }

    #[test]
    fn response_cell_projection_defers_with_retained_authority_and_no_live_replica() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("response-cell-materialization.md");
        let baseline = concat!(
            "---\nagent_doc_session: materialize\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "operator prompt\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, baseline).unwrap();

        let editor_id = "intellij:response-cell-materialization";
        agent_doc_test_support::seed_durable_open_zero_live_replica(&doc, editor_id);
        let canonical = doc.canonicalize().unwrap();
        assert!(
            agent_doc_crdt_relay_io::crdt_authority_for_file(&canonical).editor_attached(),
            "durable reliable-sync authority remains attached while no relay member is live",
        );

        let response = "### Re: operator prompt — gpt-5\n\nDone.";
        let desired = baseline.replace(
            "<!-- agent:boundary:base -->",
            &format!("{response}\n<!-- agent:boundary:next -->"),
        );
        let _owner = agent_doc_document_realtime::write_authority::owner_scope_guard();
        let error = materialize_response_cell_projection(&canonical, &desired).unwrap_err();

        let detail = format!("{error:#}");
        assert!(
            detail.contains("editor_attached_model_missing")
                && detail.contains("disk remained non-authoritative")
                && detail.contains("re-registration"),
            "zero-replica editor ownership should enter binary-owned model recovery \
             without a disk projection: {detail}"
        );
        assert_eq!(
            fs::read_to_string(&canonical).unwrap(),
            baseline,
            "an editor-owned document without a relay member must remain untouched on disk"
        );
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let projection =
            agent_doc_controller_io::project_controller::load_state_backbone_projection(dir.path())
                .unwrap();
        let pending = projection
            .document(&document_hash)
            .and_then(|document| document.document.pending_write.as_ref())
            .expect("the full response-cell target should remain durable in Lazily state");
        assert!(pending.target_content.contains(response));
        assert_eq!(
            pending
                .target_content
                .matches("### Re: operator prompt")
                .count(),
            1
        );
    }

    #[test]
    fn stream_model_merge_preserves_response_and_operator_components() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("efs.md");
        let base = concat!(
            "<!-- agent:status -->\n",
            "complete\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior response.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let ours = concat!(
            "<!-- agent:status -->\n",
            "complete\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior response.\n",
            "\n",
            "### Re: route-owned start recovery — gpt-5\n\n",
            "Recovered through the document model.\n",
            "<!-- agent:boundary:ours -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:status -->\n",
            "route checked\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior response.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- [#sy71]\n",
            "- [#7jj2]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#sy71] lender modal state\n",
            "- [ ] [#7jj2] deploy\n",
            "<!-- /agent:backlog -->\n",
        );

        let merged = merge_template_document_model(&doc, base, ours, current).unwrap();

        assert!(merged.contains("### Re: route-owned start recovery — gpt-5"));
        assert!(merged.contains("Recovered through the document model."));
        assert!(merged.contains("- [#sy71]"));
        assert!(merged.contains("- [#7jj2]"));
        assert!(merged.contains("- [ ] [#sy71] lender modal state"));
        assert!(merged.contains("- [ ] [#7jj2] deploy"));
        assert!(merged.contains("route checked"));
        assert!(
            !merged.contains("cell-merge-conflict"),
            "disjoint component edits should not require conflict markers:\n{merged}"
        );
    }

    #[test]
    fn paused_closeout_merges_response_over_durable_operator_queue_deletion() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("lazily.md");
        let base = concat!(
            "---\nagent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nPrior response.\n",
            "<!-- agent:boundary:base -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#next]\n",
            "- ~~do [#old-a]~~\n",
            "- ~~do [#old-b]~~\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, base).unwrap();
        let deleted = "- ~~do [#old-a]~~\n- ~~do [#old-b]~~\n";
        let offset = base.find(deleted).unwrap();
        agent_doc_op_capture_io::record_editor_op(
            &doc,
            &agent_doc_hash::content_hash(base),
            agent_doc_merge::crdt::EditorOp::Delete {
                offset,
                len: deleted.len(),
            },
        )
        .unwrap();
        let ours = base.replacen(
            "<!-- agent:boundary:base -->",
            "### Re: resumed — gpt-5\n\nCapacity pause recovered.\n<!-- agent:boundary:base -->",
            1,
        );

        let merged =
            merge_recovery_content(&doc, base, &ours, base, "paused_closeout", false).unwrap();

        assert!(merged.replayed_pending_editor_ops);
        assert!(merged.content.contains("Capacity pause recovered."));
        assert!(merged.content.contains("- do [#next]"));
        assert!(!merged.content.contains("#old-a"));
        assert!(!merged.content.contains("#old-b"));
        assert!(
            agent_doc_op_capture_io::has_pending_editor_ops(&doc),
            "a merge alone must not clear deletion evidence before write success"
        );
        clear_replayed_editor_ops_after_write(
            &doc,
            merged.replayed_pending_editor_ops,
            "paused_closeout",
        );
        assert!(!agent_doc_op_capture_io::has_pending_editor_ops(&doc));
    }

    #[test]
    fn apply_template_from_string_compact_exchange_replaces_exchange_body() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n",
        );
        fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = "<!-- patch:exchange -->\nCompacted summary.\n<!-- /patch:exchange -->\n";
        apply_template_from_string_with_options(
            &doc,
            response,
            TemplateApplyOptions { force_disk: true },
        )
        .unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Compacted summary.\n"));
        assert!(!result.contains("Old progress."));
        assert!(!result.contains("compact exchange"));
    }
    #[test]
    fn apply_template_from_string_same_base_retry_adopts_existing_response() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: closeout follow-up — gpt-5\n\n",
            "The `spec-test-build-install-commit-push` request is complete in the response above.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: closeout follow-up — gpt-5\n\n",
            "The `spec-test-build-install-commit-push` request is complete in the response above.\n",
            "do #duppb. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: closeout follow-up — gpt-5\n\n",
            "The `spec-test-build-install-commit-push` request is complete in the response above.\n",
            "<!-- /patch:exchange -->\n",
        );
        apply_template_from_string_with_options(
            &doc,
            response,
            TemplateApplyOptions { force_disk: true },
        )
        .unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            result.matches("### Re: closeout follow-up — gpt-5").count(),
            1,
            "same-base retry must not append a duplicate response block"
        );
        assert!(result.contains("❯ do #duppb. spec-test-build-install-commit-push"));
        assert!(!result.contains("\ndo #duppb. spec-test-build-install-commit-push\n"));
    }

    #[test]
    fn recovery_merge_uses_document_model_for_crdt_documents_without_diff3_markers() {
        let dir = TempDir::new().unwrap();
        for subdir in ["crdt", "logs", "snapshots"] {
            fs::create_dir_all(dir.path().join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please reply\n",
            "<!-- agent:boundary:ba5e1234 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let content_ours = base.replace(
            "<!-- agent:boundary:ba5e1234 -->",
            "### Re: recovery crdt - gpt-5\n\nDone.\n<!-- agent:boundary:ba5e1234 -->",
        );
        let content_current = base.replace(
            "<!-- agent:boundary:ba5e1234 -->",
            "while I was typing\n<!-- agent:boundary:ba5e1234 -->",
        );
        fs::write(&doc, content_current.as_str()).unwrap();

        let merged = merge_recovery_content(
            &doc,
            base,
            &content_ours,
            &content_current,
            "test_recovery",
            false,
        )
        .unwrap();

        assert!(merged.content.contains("### Re: recovery crdt - gpt-5"));
        assert!(merged.content.contains("while I was typing"));
        assert!(
            !merged.content.contains("<<<<<<<") && !merged.content.contains(">>>>>>>"),
            "document-model recovery merge must not emit diff3 markers:\n{}",
            merged.content,
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("recovery_document_model_merge"));
        assert!(ops.contains("recovery=reconcile_document_model"));
    }

    #[test]
    fn apply_template_from_string_strips_safe_progress_before_exchange_patch() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #rspdigest. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = concat!(
            "I am checking the write path and existing replay guard before editing.\n",
            "The fix is small; next I will run the targeted regression.\n\n",
            "<!-- patch:exchange -->\n",
            "### Re: rspdigest — gpt-5\n\n",
            "Implemented and verified.\n",
            "<!-- /patch:exchange -->\n",
        );

        apply_template_from_string_with_options(
            &doc,
            response,
            TemplateApplyOptions { force_disk: true },
        )
        .unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("### Re: rspdigest — gpt-5"));
        assert!(result.contains("Implemented and verified."));
        assert!(!result.contains("I am checking the write path"));
        assert!(!result.contains("The fix is small"));
    }
    #[test]
    fn apply_template_from_string_rejects_trailing_unmatched_patchback_text() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #rspdigest. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: rspdigest — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /patch:exchange -->\n",
            "extra transcript text\n",
        );

        let err = apply_template_from_string(&doc, response).unwrap_err();

        assert!(
            err.to_string().contains("unsafe unmatched content"),
            "trailing unmatched patchback text must fail closed, got: {err:#}"
        );
    }
    #[test]
    fn apply_template_from_string_rejects_raw_component_form_without_mutating() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #churn. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Operator pipes the raw template form (component markers) instead of
        // `<!-- patch:exchange -->` blocks — this is the shape that previously
        // committed escaped directives into the live exchange.
        let raw_template_form = concat!(
            "<!-- agent:status -->\nWork complete.\n<!-- /agent:status -->\n\n",
            "<!-- agent:exchange -->\n### Re: churn — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n",
        );
        let err = apply_template_from_string(&doc, raw_template_form).unwrap_err();
        assert!(
            err.to_string().contains("escaped template patchback"),
            "raw component-form stdin must fail closed, got: {err:#}"
        );

        // The document must be untouched — no escaped markers committed.
        let after = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            after, content,
            "rejected patchback must not mutate the document"
        );
        assert!(!after.contains("### Re: churn"));
    }

    #[test]
    fn write_path_reliable_sync_authority_controls_ipc_skip() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("zero-live.md");
        fs::write(&doc, "# Doc\n\nbody\n").unwrap();
        let owner = "test-write-path-zero-live";
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let pid = std::process::id().into();
        let first_tag = format!("{owner}:open-1");
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash: document_hash.clone(),
                pid,
                tag: first_tag.clone(),
            }]);
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, owner)
            .unwrap()
            .expect("hub allocated with a registered editor replica");
        agent_doc_crdt_relay_io::record_committed_baseline_for_file(&doc);
        assert!(
            agent_doc_crdt_relay_io::deregister_replica_for_file(&doc, owner).unwrap(),
            "deregister leaves zero live editors"
        );

        assert!(
            !write_path_editor_absent(&doc),
            "zero relay members must not demote a durably open editor"
        );

        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Close {
                document_hash: document_hash.clone(),
                pid,
                observed_tags: vec![first_tag],
            }]);
        assert!(
            write_path_editor_absent(&doc),
            "an authoritative reliable-sync Close permits the IPC skip"
        );

        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid,
                tag: format!("{owner}:open-2"),
            }]);
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, owner)
            .unwrap()
            .expect("re-register an editor replica");
        assert!(
            !write_path_editor_absent(&doc),
            "a reliable-sync reopen must cancel the IPC skip"
        );
    }

    #[test]
    fn write_path_never_uses_disk_during_registered_replica_liveness_gap() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("registered-before-open.md");
        fs::write(
            &doc,
            "---\nagent_doc_session: registered-before-open\n---\nbody\n",
        )
        .unwrap();
        let canonical = doc.canonicalize().unwrap();
        let pid = std::process::id();
        let identity = format!("jetbrains-{pid}-write-authority-gap");

        assert!(
            !agent_doc_controller_io::project_controller::reliable_sync_editor_live_for_file(
                &canonical,
            ),
            "the separately scheduled reliable-sync Open must be absent"
        );
        agent_doc_crdt_relay_io::register_editor_replica_for_file_incremental(
            &canonical, &identity, None, pid,
        )
        .unwrap()
        .expect("process-scoped editor registration");

        assert!(
            !write_path_editor_absent(&canonical),
            "an allocated routed editor model must block the disk-detached write path"
        );

        agent_doc_crdt_relay_io::deregister_editor_replica_for_file(&canonical, &identity, pid)
            .unwrap();
    }
}
