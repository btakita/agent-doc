//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

/// Options controlling a `preflight` invocation.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreflightOptions {
    /// Pure inspection probe (`#preflight-probe-side-effect-free`): compute and
    /// emit the same JSON, but do NOT open a `preflight_started` cycle. A
    /// diagnostic preflight is not dispatch/response-bound, so opening a cycle
    /// only leaves open state that later wedges `session-check`.
    pub probe: bool,
}

/// Run preflight with default (dispatch/response-bound) options.
pub fn run(file: &Path) -> Result<()> {
    run_with_options(file, PreflightOptions::default())
}

pub fn run_with_options(file: &Path, options: PreflightOptions) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let disk = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    // #rtwwire (rung 3): classify against the realtime document model — newest of
    // disk vs the editor's unsaved buffer — so preflight never treats a buffer
    // the user is actively editing as a "differs from disk" drift to block. The
    // feed is staleness-gated (`#rtwfeed`): the buffer only supersedes disk when
    // it provably holds unsaved edits ahead of disk, so a stale buffer or
    // agent-doc's own just-written disk content can never override disk here.
    // With no editor attached (the common/CI case) this returns disk unchanged.
    let content = crate::realtime_model::resolve_current_doc(file, &disk).content;
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (initial_frontmatter, _) = frontmatter::parse_for_file_with_context(&content, file, &rc)?;
    let active_harness = rc.harness();
    let mut warnings = Vec::new();
    if let Some(warning) =
        harness_mismatch_warning(initial_frontmatter.agent.as_deref(), &active_harness)
    {
        eprintln!("[preflight] warning: {}", warning.message);
        warnings.push(warning);
    }

    if initial_frontmatter.codex_network_access.is_some()
        && canonical_harness_name(&active_harness).as_deref() != Some("codex")
    {
        let msg = format!(
            "{}: `codex_network_access` is Codex-specific and has no effect when the active harness is {}. \
             Either remove it from the document frontmatter or switch the agent to codex.",
            file.display(),
            active_harness
        );
        eprintln!("[preflight] warning: {msg}");
        warnings.push(PreflightWarning {
            code: "codex_network_access_non_codex_harness".to_string(),
            message: msg,
            document_agent: initial_frontmatter.agent.as_deref().map(|s| s.to_string()),
            active_harness: Some(active_harness.to_string()),
        });
    }

    // Step 0a: Auto-GC (at most once per day).
    // Checks .agent-doc/gc.stamp — if missing or >24 hours old, runs lightweight GC.
    {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(root) = snapshot::find_project_root(&canonical) {
            match crate::project_controller::close_stale_starting_actors_for_caller(
                &root,
                std::time::Duration::from_secs(3600),
                false,
                "preflight",
            ) {
                Ok((closed, kept)) if closed > 0 => {
                    eprintln!(
                        "[preflight] actors: {} stale starting closed, {} still active",
                        closed, kept
                    );
                }
                Ok(_) => {}
                Err(e) => eprintln!("[preflight] actor gc warning: {}", e),
            }

            let stamp = root.join(".agent-doc/gc.stamp");
            let needs_gc = match std::fs::metadata(&stamp) {
                Ok(meta) => meta
                    .modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|age| age > std::time::Duration::from_secs(86400))
                    .unwrap_or(true),
                Err(_) => true,
            };
            if needs_gc {
                eprintln!("[preflight] step 0a: auto-gc");
                match crate::gc::run(Some(&root), false) {
                    Ok(result) => {
                        if result.deleted > 0 {
                            eprintln!("[preflight] gc: {} files cleaned", result.deleted);
                        }
                        let _ = std::fs::write(&stamp, "");
                    }
                    Err(e) => eprintln!("[preflight] gc warning: {}", e),
                }
            }
        }
    }

    // Pre-mutation debounce: recovery, pending maintenance, commit, and
    // duplicate-residue cleanup all write the visible document or its sidecars.
    // Do not let those paths race an editor buffer that is still publishing a
    // prompt.
    let debounce_ms = preflight_debounce_ms(file);
    wait_for_typing_idle_before_mutation(file, debounce_ms)?;

    // Step 0-pre: interrupted-cycle guard (#cyc1). Use exact persisted cycle
    // state instead of inferring solely from `ops.log`.
    let (recovered_prior, committed_prior) = enforce_cycle_completion(file)?;

    // Step 0: Check tmux layout health.
    eprintln!("[preflight] step 0: layout check");
    let mut layout_issues = check_layout();
    for issue in &layout_issues {
        eprintln!("[preflight] layout issue: {}", issue);
    }

    // Step 0b (#a014): Session drift auto-resync — when drift is detected on
    // consecutive preflights, auto-run `resync --fix` to clean the registry.
    // State lives in `.agent-doc/state/drift.count` so we only auto-fix after
    // the second consecutive detection (one false positive is tolerated).
    maybe_auto_resync_on_drift(file, &layout_issues);

    // Step 0c: Auto-repair base-index compliance — when window index 0 is
    // missing, run repair_layout immediately so this preflight reports the
    // post-repair layout state.
    if maybe_auto_repair_base_index(file, &layout_issues) {
        layout_issues = check_layout();
        if layout_issues.is_empty() {
            eprintln!("[preflight] layout repair cleared base-index issues");
        } else {
            for issue in &layout_issues {
                eprintln!("[preflight] layout issue after repair: {}", issue);
            }
        }
    }

    // Step 0d: Fail closed on out-of-band closeout drift before transcript
    // repair can normalize a dirty response body into prompt-looking lines.
    // Open cycles still go through repair first so interrupted write/commit
    // boundaries can recover normally.
    let open_cycle = crate::cycle_state::load(file)?
        .map(|state| state.is_open())
        .unwrap_or(false);
    if !open_cycle && crate::session_check::detect_unstarted_prompt_bearing_diff(file)?.is_none() {
        enforce_no_uncommitted_closeout_drift(file, &rc)?;
    }

    // Step 1: Recover orphaned pending responses.
    eprintln!("[preflight] step 1: repair");
    // #queue-active-deprecated-line-stuck: drop a legacy `queue_active:` line that
    // is stuck in the document because the diff layer classifies it as managed
    // state (so its removal never reads as a diff and is never committed) and the
    // byte-precise hot path never re-serializes frontmatter through `write()`
    // (which would drop it). Strip it directly on disk + snapshot, but ONLY when
    // the canonical `queue:` control is present so no queue state is lost. Idempotent.
    if let Ok(current) = std::fs::read_to_string(file) {
        let migrated = frontmatter::strip_deprecated_queue_active_line(&current);
        if migrated != current {
            match crate::write::atomic_write_pub(file, &migrated) {
                Ok(()) => {
                    if let Err(err) = crate::snapshot::save(file, &migrated) {
                        eprintln!(
                            "[preflight] warning: dropped deprecated queue_active line but failed to update snapshot for {}: {err}",
                            file.display()
                        );
                    }
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "deprecated_queue_active_line_dropped file={}",
                            file.display()
                        ),
                    );
                    eprintln!(
                        "[preflight] dropped deprecated `queue_active:` line (canonical `queue:` retained) for {}",
                        file.display()
                    );
                }
                Err(err) => {
                    eprintln!(
                        "[preflight] warning: failed to drop deprecated queue_active line for {}: {err}",
                        file.display()
                    );
                }
            }
        }
    }
    // Detect the stuck-captured-cycle wedge: cycle_state advanced to Committed
    // while the active capture body never landed in HEAD. Emit as a non-blocking
    // warning so the harness can take a recovery path (e.g. force write --commit)
    // instead of silently retrying the same finalize.
    // See tasks/agent-doc/plan-stuck-cycle-causes-duplicated-uncommitted-response.md.
    // #stuck-capture-compact-false-positive: first durably settle any
    // committed-cycle capture whose response is absent from HEAD only because
    // `compact` archived it. This converts the per-pass archive-suppression into
    // a one-time terminal `Discarded`, so a later archive GC cannot resurface the
    // false-positive stuck warning. After this, `stuck_captured_cycle` sees a
    // discarded capture and returns None for the same case.
    match crate::flow::closeout::reconcile_compacted_committed_capture(file) {
        Ok(true) => {
            eprintln!(
                "[preflight] reconciled compacted committed capture for {}",
                file.display()
            );
        }
        Ok(false) => {}
        Err(err) => {
            eprintln!(
                "[preflight] warning: failed to reconcile compacted committed capture for {}: {err}",
                file.display()
            );
        }
    }
    if let Some(info) = crate::flow::closeout::stuck_captured_cycle(file) {
        warnings.push(PreflightWarning {
            code: "stuck_captured_cycle".to_string(),
            message: format!(
                "Cycle {} reached state `committed` but the captured response body ({} bytes, capture {}, state `{}`) is not present in HEAD for {}. Recover via `agent-doc write --commit {}` once the visible response body is final.",
                info.cycle_id,
                info.response_body_len,
                info.capture_id,
                info.capture_state,
                file.display(),
                file.display()
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    let mut recovered = recovered_prior
        || match repair::run(file) {
            Ok(outcome) => outcome.repaired(),
            Err(e) => {
                let message = e.to_string();
                if message.contains(repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
                    || message.contains(repair::EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR)
                {
                    return Err(e);
                }
                eprintln!("[preflight] repair warning: {}", e);
                false
            }
        };

    // Step 1b: Ensure document is initialized (snapshot + git baseline).
    // If no snapshot exists, creates one and commits the file.
    if let Err(e) = snapshot::ensure_initialized(file) {
        eprintln!("[preflight] warning: auto-init failed: {}", e);
    }

    // Step 1b2: Fail closed on out-of-band closeout drift before this preflight
    // mutates backlog state or runs the generic commit path. Otherwise a
    // snapshot/file pair that already contains a visible response could be
    // normalized into a misleading `no_changes` result.
    enforce_no_uncommitted_closeout_drift(file, &rc)?;

    // Step 1c: Pending component maintenance — lazy backfill, reap, archive, and
    // reorder detection. MUST run BEFORE step 2 commit so the single step-2
    // commit bundles the pending mutations with the previous-cycle response,
    // producing exactly one HEAD advance per preflight. Running after step 2
    // caused #64mb (double commit_staging: step 2 committed, then maintenance
    // mutated and committed again).
    //
    // Maintenance applies its mutations to BOTH the working tree file AND the
    // snapshot (surgically, via component replace), so the upcoming step-2
    // commit which stages from snapshot picks them up atomically.
    let pending_report = run_pending_maintenance(file)?;
    let pending_reordered = pending_report.reordered;
    let pending_gated_count = pending_report.pending_gated_count;

    // `#optverify`: opportunistic gated-review auto-verification. Runs before the
    // step-2 commit so any opt-in `[/]→[x]` flip is staged atomically (the
    // mutation touches both the working-tree file and the snapshot). Default off
    // — without the opt-in the gate status is only surfaced, never flipped.
    let gate_autoverify_optin = initial_frontmatter
        .gate_autoverify
        .or(rc.project_config().agent_doc_gate_autoverify)
        .unwrap_or(false);
    let gate_verify_results = match run_gate_verify(file, gate_autoverify_optin) {
        Ok(results) => results,
        Err(e) => {
            eprintln!("[preflight] optverify: scan skipped: {}", e);
            Vec::new()
        }
    };
    if pending_report.legacy_gated_in_backlog_count > 0 {
        warnings.push(PreflightWarning {
            code: "legacy_gated_in_backlog".to_string(),
            message: format!(
                "{} gated item(s) still live in agent:backlog; run `agent-doc migrate {}` to move them into agent:review.",
                pending_report.legacy_gated_in_backlog_count,
                file.display()
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    enforce_no_shadow_open_backlog(file)?;
    enforce_no_dropped_backlog(file, &rc)?;
    if remove_duplicate_answered_exchange_prompt_tail_for_preflight(file)? {
        recovered = true;
    }
    if remove_post_exchange_duplicate_prompt_comments_for_preflight(file, &rc)? {
        recovered = true;
    }

    // Step 2: Commit previous cycle.
    eprintln!("[preflight] step 2: commit");
    let committed = committed_prior
        || match git::commit(file) {
            Ok(did_commit) => {
                if did_commit {
                    rc.invalidate_head_content();
                }
                did_commit
            }
            Err(e) => {
                eprintln!("[preflight] commit warning: {}", e);
                false
            }
        };

    if let Some(repaired_doc) =
        relocate_out_of_exchange_prompt_before_diff(file, &std::fs::read_to_string(file)?)?
    {
        crate::write::atomic_write_pub(file, &repaired_doc)?;
        crate::ops_log::log_op(
            file,
            &format!(
                "preflight_repair_prompt_tail_outside_exchange file={}",
                file.display()
            ),
        );
        eprintln!(
            "[preflight] repaired prompt tail outside exchange in {}",
            file.display()
        );
        recovered = true;
    }
    if remove_duplicate_answered_exchange_prompt_tail_for_preflight(file)? {
        recovered = true;
    }
    if remove_post_exchange_duplicate_prompt_comments_for_preflight(file, &rc)? {
        recovered = true;
    }

    // Step 2d: Cross-document sweep (Fix 5) — commit any other tracked docs in the same
    // project that have uncommitted snapshot content. Turns preflight into a catch-all
    // backstop: even if a previous session's commit was skipped, the next preflight
    // from any document in the project will pick it up.
    {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(root) = snapshot::find_project_root(&canonical)
            && let Ok(registry) = sessions::load_in(&root)
        {
            let current_owner = current_sweep_owner(file, &root, &registry, &canonical);
            for (registry_key, entry) in &registry {
                let tracked_file = if entry.file.trim().is_empty() {
                    registry_key.as_str()
                } else {
                    entry.file.as_str()
                };
                if tracked_file.trim().is_empty() {
                    continue;
                }
                let doc_path = {
                    let path = Path::new(tracked_file);
                    let joined = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        root.join(path)
                    };
                    std::fs::canonicalize(&joined).unwrap_or(joined)
                };
                if doc_path == canonical {
                    continue;
                } // already committed in step 2
                if !doc_path.exists() {
                    continue;
                }
                // snapshot mtime > last commit? Call commit (idempotent — git skips if clean).
                let snap_rel = match snapshot::path_for(&doc_path) {
                    Ok(rel) => rel,
                    Err(_) => continue,
                };
                let snap_abs = root.join(&snap_rel);
                let snap_is_newer = (|| {
                    let snap_mtime = std::fs::metadata(&snap_abs).ok()?.modified().ok()?;
                    let doc_mtime = std::fs::metadata(&doc_path).ok()?.modified().ok()?;
                    // Proxy: snap newer than doc means an agent write landed without commit
                    Some(snap_mtime > doc_mtime)
                })()
                .unwrap_or(true); // if uncertain, try commit anyway
                if snap_is_newer {
                    let sibling_owner = sweep_owner_for_doc(file, &root, &registry, &doc_path);
                    if should_skip_foreign_owned_sweep(
                        file,
                        &doc_path,
                        current_owner.as_ref(),
                        sibling_owner.as_ref(),
                    ) {
                        continue;
                    }
                    // Guard: don't sweep-commit if the document has user additions
                    // that the agent hasn't responded to yet. For inline mode this
                    // checks ## User / ## Assistant blocks; for template mode it
                    // falls through to a content-equality check.
                    if let (Ok(snap_content), Ok(doc_content)) = (
                        std::fs::read_to_string(&snap_abs),
                        std::fs::read_to_string(&doc_path),
                    ) && !crate::diff::is_stale_snapshot(&snap_content, &doc_content)
                    {
                        // Not a stale inline snapshot — check content equality
                        // (covers template mode where is_stale_snapshot always returns false)
                        let snap_stripped = crate::diff::strip_comments(&snap_content);
                        let doc_stripped = crate::diff::strip_comments(&doc_content);
                        if snap_stripped.trim() != doc_stripped.trim() {
                            eprintln!(
                                "[preflight] sweep: skipping {} (unresponded user content)",
                                doc_path.display()
                            );
                            continue;
                        }
                    }
                    // Freshness gate: skip if another session committed this doc
                    // within the last 5s. Inside the CommitLock critical section
                    // this is a valid fast-path — a concurrent commit that just
                    // ran will have advanced HEAD's commit time, so we avoid
                    // re-spawning git (~10ms) for nothing. The gate only closes
                    // races when paired with the per-file commit flock in git::commit.
                    let fresh = git::last_commit_mtime(&doc_path)
                        .ok()
                        .flatten()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|e| e.as_secs() < 5);
                    if fresh {
                        eprintln!(
                            "[preflight] sweep: skipping {} (committed <5s ago)",
                            doc_path.display()
                        );
                        continue;
                    }
                    match git::commit(&doc_path) {
                        Ok(true) => {
                            eprintln!("[preflight] sweep: committed {}", doc_path.display())
                        }
                        Ok(false) => {
                            eprintln!("[preflight] sweep: clean {}", doc_path.display())
                        }
                        Err(e) => eprintln!(
                            "[preflight] sweep: warning for {}: {}",
                            doc_path.display(),
                            e
                        ),
                    }
                }
            }
        }
    }

    // Step 3: Read and truncate the claims log.
    eprintln!("[preflight] step 3: claims");
    let claims = read_and_truncate_claims(file);

    // Step 3b: Wait for file to settle (mtime + typing indicator debounce).
    // Check both file mtime (disk-level) and cross-process typing indicator
    // (buffer-level) to avoid picking up mid-typing edits.
    // Default: 2000ms (configurable via `agent_doc_debounce` frontmatter field).
    {
        let debounce_ms = preflight_debounce_ms(file);
        let debounce = std::time::Duration::from_millis(debounce_ms);
        let max_wait = preflight_debounce_max_wait(debounce_ms);
        let poll = std::time::Duration::from_millis(100);
        let start = std::time::Instant::now();
        let file_str = file.to_string_lossy();
        tracing::debug!(debounce_ms, file = %file.display(), "preflight debounce starting");

        loop {
            let idle_for = std::fs::metadata(file)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(debounce);

            let typing_active = crate::debounce::is_typing_via_file(&file_str, debounce_ms);
            tracing::trace!(
                idle_ms = idle_for.as_millis() as u64,
                typing_active,
                elapsed_ms = start.elapsed().as_millis() as u64,
                "preflight debounce poll"
            );

            if idle_for >= debounce && !typing_active {
                tracing::debug!(
                    idle_ms = idle_for.as_millis() as u64,
                    waited_ms = start.elapsed().as_millis() as u64,
                    "preflight debounce settled"
                );
                break;
            }
            if start.elapsed() >= max_wait {
                if typing_active {
                    tracing::warn!(
                        waited_ms = start.elapsed().as_millis() as u64,
                        "preflight debounce timeout (typing still active)"
                    );
                    eprintln!(
                        "[preflight] typing indicator active but timeout after {:.1}s — proceeding",
                        start.elapsed().as_secs_f64()
                    );
                } else {
                    tracing::warn!(
                        waited_ms = start.elapsed().as_millis() as u64,
                        "preflight debounce timeout (mtime not settled)"
                    );
                    eprintln!(
                        "[preflight] mtime debounce timeout after {:.1}s — proceeding",
                        start.elapsed().as_secs_f64()
                    );
                }
                break;
            }
            std::thread::sleep(poll);
        }
    }

    // Step 3c: Check related documents for changes.
    eprintln!("[preflight] step 3c: related docs");
    let linked_changes = check_linked_docs(file);
    for change in &linked_changes {
        eprintln!(
            "[preflight] related doc change: {} — {}",
            change.path, change.summary
        );
    }

    // Step 4: Compute diff between snapshot and current document.
    eprintln!("[preflight] step 4: diff");
    let diff_result_with_current = diff::compute_with_current(file)?;
    // Save the response baseline from the exact stable document projection used
    // for the diff. This keeps the merge baseline, visible file, and prompt
    // contract in one transaction even if an editor replay lands during the
    // earlier debounce window.
    let baseline_file = save_baseline_content(file, &diff_result_with_current.current);
    let raw_diff = diff_result_with_current.diff;
    let harness_diff = crate::harness_prompt::synthetic_diff_for_file(file)?;
    let initial_diff = raw_diff.clone().or(harness_diff.clone());

    // Step 4a: Scan diff for inline `/model <x>` command and strip the matching
    // line(s) before downstream classification. The strip prevents `/model` from
    // double-emitting in `builtin_commands`.
    let global_config = config::load().unwrap_or_default();
    let harness = rc.harness();
    let model_scan = initial_diff
        .as_ref()
        .map(|d| agent_doc_core::model_tier::scan_model_switch(d, &harness, &global_config.model));
    let mut diff_result: Option<String> = if let Some(scan) = model_scan.as_ref() {
        // Use the stripped diff for downstream consumers.
        Some(scan.stripped_diff.clone())
    } else {
        initial_diff.clone()
    };

    // Step 4b: Classify the diff for skill routing.
    let mut classification = diff_result.as_ref().map(|d| diff::classify_diff(d));
    let boundary_artifact_only = classification
        .as_ref()
        .is_some_and(|c| c.diff_type == diff::DiffType::BoundaryArtifact);
    if boundary_artifact_only {
        if raw_diff.is_some() {
            diff_result = harness_diff.clone();
            classification = diff_result.as_ref().map(|d| diff::classify_diff(d));
        } else {
            diff_result = None;
            classification = None;
        }
    }

    // Step 4b2: Queue component analysis — resolve activation, consume start
    // fences, and emit queue prompts for the skill. If the document/harness diff
    // is otherwise empty, an active queue head item becomes the prompt diff for
    // this cycle. This preserves bare no-op invocations while letting persisted
    // `queue_active: true` advance without requiring a fresh document edit.
    let queue_state = run_queue_maintenance(file, diff_result.as_deref()).unwrap_or_else(|e| {
        eprintln!("[preflight] queue maintenance warning: {}", e);
        QueueState::default()
    });
    warnings.extend(queue_state.warnings.clone());
    // `#agent-doc-bug` auto-queue stall: when there is no real user/document diff
    // this cycle, an active queue head is synthesized as the cycle's prompt diff.
    // That synthetic head is queue *continuation*, not user intent — so it must
    // NOT populate `user_intent_prompt_changes`, or the skill's auto-loop
    // precondition (`user_intent_prompt_changes` empty) never holds and the
    // `auto` queue stalls after every item. A real user prompt typed mid-queue
    // keeps `diff_result` non-None here, so this flag stays false and the
    // prompt is surfaced normally.
    let mut diff_from_queue_head_only = false;
    if diff_result.is_none()
        && let Some(head_prompt) = queue_state.queue_prompts.first()
    {
        let slash_command = crate::queue_command::slash_command_text(head_prompt);
        let prompt_source = slash_command.as_deref().unwrap_or(head_prompt);
        diff_result = Some(diff::synthetic_added_lines_diff(prompt_source, "queue"));
        classification = diff_result.as_ref().map(|d| diff::classify_diff(d));
        diff_from_queue_head_only = true;
    }

    let slash_command_only_diff_commands = diff_result
        .as_deref()
        .and_then(diff::parse_slash_command_only_added_diff);
    let no_changes = diff_result.is_none();
    if !no_changes {
        if let Some(commands) = slash_command_only_diff_commands.as_ref() {
            crate::ops_log::log_op(
                file,
                &format!(
                    "preflight_slash_command_only_handoff file={} commands={:?}",
                    file.display(),
                    commands
                ),
            );
            eprintln!(
                "[preflight] slash command diff {:?} is command-only; skipping preflight_started so the harness/supervisor can submit it without an agent-doc response cycle",
                commands
            );
        } else if options.probe {
            // `#preflight-probe-side-effect-free`: a pure inspection probe must
            // not open a `preflight_started` cycle. The probe reports the same
            // diff/queue state below, but leaving an open cycle behind is the
            // side effect that later wedges `session-check` (the empty-cycle
            // churn from the recursive owner-pane diagnostic path).
            crate::ops_log::log_op(
                file,
                &format!(
                    "preflight_probe_no_cycle file={} reason=probe_inspection_only",
                    file.display()
                ),
            );
            eprintln!("[preflight] probe: skipping preflight_started cycle (inspection only)");
        } else {
            let snap = crate::snapshot::load(file).unwrap_or(None);
            let file_content = std::fs::read_to_string(file).unwrap_or_default();
            let snap_len = snap.as_ref().map(|s| s.len()).unwrap_or(0);
            let file_len = file_content.len();
            crate::cycle_state::start_preflight(file, snap.as_deref(), Some(&file_content))?;
            crate::ops_log::log_op(
                file,
                &format!(
                    "preflight_diff_start file={} snap_len={} file_len={}",
                    file.display(),
                    snap_len,
                    file_len
                ),
            );
        }
    }

    // Step 4c: Annotate the diff with content-source markers.
    let annotated_diff = diff_result.as_ref().and_then(|d| diff::annotate_diff(d));

    // Step 4c2: Classify user-authored prompt-bearing changes across prompts, edits,
    // and response/boundary artifacts.
    let queue_active_for_prompt_extraction =
        queue_state.queue_active == Some(true) || !queue_state.queue_prompts.is_empty();
    let command_diff_result = diff_result.as_ref().map(|d| {
        if queue_active_for_prompt_extraction {
            d.clone()
        } else {
            diff::suppress_inactive_queue_additions(d, &diff_result_with_current.current)
        }
    });
    let prompt_diff_result = if slash_command_only_diff_commands.is_some() {
        None
    } else {
        command_diff_result.clone()
    };
    let mut prompt_bearing_changes = diff_result
        .as_ref()
        .map(|_| {
            prompt_diff_result
                .as_deref()
                .map(diff::classify_prompt_bearing_changes)
                .unwrap_or_default()
        })
        .unwrap_or_default();
    if raw_diff.is_some()
        && let Some(harness_only_diff) = harness_diff.as_ref()
    {
        push_unique_prompt_bearing_changes(
            &mut prompt_bearing_changes,
            diff::classify_prompt_bearing_changes(harness_only_diff),
        );
    }
    let prompt_targets =
        crate::flow::session_cycle::prompt_targets_from_changes(&prompt_bearing_changes);
    let mut added_diff_lines = prompt_diff_result
        .as_ref()
        .map(|d| crate::prompt_contract::collect_added_diff_lines(d))
        .unwrap_or_default();
    if raw_diff.is_some()
        && let Some(harness_only_diff) = harness_diff.as_ref()
    {
        push_unique_strings(
            &mut added_diff_lines,
            crate::prompt_contract::collect_added_diff_lines(harness_only_diff),
        );
    }

    // Legacy compatibility surface for older skill consumers.
    let inline_annotations = annotated_diff
        .as_ref()
        .map(|a| diff::extract_inline_annotations(a))
        .unwrap_or_default();
    let semantic_diff = semantic_diff_summary(
        &diff_result_with_current.previous,
        &diff_result_with_current.current,
        &prompt_bearing_changes,
    );

    // #op-scoped-drift-1: persist this cycle's node ops to the durable op log,
    // tagged with actor + causal (Lamport / session-origin) clock. Best effort:
    // the durable substrate must never block or fail a preflight cycle.
    if let Some(summary) = semantic_diff.as_ref() {
        persist_op_log(file, &rc, initial_frontmatter.session.as_deref(), summary);
    }

    // #op-scoped-drift-2: emit the TurnScope manifest (read/write set + driver)
    // for the prompts this turn is answering.
    let turn_scope = derive_turn_scope(&diff_result_with_current.current, &prompt_targets);

    // #nm1x: persist the scope so the later finalize-path drift gate (a separate
    // process invocation) can intersect incoming document ops against the same
    // scope. Best effort — a write failure must never block a preflight cycle, and
    // a stale scope is cleared so the gate falls back to its coarse behavior.
    match turn_scope.as_ref() {
        Some(scope) => {
            if let Err(err) = crate::turn_scope_store::save(file, scope) {
                eprintln!("[preflight] turn-scope persist skipped: {err}");
            }
        }
        None => {
            if let Err(err) = crate::turn_scope_store::delete(file) {
                eprintln!("[preflight] turn-scope clear skipped: {err}");
            }
        }
    }

    // #op-scoped-drift-3: classify this cycle's node ops against the TurnScope so
    // independent / provenance-spoofed edits integrate without affecting the turn.
    let op_affectedness = match (semantic_diff.as_ref(), turn_scope.as_ref()) {
        (Some(summary), Some(scope)) => {
            let document_path = file.to_string_lossy().to_string();
            let ops = build_ops_from_semantic_diff(
                &document_path,
                initial_frontmatter.session.as_deref(),
                "",
                summary,
            );
            Some(agent_doc_core::turn_scope::classify_cycle(&ops, scope))
        }
        _ => None,
    };

    // Step 4d: Extract slash commands from user-added diff lines (classified into skill vs built-in).
    let mut parsed_commands = command_diff_result
        .as_ref()
        .map(|d| diff::parse_slash_commands_classified(d))
        .unwrap_or_else(|| diff::ParsedSlashCommands {
            skill_commands: vec![],
            builtin_commands: vec![],
        });
    if raw_diff.is_some()
        && let Some(harness_only_diff) = harness_diff.as_ref()
    {
        let harness_commands = diff::parse_slash_commands_classified(harness_only_diff);
        push_unique_strings(
            &mut parsed_commands.skill_commands,
            harness_commands.skill_commands,
        );
        push_unique_strings(
            &mut parsed_commands.builtin_commands,
            harness_commands.builtin_commands,
        );
    }
    let slash_commands = parsed_commands.skill_commands;
    let builtin_commands = parsed_commands.builtin_commands;
    let orchestration_request = prompt_diff_result
        .as_ref()
        .and_then(|d| diff::detect_orchestration_request(d))
        .or_else(|| {
            raw_diff
                .as_ref()
                .and(harness_diff.as_ref())
                .and_then(|d| diff::detect_orchestration_request(d))
        });

    // Step 4e: Resolve model tier sources and compose effective_tier.
    // Sources (highest precedence first): inline /model command, <!-- agent:model --> component,
    // agent_doc_model_tier frontmatter, diff heuristic.
    let (
        source_frontmatter,
        frontmatter_tier,
        component_tier_value,
        frontmatter_env,
        frontmatter_model,
        frontmatter_prompt_presets,
    ) = match std::fs::read_to_string(file) {
        Ok(content) => {
            let (source_fm, fm_tier, env_map, fm_model, prompt_presets) =
                frontmatter::parse(&content)
                    .ok()
                    .map(|(fm, _)| {
                        let resolved = fm.resolve_harness_model(&harness).map(|s| s.to_string());
                        let fm_tier = fm.model_tier;
                        let env_map = fm.env.clone();
                        let prompt_presets = fm.prompt_presets.clone();
                        (fm, fm_tier, env_map, resolved, prompt_presets)
                    })
                    .unwrap_or_default();
            let comp_value = agent_doc_core::model_tier::extract_model_component(&content);
            (
                source_fm,
                fm_tier,
                comp_value,
                env_map,
                fm_model,
                prompt_presets,
            )
        }
        Err(_) => (
            frontmatter::Frontmatter::default(),
            None,
            None,
            Default::default(),
            None,
            Default::default(),
        ),
    };
    let component_tier = component_tier_value.as_deref().and_then(|v| {
        agent_doc_core::model_tier::component_value_to_tier(v, &harness, &global_config.model)
    });

    let mut prompt_presets_requested = prompt_diff_result
        .as_ref()
        .map(|d| diff::detect_prompt_preset_requests(d))
        .unwrap_or_default();
    if raw_diff.is_some()
        && let Some(harness_only_diff) = harness_diff.as_ref()
    {
        push_unique_strings(
            &mut prompt_presets_requested,
            diff::detect_prompt_preset_requests(harness_only_diff),
        );
    }
    push_unique_strings(
        &mut prompt_presets_requested,
        crate::prompt_contract::requested_prompt_presets(
            &prompt_targets,
            &added_diff_lines,
            &frontmatter_prompt_presets,
        ),
    );
    prompt_presets_requested = prompt_presets_requested
        .into_iter()
        .map(|name| {
            frontmatter::resolve_prompt_preset_key(&frontmatter_prompt_presets, &name)
                .unwrap_or(name)
        })
        .fold(Vec::new(), |mut acc, name| {
            if !acc.iter().any(|existing| existing == &name) {
                acc.push(name);
            }
            acc
        });
    let missing_prompt_presets = prompt_presets_requested
        .iter()
        .filter(|name| !frontmatter_prompt_presets.contains_key(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_prompt_presets.is_empty() {
        anyhow::bail!(
            "document references missing prompt preset(s): {}",
            missing_prompt_presets.join(", ")
        );
    }
    if let Ok(content) = std::fs::read_to_string(file) {
        if let Some(warning) =
            post_exchange_comment_prompt_preset_warning(file, &content, &frontmatter_prompt_presets)
        {
            eprintln!("[preflight] warning: {}", warning.message);
            warnings.push(warning);
        }
        if let Some(warning) = misplaced_component_attr_warning(file, &content) {
            eprintln!("[preflight] warning: {}", warning.message);
            warnings.push(warning);
        }
        if let Some(warning) = preset_item_id_collision_warning(&content) {
            eprintln!("[preflight] warning: {}", warning.message);
            warnings.push(warning);
        }
    }
    if let Ok((git_root, _)) = git::resolve_to_git_root(file)
        && let Some(warning) = stale_install_warning(&git_root)
    {
        eprintln!("[preflight] warning: {}", warning.message);
        warnings.push(warning);
    }
    let backlog_capture_required = crate::prompt_contract::prompt_requests_backlog_work(
        &prompt_targets,
        &added_diff_lines,
        &frontmatter_prompt_presets,
    );
    let explicit_backlog_targets = crate::prompt_contract::explicit_backlog_targets(
        file,
        &prompt_targets,
        &added_diff_lines,
        &frontmatter_prompt_presets,
    )?;
    let explicit_backlog_target_paths = explicit_backlog_targets
        .iter()
        .map(|path| {
            std::fs::canonicalize(path)
                .unwrap_or_else(|_| path.to_path_buf())
                .display()
                .to_string()
        })
        .collect::<Vec<_>>();
    let explicit_backlog_requirements =
        explicit_backlog_target_requirements(file, &source_frontmatter, &explicit_backlog_targets)?;
    let required_explicit_backlog_item_count = if explicit_backlog_requirements.is_empty() {
        0
    } else {
        crate::prompt_contract::required_explicit_backlog_item_count(
            &prompt_targets,
            &added_diff_lines,
            &frontmatter_prompt_presets,
            &prompt_bearing_changes,
        )
    };
    let required_plan_reference_count = crate::prompt_contract::required_plan_reference_count(
        &prompt_targets,
        &added_diff_lines,
        &frontmatter_prompt_presets,
        &prompt_bearing_changes,
    );
    // `#do-id-closeout-open-backlog`: tracked-work ids named by an explicit
    // `do [#id]` directive that are still open in the live backlog must reach a
    // lifecycle outcome before closeout. Record them so `session-check` can fail
    // closed when a directive clears the queue but leaves its target `[ ]`.
    let expect_done_or_gate_ids = {
        let directive_ids = crate::session_check::do_directive_target_ids(&prompt_targets);
        if directive_ids.is_empty() {
            Vec::new()
        } else {
            // Read the live document once for the open-backlog set.
            let parsed = std::fs::read_to_string(file).ok().and_then(|content| {
                crate::component::parse(&content)
                    .ok()
                    .map(|components| (content, components))
            });
            let open_backlog: std::collections::HashSet<String> = parsed
                .as_ref()
                .map(|(content, components)| {
                    components
                        .iter()
                        .filter(|component| crate::component::is_backlog_component(&component.name))
                        .flat_map(|component| {
                            let (_, items, _) =
                                crate::pending::parse_items(component.content(content));
                            items
                        })
                        .filter(|item| !item.is_done())
                        .map(|item| item.id)
                        .filter(|id| !id.is_empty())
                        .collect::<std::collections::HashSet<String>>()
                })
                .unwrap_or_default();
            let synced_queue_ids = queue_state
                .synced_queue_ids
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<String>>();
            filter_expect_done_or_gate_ids(&directive_ids, &open_backlog, &synced_queue_ids)
        }
    };
    if !no_changes {
        crate::cycle_state::record_backlog_capture_requirement(file, backlog_capture_required)?;
        crate::cycle_state::record_backlog_target_requirements(
            file,
            &explicit_backlog_requirements,
        )?;
        crate::cycle_state::record_expect_done_or_gate_ids(file, &expect_done_or_gate_ids)?;
        crate::cycle_state::record_required_explicit_backlog_item_count(
            file,
            required_explicit_backlog_item_count,
        )?;
        crate::cycle_state::record_required_plan_reference_count(
            file,
            required_plan_reference_count,
        )?;
    }

    // Diff heuristic — counts user-added lines (excluding +++ headers).
    let lines_added = diff_result
        .as_ref()
        .map(|d| {
            d.lines()
                .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
                .count()
        })
        .unwrap_or(0);
    let diff_type_str: Option<String> = classification.as_ref().and_then(|c| {
        serde_json::to_value(&c.diff_type)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    });
    let suggested =
        agent_doc_core::model_tier::suggested_tier(diff_type_str.as_deref(), lines_added, file);

    let model_switch_name = model_scan.as_ref().and_then(|s| s.model_switch.clone());
    let model_switch_tier = model_scan.as_ref().and_then(|s| s.model_switch_tier);
    let required_tier_value = component_tier.or(frontmatter_tier);
    let effective_tier_value = agent_doc_core::model_tier::compose_effective_tier(
        model_switch_tier,
        component_tier,
        frontmatter_tier,
        suggested,
    );

    // Step 5: Scan for pending callback requests from other processes.
    let pending_callbacks = crate::callback::scan_pending_callbacks(None).unwrap_or_default();
    if !pending_callbacks.is_empty() {
        eprintln!(
            "[preflight] found {} pending callback(s)",
            pending_callbacks.len()
        );
    }
    match crate::memory_cmd::semantic_completion_matches(file, None, 5) {
        Ok(matches) => {
            for semantic_match in matches {
                warnings.push(PreflightWarning {
                    code: "semantic_completion_match".to_string(),
                    message: crate::memory_cmd::format_semantic_completion_warning(&semantic_match),
                    document_agent: None,
                    active_harness: None,
                });
            }
        }
        Err(err) => warnings.push(PreflightWarning {
            code: "semantic_completion_retrieval_unavailable".to_string(),
            message: format!("semantic completion retrieval unavailable: {err}"),
            document_agent: None,
            active_harness: None,
        }),
    }

    let agent_model =
        resolve_agent_model(frontmatter_model.as_deref(), &harness, &global_config.model);
    let session_accretion = crate::session_accretion::inspect(file)
        .ok()
        .filter(|report| !report.is_healthy());
    // #codex-owned-pane-prompt-miss-followups: surface a structured owner-pane
    // self-invocation contract so Codex guidance can drive an in-pane response
    // cycle. Non-null only under a Codex owner-pane self-invocation with
    // unresolved exchange work (an unanswered prompt or a ready auto-queue head).
    let owned_pane_self_invocation = {
        // Derive the unresolved prompt from this cycle's diff (prompt-target
        // change) rather than the boundary-keyed exchange detector: preflight's
        // commit has already inserted a trailing boundary, which would hide a
        // freshly-committed prompt from `unresolved_exchange_prompt`.
        let unresolved_prompt = prompt_bearing_changes
            .iter()
            .find(|change| {
                matches!(
                    change.kind,
                    crate::diff::PromptBearingChangeKind::PromptTarget
                )
            })
            .map(|change| change.text.clone());
        let current = std::fs::read_to_string(file).unwrap_or_default();
        match frontmatter::parse_for_file_with_context(&current, file, &rc) {
            Ok((owner_fm, _)) => match owner_fm.session.as_deref() {
                Some(session_id) => {
                    let agent_name = owner_fm.agent.as_deref().unwrap_or("claude");
                    crate::run::detect_owned_pane_self_invocation(
                        file,
                        session_id,
                        agent_name,
                        unresolved_prompt,
                    )
                    .unwrap_or(None)
                }
                None => None,
            },
            Err(_) => None,
        }
    };

    let pipeline = resolve_pipeline_state(file)?;

    // `#queue-no-stop-unrelated-edit`: compute before the struct move so the
    // affectedness classifier can be borrowed (it is moved into the struct below).
    let user_intent_prompt_changes = compute_user_intent_prompt_changes(
        &prompt_bearing_changes,
        diff_from_queue_head_only,
        op_affectedness.as_ref(),
    );

    let output = PreflightOutput {
        warnings,
        layout_issues,
        recovered,
        committed,
        claims,
        diff: diff_result,
        no_changes,
        linked_changes,
        baseline_file,
        diff_type: diff_type_str.clone(),
        diff_type_reason: classification.map(|c| c.diff_type_reason),
        annotated_diff,
        semantic_diff,
        turn_scope,
        op_affectedness,
        user_intent_prompt_changes,
        prompt_bearing_changes,
        inline_annotations,
        slash_commands,
        builtin_commands,
        orchestration_request,
        prompt_presets_requested,
        explicit_backlog_targets: explicit_backlog_target_paths,
        effective_tier: Some(effective_tier_value.to_string()),
        required_tier: required_tier_value.map(|t| t.to_string()),
        suggested_tier: Some(suggested.to_string()),
        model_switch: model_switch_name,
        model_switch_tier: model_switch_tier.map(|t| t.to_string()),
        pending_callbacks,
        owned_pane_self_invocation,
        env: frontmatter_env,
        pending_reordered,
        pending_gated_count,
        review_count: pending_report.review_count,
        review_gated_count: pending_report.review_gated_count,
        gate_verify: gate_verify_results,
        agent_model,
        queue_prompts: queue_state.queue_prompts,
        queue_active: queue_state.queue_active,
        queue_deferred: queue_state.queue_deferred,
        queue_start_at: queue_state.queue_start_at,
        queue_trigger: queue_state.queue_trigger,
        queue_halted: queue_state.queue_halted,
        session_accretion,
        pipeline,
    };

    let json =
        serde_json::to_string_pretty(&output).context("failed to serialize preflight output")?;
    println!("{}", json);

    Ok(())
}
