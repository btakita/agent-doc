//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_diff as diff;
use agent_doc_diff::semantic::semantic_diff_summary;
use agent_doc_frontmatter::frontmatter;
use agent_doc_preflight_io::{
    PendingMaintenanceReport, QueueState, check_linked_docs, enforce_no_dropped_backlog,
    enforce_no_shadow_open_backlog, explicit_backlog_target_requirements, inspect_queue_state,
    read_and_truncate_claims, read_claims, resolve_pipeline_state, run_gate_verify,
    run_pending_maintenance, run_queue_maintenance, save_baseline_content,
    sweep::{current_sweep_owner, log_and_skip_foreign_owned_sweep_if_needed, sweep_owner_for_doc},
};
use agent_doc_preflight_runtime_io::{
    relocate_out_of_exchange_prompt_before_diff,
    remove_duplicate_answered_exchange_prompt_tail_for_preflight,
    remove_post_exchange_duplicate_prompt_comments_for_preflight,
    resolve_current_preflight_document,
};
use agent_doc_prompt_contract::{push_unique_prompt_bearing_changes, push_unique_strings};
use agent_doc_queue::queue_convergence::{
    queue_body_diff_is_non_selected_future_state, realign_baseline_to_converged_queue,
};
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_turn::drain_stall::{StallFacts, StallVerdict, classify_stall};
use agent_doc_workflow::session_cycle::{compute_user_intent_prompt_changes, derive_turn_scope};
use anyhow::Context;

/// Injects the lazily reactive CRDT model as the diff's `current` source
/// (#preflight-lazily-diff-feed). Lives here (not in `agent-doc-diff-io`)
/// because the diff crate is a leaf beneath the relay crate and cannot depend
/// on the reactive model without a dependency cycle.
struct ReactiveLiveCurrentSource;

impl agent_doc_diff_io::LiveCurrentSource for ReactiveLiveCurrentSource {
    fn live_current(&self, doc: &Path, disk: &str) -> Option<String> {
        // `durable_buffer_state` returns `Some` only when a live editor buffer
        // (CRDT relay / controller model) has diverged from disk. That read is
        // already quiescence-gated by the commit barrier (#preflight-lazily-diff-feed
        // Phase 3): the relay surfaces `CurrentText::Current` only once the
        // canonical replica *covers every live editor's ops*, so the returned
        // text is prompt-complete by construction — not a mid-typing partial.
        // (`delivery_converged` is a downstream-ACK signal, a different
        // direction, and is intentionally NOT gated on: it would reject complete
        // canonical text over a pending fan-out ACK.) Otherwise `None`, so the
        // diff keeps its disk-sourced content.
        agent_doc_document_realtime_io::durable_buffer_state(doc, disk).map(|state| state.content)
    }
}

/// Options controlling a `preflight` invocation.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreflightOptions {
    /// Pure inspection probe (`#preflight-probe-side-effect-free`): compute and
    /// emit the same JSON, but do NOT open a `preflight_started` cycle. A
    /// diagnostic preflight is not dispatch/response-bound, so opening a cycle
    /// only leaves open state that later wedges `session-check`.
    pub probe: bool,
}

fn closeout_cycle_is_open(file: &Path) -> Result<bool> {
    let projection = agent_doc_cycle_state_io::load_closeout_projection(file)?;
    if let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? {
        if let Some(projection) = projection.as_ref()
            && !projection.matches_cycle(&state.cycle_id)
            && projection.phase_is_open().unwrap_or(false)
        {
            return Ok(true);
        }
        return Ok(state.is_open());
    }
    Ok(projection
        .and_then(|projection| projection.phase_is_open())
        .unwrap_or(false))
}

/// Run preflight with default (dispatch/response-bound) options.
pub fn run(file: &Path) -> Result<()> {
    run_with_options(file, PreflightOptions::default())
}

