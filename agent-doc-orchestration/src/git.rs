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

use agent_doc_document::commit_normalization::{
    canonicalize_answered_prompt_prefixes, normalize_committed_exchange_artifacts,
};
use agent_doc_document::transient_markers::{
    exchange_prompt_prefix_equivalent, normalize_for_replay_hash,
    normalize_post_commit_re_heading_drift, normalize_transient_agent_doc_markers,
    repair_stale_agent_response_collapse_doc, strip_guard_markers, strip_head_markers,
};
use anyhow::{Context, Result};
use fs2::FileExt;
use std::collections::HashSet;
#[cfg(test)]
use std::fs;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use agent_doc_document_realtime::write_policy::{
    classify_committed_historical_agent_doc_mutation, classify_safe_out_of_band_agent_doc_mutation,
    detect_reintroduced_reaped_pending_ids, is_empty_template_scaffold_snapshot,
};
use agent_doc_element_exchange::post_commit_ipc_reposition_only_exchange_safe;
use agent_doc_git::{
    PostCommitLocalDriftKind, agent_doc_commit_message_for_file, classify_post_commit_local_drift,
    commit_retry_backoff, has_blocking_non_exchange_component_drift,
    is_safe_user_only_follow_up_after_committed_head, output_has_index_lock_contention,
    parent_submodule_pointer_commit_message, relative_to_root, render_git_process_output,
};
use agent_doc_git_io::dirs::{
    commit_lock_path_for_git_root, commit_lock_scope_path, narrow_to_submodule, resolve_to_git_root,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitOutcome {
    pub did_commit: bool,
    pub vcs_refresh_signaled: Option<bool>,
}

/// RAII guard for an exclusive advisory lock serializing `git::commit()` runs
/// per git repo / submodule across concurrent sessions. Protects the short
/// stage+commit critical section so two different docs in the same repo do not
/// race on one shared git index.
///
/// Lock file lives under the resolved git dir (`git rev-parse
/// --absolute-git-dir`) at `agent-doc-locks/commit-repo-<hash>.lock`. That
/// means nested docs in the same repo/submodule all share one lock, while
/// different repos do not block each other, and lock creation does not alter
/// `.agent-doc` project-root discovery.
///
/// Best-effort: if the path cannot be resolved or opened, returns `None` and
/// the caller proceeds unlocked. When the lock is already held, acquisition
/// blocks for the short commit critical section instead of proceeding unlocked.
struct CommitLock {
    _file: File,
}

impl CommitLock {
    fn acquire(git_root: &Path) -> Option<Self> {
        let lock_path = commit_lock_path_for_git_root(git_root)?;
        let scope = commit_lock_scope_path(git_root)?;
        let lock_dir = lock_path.parent()?.to_path_buf();
        if let Err(e) = std::fs::create_dir_all(&lock_dir) {
            eprintln!(
                "[commit] commit-lock dir create failed: {} (proceeding unlocked)",
                e
            );
            return None;
        }
        let file = match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "[commit] commit-lock open failed: {} (proceeding unlocked)",
                    e
                );
                return None;
            }
        };
        if let Err(e) = file.try_lock_exclusive() {
            eprintln!(
                "[commit] repo commit-lock contended for {}: {} (waiting)",
                scope.display(),
                e
            );
            if let Err(e) = file.lock_exclusive() {
                eprintln!(
                    "[commit] commit-lock wait failed for {}: {} (proceeding unlocked)",
                    scope.display(),
                    e
                );
                return None;
            }
        }
        Some(Self { _file: file })
    }
}

impl Drop for CommitLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

/// After a successful commit inside a submodule, stage and partial-commit the
/// updated submodule pointer in the superproject. Uses an explicit pathspec on
/// the commit so any other staged files in the parent index are preserved.
fn update_parent_submodule_pointer(super_root: &Path, submodule_root: &Path, msg: &str) {
    let _commit_lock = CommitLock::acquire(super_root);
    let rel = match submodule_root.strip_prefix(super_root) {
        Ok(r) => r,
        Err(_) => {
            eprintln!("[commit] cannot compute submodule relative path; skipping pointer update");
            return;
        }
    };
    let rel_str = rel.to_string_lossy().to_string();

    let add = Command::new("git")
        .current_dir(super_root)
        .args(["add", "--", &rel_str])
        .output();
    match add {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "[commit] parent git add for submodule pointer failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return;
        }
        Err(e) => {
            eprintln!("[commit] parent git add error: {}", e);
            return;
        }
    }

    let parent_msg = parent_submodule_pointer_commit_message(msg);
    let commit = Command::new("git")
        .current_dir(super_root)
        .args(["commit", "-m", &parent_msg, "--no-verify", "--", &rel_str])
        .output();
    match commit {
        Ok(o) if o.status.success() => {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let t = line.trim();
                if t.starts_with('[') && t.contains(']') {
                    eprintln!("{}", line);
                }
            }
            eprintln!("[commit] parent submodule pointer updated for {}", rel_str);
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            // "nothing to commit" / "no changes added" is fine — pointer was already current.
            if stderr.contains("nothing to commit")
                || stdout.contains("nothing to commit")
                || stderr.contains("no changes added")
            {
                return;
            }
            eprintln!(
                "[commit] parent submodule pointer commit failed: {}",
                stderr.trim()
            );
        }
        Err(e) => eprintln!("[commit] parent submodule pointer commit error: {}", e),
    }
}

pub fn repair_committed_historical_snapshot_drift(file: &Path) -> Result<Option<&'static str>> {
    let Some(snapshot_doc) = agent_doc_snapshot_io::load(file)? else {
        return Ok(None);
    };
    let current_doc = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    if current_doc == snapshot_doc {
        return Ok(None);
    }

    let Some(head_doc) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(None);
    };
    let historical_mutation =
        classify_committed_historical_agent_doc_mutation(&snapshot_doc, &head_doc);
    // #nm1x: intersect the drift against the current turn scope so independent
    // out-of-scope edits (e.g. a queue item added beside the running one) do not
    // block the historical snapshot repair.
    let turn_scope = agent_doc_turn_scope_io::load(file);
    let non_exchange_component_drift =
        has_blocking_non_exchange_component_drift(&snapshot_doc, &head_doc, turn_scope.as_ref());
    let historical_response_marker =
        agent_doc_turn::document_drift::detect_bypassed_response_write_between(
            &snapshot_doc,
            &head_doc,
        );
    let historical_prompt_prefix_artifact = snapshot_doc != head_doc
        && !non_exchange_component_drift
        && normalize_committed_exchange_artifacts(&snapshot_doc)
            == normalize_committed_exchange_artifacts(&head_doc);
    let Some(reason) = (match historical_mutation {
        Some("exchange") => Some("exchange"),
        None if !non_exchange_component_drift && historical_response_marker.is_some() => {
            Some("exchange")
        }
        None if historical_prompt_prefix_artifact => Some("exchange"),
        _ => None,
    }) else {
        return Ok(None);
    };

    if normalize_committed_exchange_artifacts(&current_doc)
        == normalize_committed_exchange_artifacts(&head_doc)
    {
        agent_doc_snapshot_io::save(file, &current_doc, agent_doc_ops_log_io::log_op)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis=head",
                file.display(),
                reason
            ),
        );
        return Ok(Some(reason));
    }

    if agent_doc_turn::document_drift::detect_bypassed_response_write_between(
        &head_doc,
        &current_doc,
    )
    .is_none()
    {
        if crate::write::guard_no_stale_snapshot_reset_drift(
            file,
            Some(&head_doc),
            &current_doc,
            "historical snapshot repair",
        )? {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "snapshot_repair file={} reason={} basis=visible_rebase_guard",
                    file.display(),
                    reason
                ),
            );
            return Ok(Some(reason));
        }
        let basis = if is_safe_user_only_follow_up_after_committed_head(&head_doc, &current_doc) {
            "head_follow_up"
        } else {
            "head_local_drift"
        };
        agent_doc_snapshot_io::save(file, &head_doc, agent_doc_ops_log_io::log_op)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "snapshot_repair file={} reason={} basis={}",
                file.display(),
                reason,
                basis
            ),
        );
        return Ok(Some(reason));
    }

    Ok(None)
}

/// Commit a file with an auto-generated message. Skips hooks.
/// Relative paths are resolved against the git root (superproject if in a submodule).
/// Git commands run from the resolved git root, so this works even when CWD is a submodule.
pub fn commit(file: &Path) -> Result<bool> {
    Ok(commit_with_outcome(file)?.did_commit)
}

/// `#qheadstrike` P2 — strike answered free-text queue heads at the commit seam.
///
/// Sources the answered response from the durable capture ledger (the
/// cycle-state sidecar records the `capture_id`; the capture holds the
/// `response_body`) and runs the same [`crate::write::strike_answered_free_text_queue_heads`]
/// the finalize write path uses. This makes the strike a property of reaching
/// `committed` regardless of which path committed, so a recovery-path closeout
/// (`agent-doc commit` / `reset --from-current` then commit / `--force-disk`) no
/// longer leaves an answered head unstruck. Best-effort: a missing capture, an
/// inactive queue, or a strike error never blocks the commit.
fn strike_answered_free_text_heads_at_commit_seam(file: &Path) {
    let Some(response_body) = capture_response_body_for(file) else {
        return;
    };
    if response_body.trim().is_empty() {
        return;
    }
    // The commit seam is already the binary-owned closeout boundary and runs
    // before staging under the commit lock; use the force-disk strike branch so
    // recovery commits do not silently leave answered free-text heads live when
    // no editor listener is attached.
    match crate::write::strike_answered_free_text_queue_heads(file, &response_body, true) {
        Ok(0) => {}
        Ok(n) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_seam_free_text_strike file={} struck={} (#qheadstrike)",
                file.display(),
                n
            ),
        ),
        Err(e) => {
            eprintln!("[commit] warning: commit-seam free-text head strike failed: {e} (non-fatal)")
        }
    }
}

/// Load the captured `response_body` for `file`'s current cycle, if any
/// (`#qheadstrike`). Returns `None` when there is no cycle state, no recorded
/// `capture_id`, or no readable capture record.
fn capture_response_body_for(file: &Path) -> Option<String> {
    let state = agent_doc_cycle_state_io::load(file).ok().flatten()?;
    let capture_id = state.capture_id?;
    let record = agent_doc_capture_io::load_by_id(file, &capture_id)
        .ok()
        .flatten()?;
    Some(record.response_body)
}

