//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_document::write_normalization::{
    cleanup_resolved_backlog_prompts_after_response, strip_boundary_for_dedup,
};
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
    enforce_imperative_response_contract, lift_pending_from_exchange_safe,
    normalize_template_structure_or_fail_preserving, normalize_user_prompts_in_exchange_safe,
    template_mode_overrides_for_current_doc,
};

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
        && agent_doc_template::response_materialization::response_text_has_heading(response))
    .then(|| response.to_string())
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

    let current_content = resolve_document_content_for_write_mode(
        file,
        flags.force_disk,
        "write_append_current_content",
        "write_append_force_disk_current_content",
    )
    .with_context(|| format!("failed to read {}", file.display()))?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;

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
    let (doc_lock, content_at_start) = capture_locked_undo_checkpoint(file)?;

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
        drop(doc_lock);
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

    drop(doc_lock);

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

    let current_content = resolve_document_content_for_write_mode(
        file,
        flags.force_disk,
        "write_template_current_content",
        "write_template_force_disk_current_content",
    )
    .with_context(|| format!("failed to read {}", file.display()))?;
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
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;
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
    if flags.strict_closeout {
        let template_mode = frontmatter::parse(&current_content)
            .map(|(fm, _)| fm.resolve_mode().is_template())
            .unwrap_or(false);
        agent_doc_template::response_materialization::ensure_strict_template_patch_markers(
            template_mode,
            parsed_marker_count,
            &patches,
            &unmatched,
        )?;
        agent_doc_template::response_materialization::ensure_strict_template_response_heading_for_current_doc(
            &current_content,
            &patches,
            &unmatched,
        )?;
    }
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
    let (doc_lock, content_at_start) = capture_locked_undo_checkpoint(file)?;

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
        let mut final_content = normalize_final_template_content(
            file,
            base,
            snapshot_doc.as_deref(),
            Some(content_current),
            &final_content,
            Some(&response),
        )?;
        // Prompt cleanup is scoped to the operator cut observed before this
        // turn started mutating the document. Granular agent-authored backlog
        // changes may already be visible in `content_current`; treating those
        // as operator prompts resurrects/deletes the wrong queue intent.
        let cleaned_resolved_backlog_prompts = cleanup_resolved_backlog_prompts_after_response(
            base,
            &current_content,
            &final_content,
        )?;
        let cleaned_applied = cleaned_resolved_backlog_prompts.is_some();
        if let Some(cleaned) = cleaned_resolved_backlog_prompts {
            log_resolved_backlog_prompt_cleanup(file, cleaned.removed);
            final_content = normalize_template_structure_or_fail_preserving(
                &cleaned.content,
                file,
                Some(content_current),
            )?;
        }
        Ok((final_content, cleaned_applied))
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
        drop(doc_lock);
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

    drop(doc_lock);

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

    let current_content = resolve_document_content_for_write_mode(
        file,
        force_disk,
        "write_stream_current_content",
        "write_stream_force_disk_current_content",
    )
    .with_context(|| format!("failed to read {}", file.display()))?;
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
    sanitize_template_patchback_response(&mut response)?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;
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
    if flags.strict_closeout {
        agent_doc_template::response_materialization::ensure_strict_template_response_heading_for_current_doc(
            &current_content,
            &patches,
            &unmatched,
        )?;
    }
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

    let (doc_lock, content_at_start) = capture_locked_undo_checkpoint(file)?;

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
                drop(doc_lock);
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
                drop(doc_lock);
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
                        "ipc_write_consumed file={} patches={}",
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
                drop(doc_lock);
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
    let recompute_final = |content_current: &str| -> Result<(String, Vec<u8>, bool)> {
        let (final_content, mut crdt_state, skip_final_normalize) = if let Some(repaired_current) =
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
            let doc = agent_doc_merge::crdt::CrdtDoc::from_text(&repaired_current);
            (
                repaired_current,
                doc.encode_state(),
                force_disk_editor_attached,
            )
        } else if content_current == base {
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
                    content_current.len(),
                    base.len()
                ),
            );
            let patched_current = match apply_simple_exchange_patch_to_current(
                file,
                content_current,
                &patches,
                &unmatched,
            ) {
                Some(result) => result?,
                None => build_patched_content(content_current, content_current, false, false)?,
            };
            agent_doc_element_exchange_io::check_exchange_shrink_guard_with_log(
                content_current,
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
                content_current,
                &patches,
                &unmatched,
            ) {
                Some(result) => result?,
                None => merge_template_document_model(file, base, &content_ours, content_current)?,
            };
            let doc = agent_doc_merge::crdt::CrdtDoc::from_text(&rebased);
            (rebased, doc.encode_state(), false)
        };
        let mut final_content = if skip_final_normalize {
            final_content
        } else {
            normalize_final_template_content(
                file,
                base,
                snapshot_doc.as_deref(),
                Some(content_current),
                &final_content,
                Some(&response),
            )?
        };
        let cleaned_resolved_backlog_prompts = if skip_final_normalize {
            None
        } else {
            cleanup_resolved_backlog_prompts_after_response(base, &current_content, &final_content)?
        };
        let cleaned_applied = cleaned_resolved_backlog_prompts.is_some();
        if let Some(cleaned) = cleaned_resolved_backlog_prompts {
            log_resolved_backlog_prompt_cleanup(file, cleaned.removed);
            final_content = normalize_template_structure_or_fail_preserving(
                &cleaned.content,
                file,
                Some(content_current),
            )?;
            crdt_state = agent_doc_merge::crdt::CrdtDoc::from_text(&final_content).encode_state();
        }
        Ok((final_content, crdt_state, cleaned_applied))
    };

    let initial_payload = recompute_final(&content_current)?;

    // Reconcile the visible-write guard with the CRDT merge: re-merge the
    // captured response against a foreign disk append landed after the merge
    // was computed instead of failing closed and stranding the response
    // outside HEAD (#ipc-drift-visbuf-reconcile).
    let (content_current, (final_content, _crdt_state, cleaned_resolved_backlog_prompts_applied)) =
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
                        "run_stream",
                        expected,
                        Some(&payload.0),
                    )
                },
                recompute_final,
                |f, current, payload| {
                    guard_visible_write_expected_current_or_target(
                        f,
                        "run_stream",
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
            "write_stream_dedup",
            Some(&content_current),
            Some(&content_current),
        );
        drop(doc_lock);
        agent_doc_repair_io::pending::clear_pending(file)?;
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
            &content_current,
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
            &content_current,
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
        &content_current,
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

    drop(doc_lock);

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
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;

    // Parse and validate patchback shape before any visible document mutation.
    let parsed = agent_doc_template_io::parse_template_patchback(
        file,
        &response,
        "run_ipc",
        agent_doc_ops_log_io::log_op,
    )?;
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

    // Save response to pending store (survives context compaction)
    agent_doc_repair_io::pending::save_pending_with_current_content_and_plan(
        file,
        &response,
        &current_content,
        flags.mutation_plan_json.as_deref(),
    )?;

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
    if flags.strict_closeout {
        agent_doc_template::response_materialization::ensure_strict_template_response_heading_for_current_doc(
            &current_content,
            &patches,
            &unmatched,
        )?;
    }
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
    agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached()
}

