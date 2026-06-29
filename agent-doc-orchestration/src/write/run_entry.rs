//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

/// Run the write command: append assistant response to document.
///
/// `baseline` is the document content at the time the response was generated.
/// If omitted, the current document content is used (no merge needed).
pub fn run(file: &Path, baseline: Option<&str>, flags: WriteFlags) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;

    let response = read_response_input()?;

    if response.trim().is_empty() {
        if recover_empty_response_for_strict_closeout(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!("empty response — nothing to write");
    }

    let mut current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;

    // Strip leading "## Assistant" heading if present — the write command adds its own
    let response = strip_assistant_heading(&response);
    prewrite_pending_capture_check(file, &response, &flags)?;
    auto_apply_pending_done_if_enabled(file, &response, &flags, &mut current_content)?;
    prewrite_pending_done_check(file, &response, &flags)?;

    // Save response to pending store (survives context compaction)
    repair::save_pending(file, &response)?;

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let (doc_lock, content_at_start) = capture_locked_pre_response(file)?;

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

    // Re-read file to check for user edits since lock acquisition. #rtwwire rung
    // 3b: source the merge "theirs" from the realtime model (newest of disk vs the
    // editor's unsaved buffer) so the 3-way merge incorporates a queue/exchange
    // edit that lives only in the unsaved buffer instead of clobbering it
    // (#queue-user-edit-overwrite). Staleness-gated (`#rtwfeed`) — the buffer wins
    // only when it provably holds unsaved edits ahead of disk; no editor attached
    // returns disk unchanged.
    let disk_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;
    let content_current = crate::realtime_model::resolve_current_doc(file, &disk_current).content;

    let final_content = if content_current == base {
        // No edits — use our version directly
        content_ours.clone()
    } else {
        eprintln!("[write] File was modified during response generation. Merging...");
        merge::merge_contents(base, &content_ours, &content_current)?
    };

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        let _ = crate::cycle_state::mark_write_applied(
            file,
            "write_inline_dedup",
            Some(&content_current),
            Some(&content_current),
        );
        drop(doc_lock);
        repair::clear_pending(file)?;
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
    guard_visible_write_idle_and_current(file, "write_inline", &content_current)?;
    snapshot::save(file, snapshot_content)?;

    atomic_write(file, &final_content)?;

    crate::ops_log::log_cycle(
        file,
        "write_inline",
        Some(&content_ours),
        Some(&final_content),
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "write_inline_done file={} snap_len={}",
            file.display(),
            final_content.len()
        ),
    );
    if let Err(e) = crate::cycle_state::mark_write_applied(
        file,
        "write_inline",
        Some(&final_content),
        Some(&final_content),
    ) {
        eprintln!("[write] cycle-state update failed: {} (non-fatal)", e);
    }
    // #22a8: mirror the live pipeline phase into the document frontmatter now the
    // response is fully on disk (doc lock still held, so no writer races).
    if let Ok(Some(st)) = crate::cycle_state::load(file) {
        crate::cycle_state::mirror_pipeline_frontmatter(file, &st);
    }

    drop(doc_lock);

    // Clear pending response after successful write
    repair::clear_pending(file)?;

    eprintln!("[write] Response appended to {}", file.display());
    Ok(())
}