fn maybe_record_preflight_terminal_closeout_proof(file: &Path, did_commit: bool) {
    let Ok(agent_doc_session_check_io::SessionCheckStatus::Ok(_)) =
        agent_doc_session_check_io::inspect(
            file,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
    else {
        return;
    };
    if let Err(err) = agent_doc_controller_io::project_controller::persist_session_actor_closeout(
        file,
    )
    .and_then(|_| {
        agent_doc_flow_io::closeout::record_terminal_closeout_proof(
            file,
            did_commit,
            &agent_doc_closeout_runtime_io::closeout_effects(),
            agent_doc_flow_io::closeout::CompleteRequiredCloseoutOptions::default(),
        )
    }) {
        eprintln!("[preflight] terminal proof warning: {err}");
    }
}

pub fn run_with_options(file: &Path, options: PreflightOptions) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // #rtwwire (rung 3): classify against the realtime document model. When an
    // editor is active, the CRDT relay is the authority and disk is not read as
    // a substitute; with no editor attached, disk is the fallback replica.
    let content = resolve_current_preflight_document(file, "initial")?;
    // Phase 1 lossless-tree shadow projection (#lzlosstree): build the lossless
    // tree from the authoritative current text, render it, and log whether it
    // round-trips. Pure and non-blocking — the flat CRDT stays the authority and
    // this never influences any closeout decision. A `match=false` line is the
    // signal to investigate the adapter before later phases can trust the tree.
    if !options.probe {
        agent_doc_ops_log_io::log_op(
            file,
            &agent_doc_markdown_lossless::shadow_audit_ops_log_line(&content, "preflight_initial"),
        );
    }
    let pre_mutation_unresolved_exchange_prompt =
        agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&content);
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    let (initial_frontmatter, _) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &content,
        file,
        &rc.ssh_context(),
    )?;
    let active_harness = rc.harness();
    let mut warnings = Vec::new();
    for warning in agent_doc_preflight_io::warnings::initial_warnings(
        file,
        initial_frontmatter.agent.as_deref(),
        &active_harness,
        initial_frontmatter.codex_network_access.is_some(),
    ) {
        eprintln!("[preflight] warning: {}", warning.message);
        warnings.push(warning);
    }

    // Step 0a: Auto-GC (at most once per day).
    // Checks .agent-doc/gc.stamp — if missing or >24 hours old, runs lightweight GC.
    if !options.probe {
        agent_doc_preflight_io::gc::run_preflight_auto_gc(file);
    }

    // Pre-mutation debounce: recovery, pending maintenance, commit, and
    // duplicate-residue cleanup all write the visible document or its sidecars.
    // Do not let those paths race an editor buffer that is still publishing a
    // prompt.
    if !options.probe {
        agent_doc_preflight_io::debounce::wait_for_typing_idle_before_mutation(file)?;
    }

    // Step 0-pre: interrupted-cycle guard (#cyc1). Use exact persisted cycle
    // state instead of inferring solely from `ops.log`.
    let (recovered_prior, committed_prior) = if options.probe {
        (false, false)
    } else {
        enforce_cycle_completion(file)?
    };

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
    if !options.probe {
        maybe_auto_resync_on_drift(file, &layout_issues);
    }

    // Step 0c: Auto-repair base-index compliance — when window index 0 is
    // missing, run repair_layout immediately so this preflight reports the
    // post-repair layout state.
    if !options.probe && maybe_auto_repair_base_index(file, &layout_issues) {
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
    let open_cycle = closeout_cycle_is_open(file)?;
    if !options.probe
        && !open_cycle
        && agent_doc_session_check_io::detect_unstarted_prompt_bearing_diff(file)?.is_none()
    {
        agent_doc_preflight_runtime_io::enforce_no_uncommitted_closeout_drift(
            file,
            &rc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )?;
    }

    // Step 1: Recover orphaned pending responses.
    eprintln!("[preflight] step 1: repair");
    // #queue-active-deprecated-line-stuck: drop a legacy `queue_active:` line that
    // is stuck in the document because the diff layer classifies it as managed
    // state (so its removal never reads as a diff and is never committed) and the
    // byte-precise hot path never re-serializes frontmatter through `write()`
    // (which would drop it). Strip it directly on disk + snapshot, but ONLY when
    // the canonical `queue:` control is present so no queue state is lost. Idempotent.
    if !options.probe {
        let migrated = frontmatter::strip_deprecated_queue_active_line(&content);
        if migrated != content {
            match agent_doc_document_realtime_io::atomic_write_through_authority(file, &migrated) {
                Ok(()) => {
                    if let Err(err) =
                        agent_doc_snapshot_io::save(file, &migrated, agent_doc_ops_log_io::log_op)
                    {
                        eprintln!(
                            "[preflight] warning: dropped deprecated queue_active line but failed to update snapshot for {}: {err}",
                            file.display()
                        );
                    }
                    agent_doc_ops_log_io::log_op(
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
    if !options.probe {
        match agent_doc_flow_io::closeout::reconcile_compacted_committed_capture(file) {
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
    }
    if let Some(info) = agent_doc_flow_io::closeout::stuck_captured_cycle(file) {
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
        || if options.probe {
            false
        } else {
            match agent_doc_repair_io::run(
                agent_doc_repair_runtime_io::repair_coordinator_effects(
                    &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
                ),
                file,
            ) {
                Ok(outcome) => outcome.repaired(),
                Err(e) => {
                    let message = e.to_string();
                    if message.contains(
                        agent_doc_turn::repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR,
                    ) || message
                        .contains(agent_doc_turn::repair::EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR)
                    {
                        return Err(e);
                    }
                    eprintln!("[preflight] repair warning: {}", e);
                    false
                }
            }
        };

    // Step 1b: Ensure document is initialized (snapshot + git baseline).
    // If no snapshot exists, creates one and commits the file.
    if !options.probe
        && let Err(e) = agent_doc_workflow_io::document_init::ensure_initialized(
            file,
            agent_doc_commit_io::commit,
            agent_doc_ops_log_io::log_op,
        )
    {
        eprintln!("[preflight] warning: auto-init failed: {}", e);
    }

    // Step 1b2: Fail closed on out-of-band closeout drift before this preflight
    // mutates backlog state or runs the generic commit path. Otherwise a
    // snapshot/file pair that already contains a visible response could be
    // normalized into a misleading `no_changes` result.
    if !options.probe {
        agent_doc_preflight_runtime_io::enforce_no_uncommitted_closeout_drift(
            file,
            &rc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )?;
    }

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
    let pending_report = if options.probe {
        PendingMaintenanceReport::default()
    } else {
        run_pending_maintenance(file, &PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)?
    };
    let backlog_reordered = pending_report.reordered;
    let backlog_gated_count = pending_report.backlog_gated_count;

    // `#optverify`: opportunistic gated-review auto-verification. Runs before the
    // step-2 commit so any opt-in `[/]→[x]` flip is staged atomically (the
    // mutation touches both the working-tree file and the snapshot). Default off
    // — without the opt-in the gate status is only surfaced, never flipped.
    let gate_autoverify_optin = initial_frontmatter
        .gate_autoverify
        .or(rc.project_config().agent_doc_gate_autoverify)
        .unwrap_or(false);
    let gate_verify_results = if options.probe {
        Vec::new()
    } else {
        match run_gate_verify(
            file,
            gate_autoverify_optin,
            &PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
        ) {
            Ok(results) => results,
            Err(e) => {
                eprintln!("[preflight] optverify: scan skipped: {}", e);
                Vec::new()
            }
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
    enforce_no_dropped_backlog(file, rc.head_content().as_deref().map(String::as_str))?;
    if !options.probe && remove_duplicate_answered_exchange_prompt_tail_for_preflight(file)? {
        recovered = true;
    }
    if !options.probe && remove_post_exchange_duplicate_prompt_comments_for_preflight(file, &rc)? {
        recovered = true;
    }

    // Step 2: Commit previous cycle.
    eprintln!("[preflight] step 2: commit");
    let mut did_commit_this_preflight = committed_prior;
    let committed = committed_prior
        || if options.probe {
            false
        } else {
            match commit_previous_cycle_for_preflight(file) {
                Ok(did_commit) => {
                    if did_commit {
                        rc.invalidate_head_content();
                        did_commit_this_preflight = true;
                    }
                    did_commit
                }
                Err(e) => {
                    eprintln!("[preflight] commit warning: {}", e);
                    false
                }
            }
        };
    if !options.probe && committed {
        maybe_record_preflight_terminal_closeout_proof(
            file,
            did_commit_this_preflight || committed_prior,
        );
    }

    if !options.probe && relocate_out_of_exchange_prompt_before_diff(file)? {
        recovered = true;
    }
    if !options.probe && remove_duplicate_answered_exchange_prompt_tail_for_preflight(file)? {
        recovered = true;
    }
    if !options.probe && remove_post_exchange_duplicate_prompt_comments_for_preflight(file, &rc)? {
        recovered = true;
    }

    // Step 2d: Cross-document sweep (Fix 5) — commit any other tracked docs in the same
    // project that have uncommitted snapshot content. Turns preflight into a catch-all
    // backstop: even if a previous session's commit was skipped, the next preflight
    // from any document in the project will pick it up.
    if !options.probe {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(root) = agent_doc_project_root_io::project_root_containing(&canonical)
            && let Ok(registry) = agent_doc_session_registry_io::load_in(&root)
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
                let snap_rel = match agent_doc_fs::snapshot_path_for(&doc_path) {
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
                    if log_and_skip_foreign_owned_sweep_if_needed(
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
                    ) && !agent_doc_diff::is_stale_snapshot(&snap_content, &doc_content)
                    {
                        // Not a stale inline snapshot — check content equality
                        // (covers template mode where is_stale_snapshot always returns false)
                        let snap_stripped = agent_doc_diff::strip_comments(&snap_content);
                        let doc_stripped = agent_doc_diff::strip_comments(&doc_content);
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
                    let fresh = agent_doc_git_io::revision::last_commit_mtime(&doc_path)
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
                    match agent_doc_commit_io::commit(&doc_path) {
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
    if !options.probe {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(root) = agent_doc_project_root_io::project_root_containing(&canonical) {
            match agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
                &root,
                false,
                "preflight",
                "stale_dead_pane_actor",
            ) {
                Ok((closed, kept)) if closed > 0 => {
                    eprintln!(
                        "[preflight] actors: {} stale dead-pane closed, {} still active",
                        closed, kept
                    );
                }
                Ok(_) => {}
                Err(e) => eprintln!("[preflight] dead-pane actor gc warning: {}", e),
            }
        }
    }

    // Step 3: Read and truncate the claims log.
    eprintln!("[preflight] step 3: claims");
    let claims = if options.probe {
        read_claims(file)
    } else {
        read_and_truncate_claims(file)
    };

    // Step 3b: Wait for file to settle (mtime + typing indicator debounce).
    // Check both file mtime (disk-level) and cross-process typing indicator
    // (buffer-level) to avoid picking up mid-typing edits.
    // Default: 2000ms (configurable via `agent_doc_debounce` frontmatter field).
    if !options.probe {
        agent_doc_preflight_io::debounce::wait_for_preflight_debounce(file);
    }

    // Step 3c: Check related documents for changes.
    eprintln!("[preflight] step 3c: related docs");
    let linked_changes = if options.probe {
        Vec::new()
    } else {
        check_linked_docs(file)
    };
    for change in &linked_changes {
        eprintln!(
            "[preflight] related doc change: {} — {}",
            change.path, change.summary
        );
    }

    // Step 4: Compute diff between snapshot and current document.
    eprintln!("[preflight] step 4: diff");
    // Lazily reactive state-diff feed (#preflight-lazily-diff-feed): after the
    // diff settles on the disk buffer for prompt-completeness, source the
    // coherent `current` from the reactive CRDT model when the operator's live
    // edits have not yet reached disk. `durable_buffer_state` returns `Some`
    // only when a live editor buffer has diverged from disk; otherwise the diff
    // keeps its disk-sourced content. This is the seam that lets the realtime
    // document model — not a disk read race — own concurrent typing.
    let live_current_source = ReactiveLiveCurrentSource;
    let diff_result_with_current = agent_doc_diff_io::compute_with_current(
        &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
        file,
        Some(&live_current_source),
    )?;
    // Save the response baseline from the exact stable document projection used
    // for the diff. This keeps the merge baseline, visible file, and prompt
    // contract in one transaction even if an editor replay lands during the
    // earlier debounce window.
    let baseline_file = if options.probe {
        None
    } else {
        save_baseline_content(file, &diff_result_with_current.current)
    };
    let raw_diff = diff_result_with_current.diff;
    let harness_diff = agent_doc_harness::prompt_source::synthetic_diff_for_file(
        file,
        agent_doc_codex_hook_io::load_prompt_for_current_session,
    )?;
    let initial_diff = raw_diff.clone().or(harness_diff.clone());

    // Step 4a: Scan diff for inline `/model <x>` command and strip the matching
    // line(s) before downstream classification. The strip prevents `/model` from
    // double-emitting in `builtin_commands`.
    let global_config = agent_doc_config::load().unwrap_or_default();
    let harness = rc.harness();
    let model_scan = initial_diff
        .as_ref()
        .map(|d| agent_doc_model_tier::scan_model_switch(d, &harness, &global_config.model));
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
    let queue_state = if options.probe {
        inspect_queue_state(file, diff_result.as_deref()).unwrap_or_else(|e| {
            eprintln!("[preflight] queue probe warning: {}", e);
            QueueState::default()
        })
    } else {
        run_queue_maintenance(file, diff_result.as_deref()).unwrap_or_else(|e| {
            eprintln!("[preflight] queue maintenance warning: {}", e);
            QueueState::default()
        })
    };
    warnings.extend(queue_state.warnings.clone());
    // #qconvbaseline: the queue maintenance above may converge the live editor
    // buffer to a corrected queue shape (auto-pins, backlog→queue mirrors, do-prompt
    // sort, queue-control frontmatter) via IPC WITHOUT a disk write when a plugin
    // listener is active — the converged shape lands in the editor buffer + the
    // snapshot, but disk (and the baseline saved just above, from pre-maintenance
    // disk) stay behind. At finalize the converged editor buffer then reads as
    // `live_prompt_drift_after_preflight` on EVERY cycle: its queue body differs
    // outside `exchange` from both the baseline and `content_ours`, so the drift
    // guard blocks the response write and forces the `content_ours` carry-forward +
    // a recovery commit (the race the operator hit repeatedly editing the doc).
    // Re-align the baseline with the post-maintenance snapshot (the queue shape now
    // in the editor buffer) so `content_ours` matches the buffer outside `exchange`
    // and ONLY genuine concurrent user edits trip the guard. No-listener sessions
    // are unaffected — their disk write already matches the snapshot, and when
    // nothing converged the snapshot equals the pre-maintenance content so this is
    // a no-op.
    if !options.probe
        && let Ok(Some(converged_snapshot)) = agent_doc_snapshot_io::load(file)
        && let Some(realigned) = realign_baseline_to_converged_queue(
            &diff_result_with_current.current,
            &converged_snapshot,
        )
    {
        let _ = save_baseline_content(file, &realigned);
        eprintln!("[preflight] baseline re-aligned to converged queue shape (#qconvbaseline)");
    }
    if diff_result.is_some()
        && queue_state.queue_active == Some(true)
        && let Some(selected_head) = queue_state
            .selected_queue_prompts
            .first()
            .or_else(|| queue_state.queue_prompts.first())
        && queue_body_diff_is_non_selected_future_state(
            &diff_result_with_current.previous,
            &diff_result_with_current.current,
            selected_head,
        )
    {
        diff_result = None;
        classification = None;
        eprintln!("[preflight] queue: absorbed non-selected queue edit into future queue state");
    }
    let diff_has_prompt_bearing_changes = diff_result.as_deref().is_some_and(|d| {
        diff::classify_prompt_bearing_changes(d)
            .iter()
            .any(|change| {
                matches!(
                    change.kind,
                    agent_doc_diff::PromptBearingChangeKind::PromptTarget
                        | agent_doc_diff::PromptBearingChangeKind::ContentEdit
                )
            })
    });
    if !diff_has_prompt_bearing_changes {
        let unresolved_exchange_prompt = match pre_mutation_unresolved_exchange_prompt.clone() {
            Some(prompt) => Some(prompt),
            None => match agent_doc_session_check_io::unresolved_exchange_prompt(file) {
                Ok(prompt) => prompt,
                Err(err) => {
                    warnings.push(PreflightWarning {
                        code: "unresolved_exchange_prompt_unavailable".to_string(),
                        message: format!(
                            "could not inspect unresolved exchange prompt before queue selection: {err}"
                        ),
                        document_agent: None,
                        active_harness: None,
                    });
                    None
                }
            },
        };
        if let Some(unresolved) = unresolved_exchange_prompt
            && !unresolved.trim().is_empty()
        {
            let unresolved = unresolved.trim();
            let prompt_source = if unresolved.starts_with('❯') {
                unresolved.to_string()
            } else {
                format!("❯ {unresolved}")
            };
            diff_result = Some(diff::synthetic_added_lines_diff(&prompt_source, "exchange"));
            classification = diff_result.as_ref().map(|d| diff::classify_diff(d));
            eprintln!(
                "[preflight] queue: deferred active queue for unresolved exchange prompt \
                 (#prompt-preempts-auto-queue)"
            );
        }
    }
    // `#agent-doc-bug` auto-queue stall: when there is no real user/document diff
    // this cycle, an active queue head is synthesized as the cycle's prompt diff.
    // That synthetic head is queue *continuation*, not user intent — so it must
    // NOT populate `user_intent_prompt_changes`, or the skill's auto-loop
    // precondition (`user_intent_prompt_changes` empty) never holds and the
    // `auto` queue stalls after every item. A real user prompt typed mid-queue
    // keeps `diff_result` non-None here, so this flag stays false and the
    // prompt is surfaced normally.
    // `#rt83`: only synthesize the queue head as a prompt diff when SOME drainer
    // will actually act on it — the queue is not controller-paused AND the head is
    // supervisor-scope drainable (`[operator-verify]`/noise heads excluded). An
    // operator-verify-only or paused queue previously synthesized a phantom
    // `+:pushpin: do [#id]` add every preflight, keeping `no_changes:false` and
    // sustaining the qchurn flood even though no head was actionable. The supervisor
    // idle-watch dispatches `[focused-cycle]`/`[clean-session]` heads via its own
    // `live_drainable_continuation_head` check, so suppressing the synthetic diff for
    // non-drainable heads does not strand legitimate supervisor-driven continuation.
    let mut diff_from_queue_head_only = false;
    if diff_result.is_none()
        && !queue_state.queue_paused
        && queue_state.queue_supervisor_drainable
        && let Some(head_prompt) = queue_state
            .selected_queue_prompts
            .first()
            .or_else(|| queue_state.queue_prompts.first())
    {
        let slash_command = agent_doc_queue::queue_command::slash_command_text(head_prompt);
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
            if !options.probe {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "preflight_slash_command_only_handoff file={} commands={:?}",
                        file.display(),
                        commands
                    ),
                );
            }
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
            eprintln!("[preflight] probe: skipping preflight_started cycle (inspection only)");
        } else {
            let snap = agent_doc_snapshot_io::load(file).unwrap_or(None);
            let file_content = resolve_current_preflight_document(file, "start_preflight")?;
            let snap_len = snap.as_ref().map(|s| s.len()).unwrap_or(0);
            let file_len = file_content.len();
            agent_doc_cycle_state_io::start_preflight(file, snap.as_deref(), Some(&file_content))?;
            agent_doc_ops_log_io::log_op(
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
        agent_doc_workflow::session_cycle::prompt_targets_from_changes(&prompt_bearing_changes);
    let directive_target_ids =
        agent_doc_queue::queue_directive::do_directive_target_ids(&prompt_targets);
    let checkpoint_queue_task_id = directive_target_ids.first().map(String::as_str);
    if !options.probe {
        agent_doc_cycle_state_io::record_turn_checkpoint(
            file,
            baseline_file.as_deref(),
            &prompt_targets,
            checkpoint_queue_task_id,
            checkpoint_queue_task_id,
        )?;
    }
    let mut added_diff_lines = prompt_diff_result
        .as_ref()
        .map(|d| agent_doc_prompt_contract::collect_added_diff_lines(d))
        .unwrap_or_default();
    if raw_diff.is_some()
        && let Some(harness_only_diff) = harness_diff.as_ref()
    {
        push_unique_strings(
            &mut added_diff_lines,
            agent_doc_prompt_contract::collect_added_diff_lines(harness_only_diff),
        );
    }

    // Surface inline annotations separately from prompt-intent routing.
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
    if !options.probe
        && let Some(summary) = semantic_diff.as_ref()
        && let Some(project_root) = rc.project_root()
    {
        let document_path = file.to_string_lossy().to_string();
        if let Err(err) = agent_doc_sqlite::op_log::append_semantic_diff_ops(
            &project_root,
            &document_path,
            initial_frontmatter.session.as_deref(),
            summary,
        ) {
            eprintln!("[preflight] op-log persist skipped: {err}");
        }
    }

    // #op-scoped-drift-2: emit the TurnScope manifest (read/write set + driver)
    // for the prompts this turn is answering.
    let turn_scope = derive_turn_scope(&diff_result_with_current.current, &prompt_targets);

    // Capture the previously persisted owner-turn scope before this preflight
    // considers writing a new scope. A same-owner recursive preflight caused by
    // a sibling queue edit must compare against, and preserve, the active
    // owner's original turn scope (#cwsp).
    let persisted_turn_scope = agent_doc_turn_scope_io::load(file);

    // #op-scoped-drift-3: classify this cycle's node ops against the TurnScope so
    // independent / provenance-spoofed edits integrate without affecting the turn.
    let op_affectedness = match (semantic_diff.as_ref(), turn_scope.as_ref()) {
        (Some(summary), Some(scope)) => {
            let document_path = file.to_string_lossy().to_string();
            let ops = agent_doc_turn::op_log::build_ops_from_semantic_diff(
                &document_path,
                initial_frontmatter.session.as_deref(),
                "",
                summary,
            );
            Some(agent_doc_turn::turn_scope::classify_cycle(&ops, scope))
        }
        _ => None,
    };
    let active_scope_cycle_is_open = closeout_cycle_is_open(file).unwrap_or(false);
    let active_turn_affectedness = match (
        active_scope_cycle_is_open,
        semantic_diff.as_ref(),
        persisted_turn_scope.as_ref(),
    ) {
        (true, Some(summary), Some(scope)) => {
            let document_path = file.to_string_lossy().to_string();
            let ops = agent_doc_turn::op_log::build_ops_from_semantic_diff(
                &document_path,
                initial_frontmatter.session.as_deref(),
                "",
                summary,
            );
            Some(agent_doc_turn::turn_scope::classify_cycle(&ops, scope))
        }
        _ => None,
    };
    let prompt_edit_independent_of_active_turn =
        active_turn_affectedness
            .as_ref()
            .is_some_and(|affectedness| {
                !affectedness.turn_affected && !affectedness.classified.is_empty()
            });

    // #nm1x: persist the scope so the later finalize-path drift gate (a separate
    // process invocation) can intersect incoming document ops against the same
    // scope. Best effort — a write failure must never block a preflight cycle, and
    // a stale scope is cleared so the gate falls back to its coarse behavior.
    //
    // #cwsp: when the diff is independent of an already-open active owner turn,
    // do not replace that owner's persisted scope with the sibling queue edit's
    // derived scope. The edit must stay as document state until the current
    // closeout merges it.
    if !options.probe && !prompt_edit_independent_of_active_turn {
        match turn_scope.as_ref() {
            Some(scope) => {
                if let Err(err) = agent_doc_turn_scope_io::save(file, scope) {
                    eprintln!("[preflight] turn-scope persist skipped: {err}");
                }
            }
            None => {
                if let Err(err) = agent_doc_turn_scope_io::delete(file) {
                    eprintln!("[preflight] turn-scope clear skipped: {err}");
                }
            }
        }
    }

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
        model_source_content,
    ) = {
        let content = resolve_current_preflight_document(file, "model_tier_sources")?;
        let (source_fm, fm_tier, env_map, fm_model, prompt_presets) = frontmatter::parse(&content)
            .ok()
            .map(|(fm, _)| {
                let resolved = fm.resolve_harness_model(&harness).map(|s| s.to_string());
                let fm_tier = fm.model_tier;
                let env_map = fm.env.clone();
                let prompt_presets = fm.prompt_presets.clone();
                (fm, fm_tier, env_map, resolved, prompt_presets)
            })
            .unwrap_or_default();
        let comp_value = agent_doc_model_tier::extract_model_component(&content);
        (
            source_fm,
            fm_tier,
            comp_value,
            env_map,
            fm_model,
            prompt_presets,
            content,
        )
    };
    let component_tier = component_tier_value.as_deref().and_then(|v| {
        agent_doc_model_tier::component_value_to_tier(v, &harness, &global_config.model)
    });

    let prompt_preset_resolution = agent_doc_prompt_contract::resolve_prompt_preset_requests(
        prompt_diff_result.as_deref(),
        raw_diff
            .as_ref()
            .and(harness_diff.as_ref())
            .map(String::as_str),
        &prompt_targets,
        &added_diff_lines,
        &frontmatter_prompt_presets,
    );
    if !prompt_preset_resolution.missing.is_empty() {
        anyhow::bail!(
            "document references missing prompt preset(s): {}",
            prompt_preset_resolution.missing.join(", ")
        );
    }
    let prompt_presets_requested = prompt_preset_resolution.requested;
    for warning in agent_doc_preflight_io::warnings::content_and_staleness_warnings(
        file,
        &model_source_content,
        &frontmatter_prompt_presets,
    ) {
        eprintln!("[preflight] warning: {}", warning.message);
        warnings.push(warning);
    }
    let backlog_capture_required = agent_doc_prompt_contract::prompt_requests_backlog_work(
        &prompt_targets,
        &added_diff_lines,
        &frontmatter_prompt_presets,
    );
    let explicit_backlog_targets = agent_doc_prompt_contract::explicit_backlog_targets(
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
        agent_doc_prompt_contract::required_explicit_backlog_item_count(
            &prompt_targets,
            &added_diff_lines,
            &frontmatter_prompt_presets,
            &prompt_bearing_changes,
        )
    };
    let required_plan_reference_count = agent_doc_prompt_contract::required_plan_reference_count(
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
        if directive_target_ids.is_empty() {
            Vec::new()
        } else {
            let open_backlog: std::collections::HashSet<String> =
                agent_doc_element_backlog::backlog::open_backlog_ids_in_content(
                    &model_source_content,
                )
                .into_iter()
                .collect();
            let synced_queue_ids = queue_state
                .synced_queue_ids
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<String>>();
            agent_doc_queue::queue_directive::filter_expect_done_or_gate_ids(
                &directive_target_ids,
                &open_backlog,
                &synced_queue_ids,
            )
        }
    };
    if !options.probe && !no_changes {
        agent_doc_cycle_state_io::record_backlog_capture_requirement(
            file,
            backlog_capture_required,
        )?;
        agent_doc_cycle_state_io::record_backlog_target_requirements(
            file,
            &explicit_backlog_requirements,
        )?;
        agent_doc_cycle_state_io::record_expect_done_or_gate_ids(file, &expect_done_or_gate_ids)?;
        agent_doc_cycle_state_io::record_required_explicit_backlog_item_count(
            file,
            required_explicit_backlog_item_count,
        )?;
        agent_doc_cycle_state_io::record_required_plan_reference_count(
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
        agent_doc_model_tier::suggested_tier(diff_type_str.as_deref(), lines_added, file);

    let model_switch_name = model_scan.as_ref().and_then(|s| s.model_switch.clone());
    let model_switch_tier = model_scan.as_ref().and_then(|s| s.model_switch_tier);
    let required_tier_value = component_tier.or(frontmatter_tier);
    let effective_tier_value = agent_doc_model_tier::compose_effective_tier(
        model_switch_tier,
        component_tier,
        frontmatter_tier,
        suggested,
    );

    // Step 5: Scan for pending callback requests from other processes.
    let pending_callbacks = agent_doc_callback_io::scan_pending_callbacks(None).unwrap_or_default();
    if !pending_callbacks.is_empty() {
        eprintln!(
            "[preflight] found {} pending callback(s)",
            pending_callbacks.len()
        );
    }
    warnings.extend(agent_doc_preflight_io::warnings::semantic_completion_warnings(file));

    let agent_model = agent_doc_model_tier::resolve_agent_model(
        frontmatter_model.as_deref(),
        &harness,
        &global_config.model,
    );
    let session_accretion = agent_doc_session_accretion_io::inspect(file)
        .ok()
        .filter(|report| !report.is_healthy());

    // `#queue-no-stop-unrelated-edit`: compute before owner-pane detection so
    // same-pane recursion signals use only prompt changes that affect this turn.
    let mut user_intent_prompt_changes = compute_user_intent_prompt_changes(
        &prompt_bearing_changes,
        diff_from_queue_head_only,
        op_affectedness.as_ref(),
    );
    if prompt_edit_independent_of_active_turn {
        user_intent_prompt_changes.clear();
    }
    let exchange_prompt_preempts_queue = !user_intent_prompt_changes.is_empty();

    // #codex-owned-pane-prompt-miss-followups: surface a structured owner-pane
    // self-invocation contract so Codex guidance can drive an in-pane response
    // cycle. Non-null only under a Codex owner-pane self-invocation with
    // unresolved exchange work (an unanswered prompt or a ready auto-queue head).
    let owned_pane_self_invocation = {
        // Derive the unresolved prompt from this cycle's diff (prompt-target
        // change) rather than the boundary-keyed exchange detector: preflight's
        // commit has already inserted a trailing boundary, which would hide a
        // freshly-committed prompt from `unresolved_exchange_prompt`.
        let unresolved_prompt = user_intent_prompt_changes
            .iter()
            .find(|change| {
                matches!(
                    change.kind,
                    agent_doc_diff::PromptBearingChangeKind::PromptTarget
                )
            })
            .map(|change| change.text.clone());
        let suppress_active_queue_head = exchange_prompt_preempts_queue
            || (!diff_from_queue_head_only
                && !prompt_bearing_changes.is_empty()
                && user_intent_prompt_changes.is_empty()
                && (prompt_edit_independent_of_active_turn
                    || op_affectedness.as_ref().is_some_and(|affectedness| {
                        !affectedness.turn_affected && !affectedness.classified.is_empty()
                    })));
        match agent_doc_frontmatter_io::session::parse_for_file_with_context(
            &model_source_content,
            file,
            &rc.ssh_context(),
        ) {
            Ok((owner_fm, _)) => match owner_fm.session.as_deref() {
                Some(session_id) => {
                    let agent_name = owner_fm.agent.as_deref().unwrap_or("claude");
                    agent_doc_run_io::detect_owned_pane_self_invocation_with_options(
                        file,
                        session_id,
                        agent_name,
                        unresolved_prompt,
                        agent_doc_workflow::owner_pane_self_invocation::OwnedPaneSelfInvocationOptions {
                            suppress_active_queue_head,
                        },
                    )
                    .unwrap_or(None)
                }
                None => None,
            },
            Err(_) => None,
        }
    };

    let pipeline = resolve_pipeline_state(file)?;

    // #semmerge-ack-turn (Phase 4): surface acks carried forward by
    // `start_preflight` from the prior cycle's convergence semantic merge. Also
    // emit a companion warning so the existing "surface warnings" skill path
    // drives the acknowledgement without a SKILL.md change.
    let document_cell_merge_acks =
        agent_doc_cycle_state_io::load_pending_semantic_merge_acks(file).unwrap_or_default();
    if let Some(warning) =
        agent_doc_preflight_io::warnings::document_cell_merge_ack_warning(&document_cell_merge_acks)
    {
        warnings.push(warning);
    }

    // `#wd40` / `#staleloop-recycle-restart`: a stale route-owned supervisor that
    // can never reach its own recycle boundary during a continuously self-draining
    // session asks the in-session loop to yield. While that request is live, drop
    // `queue_continuation_required` so the loop ends its turn (the idle boundary
    // lets the `execve` recycle fire), and surface the recycle-yield guidance so
    // the agent understands the yield is intentional and resumes after recycle.
    let recycle_yield_pending =
        agent_doc_controller_io::project_controller::supervisor_recycle_yield_pending_for_file(
            file,
        );
    let effective_queue_continuation_required =
        queue_state.queue_continuation_required && !exchange_prompt_preempts_queue;
    let effective_continuation = agent_doc_queue::queue_continuation::effective_continuation_output(
        effective_queue_continuation_required,
        recycle_yield_pending,
        queue_state.queue_pause_reason.as_deref(),
    );
    let queue_continuation_required = effective_continuation.required;
    let queue_continuation_guidance = effective_continuation.guidance;

    // `#qstallguard` Layer B: reconcile the continuation-pending projection recorded by
    // the prior clean closeout (session-check). If drainable work remained, the
    // loop did not continue (no fresh drain-owner lease), and no valid stop reason
    // applies, the previous turn STALLED the queue — emit a hard diagnostic instead
    // of letting the silent stall pass. The projection is one-shot: every branch
    // clears it so the diagnostic fires once per stall, not every preflight.
    {
        let file_str = file.to_string_lossy().to_string();
        if agent_doc_controller_io::project_controller::queue_drain_stall_continuation_pending_for_file(file)
            .ok()
            .flatten()
            .is_some()
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let facts = StallFacts {
                continuation_pending_projection: true,
                continuation_required_now: queue_continuation_required,
                drainable_head_count: queue_state.queue_drainable_head_count,
                user_prompt_preempts: !user_intent_prompt_changes.is_empty(),
                queue_stopped: queue_state.queue_active != Some(true)
                    || queue_state.queue_halted.is_some(),
                loop_is_continuing: agent_doc_queue::drain_owner::fresh_loop_drain_owner_lease(
                    &file_str, now,
                )
                .is_some(),
            };
            if let StallVerdict::Stalled(message) = classify_stall(&facts) {
                if !options.probe {
                    agent_doc_ops_log_io::log_op(file, &message);
                }
                warnings.push(PreflightWarning {
                    code: "queue_stall_detected".to_string(),
                    message,
                    document_agent: None,
                    active_harness: None,
                });
            }
            // One-shot: clear the projection on every reconciliation outcome.
            if !options.probe {
                let _ = agent_doc_controller_io::project_controller::clear_queue_drain_stall_continuation_pending_for_file(
                    file,
                    "preflight_reconciled",
                );
            }
        }
    }

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
        backlog_reordered,
        backlog_gated_count,
        review_count: pending_report.review_count,
        review_gated_count: pending_report.review_gated_count,
        gate_verify: gate_verify_results,
        agent_model,
        queue_prompts: queue_state.queue_prompts,
        selected_queue_prompts: if exchange_prompt_preempts_queue {
            Vec::new()
        } else {
            queue_state.selected_queue_prompts
        },
        queue_active: queue_state.queue_active,
        queue_deferred: queue_state.queue_deferred,
        queue_start_at: queue_state.queue_start_at,
        queue_trigger: queue_state.queue_trigger,
        queue_halted: queue_state.queue_halted,
        queue_paused: queue_state.queue_paused,
        queue_pause_reason: queue_state.queue_pause_reason,
        queue_drainable_head_count: queue_state.queue_drainable_head_count,
        queue_continuation_required,
        queue_continuation_guidance,
        session_accretion,
        pipeline,
        document_cell_merge_acks,
    };

    let json =
        serde_json::to_string_pretty(&output).context("failed to serialize preflight output")?;
    println!("{}", json);

    Ok(())
}

fn commit_previous_cycle_for_preflight(file: &Path) -> Result<bool> {
    match agent_doc_commit_io::commit(file) {
        Ok(did_commit) => Ok(did_commit),
        Err(err)
            if preflight_commit_error_is_live_editor_pending(&err)
                && !agent_doc_git_io::status::focused_tracked_file_modified(file)? =>
        {
            eprintln!(
                "[preflight] commit: live editor convergence pending but focused file is clean; closing no-op cycle from disk authority"
            );
            agent_doc_commit_io::commit_for_authority(file, true)
        }
        Err(err) => Err(err),
    }
}

fn preflight_commit_error_is_live_editor_pending(err: &anyhow::Error) -> bool {
    let message = format!("{err:#}");
    message.contains("CRDT relay convergence is still pending")
        || message.contains("editor_sync_pending")
        || message.contains("editor_attached_model_missing")
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_document_realtime::write_policy::ipc_snapshot_would_absorb_live_prompt_drift_after_preflight;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn closeout_cycle_is_open_prefers_terminal_projection_over_stale_open_sidecar() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = "---\nsession: test-session\n---\n\ncontent";
        std::fs::write(&doc, content).unwrap();

        let opened =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .expect("cycle state path");
        let stale_open_sidecar = std::fs::read(&sidecar_path).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        std::fs::write(&sidecar_path, stale_open_sidecar).unwrap();

        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "raw compatibility sidecar should remain stale-open for this regression"
        );
        assert_eq!(
            agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
                .unwrap()
                .unwrap()
                .cycle_id,
            opened.cycle_id
        );
        assert!(
            !closeout_cycle_is_open(&doc).unwrap(),
            "preflight should trust the terminal closeout projection before stale-open JSON"
        );
    }

    #[test]
    fn preflight_produces_valid_json() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello\n").unwrap();

        // Snapshot matches document → no_changes = true.
        agent_doc_snapshot_io::save(
            &doc,
            &std::fs::read_to_string(&doc).unwrap(),
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run(&doc).unwrap();
        // If run() returns Ok(()), the JSON was printed to stdout without error.
        // The test verifies no panic and no error return.
    }

    #[test]
    fn preflight_persists_durable_turn_checkpoint() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let base = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#durablerecycle] Durable recycle\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::save(&doc, base, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = base.replace(
            "<!-- agent:boundary:abc -->\n",
            "do [#durablerecycle]\n<!-- agent:boundary:abc -->\n",
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert_eq!(state.queue_task_id.as_deref(), Some("#durablerecycle"));
        assert_eq!(state.turn_id.as_deref(), Some("#durablerecycle"));
        assert!(
            state
                .prompt_targets
                .iter()
                .any(|target| target.contains("do [#durablerecycle]")),
            "prompt targets should persist the active prompt: {:?}",
            state.prompt_targets
        );
        let baseline_file = state
            .baseline_file
            .as_deref()
            .expect("preflight should persist baseline_file in cycle state");
        assert!(
            Path::new(baseline_file).exists(),
            "baseline path should be durable: {baseline_file}"
        );
    }

    #[test]
    fn preflight_fails_closed_when_required_ssh_doc_mapping_resolves_no_targets() {
        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[ssh.docs.\"tasks/sampleorders.md\"]\nprofile = \"missing\"\n",
        )
        .unwrap();
        let doc = dir.path().join("tasks/sampleorders.md");
        std::fs::write(&doc, "---\nagent: codex\n---\n\n## User\n\nHello\n").unwrap();

        let err = run(&doc).unwrap_err();
        assert!(err.to_string().contains("requires SSH profile `missing`"));
    }
    #[test]
    fn preflight_fails_closed_on_uncommitted_closeout_drift_even_without_diff() {
        let dir = setup_project();
        let root = dir.path();
        std::fs::create_dir_all(root.join("news/2026-05-01")).unwrap();

        let doc = root.join("session.md");
        let news_index = root.join("news/README.md");
        let news_day = root.join("news/2026-05-01/README.md");
        let old_doc = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nold body\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, old_doc).unwrap();
        std::fs::write(&news_index, "old news index\n").unwrap();
        std::fs::write(&news_day, "old news day\n").unwrap();
        agent_doc_snapshot_io::save(&doc, old_doc, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args([
                "add",
                "session.md",
                "news/README.md",
                "news/2026-05-01/README.md",
            ])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let new_doc = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nold body\n### Re: create today's news — codex\nresponse\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, new_doc).unwrap();
        agent_doc_snapshot_io::save(&doc, new_doc, agent_doc_ops_log_io::log_op).unwrap();
        std::fs::write(&news_index, "new news index\n").unwrap();
        std::fs::write(&news_day, "new news day\n").unwrap();

        let err =
            run(&doc).expect_err("preflight should fail before diffing hidden closeout drift");
        let message = err.to_string();
        assert!(message.contains("snapshot differs from HEAD"));
        assert!(message.contains("tracked side-effect edits"));
        assert!(message.contains("news/README.md"));
        assert!(message.contains("news/2026-05-01/README.md"));
        assert!(message.contains("agent-doc write --commit"));
    }
    #[test]
    fn preflight_fails_closed_on_uncommitted_exchange_drift_without_response_heading() {
        let dir = setup_project();
        let root = dir.path();

        let doc = root.join("sampleorders.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ deploy v0.4.9\n",
            "### Re: shopcozi mobile CSS fix — glm-5.1\n\n",
            "Patched the mobile CSS and deployed v0.4.9.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "sampleorders.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let dirty = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ deploy v0.4.9\n",
            "### Re: shopcozi mobile CSS fix — glm-5.1\n\n",
            "Patched the mobile CSS and deployed v0.4.9.\n\n",
            "Verification:\n",
            "- npm test\n",
            "- docker compose run post-deploy\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, dirty).unwrap();

        let err = run(&doc).expect_err("preflight should block uncommitted exchange drift");
        let message = err.to_string();
        assert!(message.contains("uncommitted exchange changes"));
        assert!(message.contains("agent-doc write --commit"));
        assert!(
            !message.contains("snapshot differs from HEAD"),
            "body-only exchange drift should be diagnosed before generic snapshot drift: {message}"
        );
    }
    #[test]
    fn preflight_file_not_found() {
        let err = run(Path::new("/nonexistent/missing.md")).unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }
    #[test]
    fn preflight_closes_stale_starting_actors_even_when_daily_gc_stamp_is_fresh() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        std::fs::write(dir.path().join(".agent-doc/gc.stamp"), "").unwrap();

        let stale_doc = dir.path().join("tasks/stale-starting.md");
        std::fs::create_dir_all(stale_doc.parent().unwrap()).unwrap();
        std::fs::write(&stale_doc, "body").unwrap();
        let stale_record = agent_doc_sqlite::state_store::ActorRecord {
            document_id: stale_doc.to_string_lossy().to_string(),
            session_id: "session-stale-starting".to_string(),
            generation: 1,
            pane_id: "%71".to_string(),
            window_id: "@7".to_string(),
            harness: "codex".to_string(),
            state: agent_doc_sqlite::state_store::ActorState::Starting,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 1,
                prior_generation: 0,
                new_generation: 1,
            },
        };
        agent_doc_controller_io::project_controller::store_actor_record(
            dir.path(),
            Some(0),
            &stale_record,
        )
        .unwrap();

        run(&doc).unwrap();

        let updated = agent_doc_controller_io::project_controller::load_actor_record(
            dir.path(),
            &stale_record.document_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            updated.state,
            agent_doc_sqlite::state_store::ActorState::Closed
        );
        assert_eq!(updated.last_transition.caller, "preflight");
        assert_eq!(updated.last_transition.reason, "stale_starting_actor");
    }
    #[test]
    fn preflight_opens_cycle_from_harness_prompt_when_document_has_no_diff() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "prompt_presets:\n",
            "  '#code-review': Please review the codebase. '#follow-up-backlog'\n",
            "  '#follow-up-backlog': Any follow-up items to place in the backlog?\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #code-review", doc.display()),
        );

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert!(
            state.requires_backlog_capture,
            "harness prompt preset expansion should record backlog capture requirement"
        );
    }
    #[test]
    fn preflight_opens_cycle_from_active_queue_when_document_has_no_diff() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#oobpmt]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#oobpmt] Fix OOB prompt absorption.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "active queue prompt should open a cycle even when the file matches the snapshot"
        );
    }

    #[test]
    fn preflight_non_active_queue_edit_updates_future_state_without_retargeting_turn() {
        // A settled edit to a non-selected queue head must update the future queue
        // state, but it must not become this turn's prompt while the selected
        // head remains unchanged.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 do [#active]\n",
            "- do [#future-old]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#active] active work\n",
            "- [ ] [#future-new] future work\n",
            "<!-- /agent:backlog -->\n"
        );
        let current_content = snapshot_content.replace("do [#future-old]", "do [#future-new]");
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.queue_task_id.as_deref(), Some("#active"));
        assert!(
            state
                .prompt_targets
                .iter()
                .all(|target| !target.contains("#future-new")),
            "future queue edit must not retarget the active turn: {:?}",
            state.prompt_targets
        );
        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("do [#future-new]"),
            "snapshot must adopt the future queue edit for later turns:\n{snap}"
        );
    }

    #[test]
    fn preflight_auto_dag_queue_edit_that_changes_selected_head_affects_turn() {
        // The exception to non-active isolation: if a future queue edit changes
        // the auto-DAG's selected head, the current turn must follow the new
        // selected head.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- 🚧 do [#active] after=#blocker\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#active] active work after=#blocker\n",
            "- [ ] [#blocker] blocker work\n",
            "<!-- /agent:backlog -->\n"
        );
        let current_content = snapshot_content.replace("do [#old]", "do [#blocker]");
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 📍 do [#blocker]\n- do [#active] after=#blocker"),
            "auto-DAG must reselect the blocker as the active head:\n{updated}"
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.queue_task_id.as_deref(), Some("#blocker"));
    }

    #[test]
    fn preflight_exchange_edit_stays_active_turn_affecting_with_future_queue_edit() {
        // Exchange edits are direct turn input. Even when the same edit also
        // updates a non-selected future queue head, the exchange delta must keep
        // the turn scoped to the operator's latest exchange text instead of being
        // absorbed as future queue state.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 do [#active]\n",
            "- do [#future-old]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#active] active work\n",
            "- [ ] [#future-new] future work\n",
            "<!-- /agent:backlog -->\n"
        );
        let current_content = snapshot_content
            .replace("do [#future-old]", "do [#future-new]")
            .replace(
                "Done.\n<!-- /agent:exchange -->",
                "Done.\n\nOperator follow-up.\n<!-- /agent:exchange -->",
            );
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_ne!(
            state.queue_task_id.as_deref(),
            Some("#active"),
            "exchange edit must not be collapsed into queue-head-only continuation"
        );
        assert!(
            state
                .prompt_targets
                .iter()
                .any(|target| target.contains("Operator follow-up")),
            "exchange edit must remain prompt-bearing: {:?}",
            state.prompt_targets
        );
    }
    #[test]
    fn preflight_unresolved_exchange_tail_preempts_queue_even_when_snapshot_matches() {
        // #prompt-preempts-auto-queue: a live unresolved exchange prompt is
        // authoritative turn input even when it was already mirrored into the
        // snapshot, leaving ordinary diff with no user-prompt delta. Preflight
        // must synthesize the exchange prompt instead of selecting the active
        // queue head.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:test -->\n",
            "Please answer this exchange prompt before the queue.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" -->\n",
            "- do [#active]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#active] active work\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_ne!(
            state.queue_task_id.as_deref(),
            Some("#active"),
            "baselined unresolved exchange prompt must preempt active queue head"
        );
        assert!(
            state
                .prompt_targets
                .iter()
                .any(|target| target
                    .contains("Please answer this exchange prompt before the queue.")),
            "preflight should synthesize the unresolved exchange prompt: {:?}",
            state.prompt_targets
        );
    }
    #[test]
    fn preflight_does_not_open_cycle_from_active_queue_slash_command() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "-   /clear  \n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        if let Some(state) = agent_doc_cycle_state_io::load(&doc).unwrap() {
            assert!(
                !state.is_open(),
                "slash-only active queue heads must be supervisor handoffs, not response cycles: {:?}",
                state
            );
        }
        assert_eq!(
            agent_doc_queue_io::queue_continuation::detect(&doc)
                .unwrap()
                .map(|continuation| continuation.head_prompt),
            Some("  /clear  ".to_string()),
            "the literal queue head must stay live for the supervisor"
        );
    }
    #[test]
    fn preflight_probe_is_read_only_even_with_dispatchable_queue_work() {
        // #preflight-probe-side-effect-free: the SAME active-queue input that
        // opens a `preflight_started` cycle in the dispatch path (see
        // `preflight_opens_cycle_from_active_queue_when_document_has_no_diff`)
        // must leave no document, snapshot, baseline, claims, or cycle mutation
        // when run as a pure inspection probe, so a diagnostic preflight never
        // wedges a later `session-check`.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#oobpmt] Fix OOB prompt absorption.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let snapshot_before = agent_doc_snapshot_io::load(&doc).unwrap();
        let baseline_path = agent_doc_fs::baseline_path_for(&doc).unwrap();
        let claims_log = dir.path().join(".agent-doc/claims.log");
        std::fs::write(&claims_log, "claim-one\n").unwrap();

        run_with_options(&doc, PreflightOptions { probe: true }).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
        assert_eq!(agent_doc_snapshot_io::load(&doc).unwrap(), snapshot_before);
        assert_eq!(std::fs::read_to_string(&claims_log).unwrap(), "claim-one\n");
        assert!(
            !baseline_path.exists(),
            "probe must not save a turn baseline at {}",
            baseline_path.display()
        );
        assert!(
            agent_doc_cycle_state_io::load(&doc).unwrap().is_none(),
            "probe must not create or advance cycle state"
        );
    }
    #[test]
    fn queue_convergence_realigns_baseline_so_finalize_sees_no_false_drift() {
        // #qconvbaseline regression: when queue maintenance converges the live
        // buffer to a corrected queue shape, the baseline saved BEFORE maintenance
        // (pre-convergence) makes every finalize read the converged queue as
        // `live_prompt_drift_after_preflight`. Re-aligning the baseline with the
        // post-maintenance snapshot must clear that false drift while a genuine
        // user edit would still trip the guard.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let pre = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#alpha] run the alpha task\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, pre).unwrap();
        agent_doc_snapshot_io::save(&doc, pre, agent_doc_ops_log_io::log_op).unwrap();

        // Simulate run.rs:570 — baseline captured from pre-maintenance content.
        save_baseline_content(&doc, pre);

        // Queue maintenance mirrors the backlog `queue`-attr id into the queue and
        // updates the snapshot to that converged shape.
        let _ = run_queue_maintenance(&doc, None).unwrap();
        let converged = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_ne!(
            converged, pre,
            "precondition: queue maintenance must have converged the queue shape"
        );

        // Helper: splice an agent response into the exchange (no user edit).
        let with_response = |d: &str| {
            d.replace(
                "<!-- /agent:exchange -->",
                "### Re: do [#alpha] — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->",
            )
        };
        let candidate = with_response(&converged); // editor buffer + response
        let content_ours_pre = with_response(pre); // OLD baseline + response

        // The bug: with the pre-maintenance baseline, the converged queue reads as drift.
        assert!(
            ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
                pre,
                &candidate,
                &content_ours_pre,
            ),
            "pre-maintenance baseline must (incorrectly) flag the convergence as drift"
        );

        // The fix: re-align the baseline by splicing the converged queue into the
        // pre-maintenance content (preserving exchange/boundary).
        let realigned = realign_baseline_to_converged_queue(pre, &converged)
            .expect("queue convergence delta must produce a re-aligned baseline");
        save_baseline_content(&doc, &realigned);
        let baseline_after =
            std::fs::read_to_string(agent_doc_fs::baseline_path_for(&doc).unwrap()).unwrap();
        assert!(
            baseline_after.contains("[#alpha]"),
            "re-aligned baseline must carry the converged queue shape"
        );
        let content_ours_aligned = with_response(&realigned);
        assert!(
            !ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
                &realigned,
                &candidate,
                &content_ours_aligned,
            ),
            "re-aligned baseline must NOT flag the queue convergence as drift"
        );

        // Guard preserved: a GENUINE concurrent user edit still trips the drift guard.
        let user_edited = candidate.replace(
            "<!-- /agent:exchange -->",
            "❯ a brand new user prompt typed mid-turn\n<!-- /agent:exchange -->",
        );
        assert!(
            ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
                &realigned,
                &user_edited,
                &content_ours_aligned,
            ),
            "a real concurrent user prompt edit must STILL be detected as drift"
        );
    }

    #[test]
    fn run_queue_maintenance_does_not_sync_icebox_into_empty_queue() {
        // Parked icebox work must not become the next active prompt just because the
        // queue and backlog are drained. Move the item to backlog or mark the item
        // with an explicit enqueue token when it should run.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox queue=append -->\n",
            "- [ ] [#parked] parked follow-up\n",
            "<!-- /agent:icebox -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            !updated.contains("- do [#parked]"),
            "icebox queue attr must not auto-populate a drained queue:\n{updated}"
        );
        assert!(
            state.synced_queue_ids.is_empty(),
            "icebox ids must not be reported as synced queue ids: {:?}",
            state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_normalizes_boolean_true_queue_attrs() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority=true preset=\"#spec-test-build-install-commit-push\"=true go=true -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] run the alpha task\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert!(
            updated.contains(
                "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->"
            ),
            "queue tag should be canonical:\n{updated}"
        );
        assert!(
            !updated.contains("=true"),
            "malformed attrs repaired:\n{updated}"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains(
                "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->"
            ),
            "snapshot queue tag should be canonical:\n{snap}"
        );
        assert!(
            !snap.contains("=true"),
            "snapshot malformed attrs repaired:\n{snap}"
        );
    }
    #[test]
    fn preflight_flags_inactive_queue_when_changed_this_cycle() {
        // Counterpart guard (Scenario B): when the operator adds content to an
        // inactive queue this cycle (snapshot empty queue, file has a new live
        // item), the residue warning must still fire so the user knows the
        // `do [#id]` they added will not run while the queue is inactive.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            "<!-- agent:queue -->\n- do [#freshly-added]\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert!(
            state
                .warnings
                .iter()
                .any(|w| w.code == "inactive_queue_residue"),
            "inactive queue changed this cycle must warn residue: {:?}",
            state.warnings
        );
    }
    #[test]
    fn preflight_clears_completed_auto_queue_when_no_prompts_remain() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#crossdocpend]~\n",
            "- ~do [#spfxnorm]~\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(!updated.contains("preset #spec-test-build-install-commit-push"));
        assert!(!updated.contains("[#crossdocpend]"));
        assert!(!updated.contains("[#spfxnorm]"));
        assert!(updated.contains("queue: stop"));

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(snap.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"));
        assert!(!snap.contains("agent:queue auto"));
        assert!(!snap.contains("[#crossdocpend]"));
        assert!(!snap.contains("[#spfxnorm]"));
    }
    #[test]
    fn preflight_clears_completed_non_auto_queue_without_snapshot_proof() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- ~do [#item-a]~\n",
            "- ~do [#item-b]~\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
            "completed non-auto queue should be cleared without snapshot proof:\n{updated}"
        );
        assert!(!updated.contains("[#item-a]"));
        assert!(!updated.contains("[#item-b]"));
        assert!(updated.contains("queue: stop"));
    }
    #[test]
    fn preflight_does_not_clear_live_inactive_queue_without_snapshot_proof() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- ~do [#done-item]~\n",
            "- do [#still-live]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- do [#still-live]"),
            "queue with live prompts must not be cleared:\n{updated}"
        );
    }
    #[test]
    fn preflight_clears_completed_non_auto_queue_when_snapshot_was_active() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- ~do [#cspe]~\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("queue_active: true", "queue_active: false");
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
            "proven drained non-auto queue should be cleared:\n{updated}"
        );
        assert!(!updated.contains("dispatch #spec-test-build-install-commit-push"));
        assert!(!updated.contains("[#cspe]"));
        assert!(updated.contains("queue: stop"));

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(snap.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"));
        assert!(!snap.contains("dispatch #spec-test-build-install-commit-push"));
        assert!(!snap.contains("[#cspe]"));
        assert!(snap.contains("queue: stop"));
    }
    #[test]
    fn preflight_does_not_swallow_user_prose_that_mentions_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "`❯ ` prompt prefix is being stripped away by the uncommitted user affordance that adds the ` (HEAD)` suffix. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
    }
    #[test]
    fn preflight_auto_commits_open_write_applied_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nAnswer\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(content),
            Some(content),
        )
        .unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_success");
    }
    /// Phase 3 (#jbccc3): the jb_cache_conflict_cancel pattern leaves a cycle
    /// marked `Committed` while the snapshot still has the visible response
    /// and `HEAD` does not — the commit boundary never actually landed (e.g.
    /// the user canceled the JB File Cache Conflict dialog mid-IPC, or a
    /// sibling compact-exchange closed the cycle while a separate `finalize`
    /// race lost its write). Without recovery, `preflight` bails on the next
    /// invocation. With Phase 3, the recoverable pattern triggers an
    /// automatic `git::commit` and the cycle lands cleanly.
    #[test]
    fn preflight_auto_recovers_jb_cache_conflict_cancel_committed_with_snapshot_drift() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::save(&doc, original, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Simulate the post-cancel state: snapshot and working tree both
        // contain the response, HEAD does not, cycle is marked Committed.
        let patched = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n";
        std::fs::write(&doc, patched).unwrap();
        agent_doc_snapshot_io::save(&doc, patched, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(patched),
            Some(patched),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(patched),
            Some(patched),
        )
        .unwrap();
        let pre_state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(pre_state.phase, agent_doc_turn::CyclePhase::Committed);
        assert!(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
            agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ));
        assert!(
            agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
            "preconditions: cancel pattern should be detected before recovery"
        );

        run(&doc).unwrap();

        assert!(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
            agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        ));
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&show.stdout).contains("Reply"),
            "HEAD should now contain the response after auto-recovery"
        );
    }
    /// Phase 3 (#jbccc3): the direct Cancel shape can also leave the cycle at
    /// `write_applied` rather than `committed`: the response is visible and
    /// saved in the snapshot, but the post-write commit never landed in HEAD.
    /// The next preflight must treat that as the same recoverable
    /// jb_cache_conflict_cancel pattern and close the missing commit boundary.
    #[test]
    fn preflight_auto_recovers_jb_cache_conflict_cancel_write_applied_with_snapshot_drift() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::save(&doc, original, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let patched = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n";
        std::fs::write(&doc, patched).unwrap();
        agent_doc_snapshot_io::save(&doc, patched, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(patched),
            Some(patched),
        )
        .unwrap();

        let pre_state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(pre_state.phase, agent_doc_turn::CyclePhase::WriteApplied);
        assert!(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
            agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ));
        assert!(
            agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
            "preconditions: write_applied cancel pattern should be detected before recovery"
        );

        run(&doc).unwrap();

        assert!(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
            agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        ));
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&show.stdout).contains("Reply"),
            "HEAD should now contain the response after write_applied auto-recovery"
        );
    }
    #[test]
    fn preflight_recovers_jb_cache_conflict_cancel_orphaned_capture_once() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let original = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #0ep7\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::save(&doc, original, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: #0ep7 — gpt-5\n\n",
            "Recovered once.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();
        let capture = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();
        let pending_path = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        assert!(
            pending_path.exists(),
            "precondition: orphaned pending response"
        );

        let materialized = original.replace(
            "<!-- agent:boundary:test -->",
            concat!(
                "### Re: #0ep7 — gpt-5\n\n",
                "Recovered once.\n",
                "<!-- agent:boundary:test -->"
            ),
        );
        std::fs::write(&doc, &materialized).unwrap();
        agent_doc_snapshot_io::save(&doc, &materialized, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&materialized),
            Some(&materialized),
        )
        .unwrap();

        assert!(
            agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
            "preconditions: committed cancel pattern should be recoverable before preflight"
        );

        run(&doc).unwrap();

        let count = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "2");
        assert!(
            !pending_path.exists(),
            "orphaned pending response should be retired"
        );

        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            content.matches("### Re: #0ep7 — gpt-5").count(),
            1,
            "visible response must not be replayed a second time:\n{content}"
        );
        assert_eq!(
            content.matches("<!-- agent:queue -->").count(),
            1,
            "template queue scaffold should stay balanced:\n{content}"
        );
        assert!(matches!(
            agent_doc_session_check_io::inspect(
                &doc,
                &agent_doc_closeout_runtime_io::session_check_effects()
            )
            .unwrap(),
            agent_doc_session_check_io::SessionCheckStatus::Ok(_)
        ));

        let refreshed = agent_doc_capture_io::load_by_id(&doc, &capture.capture_id)
            .unwrap()
            .unwrap();
        let snapshot_content = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            refreshed.state,
            agent_doc_workflow::capture::CaptureState::Committed
        );
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(agent_doc_hash::content_hash(&content).as_str()),
            "capture file hash should refresh to the recovered visible file"
        );
        assert_eq!(
            refreshed.snapshot_hash.as_deref(),
            Some(agent_doc_hash::content_hash(&snapshot_content).as_str()),
            "capture snapshot hash should refresh to the recovered snapshot"
        );
    }
    #[test]
    fn preflight_repairs_jb_cache_conflict_accept_duplicate_replay() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: #gsqlwrite — gpt-5\n\n",
            "Committed response.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed response", "--no-verify"])
            .output()
            .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        let replayed = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: #gsqlwrite — gpt-5 (HEAD)\n\nCommitted response.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, replayed).unwrap();
        assert!(
            agent_doc_session_check_io::detect_jb_cache_conflict_accept_duplicate_replay(&doc)
                .unwrap()
                .is_some(),
            "preconditions: accepted-conflict duplicate replay should be detected"
        );

        run(&doc).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().unwrap(),
            committed
        );
        let diff = Command::new("git")
            .current_dir(root)
            .args(["diff", "--", "session.md"])
            .output()
            .unwrap();
        assert!(
            diff.stdout.is_empty(),
            "preflight repair should restore the working tree to committed HEAD"
        );
    }
    #[test]
    fn preflight_repairs_late_ipc_response_overapplication() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        // HEAD has two distinct committed responses, A then B.
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first answer — opus-4-8\n\n",
            "Answer A.\n",
            "### Re: second answer — opus-4-8\n\n",
            "Answer B.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed responses", "--no-verify"])
            .output()
            .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        // Late-IPC replay re-inserts an EARLIER committed response (A) at the
        // tail, separated from its original by response B. This is NOT a
        // consecutive duplicate, so the JB-cache-conflict replay detector misses
        // it, but it is still a committed-response over-application.
        let overapplied = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: first answer — opus-4-8\n\nAnswer A.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, overapplied).unwrap();

        assert!(
            agent_doc_session_check_io::detect_jb_cache_conflict_accept_duplicate_replay(&doc)
                .unwrap()
                .is_none(),
            "preconditions: non-adjacent duplicate is missed by the consecutive replay detector"
        );
        assert!(
            agent_doc_session_check_io::detect_late_ipc_response_overapplication(&doc)
                .unwrap()
                .is_some(),
            "preconditions: late-IPC over-application should be detected"
        );

        run(&doc).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().unwrap(),
            committed
        );
        let diff = Command::new("git")
            .current_dir(root)
            .args(["diff", "--", "session.md"])
            .output()
            .unwrap();
        assert!(
            diff.stdout.is_empty(),
            "preflight repair should restore the working tree to committed HEAD"
        );
    }
    #[test]
    fn preflight_repairs_stale_jb_cache_conflict_accept_replay() {
        // #jb-cache-conflict-stale-accept-replay: a JB File Cache Conflict
        // accepted hours later replayed a STALE queued IPC reposition patch — an
        // earlier draft of a response whose final version is already committed.
        // Disk becomes HEAD plus a surplus block with the same `### Re:` topic
        // (and a `(HEAD)` marker) but a DRIFTED body. The strict over-application
        // detector misses it (bodies differ); the topic-tolerant fallback must
        // still auto-repair to committed HEAD instead of accusing a patchback.
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: fix thing — opus-4-8\n\n",
            "Final answer.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "committed response", "--no-verify"])
            .output()
            .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();

        // Surplus STALE replay of the same topic, body drifted, `(HEAD)` marked.
        let replayed = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: fix thing — opus-4-8 (HEAD)\n\nFinal answer.\nNote: stale draft paragraph the committed copy dropped.\n<!-- agent:boundary:stale -->\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &replayed).unwrap();

        assert!(
            !agent_doc_turn::response_replay::is_committed_response_overapplication(
                &replayed, committed
            ),
            "preconditions: strict over-application must NOT match a drifted-body replay"
        );
        assert!(
            agent_doc_session_check_io::detect_late_ipc_response_overapplication(&doc)
                .unwrap()
                .is_some(),
            "the stale-replay fallback should detect the over-application"
        );

        run(&doc).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap().unwrap(),
            committed
        );
        let diff = Command::new("git")
            .current_dir(root)
            .args(["diff", "--", "session.md"])
            .output()
            .unwrap();
        assert!(
            diff.stdout.is_empty(),
            "preflight repair should restore the working tree to committed HEAD"
        );
    }
    #[test]
    fn preflight_refreshes_capture_after_user_committed_baseline_drift() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let original = concat!(
            "---\n",
            "session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #bdauc\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#bdauc] Baseline drift task\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::save(&doc, original, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: #bdauc — gpt-5\n\n",
            "Implemented and verified.\n",
            "❯ Submodule pointer updated.\n",
            "<!-- /patch:exchange -->\n"
        );
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();

        let current = original
            .replace(
                "<!-- agent:boundary:test -->",
                concat!(
                    "### Re: #bdauc — gpt-5\n\n",
                    "Implemented and verified.\n",
                    "Submodule pointer updated.\n",
                    "<!-- agent:boundary:test -->"
                ),
            )
            .replace(
                "- [ ] [#bdauc] Baseline drift task\n",
                concat!(
                    "- [ ] [#bdauc] Baseline drift task\n",
                    "- [ ] [#manual] User committed unrelated follow-up\n"
                ),
            );
        std::fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::save(&doc, &current, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual baseline drift", "--no-verify"])
            .output()
            .unwrap();

        run(&doc).unwrap();

        let refreshed = agent_doc_capture_io::load_by_id(&doc, &capture.capture_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(agent_doc_hash::content_hash(&current).as_str()),
            "preflight should refresh the capture file hash before replay"
        );
        assert_eq!(
            refreshed.snapshot_hash.as_deref(),
            Some(agent_doc_hash::content_hash(&current).as_str()),
            "preflight should refresh the capture snapshot hash before replay"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_baseline_refreshed_for_benign_drift"),
            "preflight must drive validate_replay's baseline refresh path:\n{log}"
        );
    }

    #[test]
    fn stuck_capture_detection_prefers_lazily_committed_projection_over_stale_sidecar() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let original = concat!(
            "---\n",
            "session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #stuckproj\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::save(&doc, original, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let opened =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(original), Some(original))
                .unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: #stuckproj — gpt-5\n\n",
            "This response never reached HEAD.\n",
            "<!-- /patch:exchange -->\n"
        );
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(original),
            Some(original),
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(original),
            Some(original),
        )
        .unwrap();

        let sidecar_path = agent_doc_fs::cycle_state_path_for(&doc)
            .unwrap()
            .expect("cycle sidecar path");
        std::fs::write(sidecar_path, serde_json::to_string_pretty(&opened).unwrap()).unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::PreflightStarted
        );

        let stuck = agent_doc_flow_io::closeout::stuck_captured_cycle(&doc)
            .expect("committed lazily projection should drive stuck-capture detection");
        assert_eq!(stuck.cycle_id, opened.cycle_id);
        assert_eq!(stuck.capture_id, capture.capture_id);
    }

    #[test]
    fn preflight_fails_closed_when_open_backlog_item_exists_only_in_shadow_copy() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me live\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked digest\n",
            "- [ ] [#lost1] Drifted out of backlog\n",
            "-->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("open backlog item(s) exist only outside")
        );
        assert!(err.to_string().contains("#lost1"));
    }
    #[test]
    fn preflight_allows_shadow_copy_when_live_backlog_entry_still_exists() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me live\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked digest\n",
            "- [ ] [#keep1] Duplicate parked copy\n",
            "-->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        run(&doc).unwrap();
    }
    #[test]
    fn preflight_reruns_cleanly_after_open_preflight_started_cycle() {
        let dir = setup_project();
        let root = dir.path();
        let doc = dir.path().join("session.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_commit_io::commit(&doc).unwrap();
        let prior =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello again\n").unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert_ne!(
            state.cycle_id, prior.cycle_id,
            "rerun should close the old preflight and open a fresh one"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_already_current file="),
            "rerun should close the previous preflight via the no-op commit path:\n{log}"
        );
    }
    #[test]
    fn preflight_abandons_stale_empty_preflight_started_prompt_drift_without_capture() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n",
            "old body\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();
        let prior = agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(snapshot))
            .unwrap();

        let live = snapshot.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#root-empty-preflight]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(
            &doc,
            agent_doc_turn::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS + 1,
        );

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert_ne!(
            state.cycle_id, prior.cycle_id,
            "preflight should abandon the stale empty cycle and open a fresh cycle for the prompt"
        );
        assert_eq!(state.last_event, "preflight_started");

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
            "preflight should log the abandoned empty cycle:\n{log}"
        );
    }
    #[test]
    fn preflight_abandoned_stale_next_steps_prompt_stays_actionable() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "agent_doc_session: test\n",
            "prompt_presets:\n",
            "  '#next-steps': Any follow-up items to place in the backlog?\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "Compacted.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();
        let prior = agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(snapshot))
            .unwrap();

        let prompt = "Left/Right buttons still do not work with agent-doc opencode. #next-steps";
        let live = snapshot.replace(
            "<!-- agent:boundary:abc123 -->\n",
            &format!("{prompt}\n<!-- agent:boundary:abc123 -->\n"),
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(
            &doc,
            agent_doc_turn::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS + 1,
        );

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert_ne!(
            state.cycle_id, prior.cycle_id,
            "preflight should abandon the stale empty cycle and open a fresh cycle for the prompt"
        );
        assert!(
            state.requires_backlog_capture,
            "the inline #next-steps prompt should still require backlog capture"
        );
        let diff = agent_doc_diff_io::compute(
            &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
            &doc,
        )
        .unwrap()
        .unwrap();
        let prompt_targets = agent_doc_diff::classify_prompt_bearing_changes(&diff)
            .into_iter()
            .filter(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
            .map(|change| change.text)
            .collect::<Vec<_>>();
        assert!(
            prompt_targets.iter().any(|target| target.contains(prompt)),
            "fresh preflight should surface the abandoned #next-steps prompt as actionable, got {prompt_targets:?}"
        );

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
            "preflight should log the abandoned empty cycle:\n{log}"
        );
        assert!(
            log.contains("post_commit_user_follow_up file="),
            "step-2 commit should classify the prompt-bearing drift as a follow-up, not absorb it:\n{log}"
        );
    }
    #[test]
    fn preflight_compact_follow_up_next_steps_is_not_swallowed_by_commit_recovery() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "agent_doc_session: test\n",
            "prompt_presets:\n",
            "  '#next-steps': Any follow-up items to place in the backlog?\n",
            "---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Compacted.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "Compacted content archived.\n",
            "<!-- agent:boundary:compact -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "compact exchange", "--no-verify"])
            .output()
            .unwrap();

        let live = snapshot.replace(
            "<!-- agent:boundary:compact -->\n",
            "#next-steps\n<!-- agent:boundary:compact -->\n",
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "compact follow-up should open a response cycle instead of becoming no_changes"
        );
        assert!(
            state.requires_backlog_capture,
            "compact follow-up #next-steps should carry the backlog-capture contract"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot_after, snapshot,
            "preflight must not absorb the compact follow-up prompt into the snapshot"
        );
        let head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(head.status.success(), "git show HEAD:session.md failed");
        assert_eq!(
            String::from_utf8_lossy(&head.stdout).as_ref(),
            snapshot,
            "step-2 commit must not silently commit the compact follow-up prompt"
        );
    }
    #[test]
    fn preflight_commits_route_queue_snapshot_before_live_prompt_edit() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let original_prompt =
            "Run Agent Doc queued this prompt. #spec-test-build-install-commit-push";
        let edited_prompt = "Run Agent Doc queued this prompt. Same with this file. #spec-test-build-install-commit-push";
        let head = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "agent_doc_session: test\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        let queued = head
            .replace("queue_active: false", "queue_active: true")
            .replace(
                "<!-- agent:boundary:abc123 -->\n",
                &format!("{original_prompt}\n<!-- agent:boundary:abc123 -->\n"),
            )
            .replace(
                "<!-- agent:queue -->\n<!-- /agent:queue -->",
                &format!("<!-- agent:queue auto -->\n- {original_prompt}\n<!-- /agent:queue -->"),
            );
        let live = queued.replacen(original_prompt, edited_prompt, 1);

        std::fs::write(&doc, head).unwrap();
        agent_doc_snapshot_io::save(&doc, head, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_snapshot_io::save(&doc, &queued, agent_doc_ops_log_io::log_op).unwrap();
        std::fs::write(&doc, &live).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head), Some(head)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(&queued),
            Some(&queued),
        )
        .unwrap();

        run(&doc).unwrap();

        let committed = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(
            committed.status.success(),
            "git show HEAD:session.md failed"
        );
        let committed = String::from_utf8_lossy(&committed.stdout);
        assert!(
            committed.contains(original_prompt),
            "route queued prompt should be committed from the saved snapshot:\n{committed}"
        );
        assert!(
            !committed.contains("Same with this file"),
            "live prompt edit must not be swallowed into the queue snapshot commit:\n{committed}"
        );
        let working = std::fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains(edited_prompt),
            "later live prompt edit should remain visible for the fresh preflight cycle:\n{working}"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snapshot_after.contains(original_prompt),
            "snapshot should stay on the route queued prompt:\n{snapshot_after}"
        );
        assert!(
            !snapshot_after.contains("Same with this file"),
            "preflight must not absorb the live edit into the committed snapshot:\n{snapshot_after}"
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "after committing the queued snapshot, preflight should open a fresh cycle for the live edit"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("route_queue_snapshot_auto_recovery_succeeded file="),
            "route queue commit-boundary recovery should be logged:\n{log}"
        );
    }
    #[test]
    fn preflight_started_cycle_does_not_revert_stale_snapshot_head() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = "---\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = "---\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n### Re: newer\nnew body\n❯ follow-up question\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, live).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(live)).unwrap();

        run(&doc).unwrap();

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            committed.contains("### Re: newer"),
            "HEAD should stay at the newer manual content instead of reverting:\n{committed}"
        );
        assert!(
            committed.contains("❯ follow-up question"),
            "HEAD should keep the live follow-up question instead of reverting:\n{committed}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }
    #[test]
    fn preflight_fails_closed_on_ambiguous_preflight_started_patchback_without_artifact() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

        let live = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(agent_doc_turn::repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR),
            "expected fail-closed ambiguous patchback error, got: {message}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "ambiguous patchback must not be auto-committed"
        );
    }
    #[test]
    fn preflight_started_repair_fails_when_matching_cycle_file_has_uncommitted_patchback() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, live).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(live)).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(agent_doc_turn::repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR),
            "expected uncommitted response patchback error, got: {message}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "recovery must not mark the stale cycle committed while HEAD lacks the visible response"
        );
    }
    #[test]
    fn preflight_completed_backlog_reap_does_not_swallow_live_prompt() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do #scopeid — gpt-5\n",
            "Implemented.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#scopeid] completed item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do #scopeid — gpt-5\n",
            "Implemented.\n",
            "do #statusws. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#scopeid] completed item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "preflight should still open a response cycle for the live prompt"
        );

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(file_after.contains("do #statusws. spec-test-build-install-commit-push"));
        assert!(!file_after.contains("- [x] [#scopeid] completed item"));

        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after.contains("do #statusws. spec-test-build-install-commit-push"),
            "snapshot must not absorb the live prompt during backlog reap"
        );
        assert!(!snapshot_after.contains("- [x] [#scopeid] completed item"));

        let head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(head.status.success(), "git show HEAD:session.md failed");
        let head_text = String::from_utf8_lossy(&head.stdout);
        assert!(
            !head_text.contains("do #statusws. spec-test-build-install-commit-push"),
            "repair/commit must not silently commit the live prompt:\n{head_text}"
        );
    }
    #[test]
    fn preflight_relocates_out_of_exchange_prompt_without_swallowing_live_diff() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "do [#oobprompt]. spec-test-build-install-commit-push\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "preflight should still open a response cycle for the relocated prompt"
        );

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = file_after.find("<!-- /agent:exchange -->").unwrap();
        let prompt = file_after
            .find("❯ do [#oobprompt]. spec-test-build-install-commit-push")
            .unwrap();
        let gap_marker = file_after.find("\n###\n\n").unwrap();
        assert!(
            prompt < exchange_close,
            "preflight should move the prompt back inside exchange:\n{file_after}"
        );
        assert!(
            gap_marker > exchange_close,
            "preflight should leave the gap marker outside exchange:\n{file_after}"
        );
        assert!(
            !file_after.contains(
                "\n<!-- /agent:exchange -->\n\ndo [#oobprompt]. spec-test-build-install-commit-push"
            ),
            "out-of-exchange prompt should not remain in the gap:\n{file_after}"
        );

        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after.contains("oobprompt"),
            "snapshot must not absorb the live prompt during preflight relocation:\n{snapshot_after}"
        );
    }
    #[test]
    fn preflight_does_not_relocate_prompt_text_inside_post_exchange_html_comment() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "Content that I added into the html comment below agent:exchange in this doc was deleted by agent-doc.\n",
            "spec-test-build-install-commit-push\n",
            "---\n",
            "older scratch note\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = file_after.find("<!-- /agent:exchange -->").unwrap();
        let hidden_prompt = file_after
            .find("Content that I added into the html comment below agent:exchange")
            .unwrap();
        let comment_open = file_after.find("\n<!--\n").unwrap();
        let comment_close = file_after.find("\n-->\n\n<!-- agent:backlog -->").unwrap();
        assert!(
            hidden_prompt > exchange_close,
            "scratch-comment prompt text must stay outside exchange:\n{file_after}"
        );
        assert!(
            hidden_prompt > comment_open && hidden_prompt < comment_close,
            "scratch-comment prompt text must remain inside the ordinary HTML comment:\n{file_after}"
        );
        assert!(
            !file_after.contains(
                "\nContent that I added into the html comment below agent:exchange in this doc was deleted by agent-doc.\nspec-test-build-install-commit-push\n<!-- /agent:exchange -->"
            ),
            "preflight must not move scratch-comment text into exchange:\n{file_after}"
        );
    }
    #[test]
    fn post_exchange_comment_with_horizontal_rule_and_prose_is_user_note() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "  '#next-steps': Any follow-up items?\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "The last run had a code fence stripped away by agent-doc.\n",
            "#spec-test-build-install-commit-push\n",
            "---\n",
            "What are #next-steps to fix bugs?\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        let warning =
            agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
                "session.md",
                content,
                &fm.prompt_presets,
            );
        assert!(
            warning.is_none(),
            "post-exchange comment with horizontal rule and prose is a user note, not a directive: {:?}",
            warning
        );
    }
    #[test]
    fn preflight_preserves_post_exchange_duplicate_prompt_comment_before_diff() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "Keep this unrelated scratch note hidden.\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push";
        let live = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n",
                "Done.\n",
                "<!-- agent:boundary:head -->\n",
                "{prompt}\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!--\n",
                "Keep this unrelated scratch note hidden.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#keepme] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let duplicate_comment = format!("\n<!--\n{prompt}\n-->\n");
        assert!(
            file_after.contains(&duplicate_comment),
            "preflight must preserve visible post-exchange scratch comments even when they duplicate prompt text:\n{file_after}"
        );
        assert!(
            file_after.contains("Keep this unrelated scratch note hidden."),
            "unrelated scratch comments must remain outside exchange:\n{file_after}"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after.contains(prompt),
            "snapshot must not absorb the live prompt during preflight:\n{snapshot_after}"
        );
    }
    #[test]
    fn preflight_preserves_unrelated_lines_in_mixed_post_exchange_duplicate_prompt_comment() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let exchange_prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let duplicate_prompt_line = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let live = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior - gpt-5\n",
                "Done.\n",
                "<!-- agent:boundary:head -->\n",
                "{exchange_prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{duplicate_prompt_line}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#keepme] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            exchange_prompt = exchange_prompt,
            duplicate_prompt_line = duplicate_prompt_line,
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            file_after.contains(&format!("<!--\n{duplicate_prompt_line}")),
            "preflight must preserve visible duplicate-looking lines in post-exchange scratch comments:\n{file_after}"
        );
        assert!(
            file_after.contains("Look through the Claude + Codex + agent-doc session logs"),
            "preflight must preserve unrelated scratch lines in the same ordinary comment:\n{file_after}"
        );
        assert!(
        file_after.contains(&format!(
            "<!--\n{duplicate_prompt_line}\n#spec-test-build-install-commit-push\n---\nLook through"
        )),
        "preflight must keep the full mixed ordinary comment body:\n{file_after}"
    );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after.contains(exchange_prompt),
            "snapshot must not absorb the live prompt during preflight:\n{snapshot_after}"
        );
    }
    #[test]
    fn preflight_scrubs_duplicate_answered_prompt_tail_before_diff() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. Should we diff line by line?";
        let snapshot = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: mixed scratch comment deletion - gpt-5\n\n",
                "Answered already.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "Keep this scratch note.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#keepme] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        std::fs::write(&doc, &snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Genuine replay residue carries the `❯ ` answered-form marker — that is
        // the ownership proof that lets the scrub remove it without eating a live
        // re-typed prompt (#ipcfullprompt-recur).
        let live = snapshot.replace(
            "<!-- agent:boundary:head -->\n<!-- /agent:exchange -->",
            &format!(
                "<!-- agent:boundary:head -->\n❯ {prompt}\n❯ #spec-test-build-install-commit-push\n<!-- /agent:exchange -->"
            ),
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !file_after.contains(&format!(
                "head -->\n❯ {prompt}\n❯ #spec-test-build-install-commit-push"
            )),
            "preflight should scrub duplicate answered-form prompt tails before diffing:\n{file_after}"
        );
        assert!(
            file_after.contains("Keep this scratch note."),
            "preflight cleanup must preserve unrelated scratch comments:\n{file_after}"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after.contains(&format!(
                "head -->\n❯ {prompt}\n❯ #spec-test-build-install-commit-push"
            )),
            "snapshot must not absorb the duplicate tail cleanup prompt"
        );
    }
    #[test]
    fn preflight_preserves_duplicate_prompt_comment_after_typing_settles() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push";
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_debounce: 3000\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keepme] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_debounce: 3000\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n",
                "Done.\n",
                "<!-- agent:boundary:head -->\n",
                "❯ {prompt}\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#keepme] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        std::fs::write(&doc, &live).unwrap();

        let doc_for_thread = doc.clone();
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);
        let handle = std::thread::spawn(move || run(&doc_for_thread));
        std::thread::sleep(std::time::Duration::from_millis(500));
        let during_debounce = std::fs::read_to_string(&doc).unwrap();
        let result = handle.join().unwrap();
        result.unwrap();

        let duplicate_comment = format!("<!--\n{prompt}\n-->");
        assert!(
            during_debounce.contains(&duplicate_comment),
            "preflight must not mutate duplicate prompt comments while the editor typing indicator is active:\n{during_debounce}"
        );

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            file_after.contains(&duplicate_comment),
            "preflight must preserve visible scratch comments after typing settles:\n{file_after}"
        );
    }
    #[test]
    fn preflight_session_accretion_does_not_auto_compact_exchange() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\nExisting summary.\n\n",
            "### Re: first topic — gpt-5\n\nFirst response.\n\n",
            "### Re: second topic — gpt-5\n\nSecond response.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let relative = doc
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        write_cycles_log(
            &doc,
            &[
                agent_doc_ops_log_io::CycleEntry {
                    timestamp: now.saturating_sub(10).to_string(),
                    file: relative.clone(),
                    op: "commit_noop".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    timestamp: now.saturating_sub(5).to_string(),
                    file: relative,
                    op: "commit_noop".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let live = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\nExisting summary.\n\n",
            "### Re: first topic — gpt-5\n\nFirst response.\n\n",
            "### Re: second topic — gpt-5\n\nSecond response.\n",
            "<!-- agent:boundary:head -->\n",
            "do #autocmp. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(!file_after.contains("1 earlier topic(s) archived"));
        assert!(file_after.contains("### Re: second topic — gpt-5"));
        assert!(file_after.contains("### Re: first topic — gpt-5"));
        assert!(file_after.contains("do #autocmp. spec-test-build-install-commit-push"));

        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(snapshot_after, snapshot);
    }
    #[test]
    fn preflight_reaps_flush_left_spill_with_completed_backlog_item() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do #scopeid — gpt-5\n",
            "Implemented.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#scopeid] completed item\n",
            "Commands:\n",
            "  cargo test -p agent-doc backlog::\n",
            "Diff:\n",
            "@@ -1 +1 @@\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do #scopeid — gpt-5\n",
            "Implemented.\n",
            "do #statusws. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#scopeid] completed item\n",
            "Commands:\n",
            "  cargo test -p agent-doc backlog::\n",
            "Diff:\n",
            "@@ -1 +1 @@\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&file_after).unwrap();
        let backlog_after = backlog_after
            .iter()
            .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
            .map(|component| component.content(&file_after))
            .unwrap();
        assert!(file_after.contains("do #statusws. spec-test-build-install-commit-push"));
        assert!(!backlog_after.contains("- [x] [#scopeid] completed item"));
        assert!(!backlog_after.contains("Commands:"));
        assert!(!backlog_after.contains("@@ -1 +1 @@"));

        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        let snapshot_backlog = agent_doc_element::element::parse(&snapshot_after).unwrap();
        let snapshot_backlog = snapshot_backlog
            .iter()
            .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
            .map(|component| component.content(&snapshot_after))
            .unwrap();
        assert!(!snapshot_backlog.contains("- [x] [#scopeid] completed item"));
        assert!(!snapshot_backlog.contains("Commands:"));
        assert!(!snapshot_backlog.contains("@@ -1 +1 @@"));
        assert!(
            !snapshot_after.contains("do #statusws. spec-test-build-install-commit-push"),
            "snapshot must not absorb the live prompt during backlog reap"
        );
    }
    #[test]
    fn preflight_status_prompt_preset_addition_does_not_swallow_diff() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#next-steps': Print the top backlog item.\n",
            "---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Compacted.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "Compacted.\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let live = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#next-steps': Print the top backlog item.\n",
            "---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Compacted.\n",
            "#next-steps for calibrating session benchmarks with expected scores\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "Compacted.\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "preflight should still open a response cycle for the prompt-preset status edit"
        );

        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot_after, snapshot,
            "snapshot must not absorb prompt-bearing status drift"
        );

        let head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(head.status.success(), "git show HEAD:session.md failed");
        let head_text = String::from_utf8_lossy(&head.stdout);
        assert_eq!(
            head_text.as_ref(),
            snapshot,
            "step 2 commit must not silently commit the prompt-preset status edit:\n{head_text}"
        );
    }
    #[test]
    fn preflight_boundary_artifact_only_diff_does_not_start_cycle() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let tracked = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, tracked).unwrap();
        agent_doc_snapshot_io::save(&doc, tracked, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let visible = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, visible).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap();
        assert!(
            state.as_ref().is_none_or(|state| !state.is_open()),
            "boundary-artifact-only preflight must not leave an open cycle"
        );

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("preflight_diff_start file="),
            "boundary-artifact-only diff must not log preflight_diff_start:\n{log}"
        );
        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(_) => {}
            status => {
                panic!(
                    "expected clean closeout after boundary-artifact-only preflight, got {status:?}"
                )
            }
        }
    }
    #[test]
    fn preflight_recovers_response_captured_cycle_without_pending_file() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_repair_io::pending::save_pending(&doc, "Recovered answer.").unwrap();
        let pending = agent_doc_fs::pending_response_path_for(&doc).unwrap();
        std::fs::remove_file(&pending).unwrap();

        run(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Recovered answer."));
    }
    #[test]
    fn maybe_auto_repair_base_index_removes_stale_counter_without_tmux() {
        let dir = tempfile::tempdir().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("state")).unwrap();
        let counter_path = agent_doc_dir.join("state/base-index-repair.count");
        std::fs::write(&counter_path, "1").unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\n---\n").unwrap();
        let issues =
            vec!["window index 0 missing in session '0' (base-index compliance)".to_string()];

        let _env_guard = agent_doc_test_support::env_lock();
        let saved_tmux = std::env::var("TMUX").ok();
        // SAFETY: this test restores the process env before returning.
        unsafe { std::env::remove_var("TMUX") };
        let repaired = maybe_auto_repair_base_index(&file, &issues);
        if let Some(val) = saved_tmux {
            unsafe { std::env::set_var("TMUX", val) };
        }
        assert!(!repaired, "outside tmux no repair should run");
        assert!(
            !counter_path.exists(),
            "stale deferred-repair counter should be removed"
        );
    }
    #[test]
    fn preflight_sweep_commits_other_tracked_docs() {
        use std::fs;
        let dir = setup_project();
        let root = dir.path();

        // Create initial commit so HEAD exists
        let readme = root.join("README.md");
        fs::write(&readme, "# project\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Primary doc (the one preflight runs on)
        let primary = root.join("primary.md");
        let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
        fs::write(&primary, primary_content).unwrap();
        agent_doc_snapshot_io::save(&primary, primary_content, agent_doc_ops_log_io::log_op)
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "primary.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add primary", "--no-verify"])
            .output()
            .unwrap();

        // Secondary doc (tracked in the durable registry, snapshot newer than file — needs sweep)
        let secondary = root.join("secondary.md");
        let secondary_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&secondary, secondary_content).unwrap();
        agent_doc_snapshot_io::save(&secondary, secondary_content, agent_doc_ops_log_io::log_op)
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "secondary.md"])
            .output()
            .unwrap();
        // Backdate the commit so the <5s freshness gate in sweep doesn't skip it.
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add secondary", "--no-verify"])
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .output()
            .unwrap();

        // Touch snapshot to make it newer than the file (simulates agent write without commit)
        let snap_rel = agent_doc_fs::snapshot_path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        let new_snap = format!("{}\n<!-- agent updated -->", secondary_content);
        fs::write(&snap_abs, &new_snap).unwrap();

        write_session_registry(
            root,
            &[("secondary-session", "%1", &secondary, "@1", "2026-01-01")],
        );

        // Run preflight on primary — sweep should commit secondary
        run(&primary).unwrap();

        // Verify secondary was committed by the sweep
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-4"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(secondary):"),
            "preflight sweep should have committed secondary.md, got:\n{log_str}"
        );
    }
    #[test]
    fn preflight_sweep_skips_doc_with_unresponded_user_content() {
        use std::fs;
        let dir = setup_project();
        let root = dir.path();

        // Create initial commit so HEAD exists
        let readme = root.join("README.md");
        fs::write(&readme, "# project\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Primary doc (the one preflight runs on)
        let primary = root.join("primary.md");
        let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
        fs::write(&primary, primary_content).unwrap();
        agent_doc_snapshot_io::save(&primary, primary_content, agent_doc_ops_log_io::log_op)
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "primary.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add primary", "--no-verify"])
            .output()
            .unwrap();

        // Secondary doc with agent response in snapshot but user added new content in document
        let secondary = root.join("secondary.md");
        let snap_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
        // Document has user additions not in the snapshot
        let doc_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\nNew question from user\n";
        fs::write(&secondary, doc_content).unwrap();
        agent_doc_snapshot_io::save(&secondary, snap_content, agent_doc_ops_log_io::log_op)
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "secondary.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add secondary", "--no-verify"])
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .output()
            .unwrap();

        // Touch snapshot to make it newer than the file
        let snap_rel = agent_doc_fs::snapshot_path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&snap_abs, snap_content).unwrap();

        write_session_registry(
            root,
            &[("secondary-session", "%1", &secondary, "@1", "2026-01-01")],
        );

        // Count commits before sweep
        let log_before = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline"])
            .output()
            .unwrap();
        let count_before = String::from_utf8_lossy(&log_before.stdout).lines().count();

        // Run preflight on primary — sweep should SKIP secondary due to user additions
        run(&primary).unwrap();

        // Verify secondary was NOT committed
        let log_after = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log_after.stdout);
        assert!(
            !log_str.contains("agent-doc(secondary):"),
            "preflight sweep should NOT have committed secondary.md (has unresponded user content), got:\n{log_str}"
        );
        // Only primary should have been committed (by step 2, not sweep)
        let count_after = log_str.lines().count();
        assert!(
            count_after <= count_before + 1,
            "expected at most one new commit (primary), got {} new commits",
            count_after - count_before
        );
    }
    #[test]
    fn preflight_sweep_skips_foreign_owned_doc() {
        use std::fs;
        let dir = setup_project();
        let root = dir.path();
        initialize_git_head(root);

        let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
        let primary = write_committed_doc(root, "primary.md", primary_content, "add primary", None);

        let secondary_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
        let secondary = write_committed_doc(
            root,
            "secondary.md",
            secondary_content,
            "add secondary",
            Some("2026-01-01T00:00:00Z"),
        );

        let snap_rel = agent_doc_fs::snapshot_path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(
            &snap_abs,
            format!("{}\n<!-- agent updated -->", secondary_content),
        )
        .unwrap();

        write_session_registry(
            root,
            &[
                ("primary-session", "%70", &primary, "@1", "2026-01-01"),
                ("secondary-session", "%73", &secondary, "@2", "2026-01-01"),
            ],
        );

        run(&primary).unwrap();

        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-4"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            !log_str.contains("agent-doc(secondary):"),
            "foreign-owned secondary.md must not be sweep-committed, got:\n{log_str}"
        );

        let head_secondary = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:secondary.md"])
            .output()
            .unwrap();
        let head_secondary = String::from_utf8_lossy(&head_secondary.stdout);
        assert!(
            !head_secondary.contains("agent updated"),
            "foreign-owned snapshot drift must stay out of HEAD:\n{head_secondary}"
        );

        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("foreign_owned_sweep_skip")
                && ops_log.contains("owner_pane=%73")
                && ops_log.contains("current_pane=%70"),
            "foreign-owned skip should be logged for audit:\n{ops_log}"
        );
    }
    #[test]
    fn preflight_sweep_commits_same_owner_doc() {
        use std::fs;
        let dir = setup_project();
        let root = dir.path();
        initialize_git_head(root);

        let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
        let primary = write_committed_doc(root, "primary.md", primary_content, "add primary", None);

        let secondary_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
        let secondary = write_committed_doc(
            root,
            "secondary.md",
            secondary_content,
            "add secondary",
            Some("2026-01-01T00:00:00Z"),
        );

        let snap_rel = agent_doc_fs::snapshot_path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(
            &snap_abs,
            format!("{}\n<!-- agent updated -->", secondary_content),
        )
        .unwrap();

        write_session_registry(
            root,
            &[
                ("primary-session", "%70", &primary, "@1", "2026-01-01"),
                ("secondary-session", "%70", &secondary, "@1", "2026-01-01"),
            ],
        );
        agent_doc_session_actor_io::project_binding_in(
            root,
            &primary.to_string_lossy(),
            "primary-session",
            "%70",
            "@1",
            "test",
            "primary_owner",
        )
        .unwrap();
        // Leave the sibling owner in the durable registry so this exercises the sweep
        // fallback projection without seeding an invalid two-document actor
        // alias for pane %70.

        run(&primary).unwrap();

        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-4"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(secondary):"),
            "same-owner secondary.md should still be sweep-committed, got:\n{log_str}"
        );

        let head_secondary = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:secondary.md"])
            .output()
            .unwrap();
        let head_secondary = String::from_utf8_lossy(&head_secondary.stdout);
        assert!(
            head_secondary.contains("agent updated"),
            "same-owner snapshot drift should land in HEAD:\n{head_secondary}"
        );
    }
}