/// Commit a file and report whether the VCS refresh signal was written.
///
/// `vcs_refresh_signaled` is:
/// - `Some(true)` when the commit path wrote `.agent-doc/patches/vcs-refresh.signal`
/// - `Some(false)` when a refresh target was available but writing it failed
/// - `None` when no refresh target was available or no new git commit was created
pub fn commit_with_outcome(file: &Path) -> Result<CommitOutcome> {
    let t_total = std::time::Instant::now();

    let (super_root, resolved) = resolve_to_git_root(file)?;
    // If the file lives inside a submodule, run git ops in the submodule itself.
    // The parent repo refuses to stage/commit paths that cross a submodule boundary
    // (`update-index --cacheinfo` and `git add` both fail with "appears as both a
    // file and as a directory" / "Pathspec ... is in submodule"). Routing the commit
    // through the submodule's own repo sidesteps the boundary entirely.
    let (git_root, in_submodule) = narrow_to_submodule(&super_root, &resolved);
    if in_submodule {
        eprintln!(
            "[commit] file is in submodule {} — running git ops there",
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
    // Serialize the short git index transaction per resolved repo / submodule.
    // Without this, two different docs in the same repo can still interleave on
    // one shared index even if their document hashes differ.
    let _commit_lock = CommitLock::acquire(&git_root);

    // `#qheadstrike` P2 — strike answered free-text queue heads at the COMMIT
    // seam, BEFORE staging, so the struck state lands in this same commit. The
    // finalize write path strikes before its commit, but the standalone
    // `agent-doc commit` recovery path (used by `reset --from-current` → commit
    // and `live_prompt_drift` auto-recovery) never ran the strike, leaving an
    // answered free-text head unstruck so it re-surfaced as a phantom queue head
    // (`#rt83`/`#qflood` churn). Idempotent (already-struck heads are skipped),
    // so the finalize path's earlier strike is unaffected; best-effort, never
    // blocks the commit.
    strike_answered_free_text_heads_at_commit_seam(file);

    let timestamp = chrono_timestamp();
    let msg = agent_doc_commit_message_for_file(file, &timestamp);

    // Selective commit: stage only the snapshot content (agent response),
    // leaving user edits in the working tree as uncommitted.
    //
    // If a snapshot exists, use git hash-object + update-index to stage the
    // snapshot version without touching the working tree file. This means:
    // - Agent response → committed (no git gutter)
    // - User's subsequent edits → uncommitted (green git gutter)
    let mut snapshot_content = agent_doc_snapshot_io::load(file)?;
    let mut file_content = std::fs::read_to_string(file).unwrap_or_default();
    let head_doc = agent_doc_git_io::revision::show_head(file)?;
    let snapshot_matched_head_before_absorb = snapshot_content
        .as_deref()
        .zip(head_doc.as_deref())
        .is_some_and(|(snapshot, head)| strip_head_markers(snapshot) == head);
    let bypassed_response_write = snapshot_matched_head_before_absorb
        .then(|| crate::session_check::detect_bypassed_response_write(file))
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
    // #nm1x: intersect the snapshot↔HEAD drift against the current turn scope so
    // an independent out-of-scope edit is not misclassified as blocking
    // `typed_component_drift`.
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
                    agent_doc_turn::document_drift::detect_bypassed_response_write_between(
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
    // #exch-intermix: before failing closed on the `live_prompt_drift_after_preflight`
    // wedge (adopted `content_ours` snapshot larger than the fragmented visible
    // file), rebase the missing agent response onto the realtime document. This
    // is not snapshot adoption: queue/backlog/frontmatter and other
    // operator-visible state stay as they are in `file_content`.
    if let Some(snapshot) = snapshot_content.as_deref()
        && let Some(recovered) =
            crate::write::try_auto_recover_live_prompt_drift(file, snapshot, &file_content)?
    {
        file_content = recovered;
        snapshot_content = agent_doc_snapshot_io::load(file)?;
    }
    if crate::write::guard_no_stale_snapshot_reset_drift(
        file,
        snapshot_content.as_deref(),
        &file_content,
        "commit",
    )? {
        snapshot_content = agent_doc_snapshot_io::load(file)?;
    }

    let repaired_committed_historical =
        if let Some(reason) = repair_committed_historical_snapshot_drift(file)? {
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
        if ipc_snapshot_adoption_blocked {
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

    dedupe_snapshot_and_worktree_before_commit(file, &mut snapshot_content, &mut file_content)?;

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
            repair_stale_agent_response_collapse_worktree(file, head, &file_content)?
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

    // Warn on significant file/snapshot drift — may indicate an out-of-band write
    // that bypassed the agent-doc write pipeline (snapshot not updated).
    let snap_len = snapshot_content.as_ref().map(|s| s.len()).unwrap_or(0);
    let file_len_after_repair = file_content.len();
    if snap_len > 0 && file_len_after_repair > snap_len && post_commit_local_drift.is_none() {
        let drift = file_len_after_repair - snap_len;
        // Log unclassified positive drift for aggregation/root-cause analysis.
        // Classified post-commit local edits have their own markers below.
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
                "[commit] WARNING: file is {} bytes larger than snapshot for {} — possible out-of-band write (snap={}, file={})",
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

            // Extreme drift can happen when a newly-bootstrapped document still
            // has the empty scaffold snapshot but the working tree now contains
            // the real file content. Only auto-resync that bootstrap case for
            // files with no HEAD entry yet. Tracked documents stay
            // snapshot-selective here so unanswered user prompts cannot be
            // swallowed into the committed snapshot during preflight.
            if file_len_after_repair > snap_len * 5
                && let Some(ref snapshot) = snapshot_content
            {
                let head_exists = head_doc.is_some();
                let scaffold_snapshot = is_empty_template_scaffold_snapshot(snapshot);
                if !head_exists && scaffold_snapshot {
                    eprintln!(
                        "[commit] Extreme drift detected ({}x) — re-syncing bootstrap scaffold snapshot from file content",
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
                        "[commit] Extreme drift detected ({}x) — NOT re-syncing tracked/non-scaffold snapshot",
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

    // Handle missing snapshot: if no snapshot exists but file has content, create one.
    // This bootstraps the commit flow for files that were never written by agent-doc.
    //
    // NOTE: Snapshot/file divergence detection (Bug 2B) was removed here because it
    // cannot distinguish "file has user edits" from "file has a missed agent response".
    // Both cases look identical (file has content snapshot doesn't). The IPC snapshot
    // save failure case is handled by Bug 2A (non-fatal save with warning) and the
    // recover step in preflight (detects orphaned responses).
    if snapshot_content.is_none() && !file_content.is_empty() {
        eprintln!(
            "[commit] WARNING: no snapshot exists for {}. Creating from file content.",
            file.display()
        );
        agent_doc_snapshot_io::save(file, &file_content, agent_doc_ops_log_io::log_op)?;
        snapshot_content = Some(file_content.clone());
    }

    if snapshot_matches_head {
        ensure_active_capture_materialized_for_head_current_noop(
            file,
            snapshot_content.as_deref(),
            head_doc.as_deref(),
        )?;
        ensure_no_live_editor_buffer_ahead_of_disk(
            file,
            &file_content,
            "already_current",
            snapshot_content.as_deref().or(head_doc.as_deref()),
        )?;
        if let Some(kind) = post_commit_local_drift {
            if kind == PostCommitLocalDriftKind::UserFollowUp {
                eprintln!(
                    "[commit] prior response is already committed in HEAD for {} — leaving later local user follow-up edits uncommitted for the next response cycle. This is not a full closeout for the follow-up prompt; run `agent-doc {}` to answer it or pipe the response through `agent-doc write --commit {}`.",
                    file.display(),
                    file.display(),
                    file.display()
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "post_commit_user_follow_up file={} basis=head",
                        file.display()
                    ),
                );
                if cycle_is_terminal(file) {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "commit_prompt_handoff_noop file={} basis=head",
                            file.display()
                        ),
                    );
                    let elapsed_total = t_total.elapsed().as_millis();
                    if elapsed_total > 0 {
                        eprintln!("[perf] commit total: {}ms", elapsed_total);
                    }
                    return Ok(CommitOutcome {
                        did_commit: false,
                        vcs_refresh_signaled: None,
                    });
                }
            } else {
                eprintln!(
                    "[commit] detected post-commit local drift for {} — HEAD already contains the committed response; leaving {} uncommitted",
                    file.display(),
                    kind.describe()
                );
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "post_commit_local_drift file={} kind={} basis=head",
                    file.display(),
                    kind.as_str()
                ),
            );
        }
        eprintln!(
            "[commit] staged snapshot already matches HEAD for {} — closing cycle as already committed",
            file.display()
        );
        let (snapshot_after_noop, file_after_noop) =
            repair_clean_head_if_only_transient_worktree_drift(file, &file_content)?
                .unwrap_or((snapshot_content.clone(), file_content.clone()));
        finalize_already_committed_noop(
            file,
            "commit_already_current",
            snapshot_after_noop.as_deref(),
            Some(&file_after_noop),
            post_commit_local_drift,
        );

        // Even for no-op submodule commits, the parent pointer may be stale
        // (e.g., submodule committed in a previous cycle but parent never updated).
        if in_submodule && agent_doc_git_io::submodule::is_submodule_pointer_stale(file) {
            eprintln!("[commit] submodule pointer stale in parent after no-op commit — updating");
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

    // #fmdrop — stale-CRDT / synthetic-reap corruption self-heal. The commit
    // path stages the snapshot (the agent-response image), not the working
    // tree, so a corrupt snapshot — a stale-base CRDT merge, or a
    // `no_liveness_signals` synthetic auto-reap that serialized a scaffold/empty
    // base — would persist to HEAD even though the operator's live document is
    // intact. Preflight then collapses the snapshot back to that corrupt HEAD
    // every cycle, so `doc != snapshot` never converges and the supervisor spins
    // the open cycle (`suprecyclespin` `cycle_never_closed`).
    //
    // Frontmatter is config, never selectively committed, so it is always
    // authoritative from the live document. Rather than fail closed (which would
    // itself let the snapshot interfere with the hot path), overlay the live
    // frontmatter onto the staged content and regenerate the snapshot sidecar in
    // the background. A corrupt snapshot self-heals instead of poisoning HEAD.
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
                "[commit] snapshot dropped operator frontmatter key(s) [{}] — restoring from the live document and regenerating the snapshot (#fmdrop)",
                dropped.join(", ")
            );
            // Best-effort snapshot-sidecar regeneration so recovery state matches
            // the corrected hot-path content; never blocks the commit.
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

    // Reposition boundary BEFORE staging so the commit captures the new
    // boundary id atomically. Previously this ran post-commit, which left
    // the boundary-id delta to be picked up by the next turn's preflight
    // commit — producing two commits per turn (one for the prior turn's
    // stale reposition, one for the current turn's content). Running it
    // here folds both into a single commit.
    //
    // The active-run guard inside `reposition_boundary_in_snapshot` still
    // applies: if a concurrent `agent-doc write` is in flight, reposition
    // is skipped and the IPC path owns the transition, matching prior
    // behavior for that case.
    let t_reposition = std::time::Instant::now();
    let _snap_changed = reposition_boundary_in_snapshot(file);
    // Reload snapshot_content from disk — the reposition may have rewritten
    // it with a fresh boundary id. Staging must use the repositioned blob.
    if let Ok(Some(reloaded)) = agent_doc_snapshot_io::load(file) {
        snapshot_content = Some(reloaded);
    }
    file_content = std::fs::read_to_string(file).unwrap_or_default();
    dedupe_snapshot_and_worktree_before_commit(file, &mut snapshot_content, &mut file_content)?;
    ensure_no_live_editor_buffer_ahead_of_disk(
        file,
        &file_content,
        "pre_stage",
        snapshot_content.as_deref(),
    )?;
    ensure_active_capture_materialized_for_commit(
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
                eprintln!(
                    "[commit] skipped ignored untracked path {} (matched .gitignore); not staging",
                    path
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "commit_skipped_ignored_path file={} rel_path={}",
                        file.display(),
                        path
                    ),
                );
                break Err(anyhow::anyhow!(
                    "refusing to commit ignored untracked path {} (matched .gitignore)",
                    path
                ));
            }
            Err(CommitTransactionError::Fatal(err)) => break Err(err),
        }
    };
    let commit_status = commit_output.as_ref().map(|o| o.status);
    let elapsed_commit = t_commit.elapsed().as_millis();
    if elapsed_commit > 0 {
        eprintln!("[perf] commit.git_commit: {}ms", elapsed_commit);
    }

    // Log commit result line to stderr (suppress verbose git status output)
    if let Ok(ref o) = commit_output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Only print the commit result line (e.g. "[main abc123] message")
            // and skip git status output (branch info, file listings, etc.)
            if trimmed.starts_with('[') && trimmed.contains(']') {
                eprintln!("{}", line);
            }
        }
    }

    // Log commit result
    let mut did_commit = false;
    match &commit_status {
        Ok(s) if s.success() => {
            did_commit = true;
            enforce_committed_single_boundary_invariant(file, &git_root, &resolved);
            agent_doc_ops_log_io::log_cycle(file, "commit", None, None);
            agent_doc_ops_log_io::log_op(file, &format!("commit_success file={}", file.display()));
            agent_doc_flow_io::log_flow_event(
                file,
                agent_doc_flow::types::FlowEvent::new(
                    agent_doc_flow::types::FlowName::Closeout,
                    agent_doc_flow::types::FlowStage::Commit,
                    agent_doc_flow::types::FlowOutcome::Completed,
                )
                .with_reason("commit_success"),
                agent_doc_ops_log_io::log_op,
            );
            let snap = agent_doc_snapshot_io::load(file).ok().flatten();
            let file_content = std::fs::read_to_string(file).ok();
            if let Err(e) = agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
                &crate::PIPELINE_FRONTMATTER_EFFECTS,
                file,
                "commit_success",
                snap.as_deref(),
                file_content.as_deref(),
            ) {
                eprintln!("[commit] cycle-state update failed: {} (non-fatal)", e);
            }
            if let Err(e) = agent_doc_capture_io::mark_committed(file) {
                eprintln!("[commit] capture-state update failed: {} (non-fatal)", e);
            }
            // `#qdurcrash`: a successful commit makes the queue state durable in
            // the snapshot, so the crash-durability journal is emptied. This
            // bounds the journal (and thus any replay) to operator queue
            // additions observed since the last commit — the crash window.
            agent_doc_queue_io::queue_journal::clear(file);
            // Reconcile the durable auto-queue continuation marker: write it when
            // a clean closeout still owes an `agent:queue auto` continuation,
            // clear it otherwise. Binary-owned proof that survives missing Codex
            // hook session state. (#codex-auto-queue-stalled-final-gate)
            if let Some(continuation) = crate::queue_continuation::reconcile_marker(file, "commit")
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_continuation_required file={} head={}",
                        file.display(),
                        continuation.head_prompt.replace('\n', " ")
                    ),
                );
                // #mphaseloop (multi-phase auto-loop policy): a phase routed to
                // `agent:review` this cycle must NOT terminate the go-mode drain.
                // When this closeout still owes a drainable continuation AND it
                // added an open review item (a phase moved to review rather than
                // completed or genuinely blocked), emit the proof that the drain
                // advances to the next drainable head instead of stalling.
                if let (Some(prior), Some(current)) = (head_doc.as_deref(), snap.as_deref())
                    && agent_doc_queue::queue_continuation::review_phase_routed(prior, current)
                {
                    let next_head = continuation
                        .head_id
                        .as_deref()
                        .unwrap_or(continuation.head_prompt.as_str());
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "drain_continue_after_review file={} next_head={} (#mphaseloop)",
                            file.display(),
                            next_head.replace('\n', " ")
                        ),
                    );
                }
            }
            // Fire post_commit hook for cross-session coordination
            let session_id =
                agent_doc_frontmatter_io::session::read_session_id(file).unwrap_or_default();
            agent_doc_hooks_io::fire_post_commit(file, &session_id, None);
            agent_doc_hooks_io::fire_doc_event(file, "post_commit");
        }
        Ok(s) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "commit_failed file={} exit_code={}",
                    file.display(),
                    s.code().unwrap_or(-1)
                ),
            );
        }
        Err(e) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!("commit_error file={} err={}", file.display(), e),
            );
        }
    }

    // Post-commit housekeeping. The staged blob is already clean (commit
    // staging strips `(HEAD)` and guard markers from the snapshot before
    // `git hash-object`), and post-commit cleanup keeps the snapshot /
    // visible document in that same clean shape.
    let mut vcs_refresh_signaled = None;
    if let Ok(ref s) = commit_status
        && s.success()
    {
        // Boundary reposition happens pre-commit now (see above) so the
        // new boundary id lands in the same commit as the response.
        // The post-commit IPC reposition signal is intentionally skipped when
        // this commit changed non-exchange surfaces. A reposition-only editor
        // save can otherwise flush a stale live buffer and drop freshly
        // committed queue/backlog/icebox state.
        if should_send_post_commit_ipc_reposition(file) {
            crate::write::try_ipc_reposition_boundary(file);
        } else {
            eprintln!(
                "[commit] skipping IPC boundary reposition: commit changed non-exchange tracked surfaces"
            );
        }

        // Signal plugin to refresh VCS state so the gutter reflects the commit.
        // Without this, the IDE shows the entire response as uncommitted until
        // the user manually refreshes the file.
        // Uses file-based signal (vcs-refresh.signal) since the socket listener
        // may not be active — the plugin watches .agent-doc/patches/ for both
        // patch files and signal files.
        if let Some(signal_file) = vcs_refresh_signal_path(file) {
            match std::fs::write(&signal_file, "") {
                Ok(()) => {
                    eprintln!("[commit] VCS refresh signal written");
                    vcs_refresh_signaled = Some(true);
                }
                Err(e) => {
                    eprintln!("[commit] VCS refresh signal failed: {} (non-fatal)", e);
                    vcs_refresh_signaled = Some(false);
                }
            }
        }

        // Strip ephemeral guard markers from snapshot and working tree so they
        // match the committed blob (which was already stripped during staging).
        strip_guard_markers_from_disk(file);
        if let Ok(cleaned) = std::fs::read_to_string(file) {
            match repair_clean_head_if_only_transient_worktree_drift(file, &cleaned) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    if let Err(e) = refresh_live_closeout_sidecars(file, &cleaned, false) {
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

        // Emit the live worktree==HEAD proof line after post-commit cleanup +
        // transient-drift repair have run, so it reflects the residual visible
        // state. `match=false` is the #postcommit-ipc-worktree-corruption signal.
        emit_postcommit_worktree_check(file);

        // Submodule pointer update: if we just committed inside a submodule,
        // stage the new submodule HEAD in the parent and partial-commit it.
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

fn vcs_refresh_signal_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let signal_file = project_root.join(".agent-doc/patches/vcs-refresh.signal");
    signal_file.parent().filter(|p| p.exists())?;
    Some(signal_file)
}

fn dedupe_snapshot_and_worktree_before_commit(
    file: &Path,
    snapshot_content: &mut Option<String>,
    file_content: &mut String,
) -> Result<()> {
    let Some(snapshot) = snapshot_content.as_deref() else {
        return Ok(());
    };
    let deduped_snapshot = agent_doc_turn::response_replay::dedupe_responses(snapshot);
    if deduped_snapshot != snapshot {
        eprintln!(
            "[commit] deduped consecutive duplicate response block(s) before staging {}",
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_pre_stage_dedupe file={} before_commit=true",
                file.display()
            ),
        );
        agent_doc_snapshot_io::save(file, &deduped_snapshot, agent_doc_ops_log_io::log_op)?;
        *snapshot_content = Some(deduped_snapshot);
    }

    let deduped_file = agent_doc_turn::response_replay::dedupe_responses(file_content);
    if deduped_file != *file_content {
        crate::write::atomic_write_pub(file, &deduped_file).with_context(|| {
            format!(
                "failed to repair duplicate response blocks in {}",
                file.display()
            )
        })?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_pre_stage_dedupe_repaired_worktree file={} before_commit=true",
                file.display()
            ),
        );
        *file_content = deduped_file;
    }

    if let Some(snapshot) = snapshot_content.as_deref()
        && let Some(repaired_file) = crate::write::repair_commit_prompt_artifacts_against_snapshot(
            file,
            snapshot,
            file_content,
        )
    {
        let mut snapshot_updated = false;
        if exchange_prompt_prefix_equivalent(snapshot, &repaired_file) {
            let clean_snapshot = strip_head_markers(&repaired_file);
            agent_doc_snapshot_io::save(file, &clean_snapshot, agent_doc_ops_log_io::log_op)?;
            *snapshot_content = Some(clean_snapshot);
            snapshot_updated = true;
        }
        if repaired_file != *file_content {
            crate::write::atomic_write_pub(file, &repaired_file).with_context(|| {
                format!(
                    "failed to repair duplicate prompt artifacts in {}",
                    file.display()
                )
            })?;
            *file_content = repaired_file;
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_pre_stage_prompt_duplicate_repaired file={} snapshot_updated={} before_commit=true",
                file.display(),
                snapshot_updated
            ),
        );
    }

    Ok(())
}

/// Strip ephemeral guard markers from the snapshot and working-tree file on disk.
/// Best-effort: logs warnings on failure but does not propagate errors.
fn strip_guard_markers_from_disk(file: &Path) {
    if let Ok(Some(ref content)) = agent_doc_snapshot_io::load(file) {
        let cleaned = strip_guard_markers(content);
        if cleaned != *content
            && let Err(e) =
                agent_doc_snapshot_io::save(file, &cleaned, agent_doc_ops_log_io::log_op)
        {
            eprintln!("[commit] warning: failed to strip guard markers from snapshot: {e}");
        }
    }
    if let Ok(content) = std::fs::read_to_string(file) {
        let cleaned = strip_guard_markers(&content);
        // #fccaudit: route the working-tree guard-marker strip through the
        // editor-IPC converge gate so it never writes behind a live JB editor
        // buffer (File Cache Conflict). The stripped markers
        // (`<!-- no-pending-capture -->`, `<!-- no-pending-done-guard -->`) are
        // ephemeral directives, not `(HEAD)` annotations, so the editor buffer
        // should drop them too. With no listener this falls back to the same
        // byte-for-byte disk write as before.
        if cleaned != content
            && let Err(e) = crate::write::converge_or_disk_write(
                file,
                &content,
                &cleaned,
                "strip_guard_markers",
            )
        {
            eprintln!("[commit] warning: failed to strip guard markers from file: {e}");
        }
    }
}