/// Run the template write command: parse patch blocks and apply to components.
///
/// `baseline` is the document content at the time the response was generated.
pub fn run_template(
    file: &Path,
    baseline: Option<&str>,
    origin: Option<&str>,
    flags: WriteFlags,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;
    let rc = crate::graph::RunContext::new(file.to_path_buf());

    let mut response = read_response_input()?;

    if response.trim().is_empty() {
        if recover_empty_response_for_strict_closeout(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!("empty response — nothing to write");
    }

    let mut current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let snapshot_doc = snapshot::load(file).ok().flatten();
    guard_no_stale_snapshot_reset_drift(
        file,
        snapshot_doc.as_deref(),
        &current_content,
        "template write",
    )?;
    sanitize_template_patchback_response_for_write(&mut response)?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &current_content);

    // Parse and validate patchback shape before any visible document mutation.
    let parsed =
        crate::flow::document_mutation::parse_template_patchback(file, &response, "run_template")?;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    sanitize_patches(&mut patches);
    sanitize_unmatched(&mut unmatched);

    let normalized = normalize_backlog_patch_response(
        file,
        &current_content,
        patches,
        unmatched,
        flags.allow_replace_pending,
    )?;
    if let Some(response_override) = normalized.response_for_capture {
        response = response_override;
    }
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject replace:pending (and deprecated patch:pending) blocks unless allowed.
    enforce_no_replace_pending(&patches, flags.allow_replace_pending)?;
    enforce_no_destructive_todo_patch(&current_content, &patches)?;
    enforce_orchestrate_template_patch_contract(origin, &patches, &unmatched)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    if !flags.allow_replace_pending && !pending_replace_escape_hatch_enabled() {
        ensure_template_response_write_proof(&patches, &unmatched)?;
    }
    if flags.strict_closeout {
        ensure_strict_template_response_heading_for_current_doc(
            &current_content,
            &patches,
            &unmatched,
        )?;
    }
    prewrite_pending_capture_check(file, &response, &flags)?;
    auto_apply_pending_done_if_enabled(file, &response, &flags, &mut current_content)?;
    prewrite_pending_done_check(file, &response, &flags)?;

    // Save response to pending store (survives context compaction)
    repair::save_pending(file, &response)?;

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let (doc_lock, content_at_start) = capture_locked_pre_response(file)?;

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
    let snapshot_doc = snapshot::load(file).ok().flatten();

    // Apply patches to baseline
    let content_ours = template::apply_patches_with_overrides_with_context(
        base,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
        Some(&rc),
    )
    .context("failed to apply template patches")?;
    let content_ours =
        normalize_template_structure_or_fail_preserving(&content_ours, file, Some(base))?;

    // Re-read file to check for user edits since lock acquisition
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

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
            eprintln!("[write] File was modified during response generation. Merging...");
            merge::merge_contents(base, &content_ours, content_current)?
        };
        let mut final_content = normalize_final_template_content(
            file,
            base,
            snapshot_doc.as_deref(),
            Some(content_current),
            &final_content,
            Some(&response),
        )?;
        let cleaned_resolved_backlog_prompts = cleanup_resolved_backlog_prompts_after_response(
            file,
            base,
            content_current,
            &final_content,
        )?;
        let cleaned_applied = cleaned_resolved_backlog_prompts.is_some();
        if let Some(cleaned) = cleaned_resolved_backlog_prompts {
            final_content = normalize_template_structure_or_fail_preserving(
                &cleaned,
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
        reconcile_visible_write(
            file,
            content_current,
            initial_payload,
            VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS,
            |f, expected| guard_visible_write_reconcile(f, "run_template", expected),
            recompute_final,
            |f, current| guard_visible_write_idle_and_current(f, "run_template", current),
        )?;

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        let _ = crate::cycle_state::mark_write_applied(
            file,
            "write_template_dedup",
            Some(&content_current),
            Some(&content_current),
        );
        drop(doc_lock);
        repair::clear_pending(file)?;
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
    snapshot::save(file, snapshot_content)?;

    // `#fcc0`: template (non-CRDT) mode must converge through the editor path;
    // if editor convergence is unavailable or unproven, fail closed instead of
    // writing the merged document straight to disk.
    try_editor_converge(file, &final_content, &content_current, "write_template")?;

    crate::ops_log::log_cycle(
        file,
        "write_template",
        Some(&content_ours),
        Some(&final_content),
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "write_template_done file={} snap_len={} patches={}",
            file.display(),
            final_content.len(),
            patches.len()
        ),
    );
    if let Err(e) = crate::cycle_state::mark_write_applied(
        file,
        "write_template",
        Some(&final_content),
        Some(&final_content),
    ) {
        eprintln!("[write] cycle-state update failed: {} (non-fatal)", e);
    }
    // #22a8: mirror the live pipeline phase into the document frontmatter now the
    // response is fully on disk (doc lock still held, so no writer races).
    if let Ok(Some(st)) = crate::cycle_state::load(file) {
        crate::cycle_state::mirror_pipeline_frontmatter(file, &st);
    }

    drop(doc_lock);

    // Clear pending response after successful write
    repair::clear_pending(file)?;

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
/// When `force_disk` is false and `.agent-doc/patches/` exists (plugin installed),
/// tries IPC first. On IPC timeout or missing proof, retains the pending response
/// and fails closed for retry instead of writing the document behind the editor.
/// When `force_disk` is true, always uses direct disk write.
pub fn run_stream(
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
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    // #jb-tsift-pane-sync diagnostic: capture a streamed write/commit to `file`
    // executing inside a tmux pane that owns a different document.
    crate::sync::log_cross_document_execution_context(file, "stream");

    let mut response = read_response_input()?;

    if response.trim().is_empty() {
        if recover_empty_response_for_strict_closeout(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!("empty response — nothing to write");
    }

    let mut current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let mut snapshot_doc = snapshot::load(file).ok().flatten();
    if guard_no_stale_snapshot_reset_drift(
        file,
        snapshot_doc.as_deref(),
        &current_content,
        "stream write",
    )? {
        snapshot_doc = snapshot::load(file).ok().flatten();
    }
    sanitize_template_patchback_response_for_write(&mut response)?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &current_content);

    // Lint: warn if response contains future-work signals without --pending-add
    check_future_work_signals(&response, flags.has_pending_add);

    // Parse and validate patchback shape before any visible document mutation.
    let parsed =
        crate::flow::document_mutation::parse_template_patchback(file, &response, "run_stream")?;
    let parsed_marker_count = parsed.marker_count;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    sanitize_patches(&mut patches);
    sanitize_unmatched(&mut unmatched);

    let normalized = normalize_backlog_patch_response(
        file,
        &current_content,
        patches,
        unmatched,
        flags.allow_replace_pending,
    )?;
    if let Some(response_override) = normalized.response_for_capture {
        response = response_override;
    }
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject replace:pending (and deprecated patch:pending) blocks unless allowed.
    enforce_no_replace_pending(&patches, flags.allow_replace_pending)?;
    enforce_no_destructive_todo_patch(&current_content, &patches)?;
    enforce_orchestrate_template_patch_contract(origin, &patches, &unmatched)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    if !flags.allow_replace_pending && !pending_replace_escape_hatch_enabled() {
        ensure_template_response_write_proof(&patches, &unmatched)?;
    }
    if flags.strict_closeout {
        ensure_strict_template_response_heading_for_current_doc(
            &current_content,
            &patches,
            &unmatched,
        )?;
    }
    prewrite_pending_capture_check(file, &response, &flags)?;
    auto_apply_pending_done_if_enabled(file, &response, &flags, &mut current_content)?;
    prewrite_pending_done_check(file, &response, &flags)?;

    reject_marker_response_with_zero_patches(parsed_marker_count, patches.len())?;

    if patches.is_empty() {
        eprintln!(
            "[write] WARNING: 0 template patches found for {} — response may be missing or malformed. \
             Only normalization/boundary changes will be applied.",
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "zero_patches_warning file={} source=run_stream markers=0 response may be empty or malformed",
                file.display()
            ),
        );
    }

    // Save response to pending store (survives context compaction)
    repair::save_pending(file, &response)?;

    // Warn when patches target a file with no template components
    if patches.is_empty() && !unmatched.trim().is_empty() {
        let current = std::fs::read_to_string(file)
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

    let (doc_lock, content_at_start) = capture_locked_pre_response(file)?;

    // Try IPC when plugin is installed and --force-disk is not set
    if !force_disk {
        let canonical = file.canonicalize()?;
        let project_root = resolve_ipc_project_root(&canonical);
        let patches_dir = project_root.join(".agent-doc/patches");

        // `#ipc-degraded-prefers-file-ipc`: always route through `try_ipc` when
        // the plugin is installed, even if the socket is latched degraded.
        // `try_ipc` skips the wedged socket internally and prefers the file-IPC
        // patch queue (plugin applies via Document API) so a degraded stream
        // write never manufactures a raw-disk File Cache Conflict; the disk
        // unproven IPC attempt below fails closed instead of writing behind the editor.
        if patches_dir.exists() {
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
            let mut content_ours = template::apply_patches_with_overrides_with_context(
                base,
                &patches,
                &unmatched,
                file,
                &mode_overrides,
                Some(&rc),
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
            // --baseline-file. Streaming checkpoints intentionally use the original
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
                && let Ok(Some(current_snap)) = snapshot::load(file)
                && is_stale_baseline(base, &current_snap)
            {
                eprintln!(
                    "[write] WARNING: baseline missing snapshot content — stale baseline detected, using current file as baseline"
                );
                crate::ops_log::log_op(
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
                content_ours = template::apply_patches_with_overrides_with_context(
                    &content_at_start,
                    &patches,
                    &unmatched,
                    file,
                    &mode_overrides,
                    Some(&rc),
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
            check_exchange_shrink_guard(&content_at_start, &content_ours, file)?;

            // Dedup: skip IPC if patches produce no changes (strip boundary markers)
            if strip_boundary_for_dedup(&content_ours)
                == strip_boundary_for_dedup(&content_at_start)
            {
                log_dedup(file, "no changes after merge, skipping write");
                drop(doc_lock);
                repair::clear_pending(file)?;
                return Ok(());
            }

            // Plugin is installed — try IPC
            let t_ipc = std::time::Instant::now();
            let norm_lines_opt = if normalize_prefix_lines.is_empty() {
                None
            } else {
                Some(normalize_prefix_lines.as_slice())
            };
            let ipc_result = try_ipc(
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
                repair::clear_pending(file)?;
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
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_write_consumed file={} patches={}",
                        file.display(),
                        patches.len()
                    ),
                );
                // Fire post_write hook for cross-session coordination
                let session_id = frontmatter::read_session_id(file).unwrap_or_default();
                crate::hooks::fire_post_write(file, &session_id, patches.len());
                crate::hooks::fire_doc_event(file, "post_write");
                drop(doc_lock);
                repair::clear_pending(file)?;
                return Ok(());
            }
            eprintln!(
                "[write] editor IPC did not prove the write — refusing direct document write; retry after the editor applies the queued patch"
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "run_stream_ipc_retry_required_no_disk_write file={} patch_id={} patches={} recovery=retry_without_disk_write",
                    file.display(),
                    ipc_result.patch_id,
                    patches.len()
                ),
            );
            drop(doc_lock);
            anyhow::bail!(
                "editor IPC did not prove the write for {}; pending response retained for retry; refusing direct document write",
                file.display()
            );
        }
    }

    // No plugin installed or --force-disk — direct disk write
    // When --force-disk is set, clean up any pending IPC patch files to prevent
    // the plugin from applying them later (which would cause double-write).
    if force_disk && let Ok(canonical) = file.canonicalize() {
        let project_root = resolve_ipc_project_root(&canonical);
        let patches_dir = project_root.join(".agent-doc/patches");
        if let Ok(hash) = snapshot::doc_hash(file) {
            let patch_file = patches_dir.join(format!("{}.json", hash));
            if patch_file.exists() {
                eprintln!("[write] cleaning stale IPC patch file to prevent double-write");
                // Read patch_id from stale patch before deleting — write sentinel so plugin skips apply
                if let Ok(stale_content) = std::fs::read_to_string(&patch_file)
                    && let Ok(stale_json) =
                        serde_json::from_str::<serde_json::Value>(&stale_content)
                    && let Some(patch_id) = stale_json.get("patch_id").and_then(|v| v.as_str())
                {
                    write_claimed_patch_sentinel(&project_root, patch_id);
                }
                let _ = std::fs::remove_file(&patch_file);
            }
        }
    }
    let t_disk = std::time::Instant::now();

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

    // Apply patches using the mode resolution chain:
    // inline attr (patch=append on tag) > config.toml ([components] section) > built-in default.
    // The skill sends delta content for append-mode components.
    let t_apply2 = std::time::Instant::now();
    let mut content_ours = template::apply_patches_with_overrides_with_context(
        base,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
        Some(&rc),
    )
    .context("failed to apply template patches")?;
    let elapsed_apply2 = t_apply2.elapsed().as_millis();
    if elapsed_apply2 > 0 {
        eprintln!(
            "[perf] apply_patches_with_overrides (disk): {}ms",
            elapsed_apply2
        );
    }

    // Apply frontmatter patch if present (fixes #16 — disk write path was missing this)
    if let Some(fm_patch) = patches.iter().find(|p| p.name == "frontmatter") {
        content_ours = crate::frontmatter::merge_fields(&content_ours, &fm_patch.content)
            .context("failed to merge frontmatter patch")?;
    }

    // Normalize user input in exchange: add ❯  prefix to user-added lines.
    // Load snapshot to identify which lines are new (user-typed this cycle).
    if let Some(ref snap) = snapshot_doc {
        content_ours = normalize_user_prompts_in_exchange_safe(&content_ours, base, snap, file);
    }

    // Lift pending out of exchange if nested (structural repair)
    content_ours = lift_pending_from_exchange_safe(&content_ours, file);
    content_ours =
        normalize_template_structure_or_fail_preserving(&content_ours, file, Some(base))?;

    // Shrink guard: refuse if new exchange content is dramatically shorter
    check_exchange_shrink_guard(&content_at_start, &content_ours, file)?;

    // Re-read file to check for user edits since lock acquisition
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    // Recompute the CRDT-merged + normalized content (and its encoded state)
    // for a given on-disk `current`. Factored so the reconcile loop below can
    // re-merge against a fresh disk state when a foreign agent-doc writer
    // appends mid-generation (#ipc-drift-visbuf-reconcile).
    let recompute_final = |content_current: &str| -> Result<(String, Vec<u8>, bool)> {
        let (final_content, mut crdt_state) = if let Some(repaired_current) =
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
            let doc = crate::crdt::CrdtDoc::from_text(&repaired_current);
            (repaired_current, doc.encode_state())
        } else if content_current == base {
            // No edits — build CRDT state from result
            let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
            (content_ours.clone(), doc.encode_state())
        } else {
            eprintln!("[write] File was modified during response generation. CRDT merging...");
            // Use baseline as CRDT base instead of stored state from previous cycle.
            // The baseline is the exact content both sides (ours and theirs) diverged
            // from, giving clean diffs. Using a stale stored state causes character-level
            // interleaving when the agent replaces component content while the user
            // appends within the same region (lazily-rs.md corruption bug).
            let base_state = snapshot::crdt_merge_base_state(file, base)?.state;
            // Agent=client_id(2) gives native correct ordering — no skip_reorder needed.
            // Prefer the editor's real captured ops over the diff-guess when aligned
            // (#qnodemerge4wire); inert (byte-identical) until the editor reporters land.
            match merge::merge_contents_crdt_with_ops(
                file,
                Some(&base_state),
                &content_ours,
                content_current,
            ) {
                Ok(merged) => merged,
                Err(e) => {
                    eprintln!(
                        "[write] WARNING: CRDT merge failed in stream write, falling back to splice: {}",
                        e
                    );
                    let spliced = splice_pending_component(&content_ours, content_current);
                    let doc = crate::crdt::CrdtDoc::from_text(&spliced);
                    (spliced, doc.encode_state())
                }
            }
        };
        let mut final_content = normalize_final_template_content(
            file,
            base,
            snapshot_doc.as_deref(),
            Some(content_current),
            &final_content,
            Some(&response),
        )?;
        let cleaned_resolved_backlog_prompts = cleanup_resolved_backlog_prompts_after_response(
            file,
            base,
            content_current,
            &final_content,
        )?;
        let cleaned_applied = cleaned_resolved_backlog_prompts.is_some();
        if let Some(cleaned) = cleaned_resolved_backlog_prompts {
            final_content = normalize_template_structure_or_fail_preserving(
                &cleaned,
                file,
                Some(content_current),
            )?;
            crdt_state = crate::crdt::CrdtDoc::from_text(&final_content).encode_state();
        }
        Ok((final_content, crdt_state, cleaned_applied))
    };

    let initial_payload = recompute_final(&content_current)?;

    // Reconcile the visible-write guard with the CRDT merge: re-merge the
    // captured response against a foreign disk append landed after the merge
    // was computed instead of failing closed and stranding the response
    // outside HEAD (#ipc-drift-visbuf-reconcile).
    let (content_current, (final_content, _crdt_state, cleaned_resolved_backlog_prompts_applied)) =
        reconcile_visible_write(
            file,
            content_current,
            initial_payload,
            VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS,
            |f, expected| guard_visible_write_reconcile(f, "run_stream", expected),
            recompute_final,
            |f, current| guard_visible_write_idle_and_current(f, "run_stream", current),
        )?;

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        let _ = crate::cycle_state::mark_write_applied(
            file,
            "write_stream_dedup",
            Some(&content_current),
            Some(&content_current),
        );
        drop(doc_lock);
        repair::clear_pending(file)?;
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
    let mut snapshot_content =
        snapshot_content_to_persist(snapshot_mode, &content_ours, &final_content).to_string();
    let integrated_queue_plan = if flags.queue_completion_ids.is_empty() {
        None
    } else {
        plan_queue_prompt_consumption_with_snapshot(
            file,
            &final_content,
            Some(&snapshot_content),
            &flags.queue_completion_ids,
        )?
    };
    if let Some(plan) = integrated_queue_plan.as_ref() {
        record_queue_consumption_proofs(file, plan, QueueConsumptionProofStage::BeforeMutation)?;
        final_content = plan.new_document.clone();
        snapshot_content = plan.new_snapshot.clone();
    }
    let snapshot_crdt_state = crate::crdt::CrdtDoc::from_text(&snapshot_content).encode_state();

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
    // Visible-write guard already reconciled above (see #ipc-drift-visbuf-reconcile).
    snapshot::save(file, &snapshot_content)?;
    snapshot::save_document_crdt(file, &snapshot_crdt_state, &snapshot_content)?;

    atomic_write(file, &final_content)?;
    if let Some(plan) = integrated_queue_plan.as_ref() {
        record_queue_consumption_proofs(file, plan, QueueConsumptionProofStage::AfterMutation)?;
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
        if let Err(err) = crate::recguard_wedge::clear(file) {
            eprintln!(
                "[recguard-wedge] WARNING: failed to clear wedge counter for {}: {}",
                file.display(),
                err
            );
        }
    }
    crate::ops_log::log_cycle(
        file,
        "write_stream",
        Some(&content_ours),
        Some(&final_content),
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "write_stream_done file={} snap_len={}",
            file.display(),
            final_content.len()
        ),
    );
    if let Err(e) = crate::cycle_state::mark_write_applied(
        file,
        "write_stream",
        Some(&final_content),
        Some(&final_content),
    ) {
        eprintln!("[write] cycle-state update failed: {} (non-fatal)", e);
    }

    drop(doc_lock);

    // Clear pending response after successful write
    repair::clear_pending(file)?;

    let elapsed_disk = t_disk.elapsed().as_millis();
    if elapsed_disk > 0 {
        eprintln!("[perf] disk_write_path: {}ms", elapsed_disk);
    }
    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] run_stream total: {}ms", elapsed_total);
    }

    eprintln!(
        "[write] Stream patches applied to {} ({} components patched, CRDT)",
        file.display(),
        patches.len()
    );
    Ok(())
}