/// Whether the durable reliable-sync authority says no editor holds `file`.
/// Relay member count cannot answer this during a transient reattach gap.
fn write_path_editor_absent(file: &Path) -> bool {
    !agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file)
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
    _force_disk: bool,
) -> Result<String> {
    if content_current == base {
        return Ok(content_ours.to_string());
    }

    if content_uses_crdt_write(base) {
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
    }
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

    let doc_lock = acquire_doc_lock(file)?;

    let content_current =
        resolve_current_document_content(file, "apply_append_from_string_current_content")?;

    let final_content = merge_recovery_content(
        file,
        &content,
        &content_ours,
        &content_current,
        "apply_append_from_string",
        false,
    )?;

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
    drop(doc_lock);
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

    let doc_lock = acquire_doc_lock(file)?;

    let content_current = resolve_document_content_for_write_mode(
        file,
        options.force_disk,
        "apply_template_from_string_current_content",
        "apply_template_from_string_force_disk_current_content",
    )?;

    let final_content = if let Some(repaired_current) = adopt_current_response_without_duplication(
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
        repaired_current
    } else {
        merge_recovery_content(
            file,
            &content,
            &content_ours,
            &content_current,
            "apply_template_from_string",
            options.force_disk,
        )?
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
    drop(doc_lock);
    eprintln!("[write] Template patches applied to {}", file.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

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

        // A genuine hard failure retains nothing and must stay unclassified,
        // or a real error would silently apply mutations it never earned.
        let hard = anyhow::anyhow!("finalize: refusing to mutate a non-git document");
        assert!(!error_requests_retry_without_disk(&hard));
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
        agent_doc_test_support::publish_editor_text_via_crdt_relay(&doc, editor_id, baseline);
        let canonical = doc.canonicalize().unwrap();
        let identity = format!("{editor_id}:{}", canonical.display());
        assert!(
            agent_doc_crdt_relay_io::deregister_replica_for_file(&canonical, &identity).unwrap(),
            "fixture should retain the CRDT model after the live replica leaves",
        );
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

        assert!(
            error
                .to_string()
                .contains("no editor replica was registered"),
            "zero-replica editor ownership should defer without a disk projection: {error:#}"
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

        assert!(merged.contains("### Re: recovery crdt - gpt-5"));
        assert!(merged.contains("while I was typing"));
        assert!(
            !merged.contains("<<<<<<<") && !merged.contains(">>>>>>>"),
            "document-model recovery merge must not emit diff3 markers:\n{merged}"
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
}