/// Reposition boundary in snapshot AND working tree deterministically.
///
/// After commit, moves the boundary to the end of exchange in both the
/// snapshot and the working-tree file. The active-run guard
/// (`pending_path_for`) prevents racing a concurrent `agent-doc write`:
/// in-flight runs are skipped so the plugin's IPC write path owns the
/// transition. Outside of an active run — including sweep-committed
/// foreign docs that never touch `agent-doc write` — this function is
/// the canonical place the on-disk state becomes consistent.
///
/// The cleanup uses the clean boundary-only reposition helper so the on-disk
/// snapshot/file match the committed blob shape instead of introducing
/// transient `(HEAD)` / boundary-only churn.
///
/// Returns true if the snapshot OR working tree content changed.
fn reposition_boundary_in_snapshot(file: &Path) -> bool {
    // An in-flight `agent-doc write` owns the *working-tree/editor* transition
    // via IPC — but the binary-owned snapshot/staged blob is never raced by the
    // editor (git stages a snapshot, not the live file). So the active-run guard
    // must scope ONLY the disk/working-tree rewrite below; the snapshot collapse
    // that follows runs unconditionally. Previously this early-returned before
    // the snapshot collapse too, so a wedged finalize (leftover pending-response
    // file) that landed via a direct commit skipped the collapse and accreted
    // one boundary per cycle (#boundaryaccum1).
    let active_run = file
        .canonicalize()
        .ok()
        .and_then(|canonical| agent_doc_fs::pending_response_path_for(&canonical).ok())
        .map(|pending_path| pending_path.exists())
        .unwrap_or(false);

    let mut changed = false;

    // Reposition the snapshot to the same clean shape we stage into git.
    // ALWAYS runs — the snapshot is a binary-owned artifact that never races the
    // live editor buffer, so collapsing it to exactly one boundary here is the
    // invariant-enforcement point regardless of which write path delivered the
    // response (finalize, wedged direct commit, or sweep commit).
    if let Ok(Some(snap_content)) = agent_doc_snapshot_io::load(file) {
        let prompt_canonicalized = canonicalize_answered_prompt_prefixes(&snap_content);
        let new_snap = agent_doc_template::reposition_boundary_to_end_clean(&prompt_canonicalized);
        if new_snap != snap_content {
            match agent_doc_snapshot_io::save(file, &new_snap, agent_doc_ops_log_io::log_op) {
                Ok(()) => {
                    eprintln!("[commit] repositioned boundary in snapshot");
                    changed = true;
                }
                Err(e) => {
                    eprintln!(
                        "[commit] failed to update snapshot after boundary reposition: {}",
                        e
                    );
                }
            }
        }
    }

    // Working-tree/editor rewrite. Skipped during an active run — the in-flight
    // `agent-doc write` owns the disk/editor transition via IPC. The snapshot
    // collapse above already ran, so the staged/committed blob is single-boundary
    // regardless; only the live disk file is deferred here.
    if active_run {
        eprintln!("[commit] skipping working-tree boundary reposition — active run detected");
        return changed;
    }

    // Reposition in the working tree unless a live IDE listener is available.
    // A stale `.agent-doc/patches/` directory by itself is not enough — the
    // reposition signal is socket-based, so skipping the disk rewrite without
    // a listener would leave boundary-only dirtiness behind.
    //
    // When the listener is present, the IPC reposition signal (sent
    // post-commit) lets the plugin handle the working-tree boundary via
    // Document API, coordinated with user edits.
    // Doing a disk-level read-modify-write here races with the user typing
    // in the IDE, producing duplicate structural tails (bug #xbs3).
    let ipc_listener_active = file
        .canonicalize()
        .map(|c| agent_doc_project_root_io::resolve_ipc_project_root(&c))
        .map(|root| agent_doc_ipc_io::is_listener_active(&root))
        .unwrap_or(false);
    if ipc_listener_active {
        eprintln!("[commit] skipping working-tree boundary reposition — IPC listener active");
    } else if let Ok(working) = std::fs::read_to_string(file) {
        let prompt_canonicalized = canonicalize_answered_prompt_prefixes(&working);
        let snapshot_after_reposition = agent_doc_snapshot_io::load(file).ok().flatten();
        let normalize_prefix_lines = snapshot_after_reposition
            .as_deref()
            .map(|snapshot| {
                agent_doc_element_exchange::extract_post_commit_normalization_targets(
                    snapshot,
                    &prompt_canonicalized,
                )
            })
            .unwrap_or_default();
        let prefix_repaired = if normalize_prefix_lines.is_empty() {
            prompt_canonicalized
        } else {
            agent_doc_element_exchange::normalize_exchange_prefixes_for_targets(
                &prompt_canonicalized,
                &normalize_prefix_lines,
            )
        };
        let repositioned =
            agent_doc_template::reposition_boundary_to_end_preserve_head(&prefix_repaired);
        if repositioned != working {
            let committed_boundary_id = snapshot_after_reposition.as_deref().and_then(|snapshot| {
                agent_doc_element_boundary::boundary::find_boundary_id(snapshot, "exchange")
            });
            let file_ipc = crate::write::queue_file_ipc_reposition_boundary(
                file,
                committed_boundary_id.as_deref(),
                &normalize_prefix_lines,
            );
            match file_ipc {
                Ok(crate::write::FileIpcRepositionResult::Queued) => {
                    eprintln!("[commit] queued working-tree boundary reposition through file IPC");
                    changed = true;
                }
                Ok(crate::write::FileIpcRepositionResult::DeferredExistingPatch) => {
                    eprintln!(
                        "[commit] deferred working-tree boundary reposition to existing file IPC patch"
                    );
                    changed = true;
                }
                Ok(crate::write::FileIpcRepositionResult::Unavailable) => {
                    match crate::write::atomic_write_pub(file, &repositioned) {
                        Ok(()) => {
                            if normalize_prefix_lines.is_empty() {
                                eprintln!("[commit] repositioned boundary in working tree");
                            } else {
                                eprintln!(
                                    "[commit] repaired {} prefix lines and repositioned boundary in working tree",
                                    normalize_prefix_lines.len()
                                );
                            }
                            changed = true;
                        }
                        Err(e) => {
                            eprintln!(
                                "[commit] failed to reposition boundary in working tree: {}",
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[commit] failed to queue file IPC boundary reposition: {}",
                        e
                    );
                    match crate::write::atomic_write_pub(file, &repositioned) {
                        Ok(()) => {
                            eprintln!("[commit] repositioned boundary in working tree");
                            changed = true;
                        }
                        Err(e) => {
                            eprintln!(
                                "[commit] failed to reposition boundary in working tree: {}",
                                e
                            );
                        }
                    }
                }
            }
        }
    }

    changed
}

/// Enforce the single-boundary invariant on the just-committed HEAD artifact
/// (#boundaryaccum1). The pre-stage snapshot collapse in
/// `reposition_boundary_in_snapshot` should already guarantee this, but a
/// previously-accreted blob (committed before this fix, or any future
/// regression) is caught here and self-healed with a binary-owned follow-up
/// collapse commit. This never races the live editor — it re-collapses the
/// committed content, which is a binary-owned artifact, not the editor buffer.
fn enforce_committed_single_boundary_invariant(file: &Path, git_root: &Path, resolved: &Path) {
    let Ok(Some(head_blob)) = agent_doc_git_io::revision::show_head(file) else {
        return;
    };
    let boundary_count = head_blob
        .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
        .count();
    if boundary_count <= 1 {
        return;
    }
    eprintln!(
        "[commit] boundary_invariant_violation: committed HEAD carries {} agent:boundary markers (expected 1) — self-healing collapse",
        boundary_count
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "boundary_invariant_violation phase=post_commit file={} committed_boundaries={}",
            file.display(),
            boundary_count
        ),
    );
    let collapsed = agent_doc_template::reposition_boundary_to_end_clean(&head_blob);
    let collapsed_count = collapsed
        .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
        .count();
    if collapsed == head_blob || collapsed_count > 1 {
        eprintln!(
            "[commit] boundary_invariant self-heal could not reduce to a single boundary (still {}); leaving for next cycle",
            collapsed_count
        );
        return;
    }
    // Keep the snapshot aligned with the collapsed blob so the next preflight
    // does not observe snapshot/HEAD drift.
    if let Err(e) = agent_doc_snapshot_io::save(file, &collapsed, agent_doc_ops_log_io::log_op) {
        eprintln!(
            "[commit] boundary_invariant self-heal snapshot save failed: {} (non-fatal)",
            e
        );
    }
    match stage_and_commit_once(
        git_root,
        resolved,
        Some(collapsed.as_str()),
        "agent-doc: collapse accreted agent:boundary markers (#boundaryaccum1)",
    ) {
        Ok(_) => {
            eprintln!("[commit] boundary_invariant self-heal collapse committed");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "boundary_invariant_selfheal_committed file={}",
                    file.display()
                ),
            );
        }
        Err(_) => {
            eprintln!("[commit] boundary_invariant self-heal collapse failed (non-fatal)");
        }
    }
}

fn refresh_live_closeout_sidecars(
    file: &Path,
    committed_doc: &str,
    signal_editor_refresh: bool,
) -> Result<Option<bool>> {
    if agent_doc_frontmatter::frontmatter::content_uses_crdt_write(committed_doc) {
        let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(committed_doc).encode_state();
        agent_doc_merge_io::save_document_crdt(file, &crdt, committed_doc)?;
    }

    if !signal_editor_refresh {
        return Ok(None);
    }

    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };

    if agent_doc_ipc_io::is_listener_active(&root)
        && agent_doc_ipc_io::send_vcs_refresh(&root).unwrap_or(false)
    {
        return Ok(Some(true));
    }

    let Some(signal_file) = vcs_refresh_signal_path(file) else {
        return Ok(None);
    };
    match std::fs::write(&signal_file, "") {
        Ok(()) => Ok(Some(true)),
        Err(e) => {
            eprintln!(
                "[commit] VCS refresh signal failed during closeout sidecar refresh: {}",
                e
            );
            Ok(Some(false))
        }
    }
}

/// Write content to git's object database and return the blob hash.
fn hash_object(git_root: &Path, content: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(content.as_bytes())?;
            }
            child.wait_with_output()
        })?;
    if !output.status.success() {
        anyhow::bail!("git hash-object failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

enum CommitTransactionError {
    RetryableIndexLock { phase: &'static str, detail: String },
    IgnoredPath { path: String },
    Fatal(anyhow::Error),
}

fn git_path_is_tracked(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(rel_path)
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    Ok(output.status.success())
}

fn git_path_is_ignored(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["check-ignore", "--quiet", "--no-index", "--"])
        .arg(rel_path)
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(CommitTransactionError::Fatal(anyhow::anyhow!(
            "git check-ignore failed for {}: {}",
            rel_path.display(),
            render_git_process_output(&output)
        ))),
    }
}

fn git_path_is_ignored_untracked(
    git_root: &Path,
    rel_path: &Path,
) -> std::result::Result<bool, CommitTransactionError> {
    if git_path_is_tracked(git_root, rel_path)? {
        return Ok(false);
    }
    git_path_is_ignored(git_root, rel_path)
}

fn git_add_force(
    git_root: &Path,
    resolved: &Path,
) -> std::result::Result<(), CommitTransactionError> {
    let rel_path = relative_to_root(resolved, git_root);
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["add", "-f", &rel_path.to_string_lossy()])
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    if !output.status.success() {
        let detail = render_git_process_output(&output);
        if output_has_index_lock_contention(&output) {
            return Err(CommitTransactionError::RetryableIndexLock {
                phase: "git add",
                detail,
            });
        }
        return Err(CommitTransactionError::Fatal(anyhow::anyhow!(
            "git add failed: {}",
            detail
        )));
    }
    Ok(())
}

fn stage_snapshot_for_commit(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
) -> std::result::Result<(), CommitTransactionError> {
    let rel_path = relative_to_root(resolved, git_root);
    if git_path_is_ignored_untracked(git_root, &rel_path)? {
        return Err(CommitTransactionError::IgnoredPath {
            path: rel_path.to_string_lossy().into_owned(),
        });
    }

    if let Some(snap) = snapshot_content {
        // Stage a CLEAN copy of the snapshot — `(HEAD)` is transient metadata
        // and must never appear in the committed blob. Post-commit cleanup also
        // collapses the working tree/snapshot back to the same clean shape, so
        // moving the boundary across cycles cannot produce phantom marker-only
        // diffs on previously committed headings.
        let staged_content = strip_guard_markers(&strip_head_markers(
            &canonicalize_answered_prompt_prefixes(snap),
        ));
        if let Ok(hash) = hash_object(git_root, &staged_content) {
            let cacheinfo = format!("100644,{},{}", hash, rel_path.to_string_lossy());
            let output = Command::new("git")
                .current_dir(git_root)
                .args(["update-index", "--add", "--cacheinfo", &cacheinfo])
                .output()
                .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
            if !output.status.success() {
                if output_has_index_lock_contention(&output) {
                    return Err(CommitTransactionError::RetryableIndexLock {
                        phase: "update-index",
                        detail: render_git_process_output(&output),
                    });
                }
                eprintln!("[commit] update-index failed, falling back to git add");
                return git_add_force(git_root, resolved);
            }
            return Ok(());
        }
    }

    // No snapshot or hash-object fallback — stage the live file path.
    git_add_force(git_root, resolved)
}

fn stage_and_commit_once(
    git_root: &Path,
    resolved: &Path,
    snapshot_content: Option<&str>,
    msg: &str,
) -> std::result::Result<std::process::Output, CommitTransactionError> {
    stage_snapshot_for_commit(git_root, resolved, snapshot_content)?;

    // Commit — ignore failure (nothing to commit is fine).
    // Use .output() to capture stdout (prevents git status leaking to stdout
    // when called from `preflight` which reserves stdout for JSON).
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["commit", "-m", msg, "--no-verify"])
        .output()
        .map_err(|e| CommitTransactionError::Fatal(e.into()))?;
    if !output.status.success() && output_has_index_lock_contention(&output) {
        return Err(CommitTransactionError::RetryableIndexLock {
            phase: "git commit",
            detail: render_git_process_output(&output),
        });
    }
    Ok(output)
}

fn repair_stale_agent_response_collapse_worktree(
    file: &Path,
    head_doc: &str,
    file_content: &str,
) -> Result<Option<String>> {
    let Some(repaired) = repair_stale_agent_response_collapse_doc(head_doc, file_content) else {
        return Ok(None);
    };

    crate::write::atomic_write_pub(file, &repaired)?;
    if repaired == head_doc {
        agent_doc_snapshot_io::save(file, head_doc, agent_doc_ops_log_io::log_op)?;
    }
    refresh_live_closeout_sidecars(file, &repaired, true)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "stale_agent_response_collapse_cleanup file={} basis=head preserved_local_drift={}",
            file.display(),
            repaired != head_doc
        ),
    );
    Ok(Some(repaired))
}

fn repair_clean_head_if_only_transient_worktree_drift(
    file: &Path,
    file_content: &str,
) -> Result<Option<(Option<String>, String)>> {
    let Some(head_doc) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(None);
    };
    if file_content == head_doc {
        return Ok(None);
    }
    if let Some(repaired) =
        repair_stale_agent_response_collapse_worktree(file, &head_doc, file_content)?
    {
        return Ok(Some((Some(head_doc.clone()), repaired)));
    }
    if normalize_transient_agent_doc_markers(file_content)
        != normalize_transient_agent_doc_markers(&head_doc)
        && normalize_post_commit_re_heading_drift(file_content)
            != normalize_post_commit_re_heading_drift(&head_doc)
    {
        return Ok(None);
    }

    crate::write::atomic_write_pub(file, &head_doc)?;
    agent_doc_snapshot_io::save(file, &head_doc, agent_doc_ops_log_io::log_op)?;
    refresh_live_closeout_sidecars(file, &head_doc, true)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!("transient_cleanup file={} basis=head", file.display()),
    );
    Ok(Some((Some(head_doc.clone()), head_doc)))
}

/// `#postcommit-ipc-worktree-corruption` / `#realtimecutover` — emit a live
/// worktree-vs-HEAD proof line after each committed closeout, **observe-only**.
///
/// The legacy turn-based revert tower that used to live here
/// (`reconcile_postcommit_worktree_to_head`, the `content_ours` snapshot revert,
/// the `#pcwc`/`#pcwcwarn`/`#pzjy` queue/exchange reconcile-to-HEAD writes, and the
/// editor refresh/flush) was RIPPED OUT. It re-derived the working tree from HEAD
/// after a commit, which silently REVERTED operator edits that landed after the
/// snapshot (the recurring "agent:queue reverted to an old snapshot / lost free
/// text" class). The realtime CRDT replica owns disk reconciliation now — it always
/// watches the file and reconciles continuously — so the post-commit path must
/// never write back to the working tree. This check only RECORDS drift for
/// observability; it changes nothing on disk or in the editor.
///
/// The comparison is taken modulo the legitimate transient churn the working tree
/// intentionally keeps relative to the clean committed blob (boundary markers,
/// `(HEAD)` annotations, guard markers, the managed pipeline block, and independent
/// `agent:queue` maintenance) via [`normalize_for_replay_hash`]. Best-effort and
/// non-fatal: a missing HEAD blob or unreadable working tree skips the check.
fn emit_postcommit_worktree_check(file: &Path) {
    let head_doc = match agent_doc_git_io::revision::show_head(file) {
        Ok(Some(head)) => head,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[commit] postcommit worktree check: HEAD read failed (non-fatal): {e}");
            return;
        }
    };
    let working = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "[commit] postcommit worktree check: working-tree read failed (non-fatal): {e}"
            );
            return;
        }
    };
    let head_norm = normalize_for_replay_hash(&head_doc);
    let tree_norm = normalize_for_replay_hash(&working);
    let head_sha = agent_doc_hash::content_hash(&head_norm);
    let tree_sha = agent_doc_hash::content_hash(&tree_norm);
    let matches = head_norm == tree_norm;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "postcommit_worktree_check file={} head={} tree={} match={} action=observe_only_realtime_replica_owns_disk",
            file.display(),
            &head_sha[..head_sha.len().min(12)],
            &tree_sha[..tree_sha.len().min(12)],
            matches
        ),
    );
    if !matches {
        // `#realtimecutover`: NO revert. The realtime replica reconciles disk; the
        // legacy turn-based revert that clobbered operator edits is gone. We only
        // surface the drift so it stays auditable from `ops.log`.
        eprintln!(
            "[commit] postcommit_worktree_check match=false for {} — working tree differs from HEAD; realtime replica owns reconciliation, no revert (#realtimecutover)",
            file.display()
        );
    }
}