/// IPC mode: write a JSON patch file for IDE plugin consumption.
///
/// Instead of modifying the document directly, writes a JSON file to
/// `.agent-doc/patches/<hash>.json`. The IDE plugin picks it up, applies
/// patches via Document API (no external file change dialog), and deletes
/// the file as ACK. Fails closed on timeout or missing proof.
pub fn run_ipc(file: &Path, baseline: Option<&str>, flags: WriteFlags) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    let rc = crate::graph::RunContext::new(file.to_path_buf());

    let mut response = read_response_input()?;

    if response.trim().is_empty() {
        if recover_empty_response_for_strict_closeout(file, &flags)? {
            return Ok(());
        }
        anyhow::bail!("empty response — nothing to write");
    }

    // #rtwwire rung 3b: the IPC write path normalizes/parses its patches against
    // the "current document". Source it from the realtime model — newest of disk
    // vs the editor's unsaved buffer — so a component patch (e.g. the queue) is
    // computed against the buffer the user actually sees, not stale disk, and the
    // resulting patchback cannot drop a queue/exchange item that exists only in
    // the unsaved buffer (#queue-user-edit-overwrite). Staleness-gated (`#rtwfeed`):
    // the buffer wins only when it provably holds unsaved edits ahead of disk, so
    // agent-doc's own just-written disk content can never be overridden. No editor
    // attached returns disk unchanged.
    let disk = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let mut current_content = crate::realtime_model::resolve_current_doc(file, &disk).content;
    let snapshot_doc = snapshot::load(file).ok().flatten();
    guard_no_stale_snapshot_reset_drift(
        file,
        snapshot_doc.as_deref(),
        &current_content,
        "IPC write",
    )?;
    sanitize_template_patchback_response_for_write(&mut response)?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;

    // Parse and validate patchback shape before any visible document mutation.
    let parsed =
        crate::flow::document_mutation::parse_template_patchback(file, &response, "run_ipc")?;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    sanitize_patches(&mut patches);
    sanitize_unmatched(&mut unmatched);

    let normalized = normalize_backlog_patch_response(
        file,
        &current_content,
        patches,
        unmatched,
        flags.allow_replace_pending,
    )?;
    if let Some(response_override) = normalized.response_for_capture {
        response = response_override;
    }
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Save response to pending store (survives context compaction)
    repair::save_pending(file, &response)?;

    // Enforcement: reject replace:pending (and deprecated patch:pending) blocks unless allowed.
    enforce_no_replace_pending(&patches, flags.allow_replace_pending)?;
    enforce_no_destructive_todo_patch(&current_content, &patches)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    if !flags.allow_replace_pending && !pending_replace_escape_hatch_enabled() {
        ensure_template_response_write_proof(&patches, &unmatched)?;
    }
    if flags.strict_closeout {
        ensure_strict_template_response_heading_for_current_doc(
            &current_content,
            &patches,
            &unmatched,
        )?;
    }
    prewrite_pending_capture_check(file, &response, &flags)?;
    auto_apply_pending_done_if_enabled(file, &response, &flags, &mut current_content)?;
    prewrite_pending_done_check(file, &response, &flags)?;

    let (doc_lock, content_at_start) = capture_locked_pre_response(file)?;

    // Build IPC patch file
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = resolve_ipc_project_root(&canonical);
    let patches_dir = project_root.join(".agent-doc/patches");
    std::fs::create_dir_all(&patches_dir)?;
    let patch_file = patches_dir.join(format!("{}.json", hash));
    let patch_id = uuid::Uuid::new_v4().to_string();

    // Use shared helper for boundary-aware synthesis (matches try_ipc socket + file paths).
    // Seed the boundary from patch_id for deterministic, dedup-friendly rebuilds
    // (#finalize-visible-buffer-ipc-timeout-race).
    let ipc_patches = build_ipc_patches_json(file, &patches, &unmatched, None, Some(&patch_id))?;

    // Same dedup guard: don't send unmatched when it was synthesized into a patch.
    let effective_unmatched = if patches.is_empty() && !ipc_patches.is_empty() {
        ""
    } else {
        unmatched.trim()
    };

    // Separate frontmatter patch
    let frontmatter_yaml: Option<String> = patches
        .iter()
        .find(|p| p.name == "frontmatter")
        .map(|p| p.content.trim().to_string());
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &content_at_start);
    let base_cow = template_patch_application_base(TemplatePatchApplicationBase {
        file,
        baseline,
        content_at_start: &content_at_start,
        patches: &patches,
        unmatched: &unmatched,
        mode_overrides: &mode_overrides,
        source: "run_ipc",
        strict_closeout: flags.strict_closeout,
    })?;
    let base = base_cow.as_ref();
    let ipc_baseline = baseline.map(|_| base);
    let content_ours = template::apply_patches_with_overrides_with_context(
        base,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
        Some(&rc),
    )
    .context("failed to apply template patches for IPC node patch metadata")?;
    let content_ours =
        normalize_template_structure_or_fail_preserving(&content_ours, file, Some(base))?;
    let ipc_node_patches = build_ipc_node_patches_json(Some(base), Some(&content_ours));

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "node_patches": ipc_node_patches,
        "unmatched": effective_unmatched,
        "baseline": ipc_baseline.unwrap_or(""),
    });
    ipc_payload["baseline_hash"] =
        serde_json::Value::String(crate::debounce::content_hash(&content_at_start));
    ipc_payload["baseline_normalized_hash"] =
        serde_json::Value::String(crate::debounce::content_hash(
            &crate::git::normalize_transient_agent_doc_markers(&content_at_start),
        ));
    ipc_payload["patch_id"] = serde_json::Value::String(patch_id.clone());
    if let Ok(Some(ref cs)) = crate::cycle_state::load(file) {
        ipc_payload["cycle_id"] = serde_json::Value::String(cs.cycle_id.clone());
    }

    if let Some(ref yaml) = frontmatter_yaml {
        ipc_payload["frontmatter"] = serde_json::Value::String(yaml.clone());
    }

    // Atomic write of patch file
    atomic_write(&patch_file, &serde_json::to_string_pretty(&ipc_payload)?)?;

    eprintln!(
        "[write] IPC patch written to {} ({} components)",
        patch_file.display(),
        ipc_patches.len()
    );

    // Poll for ACK (plugin deletes file after applying)
    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    let mut consumed_without_materialization = false;

    while start.elapsed() < timeout {
        if !patch_file.exists() {
            // Plugin consumed the patch — update snapshot from current file
            let content = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {} after IPC", file.display()))?;
            let expected_response = response_materialization_probe(&patches, &unmatched);
            if !ipc_response_materialized_or_fallback(
                file,
                "explicit_file_ipc",
                &expected_response,
                &content,
            ) {
                log_partial_response_materialization_for_retry(
                    file,
                    "explicit_file_ipc",
                    &expected_response,
                )?;
                consumed_without_materialization = true;
                break;
            }
            if file_ipc_consumed_without_live_exchange_ack(
                file,
                "explicit_file_ipc",
                Some(&patch_id),
                baseline,
                Some(&content_at_start),
                &content,
                false,
            ) {
                consumed_without_materialization = true;
                break;
            }
            snapshot::save(file, &content)?;
            crate::ops_log::log_op(
                file,
                &format!(
                    "snapshot_saved_file_ipc file={} snap_len={}",
                    file.display(),
                    content.len()
                ),
            );
            log_exchange_write_diagnostic(
                file,
                "run_ipc",
                "ipc_file",
                Some(&patch_id),
                baseline,
                &content_at_start,
                &content,
                &patches,
                &unmatched,
            );
            let crdt_doc = crate::crdt::CrdtDoc::from_text(&content);
            snapshot::save_document_crdt(file, &crdt_doc.encode_state(), &content)?;
            drop(doc_lock);
            repair::clear_pending(file)?;
            eprintln!("[write] IPC patch consumed by plugin — snapshot updated");
            return Ok(());
        }
        std::thread::sleep(poll_interval);
    }

    if consumed_without_materialization {
        eprintln!(
            "[write] IPC patch was consumed without materializing the response — refusing direct document write; retry required"
        );
    } else {
        eprintln!(
            "[write] IPC timeout ({}s) — leaving patch for editor retry; refusing direct document write",
            timeout.as_secs()
        );
        log_ipc_proof_failure(
            file,
            "explicit_file_ipc",
            Some(&patch_id),
            "no_ack",
            "retry_without_disk_write",
            &format!(
                "timeout_secs={} patch_file={}",
                timeout.as_secs(),
                patch_file.display()
            ),
        );
    }

    // Guard: if the cycle was already committed by a concurrent closeout,
    // clean stale IPC files to prevent re-dirtying the document.
    if let Some(ref committed_id) = cycle_already_committed(file) {
        eprintln!(
            "[write] run_ipc timeout retry: cycle {} already committed — cleaning stale patch",
            committed_id
        );
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::TerminalGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "run_ipc_timeout_fallback_skip file={} cycle_id={} reason=already_committed",
                file.display(),
                committed_id
            ),
        );
        cleanup_fallback_patch_files(file);
        drop(doc_lock);
        repair::clear_pending(file)?;
        return Ok(());
    }

    drop(doc_lock);
    crate::ops_log::log_op(
        file,
        &format!(
            "run_ipc_retry_required_no_disk_write file={} patch_id={} consumed_without_materialization={} recovery=retry_without_disk_write",
            file.display(),
            patch_id,
            consumed_without_materialization
        ),
    );
    anyhow::bail!(
        "editor IPC did not prove the write for {}; pending response retained for retry; refusing direct document write",
        file.display()
    );
}

fn content_uses_crdt_write(content: &str) -> bool {
    frontmatter::parse(content)
        .map(|(fm, _)| fm.resolve_mode().is_crdt())
        .unwrap_or(false)
}

fn merge_recovery_content(
    file: &Path,
    base: &str,
    content_ours: &str,
    content_current: &str,
    source: &str,
) -> Result<String> {
    if content_current == base {
        return Ok(content_ours.to_string());
    }

    if content_uses_crdt_write(base) {
        eprintln!("[write] File was modified during response recovery. CRDT merging...");
        crate::ops_log::log_op(
            file,
            &format!(
                "recovery_crdt_merge file={} source={} recovery=retry_crdt_instead",
                file.display(),
                source
            ),
        );
        let base_state = snapshot::crdt_merge_base_state(file, base)?.state;
        let (merged, _) = merge::merge_contents_crdt_with_ops(
            file,
            Some(&base_state),
            content_ours,
            content_current,
        )
        .with_context(|| format!("CRDT merge failed during {source}"))?;
        Ok(merged)
    } else {
        merge::merge_contents(base, content_ours, content_current)
    }
}

fn save_recovery_snapshot(file: &Path, content: &str, use_crdt: bool) -> Result<()> {
    snapshot::save(file, content)?;
    if use_crdt {
        let doc = crate::crdt::CrdtDoc::from_text(content);
        snapshot::save_document_crdt(file, &doc.encode_state(), content)?;
    }
    Ok(())
}