fn ensure_active_capture_materialized_for_head_current_noop(
    file: &Path,
    snapshot_content: Option<&str>,
    head_doc: Option<&str>,
) -> Result<()> {
    ensure_active_capture_materialized_for_commit(
        file,
        snapshot_content.or(head_doc),
        "head_current",
    )
}

fn ensure_active_capture_materialized_for_commit(
    file: &Path,
    staged_content: Option<&str>,
    basis: &str,
) -> Result<()> {
    let Some(capture) = agent_doc_capture_io::load_active(file)? else {
        return Ok(());
    };
    if matches!(
        capture.state,
        agent_doc_workflow::capture::CaptureState::Committed
            | agent_doc_workflow::capture::CaptureState::Discarded
    ) {
        return Ok(());
    }
    let Some(materialized) = staged_content else {
        return Ok(());
    };
    if agent_doc_turn::response_replay::response_materialized_in_content(
        &capture.response_body,
        materialized,
    ) {
        return Ok(());
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "commit_blocked_missing_captured_response file={} capture_id={} response_sha256={} basis={}",
            file.display(),
            capture.capture_id,
            capture.response_sha256,
            basis
        ),
    );
    crate::flow::closeout::log_closeout_guard_event(
        file,
        agent_doc_flow::types::FlowStage::TerminalGuard,
        agent_doc_flow::types::FlowOutcome::FailedClosed,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::AlreadyCommitted,
    );
    anyhow::bail!(
        "captured response body is not present in the staged snapshot for {} even though the snapshot already matches HEAD; refusing already-committed closeout. Replay the captured response with `agent-doc write --commit {}` before marking the cycle committed.",
        file.display(),
        file.display()
    );
}

fn ensure_no_live_editor_buffer_ahead_of_disk(
    file: &Path,
    file_content: &str,
    basis: &str,
    staged_content: Option<&str>,
) -> Result<()> {
    let file_str = file.display().to_string();
    let Some(snapshot) =
        agent_doc_debounce::live_buffer_diverges_from_content(&file_str, file_content)
    else {
        return Ok(());
    };
    let editor_id = snapshot.editor_id.as_deref().unwrap_or("unknown");
    if let Some(staged) = staged_content
        && live_buffer_snapshot_matches_content(&snapshot, staged)
        && snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY)
        && snapshot.edit_epoch <= snapshot.last_synced_epoch
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_live_buffer_ahead_of_disk_allowed file={} basis={} editor_id={} edit_epoch={} last_synced_epoch={} buffer_len={} disk_len={} reason=staged_snapshot_matches_synced_operator_buffer",
                file.display(),
                basis,
                editor_id,
                snapshot.edit_epoch,
                snapshot.last_synced_epoch,
                snapshot.len,
                file_content.len()
            ),
        );
        return Ok(());
    }
    if let Some(staged) = staged_content
        && snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY)
        && live_buffer_insertions_are_materialized_in_file(&snapshot, staged, file_content)
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "commit_live_buffer_ahead_of_disk_allowed file={} basis={} editor_id={} edit_epoch={} last_synced_epoch={} buffer_len={} disk_len={} allowance=staged_snapshot_excludes_materialized_operator_buffer",
                file.display(),
                basis,
                editor_id,
                snapshot.edit_epoch,
                snapshot.last_synced_epoch,
                snapshot.len,
                file_content.len()
            ),
        );
        return Ok(());
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "commit_blocked_live_buffer_ahead_of_disk file={} basis={} editor_id={} edit_epoch={} last_synced_epoch={} buffer_len={} disk_len={}",
            file.display(),
            basis,
            editor_id,
            snapshot.edit_epoch,
            snapshot.last_synced_epoch,
            snapshot.len,
            file_content.len()
        ),
    );
    crate::flow::closeout::log_closeout_guard_event(
        file,
        agent_doc_flow::types::FlowStage::PreCommitGuard,
        agent_doc_flow::types::FlowOutcome::Blocked,
        agent_doc_turn::closeout_guard::CloseoutGuardReason::ReplicaDeliveryPending,
    );
    anyhow::bail!(
        "live editor buffer has unflushed changes ahead of disk for {}; refusing to commit from stale disk (editor_id={}, edit_epoch={}, last_synced_epoch={})",
        file.display(),
        editor_id,
        snapshot.edit_epoch,
        snapshot.last_synced_epoch
    );
}

fn live_buffer_snapshot_matches_content(
    snapshot: &agent_doc_debounce::LiveBufferSnapshot,
    content: &str,
) -> bool {
    if snapshot.len == content.len()
        && snapshot
            .hash
            .eq_ignore_ascii_case(&agent_doc_hash::content_hash(content))
    {
        return true;
    }
    snapshot.content.as_ref().is_some_and(|editor_text| {
        normalize_transient_agent_doc_markers(editor_text)
            == normalize_transient_agent_doc_markers(content)
    })
}

fn live_buffer_insertions_are_materialized_in_file(
    snapshot: &agent_doc_debounce::LiveBufferSnapshot,
    staged_content: &str,
    file_content: &str,
) -> bool {
    let Some(editor_text) = snapshot.content.as_deref() else {
        return false;
    };

    let normalized_file = normalize_transient_agent_doc_markers(file_content);
    let normalized_staged = normalize_transient_agent_doc_markers(staged_content);
    let diff = similar::TextDiff::from_lines(staged_content, editor_text);
    let mut saw_insert = false;
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let inserted = change.value().trim_end_matches('\n');
        let normalized_inserted = normalize_transient_agent_doc_markers(inserted);
        let trimmed = normalized_inserted.trim();
        if trimmed.is_empty() || trimmed == "(HEAD)" || trimmed.starts_with("<!-- agent:boundary:")
        {
            continue;
        }
        saw_insert = true;
        if normalized_staged.contains(trimmed) {
            continue;
        }
        if !normalized_file.contains(trimmed) {
            return false;
        }
    }
    saw_insert
}

fn finalize_already_committed_noop(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
    drift_kind: Option<PostCommitLocalDriftKind>,
) {
    agent_doc_ops_log_io::log_cycle(file, "commit_noop", snapshot_content, file_content);
    let drift_kind = drift_kind
        .map(PostCommitLocalDriftKind::as_str)
        .unwrap_or("none");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "commit_noop file={} reason=already_current drift_kind={} basis=head",
            file.display(),
            drift_kind
        ),
    );
    agent_doc_flow_io::log_flow_event(
        file,
        agent_doc_flow::types::FlowEvent::new(
            agent_doc_flow::types::FlowName::Closeout,
            agent_doc_flow::types::FlowStage::Commit,
            agent_doc_flow::types::FlowOutcome::Completed,
        )
        .with_reason(format!("already_current_{drift_kind}")),
        agent_doc_ops_log_io::log_op,
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!("commit_already_current file={} basis=head", file.display()),
    );
    if let Err(e) = agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
        &crate::PIPELINE_FRONTMATTER_EFFECTS,
        file,
        event,
        snapshot_content,
        file_content,
    ) {
        eprintln!("[commit] cycle-state update failed: {} (non-fatal)", e);
    }
    if let Err(e) = agent_doc_capture_io::mark_committed(file) {
        eprintln!("[commit] capture-state update failed: {} (non-fatal)", e);
    }
    // Reconcile the durable auto-queue continuation marker on the
    // already-committed closeout path too. (#codex-auto-queue-stalled-final-gate)
    crate::queue_continuation::reconcile_marker(file, "commit_already_current");
}

fn cycle_is_terminal(file: &Path) -> bool {
    agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .is_some_and(|state| !state.is_open())
}

fn should_send_post_commit_ipc_reposition(file: &Path) -> bool {
    let Ok(Some(parent_doc)) = agent_doc_git_io::revision::show_rev(file, "HEAD^") else {
        return false;
    };
    let Ok(Some(head_doc)) = agent_doc_git_io::revision::show_rev(file, "HEAD") else {
        return false;
    };
    post_commit_ipc_reposition_only_exchange_safe(&parent_doc, &head_doc)
}

fn chrono_timestamp() -> String {
    // Use date command for simplicity — no extra dependency
    let output = Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".to_string(),
    }
}

#[cfg(test)]
mod th {
    use super::*;
    pub(crate) fn init_repo(repo: &Path) {
        Command::new("git")
            .current_dir(repo)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
    }
    pub(crate) fn commit_file(repo: &Path, rel: &str, content: &str, msg: &str) {
        let path = repo.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["add", "--", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }
    // --- Bug 2B regression tests ---
    // Verify that commit does NOT overwrite the snapshot with user edits.
    // The divergence detection was removed from commit because is_stale_baseline
    // cannot distinguish "file has user edits" from "file has a missed agent response" —
    // both look like "file has content snapshot doesn't have".
    // --- #73tv: repo-scoped commit serialization + full transaction retry ---
    fn start_fake_listener_with_ack_status(
        project_root: &Path,
        ack_status: Option<&'static str>,
    ) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let root_clone = root.clone();
            let result = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                if let Some(status) = ack_status {
                    return Some(
                        serde_json::json!({
                            "type": "ack",
                            "id": patch_id,
                            "status": status,
                            "reason": "test_refresh_failed"
                        })
                        .to_string(),
                    );
                }
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                if let Err(err) = std::fs::create_dir_all(&ack_dir) {
                    eprintln!(
                        "[test] fake listener failed to create ack dir {}: {err}",
                        ack_dir.display()
                    );
                }
                // Model the JB plugin's behavior: refresh_content messages carry
                // the new content in the message body (the IDE applies it to its
                // in-memory buffer without reading disk). Other message types
                // fall back to disk (patch files, etc.).
                let content = if v.get("type").and_then(|t| t.as_str()) == Some("refresh_content") {
                    v.get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    let file_path = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
                    if !file_path.is_empty() {
                        std::fs::read_to_string(file_path).unwrap_or_default()
                    } else {
                        String::new()
                    }
                };
                let ack_path = ack_dir.join(format!("{patch_id}.md"));
                if let Err(err) = std::fs::write(&ack_path, &content) {
                    eprintln!(
                        "[test] fake listener failed to write ack content {}: {err}",
                        ack_path.display()
                    );
                }
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
            if let Err(err) = result {
                eprintln!("[test] fake listener stopped: {err:#}");
            }
        })
    }
    pub(crate) fn start_fake_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        start_fake_listener_with_ack_status(project_root, None)
    }
    pub(crate) fn wait_for_listener(project_root: &Path) {
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }
    // --- Retired: run_stream unproven-IPC direct write ---
    // Unproven IPC now fails closed without saving snapshots or writing the document.
    // --- Submodule-aware commit routing ---
    // --- relative_to path normalization ---
}
#[cfg(test)]
pub(crate) use th::{commit_file, init_repo, start_fake_listener, wait_for_listener};

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;

    #[test]
    fn commit_adopts_manual_escaped_tail_cleanup_after_head_current_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            The routed prompt escaped below the exchange block.\n\
            It should be cleaned up without being treated as later drift.\n\n\
            do #oobtaildel. spec-test-build-install-commit-push\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let cleaned = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, cleaned).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();

        let did_commit = commit(&doc).expect("escaped tail cleanup should commit");
        assert!(did_commit, "cleanup deletion should create a commit");

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&head),
            normalize_transient_agent_doc_markers(cleaned),
            "HEAD should contain the cleanup deletion"
        );
        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&snap),
            normalize_transient_agent_doc_markers(cleaned),
            "snapshot should advance to the cleaned file"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_escaped_tail_cleanup file="),
            "cleanup should get a specific ops-log marker:\n{log}"
        );
        assert!(
            !log.contains("post_commit_local_drift file="),
            "cleanup-only deletion must not be classified as local drift:\n{log}"
        );
    }

    #[test]
    fn commit_allows_current_snapshot_to_replace_committed_historical_patchback() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let clean = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "clean exchange\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, clean).unwrap();
        agent_doc_snapshot_io::save(&doc, clean, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let historical_head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "clean exchange\n\n",
            "#code-review\n",
            "### Re: code review — gpt-5\n\n",
            "Historical patchback.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, historical_head).unwrap();
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

        let compacted = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] [#tmv7] Release workflow\n",
            "<!-- /agent:icebox -->\n",
        );
        fs::write(&doc, compacted).unwrap();
        agent_doc_snapshot_io::save(&doc, compacted, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(compacted), Some(compacted)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(compacted),
            Some(compacted),
        )
        .unwrap();

        let did_commit =
            commit(&doc).expect("current snapshot/file should replace the historical patchback");
        assert!(did_commit, "replacement commit should be created");

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&head_doc),
            normalize_transient_agent_doc_markers(compacted),
            "HEAD should advance to the compacted document:\n{head_doc}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("commit_blocked_committed_historical_patchback file="),
            "historical patchback should not block replacement commit:\n{log}"
        );
    }

    #[test]
    fn commit_dedupes_duplicate_response_snapshot_before_staging() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let initial = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #pbdupchurn
<!-- /agent:exchange -->
";
        commit_file(root, "session.md", initial, "add session");

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #pbdupchurn
### Re: #pbdupchurn — gpt-5

Implemented.
### Re: #pbdupchurn — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let doc = root.join("session.md");
        fs::write(&doc, duplicated).unwrap();
        agent_doc_snapshot_io::save(&doc, duplicated, agent_doc_ops_log_io::log_op).unwrap();

        let before = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        let before_count: usize = String::from_utf8_lossy(&before.stdout)
            .trim()
            .parse()
            .unwrap();

        let did_commit = commit(&doc).expect("deduped closeout should commit");
        assert!(did_commit);

        let after = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        let after_count: usize = String::from_utf8_lossy(&after.stdout)
            .trim()
            .parse()
            .unwrap();
        assert_eq!(
            after_count,
            before_count + 1,
            "dedupe must happen before the first closeout commit, not in a second cleanup commit"
        );

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        let snapshot = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(head.matches("### Re: #pbdupchurn — gpt-5").count(), 1);
        assert_eq!(snapshot.matches("### Re: #pbdupchurn — gpt-5").count(), 1);
        assert_eq!(working.matches("### Re: #pbdupchurn — gpt-5").count(), 1);
    }
    #[test]
    fn commit_blocks_snapshot_absorb_after_ipc_snapshot_adoption_blocked() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/state/cycles")).unwrap();

        let initial = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #snapabsorb
<!-- /agent:exchange -->
";
        commit_file(root, "session.md", initial, "add session");

        let snapshot = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #snapabsorb
### Re: #snapabsorb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let live = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do #snapabsorb
### Re: #snapabsorb — gpt-5

Implemented.
### Re: late socket replay — gpt-5