/// Apply an append-mode response from a string (not stdin).
/// Used by `repair` to apply orphaned responses.
pub fn apply_append_from_string(file: &Path, response: &str) -> Result<()> {
    let response = strip_assistant_heading(response);
    let content = std::fs::read_to_string(file)
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

    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let final_content = merge_recovery_content(
        file,
        &content,
        &content_ours,
        &content_current,
        "apply_append_from_string",
    )?;

    guard_visible_write_idle_and_current(file, "apply_append_from_string", &content_current)?;
    atomic_write(file, &final_content)?;
    // Save snapshot as content_ours, not final_content
    save_recovery_snapshot(file, &content_ours, use_crdt)?;
    drop(doc_lock);
    eprintln!("[write] Response appended to {}", file.display());
    Ok(())
}

/// Apply template-mode patches from a string (not stdin).
/// Used by `repair` to apply orphaned template responses.
pub fn apply_template_from_string(file: &Path, response: &str) -> Result<()> {
    apply_template_from_string_with_options(file, response, TemplateApplyOptions::default())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TemplateApplyOptions {
    pub force_disk: bool,
}

pub fn apply_template_from_string_with_options(
    file: &Path,
    response: &str,
    options: TemplateApplyOptions,
) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let use_crdt = content_uses_crdt_write(&content);
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let mut response = response.to_string();
    sanitize_template_patchback_response_for_write(&mut response)?;

    let parsed = crate::flow::document_mutation::parse_template_patchback(
        file,
        &response,
        "apply_template_from_string",
    )?;
    let mut patches = parsed.patches;
    let mut unmatched = parsed.unmatched;

    // Sanitize component tags in patch content and unmatched text to prevent
    // parser corruption and duplicate exchange blocks (#dupeexchangeblock).
    sanitize_patches(&mut patches);
    sanitize_unmatched(&mut unmatched);

    let normalized = normalize_backlog_patch_response(file, &content, patches, unmatched, false)?;
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject replace:pending (and deprecated patch:pending) blocks unless allowed.
    enforce_no_replace_pending(&patches, false)?;
    enforce_no_destructive_todo_patch(&content, &patches)?;

    let mode_overrides = template_mode_overrides_for_current_doc(file, None, &content);
    let snapshot_doc = snapshot::load(file).ok().flatten();
    let content_ours = template::apply_patches_with_overrides_with_context(
        &content,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
        Some(&rc),
    )
    .context("failed to apply template patches")?;
    let content_ours =
        normalize_template_structure_or_fail_preserving(&content_ours, file, Some(&content))?;

    let doc_lock = acquire_doc_lock(file)?;

    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

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
        atomic_write(file, &final_content)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "apply_template_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                final_content.len(),
                crate::ops_log::content_hash(&final_content)
            ),
        );
    } else {
        guard_visible_write_idle_and_current(file, "apply_template_from_string", &content_current)?;
        // `#fcc0`: repair recovery applies template (component) patches through
        // the editor path; if editor convergence is unavailable or unproven,
        // fail closed instead of writing the repaired document straight to disk.
        try_editor_converge(file, &final_content, &content_current, "apply_template")?;
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
        snapshot::save(&doc, snapshot_content).unwrap();

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
        snapshot::save(&doc, snapshot_content).unwrap();

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
    fn recovery_merge_uses_crdt_for_crdt_documents_without_diff3_markers() {
        let dir = TempDir::new().unwrap();
        for subdir in ["crdt", "logs", "snapshots"] {
            fs::create_dir_all(dir.path().join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Please reply\n",
            "<!-- agent:boundary:base1234 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let content_ours = base.replace(
            "<!-- agent:boundary:base1234 -->",
            "### Re: recovery crdt - gpt-5\n\nDone.\n<!-- agent:boundary:base1234 -->",
        );
        let content_current = base.replace(
            "<!-- agent:boundary:base1234 -->",
            "while I was typing\n<!-- agent:boundary:base1234 -->",
        );
        fs::write(&doc, content_current.as_str()).unwrap();

        let merged =
            merge_recovery_content(&doc, base, &content_ours, &content_current, "test_recovery")
                .unwrap();

        assert!(merged.contains("### Re: recovery crdt - gpt-5"));
        assert!(merged.contains("while I was typing"));
        assert!(
            !merged.contains("<<<<<<<") && !merged.contains(">>>>>>>"),
            "CRDT recovery merge must not emit diff3 markers:\n{merged}"
        );
        let ops = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops.contains("recovery_crdt_merge"));
        assert!(ops.contains("recovery=retry_crdt_instead"));
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
        snapshot::save(&doc, content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

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
    fn build_ipc_patches_json_seeded_boundary_is_stable_across_rebuilds() {
        // #finalize-visible-buffer-ipc-timeout-race ROOT-CAUSE REGRESSION:
        // a single write builds its IPC patches more than once (socket attempt →
        // file-IPC fallback → old run_stream timeout re-write). Each rebuild used to
        // mint a FRESH random boundary, so the plugin saw the same response under
        // two different boundary IDs and appended it twice — doubling the editor
        // buffer (live repro: 57970 → 107235 bytes). Seeding the boundary from the
        // stable patch_id must make every rebuild carry an IDENTICAL boundary.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("agent-doc-bugs2.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nPrior response.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches = vec![crate::template::PatchBlock::new(
            "exchange",
            "### Re: fix\n\nNew response body.",
        )];
        let seed = "2ffa57c0-24e8-441c-aca9-46e6aa6f1c2a";

        // Two rebuilds of the SAME write (same seed) → identical boundary.
        let build_a = build_ipc_patches_json(&doc, &patches, "", None, Some(seed)).unwrap();
        let build_b = build_ipc_patches_json(&doc, &patches, "", None, Some(seed)).unwrap();
        let bid_a = build_a[0]["boundary_id"].as_str();
        let bid_b = build_b[0]["boundary_id"].as_str();
        assert!(
            bid_a.is_some(),
            "patch should carry a boundary_id: {build_a:?}"
        );
        assert_eq!(
            build_a[0]["op"].as_str(),
            Some("append"),
            "append-mode patches should carry an explicit IPC op"
        );
        assert_eq!(
            build_a[0]["node_id"].as_str(),
            bid_a,
            "append-mode patches should address the boundary node"
        );
        assert_eq!(
            bid_a, bid_b,
            "same patch_id seed must yield the SAME boundary across rebuilds (no double-apply)"
        );
        assert_eq!(
            bid_a,
            Some("2ffa57c0:agent-doc-bugs2"),
            "boundary must derive from the patch_id hex prefix + doc-stem slug"
        );

        // A different write (different seed) must NOT collide on one boundary.
        let other_seed = "99887766-1111-2222-3333-444455556666";
        let build_c = build_ipc_patches_json(&doc, &patches, "", None, Some(other_seed)).unwrap();
        assert_ne!(
            build_c[0]["boundary_id"].as_str(),
            bid_a,
            "distinct writes must derive distinct boundaries"
        );
    }
}