Duplicate replay should stay live.
<!-- /agent:exchange -->
";
        let doc = root.join("session.md");
        fs::write(&doc, live).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(initial), Some(initial)).unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc).unwrap();

        let did_commit = commit(&doc).expect("commit should stage content_ours snapshot");

        assert!(did_commit);
        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        let working = fs::read_to_string(&doc).unwrap();
        assert!(head.contains("### Re: #snapabsorb — gpt-5"));
        assert!(!head.contains("late socket replay"));
        assert!(!snapshot_after.contains("late socket replay"));
        assert!(
            working.contains("late socket replay"),
            "live divergent body should stay in the working tree for the next cycle"
        );
        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("snapshot_absorb_blocked_after_ipc_snapshot_adoption"),
            "blocked absorb should be logged:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("snapshot_absorb file="),
            "commit must not silently absorb the divergent disk body after IPC adoption was blocked:\n{ops_log}"
        );
    }
    #[test]
    fn reposition_boundary_to_end_basic() {
        let content = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc123 -->\nUser prompt.\n<!-- /agent:exchange -->\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        // Boundary should be after user prompt, before close tag
        assert!(result.contains("User prompt.\n<!-- agent:boundary:"));
        assert!(result.contains("-->\n<!-- /agent:exchange -->"));
        // Old boundary consumed
        assert!(!result.contains("abc123"));
    }
    #[test]
    fn reposition_boundary_no_exchange() {
        let content = "# No exchange component\nJust text.\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        // Should return unchanged if no exchange
        assert_eq!(result.trim(), content.trim());
    }
    #[test]
    fn reposition_boundary_preserves_user_edits() {
        let content = "<!-- agent:exchange patch=append -->\n### Re: Answer\nAgent response.\n<!-- agent:boundary:old-id -->\nUser's new prompt here.\nMore user text.\n<!-- /agent:exchange -->\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        assert!(
            result.contains("User's new prompt here."),
            "user edit must be preserved"
        );
        assert!(
            result.contains("More user text."),
            "user edit must be preserved"
        );
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User's new prompt here.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }
    #[test]
    fn reposition_boundary_cleans_multiple_stale() {
        // Simulate a document with multiple stale boundary markers
        let content = "<!-- agent:exchange patch=append -->\n\
            First response.\n\
            <!-- agent:boundary:aaa111 -->\n\
            Second response.\n\
            <!-- agent:boundary:bbb222 -->\n\
            User prompt.\n\
            <!-- /agent:exchange -->\n";
        let result = agent_doc_template::reposition_boundary_to_end(content);
        // All old boundaries should be removed
        assert!(
            !result.contains("aaa111"),
            "first stale boundary must be removed"
        );
        assert!(
            !result.contains("bbb222"),
            "second stale boundary must be removed"
        );
        // Exactly one fresh boundary should exist
        let boundary_count = result.matches("<!-- agent:boundary:").count();
        assert_eq!(
            boundary_count, 1,
            "exactly one boundary marker should remain"
        );
        // The single boundary should be after user prompt
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User prompt.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }
    #[test]
    fn is_stale_baseline_write_path_user_edits_in_baseline_not_stale() {
        // Write path: baseline has user edits appended, snapshot is the committed state.
        // is_stale_baseline(baseline_with_edits, snapshot) should be FALSE
        // because the baseline's exchange CONTAINS the snapshot's exchange content.
        let snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            <!-- /agent:exchange -->\n";
        let baseline_with_user_edits = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            Implement agent-kit changes.\n\
            Implement updates to agent-doc.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            !agent_doc_template::stale_baseline::is_stale_baseline(
                baseline_with_user_edits,
                snapshot
            ),
            "baseline with user edits should NOT be stale (it contains snapshot content)"
        );
    }
    #[test]
    fn is_stale_baseline_write_path_stale_baseline_detected() {
        // Write path: baseline is from before the last agent response.
        // is_stale_baseline(old_baseline, current_snapshot) should be TRUE.
        let current_snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            ### Re: Response 2\n\
            Second response.\n\
            <!-- /agent:exchange -->\n";
        let old_baseline = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            agent_doc_template::stale_baseline::is_stale_baseline(old_baseline, current_snapshot),
            "baseline missing committed response should be stale"
        );
    }
    #[test]
    fn is_in_git_repo_true_inside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(
            agent_doc_git_io::status::is_in_git_repo(&doc),
            "file inside git repo should return true"
        );
    }
    #[test]
    fn is_in_git_repo_false_outside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(
            !agent_doc_git_io::status::is_in_git_repo(&doc),
            "file outside git repo should return false"
        );
    }
    #[test]
    fn write_commit_lifecycle() {
        // Full lifecycle: git repo + snapshot + commit → verify commit in log.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        // Set up git repo
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Create and commit an initial file so HEAD exists
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        // Create a document at its pre-response state and commit it.
        let doc = root.join("session.md");
        let initial_content = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc, initial_content).unwrap();

        // Stage + initial commit so the file is tracked
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

        // Simulate a write cycle landing a new response: update both the
        // working tree and the snapshot with the post-response content so
        // commit staging has something to commit.
        let post_response = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, post_response).unwrap();

        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, post_response).unwrap();

        // Now call commit (simulating what --commit does after write)
        commit(&doc).expect("commit should succeed");

        // Verify a new commit exists with the agent-doc message
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-3"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain agent-doc commit, got:\n{log_str}"
        );
    }
    #[test]
    fn commit_retries_full_transaction_when_stage_hits_index_lock() {
        use std::fs;
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n";
        fs::write(&doc, initial).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let updated =
            "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nFixed.\n\n";
        fs::write(&doc, updated).unwrap();
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::save(&doc, updated, agent_doc_ops_log_io::log_op).unwrap();

        let index_lock = root.join(".git/index.lock");
        fs::write(&index_lock, "held").unwrap();

        let remover = thread::spawn({
            let index_lock = index_lock.clone();
            move || {
                thread::sleep(Duration::from_millis(200));
                fs::remove_file(index_lock).unwrap();
            }
        });

        let did_commit = commit(&doc).expect("commit should retry until index.lock clears");
        remover.join().unwrap();

        assert!(
            did_commit,
            "commit should create a git commit after retrying"
        );
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-2"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain the retried agent-doc commit, got:\n{log_str}"
        );
    }
    #[test]
    fn commit_succeeds_when_no_lock_contention() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
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

        // No lock present — commit should succeed on first try
        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "commit without lock should succeed: {:?}",
            result.err()
        );
    }
    #[test]
    fn commit_staged_blob_has_no_head_markers() {
        // Regression for bug #dsng: (HEAD) is a working-tree-only marker and
        // must never appear in the committed blob. If it does, the next
        // cycle's reposition produces a phantom "strip (HEAD)" diff on
        // prior-cycle headings the user is editing.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        // Initial doc + snapshot, tracked cleanly (no HEAD markers yet).
        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, initial).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, initial).unwrap();
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

        // Simulate a write cycle: snapshot has a new response whose heading
        // still carries a transient `(HEAD)` marker.
        let cycle1 = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n\n### Re: newer (HEAD)\nnew body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, cycle1).unwrap();
        fs::write(&snap_abs, cycle1).unwrap();

        commit(&doc).expect("commit should succeed");

        // Assert the committed blob has ZERO `(HEAD)` occurrences.
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let blob = String::from_utf8_lossy(&show.stdout);
        assert!(
            !blob.contains("(HEAD)"),
            "committed blob must not contain (HEAD); got:\n{blob}"
        );
        assert!(
            blob.contains("### Re: newer\n"),
            "committed blob should contain the clean new heading; got:\n{blob}"
        );
        assert!(
            blob.contains("### Re: older\n"),
            "committed blob should still contain the older heading; got:\n{blob}"
        );

        // Post-commit cleanup now converges the working tree back to committed
        // HEAD when the only remaining drift is agent-owned transient churn.
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("### Re: newer\n"),
            "working tree should keep the clean newest heading after closeout; got:\n{working}"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            0,
            "working tree should not retain transient head markers after closeout; got:\n{working}"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("### Re: newer\n"),
            "snapshot should keep the clean heading; got:\n{snap}"
        );
        assert!(
            snap.matches("(HEAD)").count() == 0,
            "snapshot should not retain transient head markers; got:\n{snap}"
        );
    }
    #[test]
    fn reposition_collapses_snapshot_boundaries_even_during_active_run() {
        // Regression for #boundaryaccum1: a wedged finalize leaves a
        // pending-response file on disk, and the response lands via a direct
        // commit. The active-run guard must scope ONLY the working-tree rewrite
        // — the binary-owned snapshot collapse must still run, so the
        // staged/committed blob always carries exactly one boundary and a
        // boundary can no longer accrete per wedged cycle.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);

        let doc = root.join("session.md");
        // Snapshot carries THREE scattered stale boundaries, as a wedged
        // multi-cycle drain would leave them.
        let multi = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            ### Re: one\nbody one\n\
            <!-- agent:boundary:aaa111 -->\n\
            ### Re: two\nbody two\n\
            <!-- agent:boundary:bbb222 -->\n\
            User prompt.\n\
            <!-- agent:boundary:ccc333 -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, multi).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, multi).unwrap();

        // Simulate an ACTIVE RUN: a leftover pending-response file makes the
        // active-run guard fire (previously this early-returned, skipping the
        // snapshot collapse entirely).
        let canonical = doc.canonicalize().unwrap();
        let pending = agent_doc_fs::pending_response_path_for(&canonical).unwrap();
        if let Some(parent) = pending.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&pending, "in-flight").unwrap();

        reposition_boundary_in_snapshot(&doc);

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        let count = snap
            .matches(agent_doc_element_boundary::boundary::BOUNDARY_PREFIX)
            .count();
        assert_eq!(
            count, 1,
            "snapshot must collapse to exactly one boundary even during an active run; got {count}:\n{snap}"
        );
    }
    #[test]
    fn commit_skips_ignored_untracked_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        commit_file(
            root,
            ".gitignore",
            "scratch/\n.agent-doc/\n",
            "ignore scratch",
        );

        let doc = root.join("scratch/session.md");
        let content = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: ignored\nbody\n<!-- /agent:exchange -->\n";
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, content).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();

        let did_commit = commit(&doc).expect("ignored path should be skipped without panicking");
        assert!(
            !did_commit,
            "ignored untracked document must not create an agent-doc commit"
        );

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:scratch/session.md"])
            .output()
            .unwrap();
        assert!(
            !show.status.success(),
            "ignored untracked document must not be present in HEAD"
        );

        let listed = Command::new("git")
            .current_dir(root)
            .args(["ls-files", "--", "scratch/session.md"])
            .output()
            .unwrap();
        assert!(
            listed.stdout.is_empty(),
            "ignored untracked document must not be staged/tracked"
        );
    }
    #[test]
    fn commit_staged_blob_restores_answered_prompt_prefixes() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        fs::write(&doc, initial).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, initial).unwrap();
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

        let cycle = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: older\nold body\n\nPlease restart Codex and deploy the 503 fixes again.\n### Re: retry production deploy — gpt-5\nNo state change.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, cycle).unwrap();
        fs::write(&snap_abs, cycle).unwrap();

        commit(&doc).expect("commit should canonicalize answered prompt prefixes");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let blob = String::from_utf8_lossy(&show.stdout);
        assert!(
            blob.contains("❯ Please restart Codex and deploy the 503 fixes again.\n"),
            "committed blob should preserve the user prompt prefix:\n{blob}"
        );
        assert!(
            !blob.contains("\nPlease restart Codex and deploy the 503 fixes again.\n"),
            "committed blob must not keep the bare prompt line:\n{blob}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ Please restart Codex and deploy the 503 fixes again.\n"),
            "working tree should preserve the user prompt prefix after closeout:\n{working}"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ Please restart Codex and deploy the 503 fixes again.\n"),
            "snapshot should preserve the user prompt prefix after closeout:\n{snap}"
        );
    }
    #[test]
    fn commit_does_not_prefix_prior_response_tail_before_answered_prompt() {
        use std::fs;
        use std::process::Command;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();
        fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", ".gitignore"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let cycle = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n### Re: prior — gpt-5\n\nCommit / push:\n- `src/agent-doc`: `abc1234` pushed to `origin/main`\n\nI did not create a superproject gitlink commit because the workspace root already had unrelated dirty changes outside this fix.\n\nThere were no actionable follow-up items to capture.\ndo [#tailpatch]. spec-test-build-install-commit-push\n### Re: `#tailpatch` closeout-gap plan — gpt-5\n\nPlan refreshed.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, cycle).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, cycle).unwrap();

        commit(&doc).expect("commit should keep prior response tail unprefixed");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let blob = String::from_utf8_lossy(&show.stdout);
        assert!(
            blob.contains(
                "\nThere were no actionable follow-up items to capture.\n❯ do [#tailpatch]. spec-test-build-install-commit-push\n"
            ),
            "assistant tail must stay bare while the real prompt is prefixed:\n{blob}"
        );
        assert!(
            !blob.contains("\n❯ There were no actionable follow-up items to capture.\n"),
            "assistant tail must not be rewritten as a prompt:\n{blob}"
        );
    }
    #[test]
    fn commit_blocks_out_of_band_exchange_and_pending_mutation() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, snapshot).unwrap();
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

        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, file).unwrap();

        let err = commit(&doc).expect_err("typed pending mutations should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the blocked bypassed patchback:\n{message}"
        );
        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(snap, snapshot, "snapshot must remain unchanged on failure");
    }
    #[test]
    fn commit_does_not_absorb_out_of_band_user_prompt() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
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

        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, file).unwrap();

        commit(&doc).expect("commit should succeed even when there's nothing new to stage");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            !committed.contains("follow-up question"),
            "user prompt should remain uncommitted:\n{committed}"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("follow-up question"),
            "snapshot should stay at the older committed state:\n{snap}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ follow-up question"),
            "working tree should retain the user prompt:\n{working}"
        );
    }
    #[test]
    fn commit_blocks_extreme_drift_resync_for_tracked_user_prompt() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let scaffold = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, scaffold).unwrap();
        agent_doc_snapshot_io::save(&doc, scaffold, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add scaffold", "--no-verify"])
            .output()
            .unwrap();

        let live = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ user question that still needs an answer\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, live).unwrap();

        commit(&doc).expect("commit should succeed without absorbing the prompt");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            !committed.contains("user question that still needs an answer"),
            "tracked extreme drift must not absorb unanswered prompt:\n{committed}"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("user question that still needs an answer"),
            "snapshot should remain selective for tracked docs:\n{snap}"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ user question that still needs an answer"),
            "working tree should retain the unanswered prompt:\n{working}"
        );
    }
    #[test]
    fn commit_resyncs_extreme_drift_for_untracked_scaffold_doc() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let scaffold = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, scaffold).unwrap();
        agent_doc_snapshot_io::save(&doc, scaffold, agent_doc_ops_log_io::log_op).unwrap();

        let live = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            ## Status\n\n\
            <!-- agent:status patch=replace -->\n\
            Ready\n\
            <!-- /agent:status -->\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: imported\n\
           body from moved file\n\
            <!-- /agent:exchange -->\n\n\
            ## Pending / Not Built\n\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] imported\n\
            <!-- /agent:pending -->\n";
        fs::write(&doc, live).unwrap();

        commit(&doc).expect("commit should resync bootstrap scaffold snapshot");

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(
            committed.contains("### Re: imported\n"),
            "bootstrap resync should stage the real file content:\n{committed}"
        );
        assert!(
            committed.contains("[#a1b2] imported"),
            "bootstrap resync should carry pending content too:\n{committed}"
        );
    }
    #[test]
    fn commit_blocks_out_of_band_status_and_exchange_mutation() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, snapshot).unwrap();
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

        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, file).unwrap();

        let err = commit(&doc).expect_err("typed status mutations should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the blocked bypassed patchback:\n{message}"
        );
        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(snap, snapshot, "snapshot must remain unchanged on failure");
    }
    #[test]
    fn commit_repairs_committed_historical_snapshot_drift() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let tracked = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, tracked).unwrap();
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

        let historical = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            #### #next-steps\n\
            Follow up.\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, historical).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();

        agent_doc_snapshot_io::save(&doc, tracked, agent_doc_ops_log_io::log_op).unwrap();

        commit(&doc).expect("commit should repair the stale snapshot");

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("### Re: historical\n"),
            "snapshot should repair to the committed historical response:\n{snap}"
        );
        assert!(
            snap.contains("#### #next-steps\n"),
            "h4 response sub-headings that look like prompt presets should not block repair:\n{snap}"
        );

        let committed = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            committed.contains("### Re: historical\n"),
            "committed blob should keep the historical response after repair:\n{committed}"
        );
    }
    #[test]
    fn commit_closes_cycle_when_staged_snapshot_already_matches_head() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let visible_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        agent_doc_snapshot_io::save(&doc, visible_snapshot, agent_doc_ops_log_io::log_op).unwrap();

        let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
        fs::write(&doc, &with_user_edit).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(visible_snapshot),
            Some(&with_user_edit),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(visible_snapshot),
            Some(&with_user_edit),
            "sha256",
            None,
        )
        .unwrap();

        let did_commit = commit(&doc).expect("commit should treat HEAD-current snapshot as no-op");
        assert!(
            !did_commit,
            "HEAD-current closeout should not create a duplicate git commit"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let capture = agent_doc_capture_io::load_active(&doc).unwrap();
        assert!(
            capture.is_none(),
            "already-committed no-op closeout should clear active capture state"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_already_current file="),
            "ops log should record the dedicated no-op closeout:\n{log}"
        );
        assert!(
            !log.contains("commit_failed"),
            "already-committed no-op must not be logged as commit_failed:\n{log}"
        );
        assert!(
            log.contains("post_commit_local_drift file=")
                && log.contains("kind=working_tree_edits"),
            "out-of-component local edits should be classified as working-tree drift:\n{log}"
        );
    }

    #[test]
    fn commit_blocks_already_current_noop_when_live_editor_buffer_ahead_of_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous\n\n",
            "previous response\n",
            "<!-- /agent:exchange -->\n"
        );
        commit_file(root, "session.md", committed, "add doc");
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(committed),
            Some(committed),
            "sha256",
            None,
        )
        .unwrap();

        let editor_visible = format!("{committed}\noperator edit accepted by editor only\n");
        agent_doc_debounce::document_changed_with_content_for_editor(
            &doc.display().to_string(),
            &editor_visible,
            Some("jetbrains:test"),
        );

        let err = commit(&doc).expect_err(
            "HEAD-current closeout must fail closed while the live editor buffer is ahead of disk",
        );
        assert!(
            err.to_string().contains("live editor buffer"),
            "error should identify the unresolved editor buffer:\n{err}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::ResponseCaptured,
            "cycle must stay open until the live editor buffer reaches disk"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_live_buffer_ahead_of_disk file="),
            "blocked live-buffer closeout should be logged:\n{log}"
        );
        assert!(
            !log.contains("commit_already_current file="),
            "unflushed live editor buffer must not be recorded as an already-current closeout:\n{log}"
        );
    }

    #[test]
    fn live_buffer_materialized_insertions_allow_force_disk_snapshot_stage() {
        let staged = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: direct disk\n\n",
            "Handled.\n",
            "<!-- /agent:exchange -->\n",
            "<!--\n",
            "-->\n"
        );
        let editor = concat!(
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n",
            "<!--\n",
            "operator prompt typed in editor\n",
            "-->\n"
        );
        let file = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: direct disk\n\n",
            "Handled.\n",
            "<!-- /agent:exchange -->\n",
            "<!--\n",
            "operator prompt typed in editor\n",
            "-->\n"
        );
        let snapshot = agent_doc_debounce::LiveBufferSnapshot {
            path: "session.md".to_string(),
            len: editor.len(),
            hash: agent_doc_hash::content_hash(editor),
            timestamp_ms: 1,
            edit_epoch: 1,
            last_synced_epoch: 0,
            state_vector_b64: None,
            editor_id: Some("jetbrains:test".to_string()),
            editor_kind: Some("jetbrains".to_string()),
            editor_version: Some("test".to_string()),
            capabilities: vec![agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY.to_string()],
            content: Some(editor.to_string()),
            no_unsaved_operator_edits: false,
        };

        assert!(
            live_buffer_insertions_are_materialized_in_file(&snapshot, staged, file),
            "operator insertion is present in the visible file and can be left uncommitted"
        );
        assert!(
            !live_buffer_insertions_are_materialized_in_file(&snapshot, staged, staged),
            "operator insertion missing from disk must still block"
        );
    }

    #[test]
    fn commit_allows_already_current_synced_operator_buffer_while_disk_lags() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let stale_disk = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous\n\n",
            "previous response\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous\n\n",
            "previous response\n",
            "### Re: current\n\n",
            "current response\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        commit_file(root, "session.md", committed, "add committed response");
        fs::write(&doc, stale_disk).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
            &doc.display().to_string(),
            committed,
            "jetbrains:test",
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let did_commit =
            commit(&doc).expect("already-current synced editor-visible snapshot should close");
        assert!(
            !did_commit,
            "HEAD-current synced snapshot should close without a duplicate commit"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap();
        assert!(
            state.is_none()
                || state
                    .as_ref()
                    .is_some_and(|state| state.phase == agent_doc_turn::CyclePhase::Committed),
            "already-current closeout should not stay blocked: {state:?}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_live_buffer_ahead_of_disk_allowed file=")
                && log.contains("basis=already_current"),
            "already-current synced live-buffer allowance should be logged:\n{log}"
        );
        assert!(
            !log.contains("commit_blocked_live_buffer_ahead_of_disk file="),
            "synced HEAD-equivalent live buffer must not block no-op closeout:\n{log}"
        );
    }

    #[test]
    fn commit_allows_synced_operator_buffer_that_matches_staged_snapshot_while_disk_lags() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous\n\n",
            "previous response\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        commit_file(root, "session.md", committed, "add doc");
        fs::write(&doc, committed).unwrap();

        let staged = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous\n\n",
            "previous response\n",
            "### Re: current\n\n",
            "current response\n",
            "<!-- /agent:exchange -->\n"
        );
        agent_doc_snapshot_io::save(&doc, staged, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
            &doc.display().to_string(),
            staged,
            "jetbrains:test",
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();
        let listener = start_fake_listener(root);
        wait_for_listener(root);

        let did_commit = commit(&doc).expect("staged synced editor-visible snapshot should commit");
        assert!(did_commit, "snapshot ahead of HEAD should create a commit");

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&head),
            normalize_transient_agent_doc_markers(staged),
            "commit should stage the synced editor-visible snapshot modulo transient boundary markers, not stale disk"
        );
        assert!(
            head.contains("### Re: current") && head.contains("current response"),
            "committed HEAD should contain the synced editor-visible response, not stale disk:\n{head}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_live_buffer_ahead_of_disk_allowed file=")
                && log.contains("reason=staged_snapshot_matches_synced_operator_buffer"),
            "synced staged live-buffer allowance should be logged:\n{log}"
        );
        assert!(
            !log.contains("commit_blocked_live_buffer_ahead_of_disk file="),
            "staged synced live buffer must not be blocked as stale disk:\n{log}"
        );
        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(root));
        drop(listener);
    }

    #[test]
    fn commit_seam_strikes_answered_free_text_head_from_capture() {
        // `#qheadstrike` P2: the recovery commit seam must strike an answered
        // free-text queue head sourced from the durable capture — the gap that
        // let answered heads re-surface (`#rt83`/`#qflood` churn) after a
        // recovery-path closeout (`agent-doc commit` / `reset --from-current`).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let doc = root.join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue: start\n---\n\n",
            "<!-- agent:queue -->\n",
            "- fix the parser bug in the lexer\n",
            "- another unanswered task left alone\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: parser\n",
            "> **Queue prompt:** fix the parser bug in the lexer\n\n",
            "Fixed.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        // The `#qstrikeexplain` gate only strikes heads present in the pre-turn
        // baseline, so seed it.
        let baseline = agent_doc_fs::baseline_path_for(&doc).unwrap();
        fs::create_dir_all(baseline.parent().unwrap()).unwrap();
        fs::write(&baseline, content).unwrap();

        // Capture a response that answers the first free-text head (quoted in a
        // blockquote, as the strike matcher requires).
        let response =
            "### Re: parser\n> **Queue prompt:** fix the parser bug in the lexer\n\nFixed.\n";
        agent_doc_capture_io::capture_response(&doc, response).unwrap();

        strike_answered_free_text_heads_at_commit_seam(&doc);

        let after = fs::read_to_string(&doc).unwrap();
        assert!(
            after.contains("~~fix the parser bug in the lexer~~"),
            "answered free-text head must be struck at the commit seam:\n{after}"
        );
        assert!(
            after.contains("- another unanswered task left alone")
                && !after.contains("~~another unanswered task left alone~~"),
            "an unanswered free-text head must be preserved (not over-struck):\n{after}"
        );
        // The snapshot must converge on the struck state too, so the staged
        // commit captures it.
        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("~~fix the parser bug in the lexer~~"),
            "snapshot must also carry the strike:\n{snap}"
        );
        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("commit_seam_free_text_strike")
                && ops_log.contains("freetext_head_strike"),
            "commit seam strike should be observable in ops.log:\n{ops_log}"
        );
    }

    #[test]
    fn commit_blocks_head_current_noop_when_active_capture_response_missing() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please answer the prompt\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered answer.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_capture_io::capture_response(&doc, response).unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let err = commit(&doc)
            .expect_err("HEAD-current snapshot must not close a missing captured response");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            err.to_string()
                .contains("captured response body is not present"),
            "error should name the missing captured response body:\n{err}"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "blocked no-op closeout must not advance HEAD"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        let capture = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Captured
        );

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !head.contains("Recovered answer."),
            "HEAD should remain prompt-only when response materialization is missing:\n{head}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_missing_captured_response file="),
            "blocked missing materialization should be logged:\n{log}"
        );
        assert!(
            !log.contains("commit_already_current file="),
            "missing response materialization must not be recorded as already-current closeout:\n{log}"
        );
    }
    #[test]
    fn commit_blocks_stale_snapshot_commit_when_active_capture_response_missing() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please answer the prompt\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: stale sidecar — gpt-5\n\n",
            "Recovered answer that must not be lost.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_capture_io::capture_response(&doc, response).unwrap();

        let stale_prompt_only = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please answer the prompt\n",
            "<!-- agent:boundary:head -->\n",
            "❯ Later user follow-up while the response is missing\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, stale_prompt_only).unwrap();
        agent_doc_snapshot_io::save(&doc, stale_prompt_only, agent_doc_ops_log_io::log_op).unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let err = commit(&doc)
            .expect_err("stale prompt-only snapshot must not commit over captured response");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            err.to_string()
                .contains("captured response body is not present"),
            "error should name the missing captured response body:\n{err}"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "blocked stale snapshot commit must not advance HEAD"
        );
        assert!(
            !agent_doc_git_io::revision::show_head(&doc)
                .unwrap()
                .unwrap()
                .contains("Later user follow-up"),
            "stale prompt-only snapshot must not be committed"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_missing_captured_response file=")
                && log.contains("basis=staged"),
            "blocked staged commit should be logged with staged basis:\n{log}"
        );
    }
    #[test]
    fn commit_preserves_fresh_prompt_when_escaped_tail_cleanup_is_mixed() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n\n\
            do #oobtaildel. spec-test-build-install-commit-push\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let mixed = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ fresh follow-up prompt\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] keep me\n\
            <!-- /agent:backlog -->\n";
        fs::write(&doc, mixed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();

        let did_commit = commit(&doc).expect("mixed cleanup should close as no-op");
        assert!(
            !did_commit,
            "mixed cleanup plus prompt must not commit the fresh prompt"
        );

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            head, committed,
            "HEAD should remain unchanged when fresh prompt drift is present"
        );
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ fresh follow-up prompt"),
            "fresh prompt must remain visible for the next cycle:\n{working}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=") && log.contains("kind=user_follow_up"),
            "mixed cleanup should be diagnosed as preserved user follow-up drift:\n{log}"
        );
        assert!(
            log.contains("post_commit_user_follow_up file="),
            "mixed cleanup should use the benign user-follow-up marker:\n{log}"
        );
        assert!(
            log.contains("commit_noop file=") && log.contains("drift_kind=user_follow_up"),
            "mixed cleanup noop should record the benign drift kind for ops summary:\n{log}"
        );
        assert!(
            !log.contains("prior_patchback_without_response_body file="),
            "fresh follow-up prompts must not be mislabeled as missing response-body repair:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "classified follow-up prompt drift must not be mislabeled as out-of-band write:\n{log}"
        );
        assert!(
            !log.contains("post_commit_escaped_tail_cleanup file="),
            "mixed cleanup must not be auto-adopted:\n{log}"
        );
    }
    #[test]
    fn commit_repairs_prompt_prefix_duplicate_drift_before_staging() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n"
        );
        commit_file(root, "session.md", head, "add doc");

        let prompt = "lucas-huang may not have the necessary packages to use the runbooks. Please add development dependencies so any programmer can use the runbooks.";
        let snapshot = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\n",
                "Done.\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let working = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\n",
                "Done.\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        fs::write(&doc, &working).unwrap();

        let did_commit = commit(&doc).expect("prompt duplicate drift should repair and commit");
        assert!(did_commit);

        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_after.contains(&format!("❯ {prompt}\n#spec-test-commit-push")),
            "committed prompt should keep one normalized line:\n{head_after}"
        );
        assert!(
            !head_after.contains(&format!("❯ {prompt}\n{prompt}")),
            "duplicate prompt must not be committed:\n{head_after}"
        );
        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            !working_after.contains(&format!("❯ {prompt}\n{prompt}")),
            "working tree must be repaired before closeout:\n{working_after}"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after.contains(&format!("❯ {prompt}\n{prompt}")),
            "snapshot must be repaired before closeout:\n{snapshot_after}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_pre_stage_prompt_duplicate_repaired file=")
                && log.contains("snapshot_updated=true"),
            "commit pre-stage prompt repair should be logged:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "repaired prefix duplicate drift must not be left as out-of-band drift:\n{log}"
        );
    }
    #[test]
    fn commit_repairs_committed_head_before_user_follow_up_noop() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let stale_snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:old -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, stale_snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, stale_snapshot, agent_doc_ops_log_io::log_op).unwrap();
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

        let committed_head = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed_head).unwrap();
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

        agent_doc_snapshot_io::save(&doc, stale_snapshot, agent_doc_ops_log_io::log_op).unwrap();

        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, working).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(stale_snapshot), Some(working))
            .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(stale_snapshot),
            Some(working),
            "sha256",
            None,
        )
        .unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let did_commit = commit(&doc).expect("commit should not rewind a stale snapshot");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            !did_commit,
            "repairing the snapshot up to committed HEAD should close as a no-op"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "HEAD should stay on the already-committed response instead of creating a rewind commit"
        );

        let committed = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            committed.contains("### Re: newer\n"),
            "HEAD should keep the newer committed response:\n{committed}"
        );
        assert!(
            !committed.contains("❯ follow-up question"),
            "HEAD should not absorb the user's follow-up prompt:\n{committed}"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("### Re: newer\n"),
            "snapshot should repair up to the already-committed response:\n{snap}"
        );
        assert!(
            !snap.contains("❯ follow-up question"),
            "snapshot repair must stop at HEAD, not absorb the follow-up prompt:\n{snap}"
        );

        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            working_after.contains("❯ follow-up question"),
            "working tree should keep the user's follow-up prompt uncommitted:\n{working_after}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=") && log.contains("kind=user_follow_up"),
            "follow-up noop closeout should classify post-commit local drift:\n{log}"
        );
        assert!(
            log.contains("post_commit_user_follow_up file="),
            "follow-up noop closeout should record the benign follow-up diagnostic:\n{log}"
        );
        assert!(
            log.contains("commit_noop file=") && log.contains("drift_kind=user_follow_up"),
            "follow-up noop closeout should record the benign drift kind for ops summary:\n{log}"
        );
        assert!(
            !log.contains("prior_patchback_without_response_body file="),
            "follow-up noop closeout must not reopen missed-response repair semantics:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "classified follow-up prompt drift must not be mislabeled as out-of-band write:\n{log}"
        );
    }
    #[test]
    fn commit_skips_terminal_user_follow_up_noop_closeout() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        commit_file(root, "README.md", "# test\n", "initial");

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: previous\n\
            previous body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let committed_state = agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        let with_user_follow_up = format!(
            "{}❯ follow-up question\n",
            committed.replace("<!-- /agent:exchange -->\n", "")
        ) + "<!-- /agent:exchange -->\n";
        fs::write(&doc, &with_user_follow_up).unwrap();

        let did_commit =
            commit(&doc).expect("terminal user-follow-up drift should remain a prompt handoff");
        assert!(!did_commit, "no new commit should be created");

        let state_after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            state_after, committed_state,
            "terminal user follow-up drift must not rewrite committed cycle state"
        );

        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            working_after.contains("❯ follow-up question"),
            "working tree should preserve the user's follow-up prompt:\n{working_after}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_user_follow_up file="),
            "prompt handoff should still be diagnosed:\n{log}"
        );
        assert!(
            log.contains("commit_prompt_handoff_noop file="),
            "prompt handoff should have a non-closeout noop marker:\n{log}"
        );
        assert!(
            !log.contains("commit_noop file=") && !log.contains("commit_already_current file="),
            "terminal prompt handoff must not emit closeout lifecycle noop markers:\n{log}"
        );
    }
    #[test]
    fn postcommit_worktree_check_logs_match_true_for_transient_only_drift() {
        // `#postcommit-ipc-worktree-corruption`: a clean closeout whose working
        // tree differs from HEAD only by the legitimate transient `(HEAD)` /
        // boundary markers must log match=true — the visible document is
        // structurally equal to HEAD, so this is NOT the corruption class.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic\n\
            response body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // Working tree keeps the transient `(HEAD)` annotation + repositioned
        // boundary the user sees post-commit — stripped by the replay normalizer.
        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic (HEAD)\n\
            response body\n\
            <!-- agent:boundary:abc123 -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, working).unwrap();

        emit_postcommit_worktree_check(&doc);

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("postcommit_worktree_check file=") && log.contains("match=true"),
            "transient-only working-tree drift must log a worktree==HEAD proof (match=true):\n{log}"
        );
        assert!(
            !log.contains("match=false"),
            "transient-only drift must not be flagged as corruption:\n{log}"
        );
    }
    #[test]
    fn postcommit_worktree_preserves_carry_forward_superset() {
        // A concurrent user edit carried forward UNCOMMITTED makes the working tree a
        // superset of HEAD (every committed line present, plus a new line). HEAD
        // content is NOT lost, so #pcwc must preserve the tree, never clobber the
        // carried-forward edit.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic\n\
            response body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // Superset: all of HEAD plus a new uncommitted user note after the boundary.
        let superset = format!("{head_doc}\na new uncommitted user note line\n");
        fs::write(&doc, &superset).unwrap();

        emit_postcommit_worktree_check(&doc);

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("match=false"),
            "superset differs from HEAD:\n{log}"
        );
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "a carry-forward superset must NOT be auto-reconciled:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            superset,
            "the carried-forward user edit must be preserved untouched"
        );
    }
    #[test]
    fn postcommit_worktree_match_does_not_flush_editor() {
        // When the working tree already equals HEAD there is no drift to clear, so
        // the post-commit check must NOT send a save_document (avoid persisting a
        // possibly-stale editor buffer over an already-correct disk).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);
        let _listener = start_fake_listener(root);
        wait_for_listener(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic\n\
            response body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");
        // Working tree already equals HEAD (no edit).

        emit_postcommit_worktree_check(&doc);

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("postcommit_editor_save_flushed"),
            "a clean match=true working tree must not flush the editor:\n{log}"
        );

        let _ = fs::remove_file(agent_doc_ipc_io::socket_path(root));
    }
    #[test]
    fn postcommit_worktree_preserves_when_content_lost_but_user_work_added() {
        // The tree dropped a committed line BUT also added a carry-forward signal (a
        // `#tag` directive = real next-cycle user work). The ambiguous case fails
        // safe toward PRESERVING the user edit rather than clobbering it to HEAD.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\n\
            ### Re: second\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // `### Re: second` dropped (content loss) AND a new `#tag` directive added.
        let drifted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n\
            follow up on #newtask\n";
        fs::write(&doc, drifted).unwrap();

        emit_postcommit_worktree_check(&doc);

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "content loss WITH new user work must not be auto-reconciled:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            drifted,
            "ambiguous drift with new user work must be preserved"
        );
    }

    #[test]
    fn postcommit_worktree_observe_only_never_reverts_lost_content() {
        // #realtimecutover: the legacy revert tower used to write HEAD back over a
        // working tree that LOST committed content (pure corruption, no new user
        // work). That revert is RIPPED OUT — the realtime replica owns disk
        // reconciliation — so the post-commit check now only LOGS match=false and
        // leaves the working tree exactly as-is (it must never clobber it).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\n\
            ### Re: second\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        // Pure corruption: `### Re: second` dropped, NOTHING added (the exact shape
        // the old code auto-reconciled back to HEAD).
        let corrupted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:boundary:abc123 -->\n";
        fs::write(&doc, corrupted).unwrap();

        emit_postcommit_worktree_check(&doc);

        // Observe-only: drift is logged, but NOTHING is reverted.
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("postcommit_worktree_check") && log.contains("match=false"),
            "drift must be logged for observability:\n{log}"
        );
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "the legacy revert must be gone — no auto-reconcile:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            corrupted,
            "the working tree must be left untouched (realtime replica owns reconciliation)"
        );
    }

    #[test]
    fn postcommit_worktree_preserves_when_content_lost_but_queue_work_added() {
        // A markdown queue mirror is real next-cycle work too. If the editor
        // buffer lost committed content and gained a pinned queue item, the
        // post-commit check must preserve the buffer instead of reconciling it
        // back to HEAD and deleting the queue addition.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\n\
            ### Re: second\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:queue priority go -->\n\
            - do [#existing]\n\
            <!-- /agent:queue -->\n\
            <!-- agent:backlog priority queue -->\n\
            - [ ] [#existing] existing task\n\
            <!-- /agent:backlog -->\n\
            <!-- agent:boundary:abc123 -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: commit response");
        let doc = root.join("session.md");

        let drifted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: first\n\
            first body\n\
            second body\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:queue priority go -->\n\
            - do [#existing]\n\
            - :pushpin: do [#advance-review]\n\
            <!-- /agent:queue -->\n\
            <!-- agent:backlog priority queue -->\n\
            - [ ] [#existing] existing task\n\
            <!-- /agent:backlog -->\n\
            <!-- agent:boundary:abc123 -->\n";
        fs::write(&doc, drifted).unwrap();

        emit_postcommit_worktree_check(&doc);

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !log.contains("postcommit_worktree_auto_reconciled"),
            "content loss WITH pinned queue work must not be auto-reconciled:\n{log}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            drifted,
            "ambiguous drift with pinned queue work must be preserved"
        );
    }

    #[test]
    fn commit_already_current_commits_preserved_queue_additions_neutralized_by_replay() {
        // #editorbufwin P2: queue-only drift is neutralized by replay hashing, so
        // an already-current snapshot used to close as a no-op and leave the
        // operator's queue addition local. The preserved queue prompt must become
        // durable in a follow-up commit while staying live for the next drain.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        init_repo(root);

        let head_doc = "---\nagent_doc_session: test\nqueue_active: true\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prior\n\
            response body\n\
            <!-- agent:boundary:abc123 -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:queue priority go -->\n\
            - do [#existing]\n\
            <!-- /agent:queue -->\n";
        commit_file(root, "session.md", head_doc, "agent-doc: prior response");
        let doc = root.join("session.md");
        agent_doc_snapshot_io::save(&doc, head_doc, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(head_doc), Some(head_doc)).unwrap();
        agent_doc_cycle_state_io::record_ipc_snapshot_adoption_blocked(&doc)
            .unwrap()
            .expect("cycle state should be present");

        let visible_with_queue_add = head_doc.replace(
            "- do [#existing]\n",
            "- do [#existing]\n- :pushpin: do [#advance-review]\n",
        );
        fs::write(&doc, &visible_with_queue_add).unwrap();

        let did_commit = commit(&doc).expect("queue-only drift should commit");
        assert!(
            did_commit,
            "queue-only preserved editor drift must create a follow-up commit"
        );

        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_after.contains("- :pushpin: do [#advance-review]\n"),
            "HEAD must include the preserved queue addition:\n{head_after}"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snapshot_after.contains("- :pushpin: do [#advance-review]\n"),
            "snapshot must make the queue addition durable for session-check:\n{snapshot_after}"
        );
        let working_after = fs::read_to_string(&doc).unwrap();
        assert!(
            working_after.contains("- :pushpin: do [#advance-review]\n"),
            "visible document must keep the queue addition live:\n{working_after}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=preserved_queue_addition_replay_neutralized"),
            "commit should log the replay-neutralized queue recovery:\n{log}"
        );
        assert!(
            !log.contains("commit_already_current file="),
            "the queue addition must not be closed as an already-current no-op:\n{log}"
        );
    }

    #[test]
    fn commit_already_current_repairs_transient_working_tree_churn() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: newer\n\
            body\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let transient = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: newer (HEAD)\n\
            body\n\
            <!-- agent:boundary:fresh-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, transient).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        let stale_crdt = agent_doc_merge::crdt::CrdtDoc::from_text(transient).encode_state();
        agent_doc_snapshot_io::save_crdt(&doc, &stale_crdt).unwrap();

        let did_commit = commit(&doc).expect("HEAD-current closeout should succeed");
        assert!(
            !did_commit,
            "transient-only churn should close as already committed"
        );

        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, committed,
            "working tree should be restored to clean HEAD when only transient churn differed"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, committed,
            "snapshot should also be restored to clean HEAD after transient cleanup"
        );

        let crdt = agent_doc_snapshot_io::load_crdt(&doc)
            .unwrap()
            .expect("CRDT state should be preserved for CRDT docs");
        let crdt_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&crdt)
            .unwrap()
            .to_text();
        assert_eq!(
            crdt_text, committed,
            "CRDT state should be refreshed to the same clean HEAD content after no-op cleanup"
        );

        assert!(
            root.join(".agent-doc/patches/vcs-refresh.signal").exists(),
            "no-op closeout cleanup should still signal the editor/VCS refresh path"
        );
    }
    #[test]
    fn commit_success_repairs_transient_working_tree_churn_after_real_commit() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let initial = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ Initial prompt\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, initial).unwrap();
        agent_doc_snapshot_io::save(&doc, initial, agent_doc_ops_log_io::log_op).unwrap();
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

        let _listener = start_fake_listener(root);
        wait_for_listener(root);

        let committed = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ Initial prompt\n\
            ### Re: closeout follow-up — gpt-5\n\
            body\n\
            <!-- agent:boundary:committed-boundary -->\n\
            <!-- /agent:exchange -->\n";
        let transient = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ Initial prompt\n\
            ### Re: closeout follow-up — gpt-5 (HEAD)\n\
            body\n\
            <!-- agent:boundary:fresh-boundary -->\n\
            <!-- /agent:exchange -->\n";
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        fs::write(&doc, transient).unwrap();
        let stale_crdt = agent_doc_merge::crdt::CrdtDoc::from_text(transient).encode_state();
        agent_doc_snapshot_io::save_crdt(&doc, &stale_crdt).unwrap();

        let did_commit = commit(&doc).expect("real closeout commit should succeed");
        assert!(did_commit, "snapshot should produce a real git commit");

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .expect("committed document should be readable from HEAD after commit");
        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, head,
            "post-commit cleanup should restore the working tree to the committed HEAD blob"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, head,
            "snapshot should stay aligned with the committed HEAD blob"
        );

        let crdt = agent_doc_snapshot_io::load_crdt(&doc)
            .unwrap()
            .expect("CRDT state should be preserved for CRDT docs");
        let crdt_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&crdt)
            .unwrap()
            .to_text();
        assert_eq!(
            crdt_text, head,
            "CRDT state should refresh to the committed HEAD blob after post-commit repair"
        );

        let status = agent_doc_git_io::status::tracked_modified_paths(&doc).unwrap();
        assert!(
            status.is_empty(),
            "post-commit cleanup should leave no tracked worktree dirtiness for the document: {status:?}"
        );
    }
    #[test]
    fn commit_fails_closed_when_committed_historical_response_mutates_status() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let stale_snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, stale_snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, stale_snapshot, agent_doc_ops_log_io::log_op).unwrap();
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

        let head = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual repair", "--no-verify"])
            .output()
            .unwrap();

        let working = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After. Tuned manually.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Completed.\n\n",
            "do #done. spec-test-commit-push\n",
            "### Re: do `#done` — codex\n\n",
            "Done.\n",
            "<!-- agent:boundary:live -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, working).unwrap();
        agent_doc_snapshot_io::save(&doc, stale_snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(stale_snapshot), Some(working))
            .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(stale_snapshot),
            Some(working),
            "sha256",
            None,
        )
        .unwrap();

        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let err =
            commit(&doc).expect_err("status-mutating historical patchback should fail closed");
        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();

        assert!(
            err.to_string()
                .contains("committed historical response patchback"),
            "error should explain the blocked historical patchback:\n{err}"
        );
        assert_eq!(
            String::from_utf8_lossy(&head_before.stdout),
            String::from_utf8_lossy(&head_after.stdout),
            "HEAD should stay on the already-committed response instead of creating a rewind commit"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, stale_snapshot,
            "snapshot must stay on the pre-repair baseline when the historical patchback is rejected"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        assert_eq!(state.last_event, "response_captured");

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_committed_historical_patchback file="),
            "blocked historical patchback should be recorded in ops.log:\n{log}"
        );
        assert!(
            !log.contains("snapshot_repair file="),
            "rejected historical patchback must not rewrite the snapshot:\n{log}"
        );
    }
    #[test]
    fn commit_already_current_repairs_response_heading_attribution_drift() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic — gpt-5\n\
            body\n\
            <!-- agent:boundary:committed-id -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let drifted = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: topic — codex (HEAD)\n\
            body\n\
            <!-- agent:boundary:stale-id -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, drifted).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();

        let did_commit = commit(&doc).expect("heading attribution drift should self-heal");
        assert!(!did_commit, "repair should close as already committed");

        let working = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working, committed,
            "working tree should be restored to the committed response heading and boundary"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, committed,
            "snapshot should also return to committed HEAD"
        );
    }
    #[test]
    fn commit_already_current_repairs_stale_agent_response_collapse_and_commits_queue_follow_up() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: #vbc1 next backlog — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#jblive160]\n\n",
            "Backlog complete.\n\n",
            "Proof:\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n\n",
            "### Re: #queueeditloss — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#queueeditloss]\n\n",
            "Implemented queue fix.\n\n",
            "Proof:\n",
            "- Changed paths: `write.rs`.\n",
            "- Verification: `make check`.\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n",
            "<!-- agent:boundary:head-boundary -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let drifted = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: #vbc1 next backlog — gpt-5\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#queueeditloss]\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#jblive160]\n\n",
            "Backlog complete.\n\n",
            "Proof:\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n",
            "Implemented queue fix.\n\n",
            "Proof:\n",
            "- Verification: `make check`.\n",
            "- Changed paths: `write.rs`.\n",
            "- Confidence: high.\n",
            "- Escalation: none.\n",
            "<!-- agent:boundary:live-boundary -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n",
            "<!-- /agent:queue -->\n"
        );
        fs::write(&doc, drifted).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        let stale_crdt = agent_doc_merge::crdt::CrdtDoc::from_text(drifted).encode_state();
        agent_doc_snapshot_io::save_crdt(&doc, &stale_crdt).unwrap();

        let did_commit = commit(&doc).expect("stale response collapse should self-heal");
        assert!(
            did_commit,
            "repair should commit the preserved queue follow-up after cleaning the exchange"
        );

        let head_after = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_after.contains("### Re: #vbc1 next backlog"),
            "committed exchange response must be restored:\n{head_after}"
        );
        assert!(
            head_after.contains("### Re: #queueeditloss"),
            "second committed exchange response must be restored:\n{head_after}"
        );
        assert!(
            !head_after.contains("<!-- agent:boundary:live-boundary -->"),
            "stale live boundary must not survive in HEAD:\n{head_after}"
        );
        assert!(
            head_after.contains(
                "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n"
            ),
            "preserved queue follow-up must be durable in HEAD:\n{head_after}"
        );
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains(
                "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n"
            ),
            "queue follow-up must remain visible:\n{working}"
        );

        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains(
                "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n"
            ),
            "snapshot must include the committed queue follow-up:\n{snap}"
        );

        let crdt = agent_doc_snapshot_io::load_crdt(&doc)
            .unwrap()
            .expect("CRDT state should be refreshed for the repaired visible document");
        let crdt_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&crdt)
            .unwrap()
            .to_text();
        assert!(
            crdt_text.contains(
                "- do [#submitdiag] Add diagnostics for JB Run Agent Doc submit misses?\n"
            ),
            "CRDT state should include the preserved queue follow-up:\n{crdt_text}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("stale_agent_response_collapse_cleanup file=")
                && log.contains("preserved_local_drift=true"),
            "repair should leave durable evidence that only the exchange collapse was cleaned:\n{log}"
        );
        assert!(
            log.contains("reason=preserved_queue_addition_replay_neutralized"),
            "queue follow-up commit should log the replay-neutralized recovery:\n{log}"
        );
    }
    #[test]
    fn commit_identifies_post_commit_local_working_tree_edits() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let working = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response plus later local edit\n\
            <!-- agent:boundary:live-boundary -->\n\
            <!-- /agent:exchange -->\n\n\
            <!-- later local note -->\n";
        fs::write(&doc, working).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();

        let did_commit = commit(&doc).expect("HEAD-current local edits should close as no-op");
        assert!(
            !did_commit,
            "later local edits on top of HEAD must stay uncommitted"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let working_after = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            working_after, working,
            "commit should not overwrite later local edits when HEAD is already current"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("post_commit_local_drift file=")
                && log.contains("kind=working_tree_edits"),
            "working-tree edits should be classified as post-commit local drift:\n{log}"
        );
        assert!(
            log.contains("commit_noop file=") && log.contains("drift_kind=working_tree_edits"),
            "working-tree noop should record its anomalous drift kind for ops summary:\n{log}"
        );
        assert!(
            !log.contains("out_of_band_write file="),
            "classified post-commit local drift should not be mislabeled as out-of-band write:\n{log}"
        );
        assert!(
            !log.contains("drift_warning file="),
            "post-commit local drift should not be mislabeled as a generic out-of-band write:\n{log}"
        );
    }
    #[test]
    fn commit_fails_closed_when_reaped_backlog_ids_reappear_before_closeout() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let cleaned = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, cleaned).unwrap();
        agent_doc_snapshot_io::save(&doc, cleaned, agent_doc_ops_log_io::log_op).unwrap();
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

        agent_doc_cycle_state_io::start_preflight(&doc, Some(cleaned), Some(cleaned)).unwrap();
        agent_doc_cycle_state_io::record_reaped_pending_ids(&doc, &["gone1".to_string()])
            .unwrap()
            .unwrap();

        let resurrected = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [/] [#gone1] Resurrected by stale editor state\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, resurrected).unwrap();

        let err = commit(&doc).expect_err("reintroduced reaped ids must fail closed");
        let message = err.to_string();
        assert!(message.contains("#gone1"), "unexpected error: {message}");
        assert!(
            message.contains("reappeared in the live file"),
            "unexpected error: {message}"
        );

        let head = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(head.status.success(), "git show HEAD:session.md failed");
        let committed = String::from_utf8_lossy(&head.stdout);
        assert!(
            !committed.contains("[#gone1]"),
            "HEAD must stay at the cleaned backlog state:\n{committed}"
        );
    }
    #[test]
    fn commit_blocks_bypassed_response_patchback_on_head_current() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
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

        let doc = root.join("session.md");
        let committed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
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

        let bypassed = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: state\n\
            clean committed response\n\
            \n\
            do #later. spec-test-build-install-commit-push\n\
            \n\
            ### Re: bypassed\n\
            landed outside agent-doc\n\
            <!-- agent:boundary:live-boundary -->\n\
            <!-- /agent:exchange -->\n";
        fs::write(&doc, bypassed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(committed), Some(bypassed)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(committed),
            Some(bypassed),
            "sha256",
            None,
        )
        .unwrap();

        let err = commit(&doc).expect_err("bypassed response patchback should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("direct response patchback without agent-doc cycle"),
            "error should explain the bypassed patchback:\n{message}"
        );
        assert!(
            message.contains("### Re: bypassed"),
            "error should surface the offending heading:\n{message}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        assert_eq!(state.last_event, "response_captured");

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !head_doc.contains("### Re: bypassed"),
            "HEAD must stay on the last binary-owned patchback:\n{head_doc}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_bypassed_patchback file="),
            "ops log should record the blocked bypassed patchback:\n{log}"
        );
    }
    #[test]
    fn commit_blocks_committed_historical_patchback_that_mutates_status() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Before.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: state\n",
            "clean committed response\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "After.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: state\n",
            "clean committed response\n\n",
            "do #patchbypass. spec-test-build-install-commit-push\n",
            "### Re: #patchbypass — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
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

        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(snapshot), Some(committed)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(snapshot),
            Some(committed),
        )
        .unwrap();

        let err =
            commit(&doc).expect_err("status-mutating historical patchback should fail closed");
        let message = err.to_string();
        assert!(
            message.contains("committed historical response patchback"),
            "error should explain the committed historical patchback:\n{message}"
        );
        assert!(
            message.contains("typed_component_drift")
                || message.contains("status+exchange")
                || message.contains("status"),
            "error should surface the out-of-band mutation kind:\n{message}"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_committed_historical_patchback file="),
            "ops log should record the blocked historical patchback:\n{log}"
        );
    }
    // #compactdrift — a clean exchange-only compaction (responses archived, every
    // NON-exchange component preserved) must NOT trip the committed-historical
    // `typed_component_drift` / out-of-band patchback guard. HEAD legitimately still
    // holds the last finalized `### Re:` response(s); the post-compact snapshot/file
    // archived them. With no non-exchange drift this is the benign steady state, so
    // `commit` must adopt the compacted document instead of failing closed with
    // "refusing to auto-adopt committed historical response patchback".
    #[test]
    fn commit_allows_clean_exchange_only_compaction() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        // HEAD: pre-compact committed state with a finalized response carrying the
        // exact `### Re: do [#rtwbcast]` marker the live repro reported, plus a stable
        // status + backlog (non-exchange components).
        let pre_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Earlier work.\n\n",
            "do #rtwbcast. spec-test-build-install-commit-push\n",
            "### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8\n\n",
            "Implemented the broadcast rung.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#follow] keep an eye on convergence\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, pre_compact).unwrap();
        agent_doc_snapshot_io::save(&doc, pre_compact, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "finalized rtwbcast", "--no-verify"])
            .output()
            .unwrap();

        // Post-compact document: exchange archived to a Session Summary, status +
        // backlog preserved exactly. Snapshot + working tree both hold this (the
        // normal post-archival state after compact refreshes the snapshot).
        let post_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived 2 response topic(s) to .agent-doc/archives/session-20260613.md\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#follow] keep an eye on convergence\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&doc, post_compact).unwrap();
        agent_doc_snapshot_io::save(&doc, post_compact, agent_doc_ops_log_io::log_op).unwrap();

        commit(&doc).expect("clean exchange-only compaction must not fail closed");

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_doc.contains("### Session Summary"),
            "HEAD should hold the compacted document after commit:\n{head_doc}"
        );
        assert!(
            !head_doc.contains("### Re: do [#rtwbcast]"),
            "the archived response must not remain in HEAD after compaction:\n{head_doc}"
        );
    }

    #[test]
    fn repair_historical_snapshot_drift_keeps_compacted_visible_file() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let stale_snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "stable status.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older - gpt-5\n\n",
            "Earlier work.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, stale_snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, stale_snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial session", "--no-verify"])
            .output()
            .unwrap();

        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let head = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
## Status\n\n\
<!-- agent:status patch=replace -->\n\
stable status.\n\
<!-- /agent:status -->\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n{old_blocks}<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue -->\n\
<!-- /agent:queue -->\n"
        );
        fs::write(&doc, &head).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "long exchange head", "--no-verify"])
            .output()
            .unwrap();

        let compacted = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
## Status\n\n\
<!-- agent:status patch=replace -->\n\
stable status.\n\
<!-- /agent:status -->\n\n\
## Exchange\n\n\
<!-- agent:exchange patch=append -->\n\
### Session Summary\n\n\
*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\n\
Compacted content:\n\
- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n\
- Prior summary/context: compacted prior responses\n\
<!-- /agent:exchange -->\n\n\
## Queue\n\n\
<!-- agent:queue -->\n\
<!-- /agent:queue -->\n";
        fs::write(&doc, compacted).unwrap();
        agent_doc_snapshot_io::save(&doc, stale_snapshot, agent_doc_ops_log_io::log_op).unwrap();
        let scope =
            agent_doc_turn::turn_scope::TurnScope::for_driver_with_exchange_tail(None, Some(0));
        agent_doc_turn_scope_io::save(&doc, &scope).unwrap();

        let repaired = repair_committed_historical_snapshot_drift(&doc)
            .expect("historical repair should not restore pre-compact HEAD");

        assert_eq!(repaired, Some("exchange"));
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap(),
            Some(compacted.to_string()),
            "snapshot repair must preserve the visible compacted document"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("snapshot_repair file=")
                && log.contains("basis=visible_rebase_guard")
                && log.contains("stale_snapshot_visible_rebased")
                && !log.contains("basis=head_local_drift"),
            "repair should be auditable as visible-rebase, not head-local rewind:\n{log}"
        );
    }

    #[test]
    fn commit_allows_clean_exchange_only_compaction_with_head_marker_worktree() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let pre_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "stable status.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older - gpt-5\n\n",
            "Earlier work.\n\n",
            "do #compactdrift. spec-test-build-install-commit-push\n",
            "### Re: #compactdrift-agent - gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#compactdrift-agent]\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        fs::write(&doc, pre_compact).unwrap();
        agent_doc_snapshot_io::save(&doc, pre_compact, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "finalized compactdrift", "--no-verify"])
            .output()
            .unwrap();

        let post_compact_snapshot = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "stable status.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived compactdrift responses.\n\n",
            "### Re: #compactdrift-agent - gpt-5\n\n",
            "Verified compact drift.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        let post_compact_worktree = post_compact_snapshot.replace(
            "### Re: #compactdrift-agent - gpt-5",
            "### Re: #compactdrift-agent - gpt-5 (HEAD)",
        );
        fs::write(&doc, &post_compact_worktree).unwrap();
        agent_doc_snapshot_io::save(&doc, post_compact_snapshot, agent_doc_ops_log_io::log_op)
            .unwrap();

        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "transient (HEAD) marker drift must not trip the committed-historical guard: {:?}",
            result.err().map(|e| e.to_string())
        );

        let head_doc = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head_doc.contains("### Session Summary"),
            "HEAD should hold the compacted document after commit:\n{head_doc}"
        );
        assert!(
            !head_doc.contains("do #compactdrift. spec-test-build-install-commit-push"),
            "the archived historical response prompt must not remain in HEAD:\n{head_doc}"
        );
    }
    // #compactdrift — the recovery shape: compact archived the exchange and refreshed
    // the working tree, but the snapshot was left STALE at the pre-compact size (the
    // reported "snapshot stale at pre-compact size vs the compacted visible file").
    // With no concurrent wedged write and no non-exchange drift, `agent-doc commit`
    // recovery must adopt the compacted file rather than fail closed on the historical
    // `### Re:` marker.
    #[test]
    fn commit_recovers_stale_pre_compact_snapshot_without_wedge() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let pre_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Earlier work.\n\n",
            "do #rtwbcast. spec-test-build-install-commit-push\n",
            "### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8\n\n",
            "Implemented the broadcast rung.\n",
            "<!-- /agent:exchange -->\n",
        );
        // HEAD is still the pre-compact committed state (compact's own commit failed).
        fs::write(&doc, pre_compact).unwrap();
        agent_doc_snapshot_io::save(&doc, pre_compact, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "finalized rtwbcast", "--no-verify"])
            .output()
            .unwrap();

        // Working tree = compacted; snapshot left STALE at the pre-compact bytes.
        let post_compact = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Archived 2 response topic(s) to .agent-doc/archives/session-20260613.md\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, post_compact).unwrap();
        // snapshot intentionally NOT refreshed — still pre_compact.

        let result = commit(&doc);
        assert!(
            result.is_ok(),
            "stale-pre-compact-snapshot recovery must not fail closed: {:?}",
            result.err().map(|e| e.to_string())
        );
    }
    #[test]
    fn commit_in_submodule_with_symlinked_absolute_path() {
        use std::fs;
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        let link_dir = tempfile::TempDir::new().unwrap();
        let link_path = link_dir.path().join("workspace");

        // Create symlink: workspace -> outer
        std::os::unix::fs::symlink(outer, &link_path).unwrap();

        // Initialize a "submodule" origin repo
        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        // Initialize the outer repo (via real path, as git would)
        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        // Add submodule
        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let submodule_path = outer.join("src/sub");
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        // Create and track the document inside the submodule
        let doc_real = submodule_path.join("session.md");
        let content =
            "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## User\n\n";
        fs::write(&doc_real, content).unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&submodule_path)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        // Modify the file and create snapshot
        let new_content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nresponse\n\n## Assistant\n\nupdated\n\n## User\n\n";
        fs::write(&doc_real, new_content).unwrap();
        let project_root =
            agent_doc_project_root_io::project_root_containing(&doc_real.canonicalize().unwrap())
                .unwrap_or_else(|| outer.to_path_buf());
        let snap_rel = agent_doc_fs::snapshot_path_for(&doc_real).unwrap();
        let snap_abs = project_root.join(&snap_rel);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, new_content).unwrap();

        // Access the file via the SYMLINK path — this is the bug scenario
        let doc_via_symlink = link_path.join("src/sub/session.md");
        assert!(doc_via_symlink.exists(), "symlinked path should exist");

        // commit() should succeed even with the symlinked absolute path
        let result = commit(&doc_via_symlink);
        assert!(
            result.is_ok(),
            "commit should succeed for submodule file accessed via symlink: {:?}",
            result.err()
        );

        // Verify the submodule has the agent-doc commit
        let sub_log = Command::new("git")
            .current_dir(&submodule_path)
            .args(["log", "--oneline", "-5"])
            .output()
            .unwrap();
        let sub_log_str = String::from_utf8_lossy(&sub_log.stdout);
        assert!(
            sub_log_str.contains("agent-doc(session)"),
            "submodule git log should contain agent-doc commit, got:\n{sub_log_str}"
        );
    }
    #[test]
    fn is_stale_baseline_write_path_replace_edits_ignored() {
        // Write path: user edited a replace-mode component in the baseline.
        // Only append-mode components are checked. Replace edits are fine.
        let snapshot = "<!-- agent:status patch=replace -->\nOriginal\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nUser changed\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(
            !agent_doc_template::stale_baseline::is_stale_baseline(baseline, snapshot),
            "user edits in replace + append components should NOT be stale"
        );
    }
    #[test]
    fn reposition_skips_working_tree_when_ipc_listener_active() {
        use std::fs;
        use std::thread;
        use std::time::Duration;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid123 -->\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        // Create snapshot
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::save(&doc, doc_content, agent_doc_ops_log_io::log_op).unwrap();

        // Initial commit
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // Start a live IPC listener to simulate an active editor plugin.
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let root_clone = root.to_path_buf();
        let server = thread::spawn(move || {
            agent_doc_ipc_io::start_listener(&root_clone, |_msg| {
                Some(serde_json::json!({"type": "ack"}).to_string())
            })
            .ok();
        });
        thread::sleep(Duration::from_millis(100));

        // Run reposition — should skip working tree because the listener is active.
        let changed = reposition_boundary_in_snapshot(&doc);

        // Snapshot should be repositioned
        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("oldid123"),
            "snapshot boundary should be repositioned"
        );
        assert!(
            snap.contains("### Re: test — opus-4-6\n"),
            "snapshot should be normalized to the clean heading"
        );
        assert_eq!(
            snap.matches("(HEAD)").count(),
            0,
            "snapshot should not retain transient head markers"
        );

        // Working tree should NOT be modified (listener owns the update)
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("oldid123"),
            "working tree should keep old boundary when listener is active"
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)\n"),
            "working tree should stay untouched before plugin reposition"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            1,
            "working tree should retain exactly one visible head marker"
        );

        assert!(changed, "snapshot change should report changed=true");

        let _ = std::fs::remove_file(agent_doc_ipc_io::socket_path(root));
        drop(server);
    }
    #[test]
    fn reposition_queues_file_ipc_when_only_patches_dir_exists() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid456 -->\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        // Create snapshot
        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::save(&doc, doc_content, agent_doc_ops_log_io::log_op).unwrap();

        // Initial commit
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        // File-watch IPC is editor-owned even without a live socket listener.
        // Queue a patch instead of rewriting the open markdown file directly.
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        // Run reposition
        reposition_boundary_in_snapshot(&doc);

        // Snapshot is repositioned for commit staging.
        let snap = agent_doc_snapshot_io::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("oldid456"),
            "snapshot boundary should be repositioned"
        );
        assert!(
            snap.contains("### Re: test — opus-4-6\n"),
            "snapshot should be normalized to the clean heading"
        );
        assert_eq!(
            snap.matches("(HEAD)").count(),
            0,
            "snapshot should not retain transient head markers"
        );

        // Working tree stays untouched; the queued file IPC patch lets the IDE
        // apply the visible cleanup through its Document API.
        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("oldid456"),
            "working tree should not be rewritten while file IPC is available"
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)\n"),
            "working tree must preserve the active editor buffer; got:\n{working}"
        );
        assert_eq!(
            working.matches("(HEAD)").count(),
            1,
            "working tree should retain exactly one (HEAD) marker; got:\n{working}"
        );

        let patch_file = root.join(".agent-doc/patches").join(format!(
            "{}.json",
            agent_doc_fs::document_state_hash(&doc).unwrap()
        ));
        assert!(
            patch_file.exists(),
            "reposition should be queued for file IPC"
        );
        let payload: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&patch_file).unwrap()).unwrap();
        assert_eq!(payload["reposition_boundary"], true);
        assert_eq!(payload["preserve_head"], true);
        let queued_boundary = payload["reposition_boundary_id"].as_str().unwrap();
        assert_ne!(queued_boundary, "oldid456");
        assert!(
            snap.contains(&format!("<!-- agent:boundary:{queued_boundary} -->")),
            "queued patch should reuse committed snapshot boundary id"
        );
        assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
        assert_eq!(payload["unmatched"], "");
    }
    #[test]
    fn reposition_updates_working_tree_when_no_editor_ipc_available() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: test — opus-4-6 (HEAD)\nResponse.\n\
            <!-- agent:boundary:oldid789 -->\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, doc_content).unwrap();

        let snap_dir = root.join(".agent-doc/snapshots");
        fs::create_dir_all(&snap_dir).unwrap();
        agent_doc_snapshot_io::save(&doc, doc_content, agent_doc_ops_log_io::log_op).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        reposition_boundary_in_snapshot(&doc);

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            !working.contains("oldid789"),
            "working tree should be rewritten when no editor IPC is available"
        );
        assert!(
            working.contains("### Re: test — opus-4-6 (HEAD)"),
            "direct fallback must preserve (HEAD) annotations; got:\n{working}"
        );
    }
    #[test]
    fn reposition_repairs_missing_working_tree_prompt_prefix_without_listener() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let snapshot_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            ❯ do #spfxnorm. spec-test-build-install-commit-push\n\
            ### Re: #spfxnorm — opus-4-6\n\
            Implemented.\n\
            <!-- agent:boundary:clean789 -->\n\
            <!-- /agent:exchange -->\n";
        let working_content = "---\nagent_doc_format: template\n---\n\
            <!-- agent:exchange patch=append -->\n\
            do #spfxnorm. spec-test-build-install-commit-push\n\
            ### Re: #spfxnorm — opus-4-6 (HEAD)\n\
            Implemented.\n\
            <!-- agent:boundary:dirty789 -->\n\
            <!-- /agent:exchange -->\n";
        let doc = root.join("plan.md");
        fs::write(&doc, working_content).unwrap();

        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        reposition_boundary_in_snapshot(&doc);

        let working = fs::read_to_string(&doc).unwrap();
        assert!(
            working.contains("❯ do #spfxnorm. spec-test-build-install-commit-push"),
            "working tree should regain the missing prompt prefix:\n{working}"
        );
        assert!(
            !working.contains("<!-- agent:boundary:dirty789 -->"),
            "working tree boundary should also be repositioned:\n{working}"
        );
    }
}
