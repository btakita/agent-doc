//! # Module: write
//!
//! All write paths for agent responses: inline append, template patch, stream
//! (CRDT), IPC-to-IDE-plugin, and recovery helpers. Each path follows the same
//! invariant: save pending → acquire lock → compute `content_ours` (baseline +
//! response) → merge with any concurrent user edits → atomic write → save
//! snapshot as `final_content` (the actual post-merge disk state) → clear pending.
//!
//! ## Write dedup (v0.28.2)
//!
//! All four write paths (`run`, `run_template`, `run_stream` disk, `run_stream`
//! IPC) skip the actual write when the merged/patched content is identical to
//! the current file on disk. Dedup events are logged to stderr and appended
//! (with backtrace) to `/tmp/agent-doc-write-dedup.log` for diagnosis.
//!
//! ## Pane ownership verification (v0.28.2)
//!
//! `verify_pane_ownership()` is called at the top of `run`, `run_template`, and
//! `run_stream`. It checks that the current tmux pane matches the session
//! registry entry for the document's `session` frontmatter field. If a
//! *different* pane definitively owns the session, the write is rejected with an
//! error suggesting `agent-doc claim`. The check is lenient: it passes silently
//! when not in tmux, when there is no session ID, or when the pane is
//! indeterminate.
//!
//! ## Spec
//!
//! - `run`: inline (User/Assistant) mode. Reads response from stdin, strips any
//!   leading `## Assistant` / trailing `## User` headings the agent may have
//!   echoed, then appends `## Assistant\n\n<response>\n\n## User\n\n` to the
//!   document. Saves a pre-response snapshot for undo. If the file changed
//!   since `baseline`, performs a 3-way git merge before writing.
//!
//! - `run_template`: template-component mode. Parses `patch:NAME` fence blocks
//!   from stdin, sanitizes any `<!-- agent:NAME -->` markers in patch content
//!   (prevents parser corruption), applies patches to the baseline via
//!   `template::apply_patches`, then performs the same lock/merge/atomic-write
//!   cycle as `run`.
//!
//! - `run_stream`: CRDT stream-flush mode. Like `run_template` but uses
//!   `merge::merge_contents_crdt` for conflict-free merge. Saves both a text
//!   snapshot and a CRDT state snapshot after every flush. Supports IPC-first
//!   writes: when `.agent-doc/patches/` exists and `--force-disk` is not set,
//!   tries `try_ipc` first; on timeout (exit code 75 / `EX_TEMPFAIL`) leaves a
//!   fallback patch file for the plugin to pick up later.
//!
//! - `run_ipc`: explicit IPC-only mode. Serialises patches as JSON to
//!   `.agent-doc/patches/<hash>.json`, polls for the plugin to delete the file
//!   as ACK (2 s timeout), then falls back to the direct CRDT disk path.
//! - `run_command(options, commit_mode)`: shared CLI entrypoint for `write` and
//!   `finalize`. `finalize` is always strict. `write --commit` stays
//!   best-effort for non-session documents and `--pending-only`, but upgrades to
//!   the same strict commit-boundary contract as `finalize` when the target file
//!   is a real session document (`agent_doc_session` / legacy `session`) and the
//!   command is writing a response.
//!
//! - `try_ipc`: low-level IPC helper used by `run_stream`. Writes a JSON patch
//!   file (component patches + optional frontmatter + `reposition_boundary`
//!   flag) and polls for ACK. Returns `Ok(true)` on success, `Ok(false)` on
//!   timeout. Safe to call unconditionally — returns `false` immediately when
//!   `.agent-doc/patches/` does not exist. Synthesises a boundary-aware
//!   exchange patch when no explicit patches exist but unmatched content and a
//!   boundary marker are present.
//!
//! - `try_ipc_full_content`: like `try_ipc` but sends a full document
//!   replacement (`fullContent` field) instead of component patches. Used by
//!   inline-mode documents without component markers.
//!
//! - `try_ipc_reposition_boundary`: fire-and-forget IPC signal with the exact
//!   committed exchange `boundary_id`. Normalizes the editor buffer back to the
//!   committed boundary marker without touching the working tree (preserves
//!   cursor/undo in the IDE). Non-fatal on timeout.
//!
//! - `apply_append_from_string`: recovery variant of `run` — takes response
//!   text directly instead of reading stdin. Used by `repair` to replay
//!   orphaned inline responses.
//!
//! - `apply_template_from_string`: recovery variant of `run_template`.
//!
//! - `apply_stream_from_string`: recovery variant of `run_stream` (CRDT merge).
//!
//! - `check_future_work_signals(response, has_pending_add)`: scans the response
//!   for deferred-work phrases ("worth revisiting", "revisit later",
//!   "follow-up needed", "future work") case-insensitively. Returns
//!   `Some(signal)` when a match is found and `has_pending_add` is false (i.e.,
//!   the caller didn't already promote to pending). `WriteFlags.has_pending_add`
//!   carries this state through the call chain (no env var dependency).
//!   Integrated into `run_stream` after patch application.
//!
//! - `enforce_imperative_response_contract(file, baseline, current, response)`:
//!   when the current document diff contains imperative user directives
//!   (`do #id`, `run tests`, `build + install`, `commit + push`, or approval
//!   words like `go`), rejects status-only/meta responses unless they include
//!   either concrete execution evidence or a concrete blocker. This is the
//!   binary-side backstop for the executable-directive contract.
//!
//! - `sanitize_component_tags`: escapes `<!-- agent:NAME -->` and
//!   `<!-- /agent:NAME -->` markers appearing in patch content to prevent the
//!   component parser from treating them as real delimiters.
//!
//! - `strip_assistant_heading`: strips a leading `## Assistant` heading and/or
//!   trailing `## User` heading from a response string. Prevents duplicate
//!   headings when the agent echoes them.
//!
//! - `atomic_write_pub`: public thin wrapper around the internal `atomic_write`
//!   (write to temp file + rename). Used by `compact` and other modules.
//!
//! ## Agentic Contracts
//!
//! - Snapshot invariant: the snapshot saved after every write contains
//!   `final_content` (the actual post-merge disk state), not `content_ours`.
//!   This eliminates ghost diffs caused by stale baselines (e.g. streaming
//!   checkpoints with an outdated baseline). Concurrent user edits made during
//!   response generation are absorbed into the snapshot; they will not appear
//!   in the next diff cycle, but this is preferable to phantom committed-line
//!   drift from a mismatched snapshot.
//! - Pending response is saved before any write attempt and cleared only after
//!   a successful write, so an interrupted write is recoverable.
//! - Pre-response snapshot is saved before acquiring the lock so `undo` can
//!   restore the document to its pre-response state regardless of merge
//!   outcome.
//! - All writes are atomic (temp file + rename). Partial writes never corrupt
//!   the document.
//! - Advisory file lock (`flock`) serialises concurrent writes to the same
//!   document; the lock is dropped immediately after `atomic_write`.
//! - `try_ipc` / `try_ipc_full_content` return `false` immediately (no I/O
//!   wait) when `.agent-doc/patches/` does not exist — callers may invoke them
//!   unconditionally without performance cost when no plugin is active.
//! - IPC writes include `reposition_boundary: true` so the plugin moves the
//!   boundary marker to end-of-exchange in the same Document API transaction as
//!   the patch, avoiding a second round-trip.
//! - CRDT snapshots are saved from the merged state (not from `content_ours`)
//!   so subsequent merges use the correct shared ancestor, preventing
//!   character-level duplication across cycles.
//! - `sanitize_component_tags` is applied to every patch block before any
//!   write path applies it, preventing agent-generated examples of component
//!   syntax from corrupting future parses.
//!
//! ## Evals
//!
//! - `write_appends_response`: inline write appends `## Assistant\n\n<text>` +
//!   `\n## User\n\n` to a document → both headings and content present in file.
//! - `write_updates_snapshot`: after a write the snapshot path resolves to
//!   `.agent-doc/snapshots/` and a roundtrip read/write is lossless.
//! - `write_preserves_user_edits_via_merge`: 3-way merge when user appends to
//!   `## User` block concurrently → merged result contains both response and
//!   user addition.
//! - `write_no_merge_when_unchanged`: when file equals baseline at lock time,
//!   `content_ours` is used directly (no merge invoked).
//! - `atomic_write_correct_content`: temp-rename write produces the exact bytes
//!   supplied.
//! - `concurrent_writes_no_corruption`: 20 threads racing on atomic_write →
//!   final file is one complete writer's content (no corruption or partial
//!   writes).
//! - `snapshot_matches_disk_state`: snapshot saved as `final_content`;
//!   snapshot always matches the actual file on disk after a write.
//! - `try_ipc_returns_false_when_no_patches_dir`: `try_ipc` with no
//!   `.agent-doc/patches/` → returns `false` immediately.
//! - `try_ipc_times_out_when_no_plugin`: `.agent-doc/patches/` exists but
//!   nothing consumes the file → returns `false` after 2 s; patch file cleaned
//!   up.
//! - `try_ipc_succeeds_when_plugin_consumes`: mock plugin thread deletes patch
//!   file within 2 s → `try_ipc` returns `true`.
//! - `try_ipc_full_content_returns_false_when_no_patches_dir`: full-content IPC
//!   with no patches dir → returns `false`.
//! - `sanitize_escapes_open_agent_tag`: `<!-- agent:exchange -->` inside patch
//!   content is escaped to `&lt;!-- agent:exchange --&gt;`.
//! - *(aspirational)* `run_stream_crdt_merge`: concurrent user keystroke during
//!   stream flush → CRDT merge produces text containing both agent response and
//!   user addition without character interleaving.
//! - *(aspirational)* `ipc_fallback_on_timeout`: `run_stream` with IPC timeout
//!   exits with code 75 and leaves a patch file for deferred plugin pickup.
//! - `detects_worth_revisiting`: response with "Worth revisiting" and no
//!   pending-add → returns `Some("worth revisiting")`.
//! - `detects_future_work`: response with "future work" → returns the signal.
//! - `detects_follow_up_needed`: response with "Follow-up needed" → returns signal.
//! - `suppressed_when_pending_add_present`: response with signal but
//!   `has_pending_add=true` → returns `None`.
//! - `no_false_positive_on_normal_text`: response without any signal phrases →
//!   returns `None`.
//! - `case_insensitive_detection`: "WORTH REVISITING" (uppercase) → detected.
//! - `imperative_contract_rejects_status_only_response`: directive diff +
//!   "In progress" response → error
//! - `imperative_contract_allows_concrete_blocker`: directive diff +
//!   blocked/error response → accepted
//! - `normalize_user_prompts_new_line_gets_prefix`: user adds "Hello" to exchange
//!   → normalized content has "❯ Hello".
//! - `normalize_user_prompts_agent_response_not_prefixed`: agent response lines in content_ours
//!   must NOT get `❯ ` prefix — only user-added lines (snapshot→baseline diff) are prefixed.
//! - `normalize_user_prompts_blank_line_skipped`: blank line added → no prefix.
//! - `normalize_user_prompts_heading_skipped`: line starting with `#` → no prefix.
//! - `normalize_user_prompts_already_prefixed_skipped`: line already starts with `❯` → unchanged.
//! - `normalize_user_prompts_existing_content_unchanged`: lines from snapshot → unchanged (no double-prefix).
//! - `normalize_user_prompts_restores_prefix_lost_in_file`: snapshot has `❯ do`, baseline (file) has `do` → restored to `❯ do`.
//! - `normalize_user_prompts_heading_replacement_does_not_swallow_next_prompt`: a synthetic heading replacement
//!   (for example ` (HEAD)` suffix churn) must not suppress `❯ ` prefixing for the next user line.
//! - `normalize_user_prompts_no_exchange_passthrough`: document without exchange → returned unchanged.
//! - `shrink_guard_blocks_truncation`: exchange shrinks from 500 to 5 bytes →
//!   `check_exchange_shrink_guard` returns error.
//! - `shrink_guard_allows_normal_write`: exchange shrinks by 50% → guard passes.
//! - `shrink_guard_skips_small_exchange`: exchange is 50 bytes → guard passes
//!   regardless of shrink ratio (below `SHRINK_GUARD_MIN_BYTES`).
//! - `splice_pending_replaces_content_when_both_have_pending`: on IPC timeout,
//!   pending-done mutations from disk are preserved in the written content.
//! - `splice_pending_noop_when_source_has_no_pending`: no source pending → target unchanged.
//! - `splice_pending_warns_when_target_missing_pending`: target has no pending component → target unchanged.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::snapshot::find_project_root;
use crate::{
    component, component::is_backlog_component, frontmatter, merge, repair, sessions, snapshot,
    template,
};

#[derive(Clone, Debug)]
pub struct CommandOptions {
    pub file: PathBuf,
    pub baseline_file: Option<PathBuf>,
    pub is_template: bool,
    pub is_stream: bool,
    pub is_ipc: bool,
    pub force_disk: bool,
    pub origin: Option<String>,
    pub pending_add: Vec<String>,
    pub pending_add_gated: Vec<String>,
    pub pending_done: Vec<String>,
    pub pending_edit: Vec<String>,
    pub pending_clear: bool,
    pub pending_reorder: Option<String>,
    pub pending_gate: Vec<String>,
    pub pending_ungate: Vec<String>,
    pub pending_resolve_gate: Vec<String>,
    pub pending_set_gate_type: Vec<String>,
    pub allow_replace_pending: bool,
    pub pending_only: bool,
    pub status: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WriteFlags {
    pub allow_replace_pending: bool,
    pub has_pending_add: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitMode {
    None,
    BestEffort,
    Required,
}

fn is_session_document(file: &Path) -> Result<bool> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&content)?;
    Ok(fm
        .session
        .as_deref()
        .is_some_and(|session| !session.trim().is_empty()))
}

fn resolve_commit_mode(
    file: &Path,
    requested: CommitMode,
    pending_only: bool,
) -> Result<CommitMode> {
    if pending_only || requested != CommitMode::BestEffort {
        return Ok(requested);
    }
    if is_session_document(file)? {
        return Ok(CommitMode::Required);
    }
    Ok(CommitMode::BestEffort)
}

pub fn run_command(options: CommandOptions, commit_mode: CommitMode) -> Result<()> {
    let file = options.file.as_path();

    if let Some(ref origin) = options.origin {
        crate::ops_log::log_op(
            file,
            &format!("write_origin file={} origin={}", file.display(), origin),
        );
    }

    let has_pending_ops = !options.pending_add.is_empty()
        || !options.pending_add_gated.is_empty()
        || !options.pending_done.is_empty()
        || !options.pending_edit.is_empty()
        || options.pending_clear
        || options.pending_reorder.is_some()
        || !options.pending_gate.is_empty()
        || !options.pending_ungate.is_empty()
        || !options.pending_resolve_gate.is_empty()
        || !options.pending_set_gate_type.is_empty();

    if options.pending_only && !has_pending_ops {
        anyhow::bail!("--pending-only requires at least one --pending-* flag");
    }
    if options.pending_only && (options.is_template || options.is_stream || options.is_ipc) {
        anyhow::bail!("--pending-only cannot be combined with --template, --stream, or --ipc");
    }
    if options.pending_only && commit_mode == CommitMode::Required {
        anyhow::bail!("finalize does not support --pending-only");
    }
    let commit_mode = resolve_commit_mode(file, commit_mode, options.pending_only)?;
    if commit_mode == CommitMode::Required && !crate::git::is_in_git_repo(file) {
        if is_session_document(file)? {
            anyhow::bail!(
                "write --commit requires a git repository for session documents so the cycle can reach a committed state"
            );
        }
        anyhow::bail!(
            "finalize requires a git repository so the cycle can reach a committed state"
        );
    }

    if has_pending_ops {
        if options.pending_clear {
            crate::pending_cmd::clear(file)?;
        }
        crate::pending_cmd::add_many(file, &options.pending_add, false)?;
        crate::pending_cmd::add_many(file, &options.pending_add_gated, true)?;
        if !options.pending_add.is_empty() || !options.pending_add_gated.is_empty() {
            crate::cycle_state::mark_pending_mutations(file)?;
        }
        for pair in &options.pending_edit {
            let (id, text) = pair
                .split_once('=')
                .with_context(|| format!("--pending-edit expects 'id=text', got: {}", pair))?;
            crate::pending_cmd::edit(file, id, text)?;
        }
        for id in &options.pending_gate {
            crate::pending_cmd::gate(file, id)?;
        }
        for pair in &options.pending_set_gate_type {
            let (id, gt) = pair.split_once('=').with_context(|| {
                format!("--pending-set-gate-type expects 'id=type', got: {}", pair)
            })?;
            crate::pending_cmd::set_gate_type(file, id, gt)?;
        }
        for id in &options.pending_ungate {
            crate::pending_cmd::ungate(file, id)?;
        }
        for gt in &options.pending_resolve_gate {
            crate::pending_cmd::resolve_gate(file, gt)?;
        }
        for id in &options.pending_done {
            crate::pending_cmd::done(file, id)?;
        }
        if !options.pending_done.is_empty() {
            crate::cycle_state::record_pending_done_ids(file, &options.pending_done)?;
        }
        if let Some(ref order) = options.pending_reorder {
            let ids: Vec<String> = order
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            crate::pending_cmd::reorder(file, &ids)?;
        }
    }

    if let Some(ref status_text) = options.status {
        crate::status_cmd::set(file, status_text)?;
    }

    if options.pending_only {
        return finalize_commit(file, commit_mode);
    }

    let write_flags = WriteFlags {
        allow_replace_pending: options.allow_replace_pending,
        has_pending_add: !options.pending_add.is_empty() || !options.pending_add_gated.is_empty(),
    };

    let baseline = options
        .baseline_file
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()
        .context("failed to read baseline file")?;

    let write_result = if options.is_ipc {
        run_ipc(file, baseline.as_deref(), write_flags)
    } else if options.is_stream {
        run_stream(
            file,
            baseline.as_deref(),
            options.force_disk,
            options.origin.as_deref(),
            write_flags,
        )
    } else if options.is_template {
        run_template(
            file,
            baseline.as_deref(),
            options.origin.as_deref(),
            write_flags,
        )
    } else {
        let content =
            std::fs::read_to_string(file).context("failed to read document for mode detection")?;
        let (fm, _) = frontmatter::parse(&content)?;
        if fm.resolve_mode().is_crdt() {
            run_stream(
                file,
                baseline.as_deref(),
                options.force_disk,
                options.origin.as_deref(),
                write_flags,
            )
        } else {
            run(file, baseline.as_deref())
        }
    };

    // Phase 3: consume queue prompt after successful write, before commit.
    // Included in the same commit as the response for atomicity.
    if write_result.is_ok()
        && commit_mode != CommitMode::None
        && let Err(e) = consume_queue_prompt(file)
    {
        eprintln!("[queue] warning: consumption failed: {}", e);
    }

    // Phase 3b: pre-commit pending closeout gates (strict mode only).
    if write_result.is_ok() && commit_mode == CommitMode::Required {
        precommit_pending_capture_check(file)?;
        precommit_pending_done_check(file)?;
    }

    let commit_result = finalize_commit(file, commit_mode);

    match (write_result, commit_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(write_err), Ok(())) => Err(write_err),
        (Ok(()), Err(commit_err)) => Err(commit_err),
        (Err(write_err), Err(commit_err)) => Err(write_err.context(commit_err.to_string())),
    }
}

fn finalize_commit(file: &Path, commit_mode: CommitMode) -> Result<()> {
    match commit_mode {
        CommitMode::None => Ok(()),
        CommitMode::BestEffort => {
            if crate::git::is_in_git_repo(file) {
                if let Err(e) = crate::git::commit(file) {
                    eprintln!("[commit] warning: {}", e);
                }
                crate::session_check::enforce_clean_closeout(file)?;
            } else {
                eprintln!("[commit] skipped (not in git repo)");
            }
            Ok(())
        }
        CommitMode::Required => {
            crate::git::commit(file)?;
            ensure_cycle_committed(file)?;
            crate::session_check::enforce_clean_closeout(file)
        }
    }
}

fn ensure_cycle_committed(file: &Path) -> Result<()> {
    let Some(state) = crate::cycle_state::load(file)? else {
        anyhow::bail!("finalize did not persist cycle state");
    };
    if state.is_open() {
        anyhow::bail!(
            "finalize left cycle `{}` open at `{}` ({})",
            state.cycle_id,
            cycle_phase_name(state.phase),
            state.last_event
        );
    }
    Ok(())
}

fn precommit_pending_capture_check(file: &Path) -> Result<()> {
    let mode = crate::session_check::resolve_pending_capture_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(());
    };
    if state.had_pending_mutations {
        return Ok(());
    }

    let Some(capture) = crate::capture::load_active(file)? else {
        return Ok(());
    };
    if capture
        .response_body
        .contains("<!-- no-pending-capture -->")
    {
        return Ok(());
    }

    let response_text = crate::session_check::response_text_for_guards(&capture.response_body);
    if response_text.trim().is_empty() {
        return Ok(());
    }

    let signal = crate::heuristics::detect_uncaptured_recommendations(&response_text);
    let skip = match signal.estimated_count {
        0 => true,
        1 => signal.confidence < 0.7,
        _ => signal.confidence < 0.5,
    };
    if skip {
        return Ok(());
    }

    anyhow::bail!(
        "[finalize] pre-commit gate: response contains ~{} recommendation-like items \
         but no --pending-add flags were used this cycle\n\
         [finalize] hint: re-run finalize with --pending-add flags, \
         add <!-- no-pending-capture --> to suppress, \
         or set pending_capture_guard = \"warn\" to downgrade",
        signal.estimated_count
    );
}

fn precommit_pending_done_check(file: &Path) -> Result<()> {
    let mode = crate::session_check::resolve_pending_done_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(());
    };

    let Some(capture) = crate::capture::load_active(file)? else {
        return Ok(());
    };
    if capture
        .response_body
        .contains("<!-- no-pending-done-guard -->")
    {
        return Ok(());
    }

    let response_text = crate::session_check::response_text_for_guards(&capture.response_body);
    let missing = crate::session_check::detect_missing_pending_done_ids(
        file,
        &response_text,
        &state.pending_done_ids,
    )?;
    if missing.is_empty() {
        return Ok(());
    }

    let ids = missing
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let hint = missing
        .iter()
        .map(|id| format!("--pending-done {}", id))
        .collect::<Vec<_>>()
        .join(" ");

    anyhow::bail!(
        "[finalize] pre-commit gate: response appears to complete existing pending {} \
         but no matching `--pending-done` was recorded this cycle\n\
         [finalize] hint: re-run finalize with {}, \
         add <!-- no-pending-done-guard --> to suppress, \
         or set pending_done_guard = \"warn\" to downgrade",
        ids,
        hint
    );
}

fn cycle_phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
    }
}

/// Consume the first queue prompt after a successful write cycle.
///
/// Called between the write step and the commit step so the consumption
/// is included in the same git commit as the response (atomic).
///
/// Reads frontmatter for `queue_active: true`; if the queue is not active
/// or has no prompts, this is a no-op. On consumption, the first prompt is
/// removed from both the file and the snapshot. When the queue drains to
/// empty, `auto` is stripped and `queue_active` is cleared.
fn consume_queue_prompt(file: &Path) -> Result<bool> {
    // Hold the document lock for the entire read-parse-write cycle to prevent
    // concurrent edits from invalidating parsed offsets (TOCTOU fix).
    let _lock = acquire_doc_lock(file)?;
    let content =
        std::fs::read_to_string(file).context("queue consume: failed to read document")?;
    let (fm, _) = frontmatter::parse(&content)?;

    if fm.queue_active != Some(true) {
        return Ok(false);
    }

    let components = component::parse(&content)?;
    let comp = match components.iter().find(|c| c.name == "queue") {
        Some(c) => c,
        None => return Ok(false),
    };

    let body = &content[comp.open_end..comp.close_start];
    let entries = match crate::queue::parse(body) {
        Ok(e) => e,
        Err(_) => return Ok(false),
    };

    let consumed_text = match crate::queue::first_prompt(&entries) {
        Some(p) => p.text.clone(),
        None => return Ok(false),
    };

    let new_entries = crate::queue::remove_first_prompt(&entries);
    let new_body = crate::queue::render(&new_entries);
    let mut current = comp.replace_content(&content, &new_body);

    let has_auto = crate::queue::has_auto_attr(&comp.attrs);
    let remaining = crate::queue::prompts(&new_entries).len();
    let drained = remaining == 0;

    if drained {
        if has_auto {
            let comps = component::parse(&current)?;
            if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                let raw = &current[q.open_start..q.open_end];
                let new_tag = crate::queue::strip_auto_from_tag(raw);
                if new_tag != raw {
                    let mut rebuilt = String::with_capacity(current.len());
                    rebuilt.push_str(&current[..q.open_start]);
                    rebuilt.push_str(&new_tag);
                    rebuilt.push_str(&current[q.open_end..]);
                    current = rebuilt;
                }
            }
        }
        current = frontmatter::merge_fields(&current, "queue_active: false")?;
    }

    std::fs::write(file, &current).context("queue consume: failed to write document")?;

    // Update snapshot in sync
    if let Ok(Some(snap)) = snapshot::load(file) {
        let mut new_snap = snap.clone();
        if let Ok(snap_comps) = component::parse(&new_snap)
            && let Some(sq) = snap_comps.iter().find(|c| c.name == "queue")
        {
            new_snap = sq.replace_content(&new_snap, &new_body);

            if drained {
                if has_auto
                    && let Ok(sc2) = component::parse(&new_snap)
                    && let Some(sq2) = sc2.iter().find(|c| c.name == "queue")
                {
                    let raw = &new_snap[sq2.open_start..sq2.open_end];
                    let new_tag = crate::queue::strip_auto_from_tag(raw);
                    if new_tag != raw {
                        let mut rebuilt = String::with_capacity(new_snap.len());
                        rebuilt.push_str(&new_snap[..sq2.open_start]);
                        rebuilt.push_str(&new_tag);
                        rebuilt.push_str(&new_snap[sq2.open_end..]);
                        new_snap = rebuilt;
                    }
                }
                if let Ok(merged) = frontmatter::merge_fields(&new_snap, "queue_active: false") {
                    new_snap = merged;
                }
            }

            if new_snap != snap {
                snapshot::save(file, &new_snap)?;
            }
        }
    }

    eprintln!(
        "[queue] consumed: {:?} (remaining: {})",
        consumed_text, remaining
    );
    if drained {
        eprintln!("[queue] drained — cleared queue_active");
    }

    Ok(true)
}

/// Enforcement: reject full-replacement blocks targeting the `pending` component
/// unless the caller explicitly opts in.
///
/// Canonical form: `<!-- replace:pending -->...<!-- /replace:pending -->` with
/// `--allow-replace-pending` (or `AGENT_DOC_ALLOW_REPLACE_PENDING=1`).
///
/// Deprecated form (`#25ag` migration — one release of dual-accept):
/// `<!-- patch:pending -->...<!-- /patch:pending -->` with `--allow-patch-pending`
/// (or `AGENT_DOC_ALLOW_PATCH_PENDING=1`). Parser emits a deprecation warning
/// when the deprecated form is used.
///
/// The pending system requires mutations via granular flags
/// (`--pending-add/done/edit/clear/reorder`); a full-replace block on a list the
/// user concurrently edits enables silent-data-loss via concurrent-edit clobber
/// and hash instability.
///
/// Phase 3 inversion (2026-04-14): the default is now reject. Library callers
/// (FFI, tests, future SDK consumers) must opt in explicitly.
pub(crate) fn enforce_no_replace_pending(
    patches: &[template::PatchBlock],
    allow: bool,
) -> Result<()> {
    if allow {
        return Ok(());
    }
    let allow_canonical = std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false);
    let allow_legacy = std::env::var("AGENT_DOC_ALLOW_PATCH_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false);
    if allow_canonical || allow_legacy {
        return Ok(());
    }
    if patches.iter().any(|p| is_backlog_component(&p.name)) {
        anyhow::bail!(
            "ERR: replace:pending block forbidden — use --pending-add/done/edit/clear/reorder. \
             See specs/pending-system.md."
        );
    }
    Ok(())
}

struct NormalizedTemplateResponse {
    response_for_capture: Option<String>,
    patches: Vec<template::PatchBlock>,
    unmatched: String,
}

fn same_ignoring_trailing_newlines(left: &str, right: &str) -> bool {
    left.trim_end_matches('\n') == right.trim_end_matches('\n')
}

fn serialize_template_response(patches: &[template::PatchBlock], unmatched: &str) -> String {
    let mut out = String::new();
    for patch in patches {
        out.push_str("<!-- patch:");
        out.push_str(&patch.name);
        if !patch.attrs.is_empty() {
            let mut attrs: Vec<_> = patch.attrs.iter().collect();
            attrs.sort_by(|(left, _), (right, _)| left.cmp(right));
            for (key, value) in attrs {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                out.push_str(&value.replace('"', "&quot;"));
                out.push('"');
            }
        }
        out.push_str(" -->\n");
        out.push_str(&patch.content);
        if !patch.content.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("<!-- /patch:");
        out.push_str(&patch.name);
        out.push_str(" -->\n");
    }
    if !unmatched.trim().is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(unmatched.trim());
        out.push('\n');
    }
    out
}

fn normalize_backlog_patch_response(
    file: &Path,
    current_content: &str,
    mut patches: Vec<template::PatchBlock>,
    unmatched: String,
    allow_replace: bool,
) -> Result<NormalizedTemplateResponse> {
    if allow_replace {
        return Ok(NormalizedTemplateResponse {
            response_for_capture: None,
            patches,
            unmatched,
        });
    }
    let allow_canonical = std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false);
    let allow_legacy = std::env::var("AGENT_DOC_ALLOW_PATCH_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false);
    if allow_canonical || allow_legacy {
        return Ok(NormalizedTemplateResponse {
            response_for_capture: None,
            patches,
            unmatched,
        });
    }

    let backlog_indexes: Vec<usize> = patches
        .iter()
        .enumerate()
        .filter_map(|(idx, patch)| is_backlog_component(&patch.name).then_some(idx))
        .collect();

    if backlog_indexes.is_empty() {
        return Ok(NormalizedTemplateResponse {
            response_for_capture: None,
            patches,
            unmatched,
        });
    }
    if backlog_indexes.len() > 1 {
        anyhow::bail!(
            "ERR: multiple pending/backlog patches in one response are not supported — use --pending-* flags"
        );
    }

    let components = component::parse(current_content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;
    let backlog_component = components
        .iter()
        .find(|component| is_backlog_component(&component.name))
        .with_context(|| {
            format!(
                "document has no pending/backlog component: {}",
                file.display()
            )
        })?;
    let current_body = backlog_component.content(current_content);
    let (current_prelude, current_items, current_postlude) =
        crate::pending::parse_items(current_body);
    let current_ids: HashSet<String> = current_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .map(|item| item.id.clone())
        .collect();
    let current_states: HashMap<String, crate::pending::PendingState> = current_items
        .iter()
        .map(|item| (item.id.clone(), item.state))
        .collect();

    let backlog_index = backlog_indexes[0];
    let doc_id = crate::pending_cmd::doc_id_for(file);
    let (target_body, _) =
        crate::pending::backfill(&patches[backlog_index].content, &doc_id, &current_ids);
    let (target_prelude, target_items, target_postlude) = crate::pending::parse_items(&target_body);
    let rendered_target =
        crate::pending::render_items(&target_prelude, &target_items, &target_postlude);
    if !same_ignoring_trailing_newlines(&rendered_target, &target_body) {
        anyhow::bail!(
            "ERR: pending/backlog patch could not be normalized into supported --pending-* operations"
        );
    }
    if !same_ignoring_trailing_newlines(&current_prelude, &target_prelude)
        || !same_ignoring_trailing_newlines(&current_postlude, &target_postlude)
    {
        anyhow::bail!(
            "ERR: pending/backlog patch changed non-list content — use granular --pending-* flags instead"
        );
    }

    if !same_ignoring_trailing_newlines(current_body, &target_body) {
        let mut normalized_body = crate::pending::op_clear(current_body)?;
        let mut saw_pending_add = false;
        let mut pending_done_ids = Vec::new();

        for item in target_items.iter().rev() {
            crate::pending::ensure_no_leading_custom_id_prefix(
                &item.text,
                "ERR: pending/backlog patch",
            )?;
            let add_text = if item.id.is_empty() {
                item.text.clone()
            } else {
                format!("id={} {}", item.id, item.text)
            };
            let (new_body, assigned_id) =
                crate::pending::op_add(&normalized_body, &add_text, &doc_id, false)?;
            normalized_body = new_body;

            if item.state == crate::pending::PendingState::Gated {
                normalized_body = crate::pending::op_gate(&normalized_body, &assigned_id)?;
                if let Some(gate_type) = item.gate_type.as_deref() {
                    normalized_body = crate::pending::op_set_gate_type(
                        &normalized_body,
                        &assigned_id,
                        gate_type,
                    )?;
                }
            } else if item.state == crate::pending::PendingState::Done {
                normalized_body = crate::pending::op_done(&normalized_body, &assigned_id)?;
            }

            if !current_ids.contains(&assigned_id) {
                saw_pending_add = true;
            }
            if item.state == crate::pending::PendingState::Done
                && current_states.get(&assigned_id).copied()
                    != Some(crate::pending::PendingState::Done)
            {
                pending_done_ids.push(assigned_id);
            }
        }

        let rewritten_doc = backlog_component.replace_content(current_content, &normalized_body);
        std::fs::write(file, &rewritten_doc).with_context(|| {
            format!(
                "failed to write normalized pending state {}",
                file.display()
            )
        })?;
        crate::ops_log::log_op(
            file,
            &format!(
                "normalize_pending_patch file={} added={} done={}",
                file.display(),
                saw_pending_add,
                pending_done_ids.len()
            ),
        );
        if saw_pending_add {
            crate::cycle_state::mark_pending_mutations(file)?;
        }
        if !pending_done_ids.is_empty() {
            crate::cycle_state::record_pending_done_ids(file, &pending_done_ids)?;
        }
    }

    patches.remove(backlog_index);
    let response_for_capture = Some(serialize_template_response(&patches, &unmatched));
    Ok(NormalizedTemplateResponse {
        response_for_capture,
        patches,
        unmatched,
    })
}

/// Resolve the IPC project root for `canonical` (an already-canonicalized file
/// path). Uses the nearest `.agent-doc/` directory to match the IDE plugin's
/// `resolveRootFor` logic — submodule documents use the submodule's own
/// `.agent-doc/`, not the superproject's. Falls back to git toplevel for
/// plain git repos without `.agent-doc/`, then the file's parent directory.
fn resolve_ipc_project_root(canonical: &Path) -> std::path::PathBuf {
    let parent = canonical.parent().unwrap_or(Path::new("/"));
    // 1. Nearest .agent-doc/ root — mirrors IDE plugin's resolveRootFor.
    //    Submodule files resolve to the submodule root, not the superproject,
    //    so ack-content and patch paths agree between Rust and Kotlin.
    if let Some(p) = find_project_root(canonical) {
        return p;
    }
    // 2. Plain git repo without .agent-doc: use the toplevel.
    if let Some(toplevel) = crate::git::git_toplevel_at(parent) {
        return toplevel;
    }
    // 3. Last resort: file's parent directory.
    parent.to_path_buf()
}

/// Public accessor for `resolve_ipc_project_root` (used by git.rs).
pub fn resolve_ipc_project_root_pub(canonical: &Path) -> std::path::PathBuf {
    resolve_ipc_project_root(canonical)
}

/// Helper: extract boundary_id for a named component from the document.
///
/// Searches for `<!-- agent:boundary:UUID -->` inside the component's content,
/// skipping matches inside fenced code blocks and inline code spans.
fn find_boundary_id(doc: &str, component_name: &str) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let comp = components.iter().find(|c| c.name == component_name)?;
    let content = &doc[comp.open_end..comp.close_start];
    let code_ranges = component::find_code_ranges(doc);

    // Scan for boundary marker in component content, skipping code blocks
    let prefix = "<!-- agent:boundary:";
    let suffix = " -->";
    let mut search_from = 0;
    while let Some(start) = content[search_from..].find(prefix) {
        let abs_start = comp.open_end + search_from + start;
        // Skip if inside a code block
        if code_ranges
            .iter()
            .any(|&(cs, ce)| abs_start >= cs && abs_start < ce)
        {
            search_from += start + prefix.len();
            continue;
        }
        let id_start = search_from + start + prefix.len();
        if let Some(end) = content[id_start..].find(suffix) {
            let id = &content[id_start..id_start + end];
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        break;
    }
    None
}

/// Check if a component is append-mode (needs boundary markers).
fn is_append_mode_component(name: &str) -> bool {
    matches!(name, "exchange" | "findings")
}

/// Extract lines that were normalized by `normalize_user_prompts_in_exchange`.
///
/// Compares `before` and `after` exchange content line-by-line and returns
/// lines where `before` had plain text and `after` has `❯ <text>` at the
/// same position — i.e., lines the normalization step added `❯ ` to this cycle.
///
/// Line-by-line comparison avoids false negatives when the exchange already
/// contains `❯ <text>` lines at OTHER positions (which would cause a
/// HashSet-based check to incorrectly skip newly normalized lines).
///
/// These are passed to the IPC plugin so it can apply the same normalization
/// to the live editor document.
pub fn extract_normalization_targets(before: &str, after: &str) -> Vec<String> {
    let before_comps = component::parse(before).unwrap_or_default();
    let after_comps = component::parse(after).unwrap_or_default();

    let before_exc = before_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(before))
        .unwrap_or("");
    let after_exc = after_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(after))
        .unwrap_or("");

    if before_exc == after_exc {
        return vec![];
    }

    // Line-by-line: find positions where before had `text` and after has `❯ text`.
    // Using position comparison prevents false negatives when the exchange already
    // contains `❯ text` lines elsewhere (HashSet membership would exclude them).
    let mut seen = std::collections::HashSet::<String>::new();
    let mut targets = Vec::new();

    for (before_line, after_line) in before_exc.lines().zip(after_exc.lines()) {
        if let Some(stripped) = after_line.strip_prefix("❯ ") {
            // after has ❯ prefix; before must have the plain version at the same position
            if before_line == stripped && seen.insert(stripped.to_string()) {
                targets.push(stripped.to_string());
            }
        }
    }

    targets
}

/// Add `❯ ` prefix to user-added lines in exchange components.
///
/// Compares the exchange content in `baseline` against `snapshot` to identify
/// lines the user typed this cycle (Insert lines in the diff). Those lines are
/// then prefixed with `❯ ` in `content` (content_ours = baseline + agent patches).
/// Prompt-bearing lines derived from the canonical diff classifier are also
/// treated as mandatory normalization targets so repair/write/session-check
/// share one prompt-prefix contract.
///
/// Using `baseline` (not `content_ours`) for the diff is critical: after
/// `apply_patches_with_overrides`, the boundary marker is repositioned to the end
/// of the exchange. Everything before it — including the agent's new response —
/// is the "user region". Diffing `snapshot → content_ours user_region` would
/// incorrectly mark agent response lines as Insert and prefix them. Diffing
/// `snapshot → baseline` identifies only genuine user additions.
///
/// Skips lines that are blank, already start with `❯`, start with `<!--`
/// (structural component/patch/boundary markers), or sit inside a fenced code
/// block. Every other added line in the exchange user region gets the prefix —
/// the component defines the context, so content shape is not second-guessed.
/// Non-destructive if no exchange component is present or no new lines are
/// found.
///
/// Both disk and IPC write paths call this after computing `content_ours` so the
/// snapshot and merged document consistently show `❯ ` on user input.
pub fn normalize_user_prompts_in_exchange(content: &str, baseline: &str, snapshot: &str) -> String {
    let Ok(content_comps) = component::parse(content) else {
        return content.to_string();
    };
    let baseline_comps = component::parse(baseline).unwrap_or_default();
    let snap_comps = component::parse(snapshot).unwrap_or_default();

    let Some(exchange) = content_comps.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };

    let baseline_exc = baseline_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(baseline))
        .unwrap_or("");
    let snap_exc = snap_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(snapshot))
        .unwrap_or("");

    let exc_content = exchange.content(content);

    // Find the LAST boundary marker in content_ours — user region is before, agent region after.
    // Must use the last boundary (most recent cycle) — historical cycles each insert their own
    // boundary marker, so stopping at the first one would misclassify later user-input lines
    // (between historical boundaries) as "agent region" and skip ❯  prefix restoration.
    let boundary_prefix = "<!-- agent:boundary:";
    let boundary_pos = {
        let mut pos = exc_content.len();
        let mut offset = 0;
        for line in exc_content.lines() {
            if line.trim().starts_with(boundary_prefix) {
                pos = offset; // keep updating — use the last boundary found
            }
            offset += line.len() + 1;
        }
        pos
    };
    let content_user_region = &exc_content[..boundary_pos];
    let content_agent_region = &exc_content[boundary_pos..];

    // Strip boundary markers from baseline and snapshot for diffing.
    // Preserves trailing newline if present in the original.
    let strip = |s: &str| -> String {
        let filtered: Vec<&str> = s
            .lines()
            .filter(|l| !l.trim().starts_with(boundary_prefix))
            .collect();
        let mut out = filtered.join("\n");
        if s.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out
    };
    let baseline_stripped = strip(baseline_exc);
    let snap_stripped = strip(snap_exc);

    // Diff snapshot → baseline to find user-added lines (not agent lines).
    // Track code-fence state so lines inside fences are excluded — they are code,
    // not user prompts, and must not receive the ❯  prefix.
    // Handles both ``` and ~~~ fences (matching CommonMark spec).
    use similar::{ChangeTag, TextDiff};

    // Option 2 invariant: inside `agent:exchange`, every added line gets the ❯ prefix.
    // The component defines the context, so content shape does not gate the decision.
    // Only structural markers (HTML comments for component/patch/boundary tags) and
    // code fences are excluded — everything else is user input.

    /// Returns Some((fence_char, fence_len)) if `trimmed` opens a new fence, else None.
    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    /// Returns true if `trimmed` closes a fence opened with `(fence_char, fence_len)`.
    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    fn heading_level(trimmed: &str) -> Option<usize> {
        let n = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&n) && trimmed.as_bytes().get(n) == Some(&b' ') {
            Some(n)
        } else {
            None
        }
    }

    let diff = TextDiff::from_lines(snap_stripped.as_str(), baseline_stripped.as_str());
    let mut user_added = std::collections::HashSet::<String>::new();
    let mut in_baseline_fence = false;
    let mut baseline_fence_char = '`';
    let mut baseline_fence_len = 3usize;
    let mut in_agent_block = false;
    let mut saw_deleted_heading = false;
    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches('\n');
        let trimmed = line.trim();
        let is_heading = heading_level(trimmed).is_some();
        // Equal and Insert lines are present in baseline — track their fence state.
        // Capture pre-update state to correctly detect closing delimiters as fence markers.
        let was_in_fence = in_baseline_fence;
        if change.tag() == ChangeTag::Delete {
            saw_deleted_heading = !in_baseline_fence && is_heading;
            continue;
        }
        let heading_replaces_deleted_heading =
            change.tag() == ChangeTag::Insert && is_heading && saw_deleted_heading;
        saw_deleted_heading = false;
        if change.tag() != ChangeTag::Delete {
            if !in_baseline_fence {
                if let Some((fc, fl)) = fence_open(trimmed) {
                    in_baseline_fence = true;
                    baseline_fence_char = fc;
                    baseline_fence_len = fl;
                }
            } else if fence_close(trimmed, baseline_fence_char, baseline_fence_len) {
                in_baseline_fence = false;
            }
            if !in_baseline_fence {
                if heading_level(trimmed).is_some() {
                    in_agent_block =
                        change.tag() == ChangeTag::Insert && !heading_replaces_deleted_heading;
                } else if in_agent_block
                    && (trimmed.starts_with('❯') || trimmed.starts_with("<!--"))
                {
                    in_agent_block = false;
                }
            }
        }
        // A line is a fence delimiter if it opens a fence (fence_open), or closes the current
        // one (was_in_fence before update, and matches close pattern).
        let is_fence_delim = fence_open(trimmed).is_some()
            || (was_in_fence && fence_close(trimmed, baseline_fence_char, baseline_fence_len));
        if change.tag() == ChangeTag::Insert
            && !in_baseline_fence
            && !in_agent_block
            && !heading_replaces_deleted_heading
            && !trimmed.is_empty()
            && !trimmed.starts_with('❯')
            && !trimmed.starts_with("<!--")
            && !is_fence_delim
        {
            user_added.insert(line.to_string());
        }
    }

    if let Some(diff_text) =
        crate::diff::unified_diff_from_contents(&snap_stripped, &baseline_stripped)
    {
        for line in crate::diff::prompt_prefix_normalization_targets(&diff_text) {
            user_added.insert(line);
        }
    }

    if user_added.is_empty() {
        return content.to_string();
    }

    // Apply ❯  prefix to user-added lines in content_user_region.
    // Agent response lines (not in user_added) pass through unchanged.
    // Track code-fence state (``` and ~~~) so prefix is never added inside fences.
    let mut in_content_fence = false;
    let mut content_fence_char = '`';
    let mut content_fence_len = 3usize;
    let mut normalized_user = String::new();
    for line in content_user_region.lines() {
        let trimmed = line.trim();
        if !in_content_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_content_fence = true;
                content_fence_char = fc;
                content_fence_len = fl;
            }
        } else if fence_close(trimmed, content_fence_char, content_fence_len) {
            in_content_fence = false;
        }
        if !in_content_fence && user_added.contains(line) {
            normalized_user.push_str("❯ ");
        }
        normalized_user.push_str(line);
        normalized_user.push('\n');
    }
    if !content_user_region.is_empty() && !content_user_region.ends_with('\n') {
        normalized_user.truncate(normalized_user.len() - 1);
    }
    if content_user_region.is_empty() {
        normalized_user.clear();
    }

    let new_exc_content = format!("{}{}", normalized_user, content_agent_region);
    exchange.replace_content(content, &new_exc_content)
}

/// Phrases that signal deferred/future work in an agent response.
/// When detected without a corresponding `--pending-add`, a warning is emitted.
const FUTURE_WORK_SIGNALS: &[&str] = &[
    "worth revisiting",
    "revisit later",
    "follow-up needed",
    "future work",
];

/// Core detection logic — no env var dependency.
pub fn check_future_work_signals(response: &str, has_pending_add: bool) -> Option<&'static str> {
    if has_pending_add {
        return None;
    }
    let lower = response.to_lowercase();
    for &signal in FUTURE_WORK_SIGNALS {
        if lower.contains(signal) {
            eprintln!(
                "[write] WARN: response contains future-work signal {:?} but no --pending-add was provided",
                signal
            );
            return Some(signal);
        }
    }
    None
}

const IMPERATIVE_STATUS_ONLY_SIGNALS: &[&str] = &[
    "in progress",
    "continuing",
    "starting",
    "working on it",
    "still working",
    "next i'll",
    "next i will",
    "i'll update",
    "i will update",
    "i'm going to",
    "i am going to",
    "let me do that",
];

const IMPERATIVE_META_REFUSAL_SIGNALS: &[&str] = &[
    "because you asked me to run agent-doc",
    "treated that text as document content",
    "not to execute",
    "say do #",
    "repeat the instruction in chat",
    "i stayed on the first layer",
    "operate on the session document",
];

const IMPERATIVE_BLOCKER_SIGNALS: &[&str] = &[
    "blocked",
    "blocker",
    "failed",
    "error",
    "cannot",
    "can't",
    "unable",
    "missing",
    "permission denied",
    "requires approval",
    "needs approval",
    "lock file",
    "timed out",
];

const IMPERATIVE_EVIDENCE_LABELS: &[&str] = &[
    "what changed:",
    "verification:",
    "commit / push:",
    "outcome:",
    "root cause:",
    "blocked:",
    "blocker:",
];

pub(crate) fn enforce_imperative_response_contract(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response: &str,
) -> Result<()> {
    let baseline_owned = baseline
        .map(ToOwned::to_owned)
        .or_else(|| snapshot::load(file).ok().flatten());
    let Some(base) = baseline_owned.as_deref() else {
        return Ok(());
    };
    let Some(diff_text) = crate::diff::unified_diff_from_contents(base, current_content) else {
        return Ok(());
    };
    enforce_imperative_response_contract_for_diff(file, &diff_text, response)
}

pub(crate) fn enforce_imperative_response_contract_for_diff(
    file: &Path,
    diff_text: &str,
    response: &str,
) -> Result<()> {
    if !crate::diff::diff_contains_imperative_directive(diff_text) {
        return Ok(());
    }
    if response_satisfies_imperative_contract(response) {
        return Ok(());
    }
    let trigger = crate::diff::extract_imperative_directives(diff_text)
        .into_iter()
        .next()
        .unwrap_or_else(|| "approval".to_string());
    crate::ops_log::log_op(
        file,
        &format!(
            "imperative_response_rejected file={} trigger={}",
            file.display(),
            truncate_signal(&trigger, 80)
        ),
    );
    anyhow::bail!(
        "imperative document directive requires concrete execution evidence or a concrete blocker; rejected status-only/meta response for `{}`",
        truncate_signal(&trigger, 80)
    );
}

fn template_mode_overrides_for_current_doc(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
) -> std::collections::HashMap<String, String> {
    let mut overrides = std::collections::HashMap::new();
    let baseline_owned = baseline
        .map(ToOwned::to_owned)
        .or_else(|| snapshot::load(file).ok().flatten());
    let Some(base) = baseline_owned.as_deref() else {
        return overrides;
    };
    let Some(diff_text) = crate::diff::unified_diff_from_contents(base, current_content) else {
        return overrides;
    };
    if crate::diff::detect_exchange_compaction_request(&diff_text) {
        overrides.insert("exchange".to_string(), "replace".to_string());
    }
    overrides
}

fn response_satisfies_imperative_contract(response: &str) -> bool {
    let lower = response.to_ascii_lowercase();
    if contains_any_signal(&lower, IMPERATIVE_BLOCKER_SIGNALS) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_META_REFUSAL_SIGNALS) {
        return false;
    }
    if contains_execution_evidence(response, &lower) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_STATUS_ONLY_SIGNALS) {
        return false;
    }
    false
}

fn contains_any_signal(haystack: &str, signals: &[&str]) -> bool {
    signals.iter().any(|signal| haystack.contains(signal))
}

fn contains_execution_evidence(response: &str, lower: &str) -> bool {
    if response.contains("```") || response.contains("~~~") {
        return true;
    }
    if IMPERATIVE_EVIDENCE_LABELS
        .iter()
        .any(|label| lower.contains(label))
    {
        return true;
    }
    if lower.contains("implemented and verified")
        || lower.contains("built and installed")
        || lower.contains("added regression coverage")
        || lower.contains("pushed to ")
    {
        return true;
    }
    response.lines().any(|line| {
        has_commandish_backticks(line)
            || has_code_path(line)
            || contains_commit_hash(line)
            || line.trim_start().starts_with("- `")
    })
}

fn has_commandish_backticks(line: &str) -> bool {
    if !line.contains('`') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("cargo ")
        || lower.contains("git ")
        || lower.contains("make ")
        || lower.contains("npm ")
        || lower.contains("pnpm ")
        || lower.contains("yarn ")
        || lower.contains("pytest")
        || lower.contains("uv run")
        || lower.contains("agent-doc ")
        || line.contains('/')
}

fn has_code_path(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains("src/")
        || line.contains("tests/")
        || line.contains("specs/")
        || line.contains("runbooks/")
        || lower.contains(".rs")
        || lower.contains(".md")
        || lower.contains(".toml")
        || lower.contains(".json")
        || lower.contains(".sh")
        || lower.contains(".kt")
        || lower.contains(".ts")
}

fn contains_commit_hash(line: &str) -> bool {
    let mut run = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            if run >= 7 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

fn truncate_signal(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let mut cut = max;
        while cut > 0 && !value.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}...", &value[..cut])
    }
}

/// Maximum number of `❯ `-prefix lines a single normalization cycle may add.
///
/// A legitimate user input rarely produces more than a few dozen prefixed lines
/// in one write cycle. When this threshold is exceeded, it indicates snapshot/
/// baseline divergence (stale baseline, boundary misalignment, or snapshot
/// reset) rather than genuine user input — applying the prefix would corrupt
/// the file at scale. See `normalize_user_prompts_in_exchange_safe`.
pub const MAX_NORMALIZE_USER_LINES: usize = 50;

/// Safe wrapper around [`normalize_user_prompts_in_exchange`] that adds:
///
/// 1. **Forensic logging** — every call writes `normalize_user_prompts`
///    metrics (`snap_len`, `base_len`, `applied`) to `ops.log` so divergence
///    incidents can be caught in the wild.
/// 2. **Safety rail** — if more than [`MAX_NORMALIZE_USER_LINES`] prefixes
///    would be applied, the normalization is discarded (content passes
///    through unchanged) and an event is logged.
/// 3. **Auto-commit recovery** — on overrun, `git::commit(file)` is invoked
///    to absorb the current working-tree state into the snapshot, giving
///    the next cycle a clean baseline to diff against.
///
/// This is the call-site-facing entry point for the write path. Tests and
/// callers that need the pure normalization behavior should continue to
/// use [`normalize_user_prompts_in_exchange`].
pub fn normalize_user_prompts_in_exchange_safe(
    content: &str,
    baseline: &str,
    snapshot: &str,
    file: &std::path::Path,
) -> String {
    let normalized = normalize_user_prompts_in_exchange(content, baseline, snapshot);

    // Count `❯ ` prefixes before/after to measure how many lines this call applied.
    // Note: also count a prefix at offset 0 (no leading newline).
    fn count_prefixes(s: &str) -> usize {
        let mut n = s.matches("\n❯ ").count();
        if s.starts_with("❯ ") {
            n += 1;
        }
        n
    }
    let before = count_prefixes(content);
    let after = count_prefixes(&normalized);
    let applied = after.saturating_sub(before);

    crate::ops_log::log_op(
        file,
        &format!(
            "normalize_user_prompts snap_len={} base_len={} applied={}",
            snapshot.len(),
            baseline.len(),
            applied
        ),
    );

    if applied > MAX_NORMALIZE_USER_LINES {
        eprintln!(
            "[normalize] WARN: {} ❯-prefixes would be applied, exceeds threshold {} for {} — \
             suspected snapshot/baseline divergence. Force-committing current file to absorb drift; \
             skipping ❯ prefix application this cycle.",
            applied,
            MAX_NORMALIZE_USER_LINES,
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "normalize_threshold_exceeded applied={} threshold={} action=force_commit_and_passthrough",
                applied, MAX_NORMALIZE_USER_LINES
            ),
        );
        if let Err(e) = crate::git::commit(file) {
            eprintln!("[normalize] WARN: force-commit failed: {}", e);
        }
        return content.to_string();
    }

    normalized
}

/// Verify that the sidecar content preserved the expected `❯ ` prefixes.
///
/// For each non-blank target in `normalize_prefix_lines`, checks whether a
/// `trimEnd()`-matched prefixed line (`❯ <target>`) exists in the sidecar.
/// Returns `true` when all expected prefixes are present (or when there are
/// no targets to check).
pub fn verify_sidecar_normalization(sidecar: &str, normalize_prefix_lines: &[String]) -> bool {
    if normalize_prefix_lines.is_empty() {
        return true;
    }

    let sidecar_lines: Vec<&str> = sidecar.lines().collect();
    for target in normalize_prefix_lines {
        let trimmed = target.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let expected = format!("❯ {}", trimmed);
        if !sidecar_lines.iter().any(|l| l.trim_end() == expected) {
            return false;
        }
    }
    true
}

fn enforce_orchestrate_template_patch_contract(
    origin: Option<&str>,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
) -> Result<()> {
    if origin != Some("orchestrate") {
        return Ok(());
    }

    if !patches.iter().any(|patch| patch.name == "exchange") {
        anyhow::bail!(
            "orchestrate template-mode responses must include a <!-- patch:exchange --> block"
        );
    }
    if !unmatched.trim().is_empty() {
        anyhow::bail!(
            "orchestrate template-mode responses must not include raw unmatched content outside patch blocks"
        );
    }
    Ok(())
}

/// Lift `agent:pending` out of `agent:exchange` if nested.
///
/// After patch application, pending may end up nested inside exchange due to
/// boundary synthesis or CRDT merge artifacts. This detects the nesting and
/// moves the entire pending block (open tag through close tag) to after
/// exchange's close tag.
pub fn lift_pending_from_exchange(content: &str) -> Option<String> {
    let components = match crate::component::parse(content) {
        Ok(c) => c,
        Err(_) => return None,
    };
    let exchange = components.iter().find(|c| c.name == "exchange")?;
    let pending = components.iter().find(|c| is_backlog_component(&c.name))?;

    if pending.open_start >= exchange.close_end {
        return None; // already a sibling — no repair needed
    }
    if pending.open_start < exchange.open_end {
        return None; // pending is before exchange, not nested
    }

    // pending is nested inside exchange — lift it out
    let pending_block = &content[pending.open_start..pending.close_end];
    let mut result = String::with_capacity(content.len() + 4);
    // Everything before pending (still inside exchange)
    result.push_str(&content[..pending.open_start]);
    // Skip pending block, continue to exchange close
    result.push_str(&content[pending.close_end..exchange.close_end]);
    // Insert pending as sibling after exchange close
    result.push('\n');
    result.push_str(pending_block);
    // Rest of document after exchange close
    result.push_str(&content[exchange.close_end..]);
    Some(result)
}

pub fn lift_pending_from_exchange_safe(content: &str, file: &std::path::Path) -> String {
    match lift_pending_from_exchange(content) {
        Some(repaired) => {
            eprintln!(
                "[write] repaired: lifted agent:pending out of agent:exchange for {}",
                file.display()
            );
            crate::ops_log::log_op(
                file,
                &format!("lift_pending_from_exchange file={}", file.display()),
            );
            repaired
        }
        None => content.to_string(),
    }
}

pub(crate) fn normalize_template_structure_or_fail(content: &str, file: &Path) -> Result<String> {
    let lifted = lift_pending_from_exchange_safe(content, file);
    let normalized = crate::component::strip_backlog_patch_attr(&lifted);
    match crate::template::repair_conversation_tail_outside_exchange(&normalized)? {
        Some(repaired) => {
            eprintln!(
                "[write] repaired: moved escaped conversation tail back into agent:exchange for {}",
                file.display()
            );
            crate::ops_log::log_op(
                file,
                &format!("repair_exchange_tail file={}", file.display()),
            );
            Ok(repaired)
        }
        None => Ok(normalized),
    }
}

/// Detect whether a baseline is stale relative to the current snapshot.
///
/// Only checks **append-mode** components (exchange, findings, etc.) — these grow
/// monotonically and must contain the snapshot's committed content. Replace-mode
/// components (status, pending) are freely user-editable and are skipped.
///
/// Returns `true` if the baseline is stale (missing committed snapshot content).
pub fn is_stale_baseline(baseline: &str, snapshot: &str) -> bool {
    let base_clean = strip_boundary_for_dedup(baseline);
    let snap_clean = strip_boundary_for_dedup(snapshot);

    // Fast path: identical content
    if base_clean == snap_clean {
        return false;
    }

    // Try structural comparison via components
    if let (Ok(snap_components), Ok(base_components)) =
        (component::parse(snapshot), component::parse(baseline))
        && !snap_components.is_empty()
    {
        // Only check append-mode components — these grow monotonically and must
        // contain the snapshot's committed content. Replace-mode components
        // (status, pending) are user-editable and should be skipped.
        for snap_comp in &snap_components {
            let is_append = snap_comp
                .patch_mode()
                .map(|m| m == "append")
                .unwrap_or(is_append_mode_component(&snap_comp.name));
            if !is_append {
                continue;
            }
            let snap_content = strip_boundary_for_dedup(snap_comp.content(snapshot).trim());
            if snap_content.is_empty() {
                continue;
            }
            // Find matching component in baseline by name
            if let Some(base_comp) = base_components.iter().find(|c| c.name == snap_comp.name) {
                let base_content = strip_boundary_for_dedup(base_comp.content(baseline).trim());
                // Baseline's append component must contain the snapshot's content
                if !base_content.contains(&snap_content) {
                    return true;
                }
            } else {
                // Snapshot has an append component that baseline lacks entirely
                return true;
            }
        }
        return false;
    }

    // Fallback for non-template docs: prefix check (original behavior)
    !base_clean.starts_with(&snap_clean)
}

/// Strip boundary markers for dedup comparison.
/// Boundary markers (`<!-- agent:boundary:XXXXXXXX -->`) get a fresh ID on each write,
/// so they must be excluded from content equality checks.
fn strip_boundary_for_dedup(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Minimum byte count for exchange content before the shrink guard triggers.
/// Below this threshold the exchange is too small to be worth protecting.
const SHRINK_GUARD_MIN_BYTES: usize = 100;

/// Maximum ratio (new / old) that the shrink guard allows without `--force`.
/// If the new exchange content is less than this fraction of the old, refuse.
const SHRINK_GUARD_MAX_RATIO: f64 = 0.10;

/// Guard against accidental exchange content truncation.
///
/// Compares the exchange component content in the current file against the
/// proposed content. If the existing exchange is substantial (>100 bytes) and
/// the new exchange is <10% of the old, refuse the write. Returns `Ok(())` if
/// the write should proceed, or an error message if it should be refused.
fn check_exchange_shrink_guard(
    content_at_start: &str,
    content_ours: &str,
    file: &Path,
) -> Result<()> {
    let old_exchange_len = extract_exchange_content_len(content_at_start);
    let new_exchange_len = extract_exchange_content_len(content_ours);

    if old_exchange_len < SHRINK_GUARD_MIN_BYTES {
        return Ok(());
    }

    let ratio = new_exchange_len as f64 / old_exchange_len as f64;
    if ratio < SHRINK_GUARD_MAX_RATIO {
        crate::ops_log::log_op(
            file,
            &format!(
                "shrink_guard_blocked file={} old_len={} new_len={} ratio={:.3}",
                file.display(),
                old_exchange_len,
                new_exchange_len,
                ratio
            ),
        );
        anyhow::bail!(
            "exchange content would shrink from {} to {} bytes ({:.0}% of original) — \
             refusing write to prevent accidental truncation. If this is intentional, \
             use `agent-doc compact` or re-run with meaningful content.",
            old_exchange_len,
            new_exchange_len,
            ratio * 100.0
        );
    }

    Ok(())
}

/// Extract the byte length of the exchange component's content.
/// Returns 0 if no exchange component is found.
fn extract_exchange_content_len(doc: &str) -> usize {
    if let Ok(components) = component::parse(doc) {
        components
            .iter()
            .find(|c| c.name == "exchange")
            .map(|c| c.content(doc).trim().len())
            .unwrap_or(0)
    } else {
        0
    }
}

/// Log a write dedup event to both stderr and a persistent file for diagnosis.
fn log_dedup(file: &Path, context: &str) {
    let msg = format!("[write] dedup: {} — {}", file.display(), context);
    eprintln!("{}", msg);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/agent-doc-write-dedup.log")
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bt = std::backtrace::Backtrace::force_capture();
        let _ = writeln!(f, "[{}] {} backtrace:\n{}", ts, msg, bt);
    }
}

/// Verify the current tmux pane owns the session for this document.
///
/// Returns `Ok(())` when the check passes or cannot be performed (not in tmux,
/// no session ID, session not registered, pane indeterminate). Returns `Err`
/// only when a *different* pane definitively owns the session.
fn verify_pane_ownership(file: &Path) -> Result<()> {
    if !sessions::in_tmux() {
        return Ok(());
    }
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let session_id = match frontmatter::parse(&content) {
        Ok((fm, _)) => match fm.session {
            Some(s) => s,
            None => return Ok(()),
        },
        Err(_) => return Ok(()),
    };
    let entry = match sessions::lookup_entry(&session_id) {
        Ok(Some(e)) => e,
        _ => return Ok(()),
    };
    let current = match sessions::current_pane() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    if entry.pane != current {
        anyhow::bail!(
            "pane ownership mismatch: session {} owned by pane {}, current pane is {}. \
             Use `agent-doc claim` to reclaim.",
            session_id,
            entry.pane,
            current
        );
    }
    Ok(())
}

/// Run the write command: append assistant response to document.
///
/// `baseline` is the document content at the time the response was generated.
/// If omitted, the current document content is used (no merge needed).
pub fn run(file: &Path, baseline: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;

    // Strip leading "## Assistant" heading if present — the write command adds its own
    let response = strip_assistant_heading(&response);

    // Save response to pending store (survives context compaction)
    repair::save_pending(file, &response)?;

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let doc_lock = acquire_doc_lock(file)?;

    // Read document state under lock
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Save pre-response snapshot for undo
    snapshot::save_pre_response(file, base)?;

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

    // Re-read file to check for user edits since lock acquisition
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

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

    // Save snapshot BEFORE document write (#wcf5): external watchers (IDE
    // file-change listeners, git hooks) trigger on the document rename and may
    // compute diffs immediately.  Writing the snapshot first guarantees they
    // always read a current baseline instead of racing against a stale one.
    snapshot::save(file, &final_content)?;

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

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &current_content);

    // Parse patch blocks from response
    let (mut patches, unmatched) =
        template::parse_patches(&response).context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

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
    enforce_orchestrate_template_patch_contract(origin, &patches, &unmatched)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    // Save response to pending store (survives context compaction)
    repair::save_pending(file, &response)?;

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let doc_lock = acquire_doc_lock(file)?;

    // Read document state under lock
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Save pre-response snapshot for undo
    snapshot::save_pre_response(file, base)?;

    // Apply patches to baseline
    let content_ours =
        template::apply_patches_with_overrides(base, &patches, &unmatched, file, &mode_overrides)
            .context("failed to apply template patches")?;
    let content_ours = normalize_template_structure_or_fail(&content_ours, file)?;

    // Re-read file to check for user edits since lock acquisition
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let final_content = if content_current == base {
        content_ours.clone()
    } else {
        eprintln!("[write] File was modified during response generation. Merging...");
        merge::merge_contents(base, &content_ours, &content_current)?
    };
    let final_content = normalize_template_structure_or_fail(&final_content, file)?;

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

    // Save snapshot BEFORE document write (#wcf5): external watchers (IDE
    // file-change listeners, git hooks) trigger on the document rename and may
    // compute diffs immediately.  Writing the snapshot first guarantees they
    // always read a current baseline instead of racing against a stale one.
    snapshot::save(file, &final_content)?;

    atomic_write(file, &final_content)?;

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
/// tries IPC first. On IPC timeout, leaves the patch file in place and exits
/// with code 75 (EX_TEMPFAIL) instead of falling back to disk write.
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

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &current_content);

    // Lint: warn if response contains future-work signals without --pending-add
    check_future_work_signals(&response, flags.has_pending_add);

    // Parse patch blocks from response
    let (mut patches, unmatched) =
        template::parse_patches(&response).context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

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
    enforce_orchestrate_template_patch_contract(origin, &patches, &unmatched)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    if patches.is_empty() {
        eprintln!(
            "[write] WARNING: 0 template patches found — response may be missing or malformed. \
             Only normalization/boundary changes will be applied."
        );
        crate::ops_log::log_op(
            file,
            "zero_patches_warning: response may be empty or malformed",
        );
    }

    // Save response to pending store (survives context compaction)
    repair::save_pending(file, &response)?;

    // Warn when patches target a file with no template components
    if patches.is_empty() && !unmatched.trim().is_empty() {
        let current = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let comps = crate::component::parse(&current).unwrap_or_default();
        if comps.is_empty() {
            eprintln!(
                "[write] WARNING: {} bytes of content but file has no template components — \
                 content may not be applied correctly. Consider running `agent-doc init` \
                 with --mode template first.",
                unmatched.trim().len()
            );
        }
    }

    // Save pre-response snapshot for undo (before IPC or disk write)
    {
        let pre_content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {} for pre-response", file.display()))?;
        snapshot::save_pre_response(file, &pre_content)?;
    }

    // Try IPC when plugin is installed and --force-disk is not set
    if !force_disk {
        let canonical = file.canonicalize()?;
        let project_root = resolve_ipc_project_root(&canonical);
        let patches_dir = project_root.join(".agent-doc/patches");

        if patches_dir.exists() {
            // Compute content_ours (baseline + patches) for snapshot saving.
            // The IPC path sends patches to the plugin but we need a clean snapshot
            // that represents baseline+response WITHOUT user's concurrent edits.
            let content_at_start = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let base = baseline.unwrap_or(&content_at_start);
            let t_apply = std::time::Instant::now();
            let mut content_ours = template::apply_patches_with_overrides(
                base,
                &patches,
                &unmatched,
                file,
                &mode_overrides,
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
                content_ours = template::apply_patches_with_overrides(
                    &content_at_start,
                    &patches,
                    &unmatched,
                    file,
                    &mode_overrides,
                )
                .context("failed to apply patches with fresh baseline")?;
            }

            // Normalize user input in exchange: add ❯  prefix to user-added lines.
            // Uses the snapshot (loaded above) to identify new lines.
            // Compute normalization targets for the IPC plugin so the editor also shows
            // the prefix immediately (not just the snapshot).
            let normalize_prefix_lines: Vec<String> =
                if let Ok(Some(ref snap)) = snapshot::load(file) {
                    let before = content_ours.clone();
                    content_ours =
                        normalize_user_prompts_in_exchange_safe(&content_ours, base, snap, file);
                    extract_normalization_targets(&before, &content_ours)
                } else {
                    vec![]
                };

            // Lift pending out of exchange if nested (structural repair)
            content_ours = lift_pending_from_exchange_safe(&content_ours, file);
            content_ours = normalize_template_structure_or_fail(&content_ours, file)?;

            // Shrink guard: refuse if new exchange content is dramatically shorter
            check_exchange_shrink_guard(&content_at_start, &content_ours, file)?;

            // Dedup: skip IPC if patches produce no changes (strip boundary markers)
            if strip_boundary_for_dedup(&content_ours)
                == strip_boundary_for_dedup(&content_at_start)
            {
                log_dedup(file, "no changes after merge, skipping write");
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
                baseline,
                Some(&content_ours),
                norm_lines_opt,
                None,
            )?;
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
                repair::clear_pending(file)?;
                return Ok(());
            }
            // IPC timeout — patch file was already cleaned up by try_ipc,
            // but we want to leave a NEW patch file in place for the plugin
            // to pick up later. Re-write it with the SAME patch_id so the
            // plugin can deduplicate if the original IPC delivery was late.
            let hash = snapshot::doc_hash(file)?;
            let patch_file = patches_dir.join(format!("{}.json", hash));

            // Use shared helper for synthesis (same boundary-aware logic as try_ipc)
            let norm_lines_for_timeout = if normalize_prefix_lines.is_empty() {
                None
            } else {
                Some(normalize_prefix_lines.as_slice())
            };
            let ipc_patches =
                build_ipc_patches_json(file, &patches, &unmatched, norm_lines_for_timeout)?;

            // Same dedup guard as try_ipc: don't send unmatched when it was synthesized into a patch.
            let effective_unmatched = if patches.is_empty() && !ipc_patches.is_empty() {
                ""
            } else {
                unmatched.trim()
            };

            // Reuse the patch_id from try_ipc so the plugin deduplicates
            // if the original socket/file delivery was applied late.
            let mut ipc_payload = serde_json::json!({
                "file": canonical.to_string_lossy(),
                "patches": ipc_patches,
                "unmatched": effective_unmatched,
                "baseline": baseline.unwrap_or(""),
                "reposition_boundary": true,
            });
            ipc_payload["patch_id"] = serde_json::Value::String(ipc_result.patch_id.clone());

            // Include normalize_prefix_lines so a later plugin pickup restores
            // the `❯ ` prefixes in the buffer (matches the primary IPC payload).
            // Without this the plugin would only apply component patches and
            // the working tree would diverge from the snapshot.
            if let Some(lines) = norm_lines_for_timeout
                && !lines.is_empty()
            {
                ipc_payload["normalize_prefix_lines"] = serde_json::Value::Array(
                    lines
                        .iter()
                        .map(|l| serde_json::Value::String(l.clone()))
                        .collect(),
                );
            }

            // Include frontmatter if present
            let frontmatter_yaml: Option<String> = patches
                .iter()
                .find(|p| p.name == "frontmatter")
                .map(|p| p.content.trim().to_string());
            if let Some(ref yaml) = frontmatter_yaml {
                ipc_payload["frontmatter"] = serde_json::Value::String(yaml.clone());
            }

            atomic_write(&patch_file, &serde_json::to_string_pretty(&ipc_payload)?)?;

            eprintln!("[write] IPC timeout — response saved as patch, awaiting plugin");
            // CRDT merge on IPC timeout: content_ours (baseline + patches) may
            // diverge from the on-disk file (user edits, pending mutations from
            // main.rs). Use the same CRDT merge as the normal disk path to
            // preserve all concurrent changes.
            let content_current =
                std::fs::read_to_string(file).unwrap_or_else(|_| content_at_start.clone());
            let (final_content, crdt_state) = if content_current == base {
                let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
                (content_ours.clone(), doc.encode_state())
            } else if response_already_in_current(base, &content_ours, &content_current) {
                // Plugin already applied the response before the sidecar ack
                // arrived. Using content_current preserves user edits and avoids
                // duplicating the response via CRDT merge.
                eprintln!(
                    "[write] IPC timeout path: response already in current file (plugin applied), skipping CRDT merge"
                );
                crate::ops_log::log_op(
                    file,
                    "ipc_timeout_plugin_already_applied: skipping CRDT merge",
                );
                let doc = crate::crdt::CrdtDoc::from_text(&content_current);
                (content_current.clone(), doc.encode_state())
            } else {
                eprintln!("[write] IPC timeout path: file modified, CRDT merging...");
                let base_state = crate::crdt::CrdtDoc::from_text(base).encode_state();
                match merge::merge_contents_crdt(Some(&base_state), &content_ours, &content_current)
                {
                    Ok(merged) => merged,
                    Err(e) => {
                        eprintln!(
                            "[write] WARNING: CRDT merge failed on exit(75), falling back to splice: {}",
                            e
                        );
                        let spliced = splice_pending_component(&content_ours, &content_current);
                        let doc = crate::crdt::CrdtDoc::from_text(&spliced);
                        (spliced, doc.encode_state())
                    }
                }
            };
            let final_content = normalize_template_structure_or_fail(&final_content, file)?;
            // Snapshot saved BEFORE document write (#wcf5).
            if let Err(e) = snapshot::save(file, &final_content) {
                eprintln!(
                    "[write] WARNING: snapshot save before exit(75) failed: {}",
                    e
                );
            }
            if let Err(e) = snapshot::save_crdt(file, &crdt_state) {
                eprintln!(
                    "[write] WARNING: CRDT state save before exit(75) failed: {}",
                    e
                );
            }
            if let Err(e) = atomic_write(file, &final_content) {
                eprintln!(
                    "[write] WARNING: failed to write to working tree before exit(75): {}",
                    e
                );
            }
            if crate::git::is_in_git_repo(file)
                && let Err(e) = crate::git::commit(file)
            {
                eprintln!("[commit] warning: commit before exit(75) failed: {}", e);
            }
            std::process::exit(75); // EX_TEMPFAIL
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
                    let claimed_dir = project_root.join(".agent-doc/claimed-patches");
                    match std::fs::create_dir_all(&claimed_dir) {
                        Err(e) => {
                            eprintln!("[write] WARNING: failed to create claimed-patches dir: {e}")
                        }
                        Ok(_) => {
                            let sentinel = claimed_dir.join(patch_id);
                            if let Err(e) = std::fs::write(&sentinel, "") {
                                eprintln!("[write] WARNING: failed to write patch sentinel: {e}");
                            } else {
                                eprintln!(
                                    "[write] patch_id {} claimed (sentinel written)",
                                    &patch_id[..8]
                                );
                            }
                        }
                    }
                }
                let _ = std::fs::remove_file(&patch_file);
            }
        }
    }
    let t_disk = std::time::Instant::now();

    // Acquire advisory lock BEFORE reading document state.
    // Closing the window between content_at_start read and lock acquire
    // prevents concurrent agent-doc writes from drifting the baseline. (#08yv)
    let doc_lock = acquire_doc_lock(file)?;

    // Read document state under lock
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Apply patches using the mode resolution chain:
    // inline attr (patch=append on tag) > config.toml ([components] section) > built-in default.
    // The skill sends delta content for append-mode components.
    let t_apply2 = std::time::Instant::now();
    let mut content_ours =
        template::apply_patches_with_overrides(base, &patches, &unmatched, file, &mode_overrides)
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
    if let Ok(Some(snap)) = snapshot::load(file) {
        content_ours = normalize_user_prompts_in_exchange_safe(&content_ours, base, &snap, file);
    }

    // Lift pending out of exchange if nested (structural repair)
    content_ours = lift_pending_from_exchange_safe(&content_ours, file);
    content_ours = normalize_template_structure_or_fail(&content_ours, file)?;

    // Shrink guard: refuse if new exchange content is dramatically shorter
    check_exchange_shrink_guard(&content_at_start, &content_ours, file)?;

    // Re-read file to check for user edits since lock acquisition
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let (final_content, crdt_state) = if content_current == base {
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
        let base_state = crate::crdt::CrdtDoc::from_text(base).encode_state();
        // Agent=client_id(2) gives native correct ordering — no skip_reorder needed.
        match merge::merge_contents_crdt(Some(&base_state), &content_ours, &content_current) {
            Ok(merged) => merged,
            Err(e) => {
                eprintln!(
                    "[write] WARNING: CRDT merge failed in stream write, falling back to splice: {}",
                    e
                );
                let spliced = splice_pending_component(&content_ours, &content_current);
                let doc = crate::crdt::CrdtDoc::from_text(&spliced);
                (spliced, doc.encode_state())
            }
        }
    };
    let final_content = normalize_template_structure_or_fail(&final_content, file)?;

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

    // Save snapshot BEFORE document write (#wcf5): external watchers (IDE
    // file-change listeners, git hooks) trigger on the document rename and may
    // compute diffs immediately.  Writing the snapshot first guarantees they
    // always read a current baseline instead of racing against a stale one.
    snapshot::save(file, &final_content)?;
    // Save the merged CRDT state — NOT a fresh state from content_ours.
    // Using content_ours would lose user edits from the merge, causing
    // the next merge cycle to re-insert them as duplicates.
    snapshot::save_crdt(file, &crdt_state)?;

    atomic_write(file, &final_content)?;
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
/// the file as ACK. Falls back to direct stream write on timeout.
pub fn run_ipc(file: &Path, baseline: Option<&str>, flags: WriteFlags) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    enforce_imperative_response_contract(file, baseline, &current_content, &response)?;

    // Save response to pending store (survives context compaction)
    // Parse patch blocks from response
    let (mut patches, unmatched) =
        template::parse_patches(&response).context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

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

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    // Build IPC patch file
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = resolve_ipc_project_root(&canonical);
    let patches_dir = project_root.join(".agent-doc/patches");
    std::fs::create_dir_all(&patches_dir)?;
    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Use shared helper for boundary-aware synthesis (matches try_ipc socket + file paths)
    let ipc_patches = build_ipc_patches_json(file, &patches, &unmatched, None)?;

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

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "unmatched": effective_unmatched,
        "baseline": baseline.unwrap_or(""),
    });

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

    while start.elapsed() < timeout {
        if !patch_file.exists() {
            // Plugin consumed the patch — update snapshot from current file
            let content = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {} after IPC", file.display()))?;
            snapshot::save(file, &content)?;
            crate::ops_log::log_op(
                file,
                &format!(
                    "snapshot_saved_file_ipc file={} snap_len={}",
                    file.display(),
                    content.len()
                ),
            );
            let crdt_doc = crate::crdt::CrdtDoc::from_text(&content);
            snapshot::save_crdt(file, &crdt_doc.encode_state())?;
            repair::clear_pending(file)?;
            eprintln!("[write] IPC patch consumed by plugin — snapshot updated");
            return Ok(());
        }
        std::thread::sleep(poll_interval);
    }

    // Timeout — fall back to direct stream write
    eprintln!(
        "[write] IPC timeout ({}s) — falling back to direct write",
        timeout.as_secs()
    );
    // Clean up the unconsumed patch file
    let _ = std::fs::remove_file(&patch_file);

    // Fall back to stream write logic
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let base = baseline.unwrap_or(&content_at_start);
    let mode_overrides = template_mode_overrides_for_current_doc(file, baseline, &content_at_start);
    let mut content_ours =
        template::apply_patches_with_overrides(base, &patches, &unmatched, file, &mode_overrides)
            .context("failed to apply template patches")?;
    content_ours = normalize_template_structure_or_fail(&content_ours, file)?;

    // Apply frontmatter patch if present
    if let Some(ref yaml) = frontmatter_yaml {
        content_ours = crate::frontmatter::merge_fields(&content_ours, yaml)
            .context("failed to apply frontmatter patch")?;
    }
    let doc_lock = acquire_doc_lock(file)?;
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;
    let (final_content, crdt_state) = if content_current == base {
        let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
        (content_ours.clone(), doc.encode_state())
    } else {
        eprintln!("[write] File was modified during response generation. CRDT merging...");
        let crdt_state = snapshot::load_crdt(file)?;
        match merge::merge_contents_crdt(crdt_state.as_deref(), &content_ours, &content_current) {
            Ok(merged) => merged,
            Err(e) => {
                eprintln!(
                    "[write] WARNING: CRDT merge failed in IPC fallback, falling back to splice: {}",
                    e
                );
                let spliced = splice_pending_component(&content_ours, &content_current);
                let doc = crate::crdt::CrdtDoc::from_text(&spliced);
                (spliced, doc.encode_state())
            }
        }
    };
    let final_content = normalize_template_structure_or_fail(&final_content, file)?;
    atomic_write(file, &final_content)?;
    snapshot::save(file, &final_content)?;
    snapshot::save_crdt(file, &crdt_state)?;
    drop(doc_lock);
    repair::clear_pending(file)?;
    eprintln!(
        "[write] Stream patches applied to {} ({} components patched, CRDT fallback)",
        file.display(),
        patches.len()
    );
    Ok(())
}

/// Apply an append-mode response from a string (not stdin).
/// Used by `repair` to apply orphaned responses.
pub fn apply_append_from_string(file: &Path, response: &str) -> Result<()> {
    let response = strip_assistant_heading(response);
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

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

    let final_content = if content_current == content {
        content_ours.clone()
    } else {
        merge::merge_contents(&content, &content_ours, &content_current)?
    };

    atomic_write(file, &final_content)?;
    // Save snapshot as content_ours, not final_content
    snapshot::save(file, &content_ours)?;
    drop(doc_lock);
    eprintln!("[write] Response appended to {}", file.display());
    Ok(())
}

/// Apply template-mode patches from a string (not stdin).
/// Used by `repair` to apply orphaned template responses.
pub fn apply_template_from_string(file: &Path, response: &str) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (mut patches, unmatched) =
        template::parse_patches(response).context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

    let normalized = normalize_backlog_patch_response(file, &content, patches, unmatched, false)?;
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;

    // Enforcement: reject replace:pending (and deprecated patch:pending) blocks unless allowed.
    enforce_no_replace_pending(&patches, false)?;

    let mode_overrides = template_mode_overrides_for_current_doc(file, None, &content);
    let content_ours = template::apply_patches_with_overrides(
        &content,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
    )
    .context("failed to apply template patches")?;
    let content_ours = normalize_template_structure_or_fail(&content_ours, file)?;

    let doc_lock = acquire_doc_lock(file)?;

    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let final_content = if content_current == content {
        content_ours.clone()
    } else {
        merge::merge_contents(&content, &content_ours, &content_current)?
    };
    let final_content = normalize_template_structure_or_fail(&final_content, file)?;

    atomic_write(file, &final_content)?;
    // Save snapshot as the repaired/merged final content.
    snapshot::save(file, &final_content)?;
    drop(doc_lock);
    eprintln!("[write] Template patches applied to {}", file.display());
    Ok(())
}

/// Read the ack-content sidecar file written by the plugin after apply.
/// Keyed by `patch_id` (same UUID the binary embedded in the patch payload).
/// Deletes the sidecar on success. Returns None if no sidecar present (old plugin).
fn read_ack_content_sidecar(project_root: &Path, patch_id: &str) -> Result<Option<String>> {
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

/// Remove any stale ipc-degraded marker left by older versions.
fn cleanup_legacy_ipc_degraded(project_root: &Path) {
    let marker = project_root.join(".agent-doc/ipc-degraded");
    if marker.exists() {
        let _ = std::fs::remove_file(&marker);
    }
}

/// Poll for the ack-content sidecar with timeout.
///
/// The plugin writes the sidecar asynchronously after applying the patch.
/// Polling eliminates the old 200ms sleep race — we get the authoritative
/// post-apply content as soon as the plugin writes it, or fall back to
/// file read only after the timeout expires.
fn poll_ack_content_sidecar(
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

/// Result of an IPC write attempt, including the patch_id used.
///
/// The `patch_id` is returned so callers (e.g., `run_stream()` timeout fallback)
/// can reuse it for deduplication — the plugin tracks applied patch_ids and skips
/// duplicates, preventing double-apply when both socket and file IPC fire.
pub struct IpcResult {
    /// Whether the plugin successfully consumed the patch.
    pub success: bool,
    /// The patch_id used for this write attempt. Reuse in fallback writes
    /// so the plugin can deduplicate.
    pub patch_id: String,
}

/// Attempt to write via IPC (socket-first, file-based fallback).
///
/// First tries socket IPC via `ipc_socket::send_message()` for lowest latency.
/// Falls back to file-based IPC (JSON patch in `.agent-doc/patches/`) if socket
/// is unavailable. Returns `IpcResult` with success flag and the patch_id used.
///
/// When `reuse_patch_id` is provided, that ID is used instead of generating a new
/// one. This ensures the plugin can deduplicate when the same logical write is
/// retried via the timeout fallback path.
#[allow(clippy::too_many_arguments)]
pub fn try_ipc(
    file: &Path,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
    frontmatter_yaml: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    reuse_patch_id: Option<&str>,
) -> Result<IpcResult> {
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = resolve_ipc_project_root(&canonical);

    // Single patch_id for all IPC paths (socket, file, caller's fallback).
    // Reusing the same ID allows the plugin to deduplicate when both socket
    // and file delivery fire for the same logical write.
    let patch_id = reuse_patch_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Clean up any legacy degraded marker from older versions
    cleanup_legacy_ipc_degraded(&project_root);

    // Try socket IPC first (lower latency, no inotify)
    if crate::ipc_socket::is_listener_active(&project_root) {
        let ipc_patches_json =
            build_ipc_patches_json(file, patches, unmatched, normalize_prefix_lines)?;
        // When unmatched content was synthesized into a patch (no explicit patch blocks),
        // don't also send it as "unmatched" — the plugin would apply both and duplicate.
        let effective_unmatched_socket = if patches.is_empty() && !ipc_patches_json.is_empty() {
            eprintln!(
                "[write] synthesis consumed unmatched content — clearing from socket payload (prevent double-apply)"
            );
            ""
        } else {
            unmatched.trim()
        };
        let mut socket_payload = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": ipc_patches_json,
            "unmatched": effective_unmatched_socket,
            "baseline": baseline.unwrap_or(""),
            "reposition_boundary": true,
        });
        socket_payload["patch_id"] = serde_json::Value::String(patch_id.clone());
        if let Some(yaml) = frontmatter_yaml {
            socket_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
        }
        if let Some(lines) = normalize_prefix_lines
            && !lines.is_empty()
        {
            socket_payload["normalize_prefix_lines"] = serde_json::Value::Array(
                lines
                    .iter()
                    .map(|l| serde_json::Value::String(l.clone()))
                    .collect(),
            );
            // Include full normalized content ONLY when there are no component patches.
            // When patches are present, the plugin applies normalize_prefix_lines before
            // component patches — fullContent would conflict by replacing the document
            // before patches run, causing duplicates on the next cycle.
            // fullContent is only safe as a fallback for append-mode (no-component) docs.
            if ipc_patches_json.is_empty()
                && let Some(ours) = content_ours
            {
                socket_payload["fullContent"] = serde_json::Value::String(ours.to_string());
            }
        }
        // Pre-write fallback patch file before socket send. If socket delivery
        // succeeds but sidecar ack times out, the file watcher can recover the
        // response from this file. patch_id dedup prevents double-apply when
        // both socket and file watcher fire. Overwrites any stale content.
        let fallback_patch_file = {
            let patches_dir = project_root.join(".agent-doc/patches");
            if patches_dir.exists() {
                let path = patches_dir.join(format!("{}.json", hash));
                match serde_json::to_string_pretty(&socket_payload) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(&path, &json) {
                            eprintln!(
                                "[write] WARNING: failed to write fallback patch file: {}",
                                e
                            );
                            None
                        } else {
                            eprintln!("[write] fallback patch file pre-written for recovery");
                            Some(path)
                        }
                    }
                    Err(e) => {
                        eprintln!("[write] WARNING: failed to serialize fallback patch: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        };
        match crate::ipc_socket::send_message(&project_root, &socket_payload) {
            Ok(Some(_ack)) => {
                eprintln!("[write] socket IPC patch delivered");
                // Poll for ack-content sidecar (written by plugin after apply).
                let sidecar = poll_ack_content_sidecar(
                    &project_root,
                    &patch_id,
                    std::time::Duration::from_millis(200),
                    std::time::Duration::from_millis(25),
                )?;
                if let Some(snap_content) = sidecar {
                    // Verify sidecar preserved normalize_prefix_lines targets.
                    // If the plugin's normalization diverged, prefer content_ours.
                    let (effective_snap, snap_source) = if let Some(lines) = normalize_prefix_lines
                        && !lines.is_empty()
                        && !verify_sidecar_normalization(&snap_content, lines)
                    {
                        if let Some(ours) = content_ours {
                            eprintln!(
                                "[write] sidecar normalization diverged — falling back to content_ours ({} bytes)",
                                ours.len()
                            );
                            crate::ops_log::log_op(
                                file,
                                &format!(
                                    "sidecar_normalization_fallback file={} snap_source=content_ours reason=prefix_divergence",
                                    file.display()
                                ),
                            );
                            (ours.to_string(), "content_ours")
                        } else {
                            eprintln!(
                                "[write] sidecar normalization diverged but no content_ours available — using sidecar"
                            );
                            (snap_content, "ack_content_sidecar")
                        }
                    } else {
                        (snap_content, "ack_content_sidecar")
                    };

                    eprintln!(
                        "[write] snapshot from {} ({} bytes)",
                        snap_source,
                        effective_snap.len()
                    );
                    if let Some(ref path) = fallback_patch_file {
                        let _ = std::fs::remove_file(path);
                    }
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "ipc_socket_delivered file={} snap_source={} snap_len={}",
                            file.display(),
                            snap_source,
                            effective_snap.len()
                        ),
                    );
                    if let Err(e) = snapshot::save(file, &effective_snap) {
                        eprintln!(
                            "[write] WARNING: IPC write succeeded but snapshot save failed: {}. \
                             Commit will auto-recover via divergence detection.",
                            e
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "snapshot_save_failed_after_ipc file={} error={}",
                                file.display(),
                                e
                            ),
                        );
                    } else {
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "snapshot_saved_socket_ipc file={} snap_len={}",
                                file.display(),
                                effective_snap.len()
                            ),
                        );
                        let crdt_doc = crate::crdt::CrdtDoc::from_text(&effective_snap);
                        if let Err(e) = snapshot::save_crdt(file, &crdt_doc.encode_state()) {
                            eprintln!("[write] WARNING: CRDT state save failed: {}", e);
                        }
                    }
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                    });
                }
                // Sidecar timed out — plugin likely applied the patch but the
                // ack write was slow. Fall through to disk write for a reliable
                // snapshot. No degradation — next write will still try socket.
                eprintln!(
                    "[write] sidecar ack timed out — socket delivery unconfirmed, falling back to disk write"
                );
                if fallback_patch_file.is_some() {
                    eprintln!("[write] fallback patch file left for file watcher recovery");
                }
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_sidecar_timeout file={} — falling back to disk write",
                        file.display()
                    ),
                );
            }
            Ok(None) => {
                eprintln!("[write] socket IPC sent but no ack — falling back to file IPC");
            }
            Err(e) => {
                eprintln!(
                    "[write] socket IPC failed: {} — falling back to file IPC",
                    e
                );
            }
        }
    }

    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt file-based IPC if the patches directory exists (plugin has started)
    if !patches_dir.exists() {
        return Ok(IpcResult {
            success: false,
            patch_id,
        });
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Build patches using shared helper (same logic as socket path)
    let ipc_patches = build_ipc_patches_json(file, patches, unmatched, normalize_prefix_lines)?;

    // Same dedup guard as socket path: don't send unmatched when it was synthesized into a patch.
    let effective_unmatched_file = if patches.is_empty() && !ipc_patches.is_empty() {
        ""
    } else {
        unmatched.trim()
    };

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "unmatched": effective_unmatched_file,
        "baseline": baseline.unwrap_or(""),
        "reposition_boundary": true,
    });
    ipc_payload["patch_id"] = serde_json::Value::String(patch_id.clone());

    if let Some(yaml) = frontmatter_yaml {
        ipc_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
    }
    if let Some(lines) = normalize_prefix_lines
        && !lines.is_empty()
    {
        ipc_payload["normalize_prefix_lines"] = serde_json::Value::Array(
            lines
                .iter()
                .map(|l| serde_json::Value::String(l.clone()))
                .collect(),
        );
        // Include full normalized content ONLY when there are no component patches.
        // When patches are present, normalize_prefix_lines + patches apply correctly
        // without fullContent. Sending fullContent alongside patches causes the plugin
        // to apply fullContent (full replacement) and skip patches → duplicate on next cycle.
        if ipc_patches.is_empty()
            && let Some(ours) = content_ours
        {
            ipc_payload["fullContent"] = serde_json::Value::String(ours.to_string());
        }
    }

    // Log IPC write details for debugging cross-contamination
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_write_attempt file={} hash={} patches={} ipc_patches={} unmatched_len={}",
            file.display(),
            hash,
            patches.len(),
            ipc_patches.len(),
            unmatched.trim().len()
        ),
    );

    // Warn when unmatched content exists but no IPC patches were synthesized —
    // this means content will be silently dropped by the plugin
    if ipc_patches.is_empty() && !unmatched.trim().is_empty() {
        eprintln!(
            "[write] WARNING: {} bytes of unmatched content with no IPC patches — content will be dropped. \
             Does the target file have template components (<!-- agent:exchange -->)?",
            unmatched.trim().len()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_unmatched_content_dropped file={} unmatched_len={}",
                file.display(),
                unmatched.trim().len()
            ),
        );
    }

    let success = write_ipc_and_poll(
        &patch_file,
        &ipc_payload,
        file,
        ipc_patches.len(),
        content_ours,
        normalize_prefix_lines,
        &project_root,
    )?;
    Ok(IpcResult { success, patch_id })
}

/// Attempt to write full document content via IPC.
///
/// Like `try_ipc()` but replaces the entire document content instead of
/// applying component patches. Used by append-mode documents that don't
/// have `<!-- agent:name -->` component markers.
///
/// Returns `Ok(true)` if the plugin consumed the patch, `Ok(false)` on timeout.
#[allow(dead_code)]
pub fn try_ipc_full_content(file: &Path, content: &str) -> Result<bool> {
    let canonical = file.canonicalize()?;
    let project_root = resolve_ipc_project_root(&canonical);

    // Try socket IPC first
    if crate::ipc_socket::is_listener_active(&project_root) {
        let socket_payload = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": [],
            "unmatched": "",
            "fullContent": content,
        });
        match crate::ipc_socket::send_message(&project_root, &socket_payload) {
            Ok(Some(_ack)) => {
                eprintln!("[write] socket IPC full content delivered");
                snapshot::save(file, content)?;
                let crdt_doc = crate::crdt::CrdtDoc::from_text(content);
                snapshot::save_crdt(file, &crdt_doc.encode_state())?;
                return Ok(true);
            }
            Ok(None) => {
                eprintln!(
                    "[write] socket IPC full content sent but no ack — falling back to file IPC"
                );
            }
            Err(e) => {
                eprintln!(
                    "[write] socket IPC full content failed: {} — falling back to file IPC",
                    e
                );
            }
        }
    }

    let hash = snapshot::doc_hash(file)?;
    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt file-based IPC if the patches directory exists (plugin has started)
    if !patches_dir.exists() {
        return Ok(false);
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    let ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "fullContent": content,
    });

    write_ipc_and_poll(
        &patch_file,
        &ipc_payload,
        file,
        0,
        Some(content),
        None,
        &project_root,
    )
}

/// Send a reposition-only IPC signal to the plugin.
///
/// No content changes — just tells the plugin to move the boundary marker
/// to the end of the exchange component. Used by `commit()` to keep the
/// boundary at end-of-exchange without writing to the working tree
/// (which would cause keystroke loss if the user is typing).
///
/// Returns `true` if the plugin consumed the signal, `false` on timeout
/// or if no plugin is active.
pub fn try_ipc_reposition_boundary(file: &Path) -> bool {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let project_root = resolve_ipc_project_root(&canonical);
    let boundary_id = crate::snapshot::load(file)
        .ok()
        .flatten()
        .as_deref()
        .and_then(|doc| find_boundary_id(doc, "exchange"))
        .or_else(|| {
            std::fs::read_to_string(file)
                .ok()
                .as_deref()
                .and_then(|doc| find_boundary_id(doc, "exchange"))
        });

    if !crate::ipc_socket::is_listener_active(&project_root) {
        return false;
    }

    match crate::ipc_socket::send_reposition(
        &project_root,
        &canonical.to_string_lossy(),
        boundary_id.as_deref(),
        true, // preserve (HEAD) in editor buffer
    ) {
        Ok(true) => {
            eprintln!("[commit] IPC reposition boundary signal sent");
            true
        }
        Ok(false) => {
            eprintln!("[commit] IPC reposition: no ack (non-fatal)");
            false
        }
        Err(e) => {
            eprintln!("[commit] IPC reposition failed (non-fatal): {}", e);
            false
        }
    }
}

/// Write an IPC patch file and poll for plugin ACK (file deletion).
///
/// Returns `Ok(true)` if consumed, `Ok(false)` on timeout.
fn write_ipc_and_poll(
    patch_file: &Path,
    payload: &serde_json::Value,
    doc_file: &Path,
    patch_count: usize,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    project_root: &Path,
) -> Result<bool> {
    // Atomic write of patch file
    atomic_write(patch_file, &serde_json::to_string_pretty(payload)?)?;

    eprintln!(
        "[write] IPC patch written to {} ({} components)",
        patch_file.display(),
        patch_count
    );

    // Poll for ACK (plugin deletes file after applying)
    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if !patch_file.exists() {
            // Plugin consumed the patch — poll for ack-content sidecar (authoritative
            // post-apply snapshot). Falls back to file read after timeout.
            let patch_id = payload
                .get("patch_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let current_on_disk = if !patch_id.is_empty() {
                match poll_ack_content_sidecar(
                    project_root,
                    patch_id,
                    std::time::Duration::from_millis(500),
                    std::time::Duration::from_millis(25),
                ) {
                    Ok(Some(content)) => {
                        // Verify sidecar preserved normalize_prefix_lines targets.
                        if let Some(lines) = normalize_prefix_lines
                            && !lines.is_empty()
                            && !verify_sidecar_normalization(&content, lines)
                        {
                            if let Some(ours) = content_ours {
                                eprintln!(
                                    "[write] sidecar normalization diverged — falling back to content_ours ({} bytes)",
                                    ours.len()
                                );
                                crate::ops_log::log_op(
                                    doc_file,
                                    &format!(
                                        "sidecar_normalization_fallback file={} snap_source=content_ours reason=prefix_divergence",
                                        doc_file.display()
                                    ),
                                );
                                ours.to_string()
                            } else {
                                eprintln!(
                                    "[write] sidecar normalization diverged but no content_ours — using sidecar ({} bytes)",
                                    content.len()
                                );
                                content
                            }
                        } else {
                            eprintln!(
                                "[write] snapshot from ack-content sidecar ({} bytes)",
                                content.len()
                            );
                            content
                        }
                    }
                    _ => {
                        eprintln!(
                            "[write] snapshot from file read (ack-content sidecar not available after 500ms)"
                        );
                        std::fs::read_to_string(doc_file).unwrap_or_default()
                    }
                }
            } else {
                eprintln!("[write] snapshot from file read (no patch_id for sidecar lookup)");
                std::fs::read_to_string(doc_file).unwrap_or_default()
            };
            let baseline_content = payload
                .get("baseline")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !baseline_content.is_empty() && current_on_disk == baseline_content {
                // File on disk hasn't changed — plugin likely failed to apply the patch.
                // Don't save snapshot with content that was never applied.
                eprintln!(
                    "[write] IPC patch consumed but file unchanged on disk — plugin may have failed to apply. Falling back to disk write."
                );
                return Ok(false);
            }

            // Verify patch content is present in the file (catches partial application).
            // Check that at least one non-empty patch's content appears in the result.
            let patch_list = payload.get("patches").and_then(|v| v.as_array());
            if let Some(patches) = patch_list {
                let has_content_patch = patches.iter().any(|p| {
                    let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    !content.trim().is_empty()
                });
                if has_content_patch {
                    let any_present = patches.iter().any(|p| {
                        let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        if content.trim().is_empty() {
                            return true;
                        }
                        // Check first meaningful line of content appears in file
                        content
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .is_none_or(|first_line| current_on_disk.contains(first_line.trim()))
                    });
                    if !any_present {
                        eprintln!(
                            "[write] IPC patch consumed but response content not found in file — plugin may have partially failed. Falling back to disk write."
                        );
                        return Ok(false);
                    }
                }
            }

            // Plugin applied the patch — update snapshot as actual post-write disk state.
            // `current_on_disk` is from ack-content sidecar when available, or 200ms file read.
            // Bug 2A fix: snapshot save failure after IPC success is non-fatal.
            let snap_content = current_on_disk;
            crate::ops_log::log_op(
                doc_file,
                &format!(
                    "ipc_file_delivered file={} snap_len={}",
                    doc_file.display(),
                    snap_content.len()
                ),
            );
            if let Err(e) = snapshot::save(doc_file, &snap_content) {
                eprintln!(
                    "[write] WARNING: IPC write succeeded but snapshot save failed: {}. \
                     Commit will auto-recover via divergence detection.",
                    e
                );
                crate::ops_log::log_op(
                    doc_file,
                    &format!(
                        "snapshot_save_failed_after_ipc file={} error={}",
                        doc_file.display(),
                        e
                    ),
                );
            } else {
                crate::ops_log::log_op(
                    doc_file,
                    &format!(
                        "snapshot_saved_file_ipc file={} snap_len={}",
                        doc_file.display(),
                        snap_content.len()
                    ),
                );
                let crdt_doc = crate::crdt::CrdtDoc::from_text(&snap_content);
                if let Err(e) = snapshot::save_crdt(doc_file, &crdt_doc.encode_state()) {
                    eprintln!("[write] WARNING: CRDT state save failed: {}", e);
                }
                eprintln!("[write] IPC patch consumed by plugin — snapshot updated");
            }
            return Ok(true);
        }
        std::thread::sleep(poll_interval);
    }

    // Timeout — clean up unconsumed patch file
    eprintln!(
        "[write] IPC timeout ({}s) — falling back to direct write",
        timeout.as_secs()
    );
    let _ = std::fs::remove_file(patch_file);
    Ok(false)
}

/// Apply `❯ ` prefix to lines in `content` that appear in `normalize_prefix_lines`.
///
/// Bakes normalization into patch content before IPC delivery so the plugin
/// receives already-prefixed lines. The plugin runs normalization *before*
/// applying patches, so it cannot normalize lines the patch is about to append.
fn normalize_patch_content(content: &str, prefix_lines: &[String]) -> String {
    if prefix_lines.is_empty() {
        return content.to_string();
    }
    let prefix_set: std::collections::HashSet<&str> =
        prefix_lines.iter().map(|s| s.as_str()).collect();
    let mut result = String::with_capacity(content.len() + 2 * prefix_lines.len());
    for line in content.lines() {
        let bare = line.strip_prefix("\u{276f} ").unwrap_or(line);
        if prefix_set.contains(bare) && !line.starts_with("\u{276f} ") {
            result.push_str("\u{276f} ");
        }
        result.push_str(line);
        result.push('\n');
    }
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.truncate(result.len() - 1);
    }
    result
}

/// Build the IPC patches JSON array (shared between socket and file-based paths).
///
/// Reads the document to find boundary IDs, filters frontmatter patches,
/// synthesizes exchange patches for unmatched content.
///
/// When `normalize_prefix_lines` is provided, applies `❯ ` prefix to matching
/// lines inside each patch's content so newly-appended lines already carry the
/// prefix. (The plugin runs normalization *before* applying patches, so it
/// cannot normalize lines that the patch is about to append.)
fn build_ipc_patches_json(
    file: &Path,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
    normalize_prefix_lines: Option<&[String]>,
) -> Result<Vec<serde_json::Value>> {
    let raw_doc = std::fs::read_to_string(file).unwrap_or_default();
    let current_doc = template::reposition_boundary_to_end_clean_with_summary(
        &raw_doc,
        file.file_stem().and_then(|s| s.to_str()),
    );

    let mut ipc_patches: Vec<serde_json::Value> = patches
        .iter()
        .filter(|p| p.name != "frontmatter")
        .map(|p| {
            let content = match normalize_prefix_lines {
                Some(prefix_lines)
                    if !prefix_lines.is_empty() && is_append_mode_component(&p.name) =>
                {
                    normalize_patch_content(&p.content, prefix_lines)
                }
                _ => p.content.clone(),
            };
            let mut patch_json = serde_json::json!({
                "component": p.name,
                "content": content,
            });
            if let Some(bid) = find_boundary_id(&current_doc, &p.name) {
                patch_json["boundary_id"] = serde_json::Value::String(bid);
            } else if is_append_mode_component(&p.name) {
                patch_json["ensure_boundary"] = serde_json::Value::Bool(true);
            }
            patch_json
        })
        .collect();

    let effective_unmatched = unmatched.trim().to_string();
    if ipc_patches.is_empty() && !effective_unmatched.is_empty() {
        // Dedup guard: parse components once, check before synthesizing.
        let parsed_comps = crate::component::parse(&current_doc).unwrap_or_default();
        for target in &["exchange", "output"] {
            // Skip synthesis if the content already exists in the target component.
            // This makes the write idempotent even when called twice with the same content.
            let already_present = parsed_comps.iter().any(|c| {
                c.name == *target && {
                    let body = &current_doc[c.open_end..c.close_start];
                    body.contains(effective_unmatched.as_str())
                }
            });
            if already_present {
                eprintln!(
                    "[write] dedup: content already present in {} — skipping synthesis",
                    target
                );
                break;
            }
            if let Some(bid) = find_boundary_id(&current_doc, target) {
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (boundary {})",
                    target,
                    &bid[..8.min(bid.len())]
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": &effective_unmatched,
                    "boundary_id": bid,
                }));
                break;
            } else if is_append_mode_component(target) {
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (ensure_boundary)",
                    target
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": &effective_unmatched,
                    "ensure_boundary": true,
                }));
                break;
            }
        }
    }

    Ok(ipc_patches)
}

// ---------------------------------------------------------------------------
// Internal helpers (same patterns as submit.rs)
// ---------------------------------------------------------------------------

/// Sanitize component tags in patch block content to prevent parser corruption.
///
/// When an agent response mentions component tags like `<!-- agent:NAME -->` in its
/// text, those raw HTML comments would be matched as real markers on subsequent
/// operations (compact, write). This escapes them to `&lt;!-- agent:NAME --&gt;`
/// so the component parser won't match them.
///
/// Only sanitizes `<!-- agent:NAME -->` and `<!-- /agent:NAME -->` patterns where
/// NAME is a valid component name (`[a-zA-Z0-9][a-zA-Z0-9-]*`).
pub fn sanitize_component_tags(content: &str) -> String {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut pos = 0;

    while pos + 4 <= len {
        if &bytes[pos..pos + 4] != b"<!--" {
            // Advance by one UTF-8 character (not one byte) to preserve multi-byte sequences
            let ch_len = utf8_char_len(bytes[pos]);
            result.push_str(&content[pos..pos + ch_len]);
            pos += ch_len;
            continue;
        }

        // Find closing -->
        let close = match find_comment_close(bytes, pos + 4) {
            Some(c) => c, // position after -->
            None => {
                result.push_str("<!--");
                pos += 4;
                continue;
            }
        };

        let inner = &content[pos + 4..close - 3];
        let trimmed = inner.trim();

        if component::is_agent_marker(trimmed) {
            // Escape the entire comment: <!-- ... --> -> &lt;!-- ... --&gt;
            let original = &content[pos..close];
            result.push_str(&original.replace('<', "&lt;").replace('>', "&gt;"));
        } else {
            // Not an agent marker — keep as-is
            result.push_str(&content[pos..close]);
        }
        pos = close;
    }

    // Append remaining content (as a str slice to preserve UTF-8)
    if pos < len {
        result.push_str(&content[pos..]);
    }

    result
}

/// Return the byte length of the UTF-8 character starting with `first_byte`.
fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xFF => 4,
        _ => 1, // continuation byte — shouldn't happen at a char boundary
    }
}

/// Find the end of an HTML comment (position after `-->`), starting search from `start`.
fn find_comment_close(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = start;
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            return Some(i + 3);
        }
        i += 1;
    }
    None
}

/// Sanitize the content of each patch block in-place.
pub(crate) fn sanitize_patches(patches: &mut [template::PatchBlock]) {
    for patch in patches.iter_mut() {
        patch.content = sanitize_component_tags(&patch.content);
    }
}

/// Strip leading `## Assistant` and trailing `## User` headings from response text.
///
/// The `agent-doc write` command adds its own `## Assistant\n\n` prefix and
/// `\n## User\n\n` suffix, so if the agent response includes these headings,
/// we'd get duplicates. This strips them to prevent that.
pub fn strip_assistant_heading(response: &str) -> String {
    let mut result = response.to_string();

    // Strip leading ## Assistant
    let trimmed = result.trim_start();
    if let Some(rest) = trimmed.strip_prefix("## Assistant") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        result = rest.to_string();
    }

    // Strip trailing ## User (with optional whitespace/newlines after)
    let trimmed_end = result.trim_end();
    if let Some(before) = trimmed_end.strip_suffix("## User") {
        result = before.trim_end_matches('\n').to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
    }

    result
}

fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = crate::snapshot::lock_path_for(path)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open doc lock {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("failed to acquire doc lock on {}", lock_path.display()))?;
    Ok(file)
}

/// Detect whether the plugin has already applied the agent's response patches.
///
/// On IPC sidecar ack timeout, the socket delivery may have succeeded but the
/// confirmation didn't arrive in time. If the plugin applied the patches, the
/// exchange component in `content_current` already contains the response content
/// from `content_ours`. CRDT merging in this state would duplicate the response.
///
/// Detection: extract the lines that `content_ours` added to the exchange
/// component (relative to `base`), and check if they're already present in
/// `content_current`'s exchange. Conservative — returns false on any parse
/// failure or ambiguous state.
fn response_already_in_current(base: &str, content_ours: &str, content_current: &str) -> bool {
    let base_comps = crate::component::parse(base).unwrap_or_default();
    let ours_comps = crate::component::parse(content_ours).unwrap_or_default();
    let current_comps = crate::component::parse(content_current).unwrap_or_default();

    let base_exc = base_comps.iter().find(|c| c.name == "exchange");
    let ours_exc = ours_comps.iter().find(|c| c.name == "exchange");
    let current_exc = current_comps.iter().find(|c| c.name == "exchange");

    let (Some(base_e), Some(ours_e), Some(current_e)) = (base_exc, ours_exc, current_exc) else {
        return false;
    };

    let base_content = base_e.content(base);
    let ours_content = ours_e.content(content_ours);
    let current_content = current_e.content(content_current);

    // No changes to exchange — nothing to detect
    if ours_content.trim() == base_content.trim() {
        return false;
    }

    // Find lines added by ours that aren't in base
    let base_lines: std::collections::HashSet<&str> = base_content.lines().collect();
    let response_lines: Vec<&str> = ours_content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !base_lines.contains(line))
        .collect();

    if response_lines.is_empty() {
        return false;
    }

    // Check if the majority of response lines are already in current
    let current_lines: std::collections::HashSet<&str> = current_content.lines().collect();
    let present_count = response_lines
        .iter()
        .filter(|line| current_lines.contains(*line))
        .count();

    // Require ≥80% of response lines present to confirm plugin applied.
    // A lower threshold risks false positives from coincidental line matches.
    let threshold = std::cmp::max(1, (response_lines.len() * 4) / 5);
    let detected = present_count >= threshold;

    if detected {
        eprintln!(
            "[write] plugin-applied detection: {}/{} response lines already in current",
            present_count,
            response_lines.len()
        );
    }

    detected
}

/// Transfer the `agent:pending` component content from `source` into `target`.
///
/// When the on-disk file has pending mutations applied (e.g. `--pending-done`)
/// that are not reflected in `content_ours` (which was built from a pre-mutation
/// baseline), this function preserves those mutations by splicing the pending
/// component from `source` into `target`.
///
/// Behaviour:
/// - If both `source` and `target` have a `pending` component: replaces the
///   content between the markers in `target` with the content from `source`.
/// - If `source` has no `pending` component: returns `target` unchanged.
/// - If `source` has a `pending` component but `target` does not: logs a warning
///   and returns `target` unchanged (can't locate insertion point without knowing
///   document structure).
fn splice_pending_component(target: &str, source: &str) -> String {
    let source_comps = match component::parse(source) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[write] WARNING: splice_pending: failed to parse source components: {}",
                e
            );
            return target.to_string();
        }
    };
    let source_pending = source_comps.iter().find(|c| is_backlog_component(&c.name));
    let Some(src_comp) = source_pending else {
        // No pending component in source — nothing to splice.
        return target.to_string();
    };
    let source_content = &source[src_comp.open_end..src_comp.close_start];

    let target_comps = match component::parse(target) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[write] WARNING: splice_pending: failed to parse target components: {}",
                e
            );
            return target.to_string();
        }
    };
    let target_pending = target_comps.iter().find(|c| is_backlog_component(&c.name));
    match target_pending {
        Some(tgt_comp) => tgt_comp.replace_content(target, source_content),
        None => {
            eprintln!(
                "[write] WARNING: splice_pending: source has agent:pending but target does not — \
                 pending mutations may be lost on IPC timeout"
            );
            target.to_string()
        }
    }
}

/// Atomic write: write to temp file then rename. Public for use by compact.
pub fn atomic_write_pub(path: &Path, content: &str) -> Result<()> {
    atomic_write(path, content)
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| "failed to write temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn write_appends_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello\n").unwrap();

        // Simulate stdin by calling run logic directly
        let base = fs::read_to_string(&doc).unwrap();
        let response = "This is the assistant response.";

        let mut content_ours = base.clone();
        if !content_ours.ends_with('\n') {
            content_ours.push('\n');
        }
        content_ours.push_str("## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push('\n');
        content_ours.push_str("\n## User\n\n");

        atomic_write(&doc, &content_ours).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("## Assistant\n\nThis is the assistant response."));
        assert!(result.contains("\n\n## User\n\n"));
        assert!(result.contains("## User\n\nHello"));
    }

    #[test]
    fn write_updates_snapshot() {
        // Use a direct snapshot write/read to avoid CWD dependency.
        // The snapshot module uses relative paths (.agent-doc/snapshots/),
        // so we verify the pattern works via snapshot::path_for + direct I/O.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();

        // Verify snapshot path computation works
        let snap_path = snapshot::path_for(&doc).unwrap();
        assert!(
            snap_path
                .to_string_lossy()
                .contains(".agent-doc/snapshots/")
        );

        // Verify atomic_write + read roundtrip (the core of snapshot save)
        let snap_abs = dir.path().join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
        let loaded = fs::read_to_string(&snap_abs).unwrap();
        assert_eq!(loaded, content);
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
        snapshot::save(&doc, snapshot_content).unwrap();

        let response = "<!-- patch:exchange -->\nCompacted summary.\n<!-- /patch:exchange -->\n";
        apply_template_from_string(&doc, response).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Compacted summary.\n"));
        assert!(!result.contains("Old progress."));
        assert!(!result.contains("compact exchange"));
    }

    #[test]
    fn write_preserves_user_edits_via_merge() {
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        let response = "My response";

        // "ours" = base + response
        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // "theirs" = user added a follow-up to the User block
        let theirs = "---\nsession: test\n---\n\n## User\n\nOriginal question\nAnd a follow-up!\n";

        let merged = merge::merge_contents(base, &ours, theirs).unwrap();

        // Both the response and the user's follow-up should be in the merge
        assert!(
            merged.contains("My response"),
            "response missing from merge"
        );
        assert!(
            merged.contains("And a follow-up!"),
            "user edit missing from merge"
        );
    }

    #[test]
    fn write_no_merge_when_unchanged() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "Response here";

        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // theirs == base (no edit)
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, base).unwrap();

        let doc_lock = acquire_doc_lock(&doc).unwrap();
        let content_current = fs::read_to_string(&doc).unwrap();

        let final_content = if content_current == base {
            ours.clone()
        } else {
            merge::merge_contents(base, &ours, &content_current).unwrap()
        };

        drop(doc_lock);
        assert_eq!(final_content, ours);
    }

    #[test]
    fn atomic_write_correct_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.md");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn concurrent_writes_no_corruption() {
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.md");
        fs::write(&path, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let p = path.clone();
            let parent = dir.path().to_path_buf();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut tmp = tempfile::NamedTempFile::new_in(&parent).unwrap();
                std::io::Write::write_all(&mut tmp, content.as_bytes()).unwrap();
                tmp.persist(&p).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_content = fs::read_to_string(&path).unwrap();
        assert!(
            final_content.starts_with("writer-") && final_content.ends_with("-content"),
            "unexpected content: {}",
            final_content
        );
    }

    #[test]
    fn snapshot_matches_disk_state() {
        // Snapshot saved after write must equal the actual post-merge file on disk.
        // Using content_ours (pre-merge) as the snapshot risks phantom diffs when
        // the baseline is stale (e.g. streaming checkpoint with an outdated baseline).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc").join("snapshots");
        fs::create_dir_all(&agent_doc_dir).unwrap();

        let doc = dir.path().join("test.md");
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        fs::write(&doc, base).unwrap();

        // Build content_ours = baseline + response
        let response = "Agent response here";
        let mut content_ours = base.to_string();
        content_ours.push_str("\n## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push_str("\n\n## User\n\n");

        // Simulate user editing the file concurrently (adding a follow-up)
        let user_edited = format!("{}Follow-up question\n", base);
        fs::write(&doc, &user_edited).unwrap();

        // Merge: content_ours + user edits
        let merged = merge::merge_contents(base, &content_ours, &user_edited).unwrap();

        // Write merged content (includes both response and user edit)
        atomic_write(&doc, &merged).unwrap();
        assert!(merged.contains(response), "response missing from merged");
        assert!(
            merged.contains("Follow-up question"),
            "user edit missing from merged"
        );

        // KEY: Save snapshot as final_content (the actual disk state after merge)
        snapshot::save(&doc, &merged).unwrap();

        // Verify: snapshot matches what's on disk exactly
        let snap = snapshot::load(&doc).unwrap().unwrap();
        let current = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            snap, current,
            "snapshot must match actual disk state after write"
        );
        assert!(
            snap.contains(response),
            "snapshot should contain agent response"
        );
        assert!(
            snap.contains("Follow-up question"),
            "snapshot should contain merged user edit"
        );
    }

    #[test]
    fn try_ipc_returns_false_when_no_patches_dir() {
        // Without .agent-doc/patches/, IPC should return false immediately
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let result = try_ipc(&doc, &patches, "", None, None, None, None, None).unwrap();
        assert!(
            !result.success,
            "should return false when patches dir doesn't exist"
        );
    }

    #[test]
    fn try_ipc_times_out_when_no_plugin() {
        // With .agent-doc/patches/ existing but no plugin consuming, should timeout
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "new content");

        // This will timeout after 2s — patch file is written but never consumed
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            !result.success,
            "should return false on timeout (no plugin)"
        );

        // Patch file should be cleaned up after timeout
        let patches_dir = agent_doc_dir.join("patches");
        let entries: Vec<_> = fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            entries.is_empty(),
            "patch file should be cleaned up after timeout"
        );
    }

    #[test]
    fn try_ipc_succeeds_when_plugin_consumes() {
        // Simulate plugin by spawning a thread that deletes the patch file
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "new content");

        // Spawn "plugin" thread that watches for patch files, writes content, then deletes
        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            // Simulate plugin applying the patch by modifying the doc
                            let _ = fs::write(
                                &doc_for_watcher,
                                "---\nsession: test\n---\n\n<!-- agent:exchange -->\nnew content\n<!-- /agent:exchange -->\n",
                            );
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "should return true when plugin consumes patch"
        );
    }

    #[test]
    fn try_ipc_full_content_returns_false_when_no_patches_dir() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let result = try_ipc_full_content(&doc, "new content").unwrap();
        assert!(
            !result,
            "should return false when patches dir doesn't exist"
        );
    }

    // --- sanitize_component_tags tests ---

    #[test]
    fn sanitize_escapes_open_agent_tag() {
        let input = "Here is an example: <!-- agent:exchange --> marker.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "open agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- agent:exchange -->"),
            "raw open agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_escapes_close_agent_tag() {
        let input = "End marker: <!-- /agent:pending --> done.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- /agent:pending --&gt;"),
            "close agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- /agent:pending -->"),
            "raw close agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_does_not_escape_patch_markers() {
        let input = "<!-- patch:exchange -->\nsome content\n<!-- /patch:exchange -->\n";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "patch markers must not be escaped");
    }

    #[test]
    fn sanitize_passes_normal_content_through() {
        let input = "Just some normal markdown content.\n\nWith paragraphs and **bold**.";
        let result = sanitize_component_tags(input);
        assert_eq!(
            result, input,
            "normal content should pass through unchanged"
        );
    }

    #[test]
    fn sanitize_preserves_utf8_em_dash() {
        // Em dash U+2014 is 3 bytes in UTF-8: 0xE2, 0x80, 0x94
        let input = "This is a test \u{2014} with em dashes \u{2014} in content.";
        let result = sanitize_component_tags(input);
        assert_eq!(
            result, input,
            "em dashes must survive sanitization unchanged"
        );

        // Verify at the byte level
        assert_eq!(
            result.as_bytes(),
            input.as_bytes(),
            "byte-level content must be identical"
        );
    }

    #[test]
    fn sanitize_preserves_mixed_utf8_and_agent_tags() {
        // Content with UTF-8 characters AND agent tags that need escaping
        let input = "Response with \u{2014} em dash and <!-- agent:exchange --> tag reference.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("\u{2014}"),
            "em dash must be preserved, got: {:?}",
            result
        );
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "agent tag must be escaped"
        );
    }

    #[test]
    fn sanitize_preserves_various_unicode() {
        // Test various multi-byte UTF-8 characters
        let input = "Caf\u{00E9} \u{2019}quotes\u{2019} \u{2014} \u{2026} \u{1F600}";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "all unicode must survive sanitization");
    }

    #[test]
    fn try_ipc_snapshot_saves_disk_state() {
        // Verify that after IPC succeeds, the snapshot contains the actual post-write
        // disk state (file read after the 200ms flush delay), NOT content_ours.
        // Using the actual disk state prevents stale baselines from perpetuating
        // ghost diffs cycle after cycle.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "---\nsession: test\n---\n\n<!-- agent:exchange -->\noriginal content\n<!-- agent:boundary:test-boundary-123 -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response content");

        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\n<!-- /agent:exchange -->\n";

        // Simulate user editing the file (working tree has additional content)
        let after_plugin_write = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\nuser typed something new\n<!-- /agent:exchange -->\n";

        // Spawn "plugin" thread that watches for patch files, writes content, then deletes
        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let after_plugin_write_owned = after_plugin_write.to_string();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            // Simulate plugin applying patch + leaving user edits in file
                            let _ = fs::write(&doc_for_watcher, &after_plugin_write_owned);
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),     // baseline
            Some(content_ours), // content_ours (no longer used for snapshot)
            None,               // normalize_prefix_lines
            None,               // reuse_patch_id
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        // KEY ASSERTION: snapshot must match actual disk state (includes user edits)
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("agent response content"),
            "snapshot must contain agent response, got: {}",
            snap
        );
        assert!(
            snap.contains("user typed something new"),
            "snapshot must match disk state (include user edits written by plugin)"
        );
        assert_eq!(
            snap, after_plugin_write,
            "snapshot must exactly match post-write disk state"
        );
    }

    #[test]
    fn ipc_json_preserves_utf8_em_dash() {
        // Verify that serde_json serialization preserves em dashes in IPC payloads
        let content = "Response with \u{2014} em dash.";
        let payload = serde_json::json!({
            "file": "/tmp/test.md",
            "patches": [{
                "component": "exchange",
                "content": content,
            }],
            "unmatched": "",
            "baseline": "",
        });

        let json_str = serde_json::to_string_pretty(&payload).unwrap();
        // Parse it back and verify the content is preserved
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let parsed_content = parsed["patches"][0]["content"].as_str().unwrap();
        assert_eq!(
            parsed_content, content,
            "em dash must survive JSON round-trip"
        );

        // Also verify the raw JSON contains the UTF-8 bytes, not escaped sequences
        assert!(
            json_str.contains("\u{2014}"),
            "JSON should contain raw UTF-8 em dash"
        );
    }

    // --- is_append_mode_component tests ---

    #[test]
    fn append_mode_component_exchange() {
        assert!(is_append_mode_component("exchange"));
        assert!(is_append_mode_component("findings"));
    }

    #[test]
    fn replace_mode_components_not_append() {
        assert!(!is_append_mode_component("pending"));
        assert!(!is_append_mode_component("backlog"));
        assert!(!is_append_mode_component("status"));
        assert!(!is_append_mode_component("output"));
        assert!(!is_append_mode_component("todo"));
    }

    #[test]
    fn find_boundary_id_skips_code_blocks() {
        // Boundary-looking text inside a fenced code block must not be returned
        let content = "<!-- agent:exchange -->\n```\n<!-- agent:boundary:fake-id -->\n```\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert!(
            result.is_none(),
            "boundary inside code block must not be found, got: {:?}",
            result
        );
    }

    #[test]
    fn find_boundary_id_finds_real_marker() {
        let content = "<!-- agent:exchange -->\nSome text.\n<!-- agent:boundary:real-uuid-5678 -->\nMore text.\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert_eq!(result, Some("real-uuid-5678".to_string()));
    }

    #[test]
    fn stale_baseline_guard_prefix_check() {
        // Baseline that starts with snapshot content (user added text) = NOT stale
        let snapshot = "## Exchange\nResponse here.\n";
        let baseline_with_user_edit = "## Exchange\nResponse here.\nNew user question\n";
        let snap_clean = strip_boundary_for_dedup(snapshot);
        let base_clean = strip_boundary_for_dedup(baseline_with_user_edit);
        assert!(
            base_clean.starts_with(&snap_clean),
            "baseline with user edits should start with snapshot content"
        );

        // Baseline that doesn't contain snapshot content = STALE
        let stale_baseline = "## Exchange\nOld content only.\n";
        let stale_clean = strip_boundary_for_dedup(stale_baseline);
        assert!(
            !stale_clean.starts_with(&snap_clean),
            "stale baseline should not start with snapshot content"
        );
    }

    // --- is_stale_baseline tests ---

    #[test]
    fn stale_baseline_identical_content_not_stale() {
        let doc = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(doc, doc));
    }

    #[test]
    fn stale_baseline_user_appended_text_not_stale() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn stale_baseline_user_edited_replace_component_not_stale() {
        // User edits replace-mode component (status) — should NOT trigger stale guard
        let snapshot = "<!-- agent:status patch=replace -->\nOld status\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nEdited status by user\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\nNew question\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "user editing replace-mode status component should NOT trigger stale guard"
        );
    }

    #[test]
    fn stale_baseline_missing_committed_content_is_stale() {
        let snapshot = "<!-- agent:exchange patch=append -->\nCommitted response from agent.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content only.\n<!-- /agent:exchange -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "baseline missing committed content should be stale"
        );
    }

    #[test]
    fn stale_baseline_missing_append_component_is_stale() {
        // Missing an append-mode component = stale
        let snapshot =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:other patch=append -->\nDifferent.\n<!-- /agent:other -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "baseline missing an append-mode component should be stale"
        );
    }

    #[test]
    fn stale_baseline_missing_replace_component_not_stale() {
        // Missing a replace-mode component is fine — user can delete it
        let snapshot = "<!-- agent:status patch=replace -->\nActive\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "missing replace-mode component should NOT trigger stale guard"
        );
    }

    #[test]
    fn stale_baseline_boundary_markers_ignored() {
        let snapshot = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:xyz -->\nUser edit\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "different boundary marker IDs should not cause false stale detection"
        );
    }

    #[test]
    fn stale_baseline_non_template_fallback_to_prefix() {
        // Non-template (no components) falls back to prefix check
        let snapshot = "## Exchange\nResponse.\n";
        let baseline = "## Exchange\nResponse.\nNew question\n";
        assert!(!is_stale_baseline(baseline, snapshot));

        let stale = "## Exchange\nDifferent content.\n";
        assert!(is_stale_baseline(stale, snapshot));
    }

    #[test]
    fn stale_baseline_empty_snapshot_component_skipped() {
        // Empty append components in snapshot should not cause false positives
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nUser added content\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn stale_baseline_default_exchange_is_append() {
        // exchange without explicit patch attr defaults to append via is_append_mode_component
        let snapshot = "<!-- agent:exchange -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange -->\nOld stuff.\n<!-- /agent:exchange -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "exchange without patch attr should default to append-mode check"
        );
    }

    #[test]
    fn strip_boundary_for_dedup_removes_markers() {
        let with_boundary = "Hello\n<!-- agent:boundary:abc123 -->\nWorld\n";
        let without = strip_boundary_for_dedup(with_boundary);
        assert!(!without.contains("agent:boundary"));
        assert!(without.contains("Hello"));
        assert!(without.contains("World"));
    }

    // --- build_ipc_patches_json / synthesis dedup tests ---

    #[test]
    fn synthesis_dedup_skips_when_content_already_present() {
        // If the unmatched content already exists in the target component,
        // synthesis should be skipped (idempotent write guard).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let existing = "This is the agent response.";
        let doc_content = format!(
            "<!-- agent:exchange patch=append -->\n{}\n<!-- /agent:exchange -->\n",
            existing
        );
        fs::write(&doc, &doc_content).unwrap();

        // No explicit patches (simulates skill sending raw content)
        let patches: Vec<crate::template::PatchBlock> = vec![];
        // Unmatched content is identical to what's already in the exchange
        let result = build_ipc_patches_json(&doc, &patches, existing, None).unwrap();

        assert!(
            result.is_empty(),
            "synthesis should be skipped when content already exists in target component, \
             got {} patches: {:?}",
            result.len(),
            result
        );
    }

    #[test]
    fn synthesis_proceeds_when_content_is_new() {
        // When unmatched content is NOT present in the target component,
        // synthesis should create an IPC patch.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nExisting content.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let new_content = "Completely new agent response.";
        let result = build_ipc_patches_json(&doc, &patches, new_content, None).unwrap();

        assert_eq!(
            result.len(),
            1,
            "synthesis should produce one patch for new content"
        );
        assert_eq!(
            result[0]["component"].as_str().unwrap(),
            "exchange",
            "synthesized patch should target exchange"
        );
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            new_content,
            "synthesized patch content should match unmatched"
        );
    }

    #[test]
    fn effective_unmatched_cleared_when_synthesis_consumes_content() {
        // When synthesis consumes the unmatched content (patches input was empty,
        // ipc_patches output is non-empty), effective_unmatched should be "".
        // This prevents the plugin from applying the content twice (IPC duplicate bug).
        let patches: Vec<crate::template::PatchBlock> = vec![];
        let unmatched = "some response content";

        // Case 1: synthesis happened (patches empty → ipc_patches non-empty)
        let ipc_patches: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": unmatched,
        })];
        let effective = if patches.is_empty() && !ipc_patches.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective, "",
            "effective_unmatched must be empty when synthesis consumed content"
        );

        // Case 2: explicit patches (no synthesis) — unmatched passes through
        let explicit_patch = crate::template::PatchBlock::new("exchange", "response");
        let patches_explicit = vec![explicit_patch];
        let ipc_explicit: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": "response",
        })];
        let effective2 = if patches_explicit.is_empty() && !ipc_explicit.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective2,
            unmatched.trim(),
            "effective_unmatched should pass through when explicit patches exist"
        );

        // Case 3: no patches, no synthesis (empty doc or dedup skipped it) — unmatched passes through
        let ipc_empty: Vec<serde_json::Value> = vec![];
        let effective3 = if patches.is_empty() && !ipc_empty.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective3,
            unmatched.trim(),
            "effective_unmatched should pass through when no synthesis occurred"
        );
    }

    // ── normalize_user_prompts_in_exchange ──────────────────────────────────

    #[test]
    fn normalize_user_prompts_new_line_gets_prefix() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\nOld content.\n<!-- /agent:exchange -->\n";
        // baseline = user added "Hello" but agent hasn't responded yet
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- /agent:exchange -->\n";
        // content_ours = baseline + agent response appended (boundary at end after pre-patch)
        let content = "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- agent:boundary:abc123 -->\n### Re: response\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Hello"),
            "user line should get ❯  prefix: {}",
            result
        );
        assert!(
            result.contains("Old content."),
            "old content should be preserved"
        );
        assert!(
            result.contains("### Re: response"),
            "agent response should be preserved"
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should not get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_agent_response_not_prefixed() {
        // Regression: agent response lines in content_ours (before boundary) must NOT get ❯  prefix.
        // Before the fix, apply_patches_with_overrides moves the boundary to the end of exchange,
        // so the agent's response lines ended up in the "user region" and were incorrectly prefixed.
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        // baseline: user added "My question"
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld.\nMy question\n<!-- /agent:exchange -->\n";
        // content_ours: boundary at end (after pre-patch), agent response before it
        let content = "<!-- agent:exchange patch=append -->\nOld.\nMy question\nAgent answer here.\n<!-- agent:boundary:xyz -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ My question"),
            "user question should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Agent answer"),
            "agent response should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("Agent answer here."),
            "agent response should be preserved: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_blank_line_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // blank line should not get prefix
        assert!(
            !result.contains("❯ \n"),
            "blank line should not be prefixed: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_heading_treated_as_agent_content() {
        // Headings in the exchange are agent response markers. A standalone heading
        // (not ❯-prefixed) is treated as agent content and does NOT get the ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\n### My heading\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### My heading\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ### My heading"),
            "heading should NOT get prefix (treated as agent content): {}",
            result
        );
        assert!(
            result.contains("### My heading"),
            "heading should be preserved: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_hash_ref_prefixed() {
        // Regression for agent-doc-bugs #vnxg: a bare hash reference like `#zj6s` inside
        // the exchange user region was being skipped by the old `starts_with('#')` guard.
        // Under Option 2, the line is user input and must receive the ❯ prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\nprior turn\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nprior turn\n#zj6s\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nprior turn\n#zj6s\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ #zj6s"),
            "hash-ref line must get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_already_prefixed_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ❯"),
            "should not double-prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Already prefixed"),
            "prefix should be preserved"
        );
    }

    #[test]
    fn normalize_user_prompts_existing_content_unchanged() {
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // Previous question already prefixed — should not double-prefix
        assert!(
            !result.contains("❯ ❯"),
            "should not double-prefix existing content: {}",
            result
        );
        // New question should get prefix
        assert!(
            result.contains("❯ New question"),
            "new line should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_code_fence_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ```"),
            "code fence marker should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ echo hello"),
            "code fence interior should not get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Some text."),
            "regular user line should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_code_fence_interior_skipped() {
        // Multi-line code block with text before and after — only non-fence lines get prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nQuestion here.\n```rust\nlet x = 1;\nlet y = 2;\n```\nFollow-up.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nQuestion here.\n```rust\nlet x = 1;\nlet y = 2;\n```\nFollow-up.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Question here."),
            "text before fence should get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Follow-up."),
            "text after fence should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ let x"),
            "fence interior should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ let y"),
            "fence interior should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ```"),
            "fence marker should not get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_tilde_fence_interior_skipped() {
        // ~~~ fences must be tracked the same as ``` fences.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nBefore.\n~~~sh\necho hello\n~~~\nAfter.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nBefore.\n~~~sh\necho hello\n~~~\nAfter.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Before."),
            "text before tilde fence should get prefix: {result}"
        );
        assert!(
            result.contains("❯ After."),
            "text after tilde fence should get prefix: {result}"
        );
        assert!(
            !result.contains("❯ echo hello"),
            "tilde fence interior should not get prefix: {result}"
        );
        assert!(
            !result.contains("❯ ~~~"),
            "tilde fence marker should not get prefix: {result}"
        );
    }

    #[test]
    fn normalize_user_prompts_quoted_string_prefixed() {
        // Option 2 invariant: a quoted string the user typed is still user input,
        // so it gets the ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ \"Merge conflict"),
            "quoted user line should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_patch_content_applies_prefix_to_matching_lines() {
        let patch_content =
            "transferred line 1\ntransferred line 2\n### Re: Response\nAgent answer\n";
        let prefix_lines = vec![
            "transferred line 1".to_string(),
            "transferred line 2".to_string(),
        ];
        let result = normalize_patch_content(patch_content, &prefix_lines);
        let expected =
            "❯ transferred line 1\n❯ transferred line 2\n### Re: Response\nAgent answer\n";
        assert_eq!(
            result, expected,
            "prefix lines should get ❯  in patch content"
        );
    }

    #[test]
    fn normalize_patch_content_idempotent_already_prefixed() {
        let patch_content = "❯ already prefixed\nnot prefixed\n";
        let prefix_lines = vec!["already prefixed".to_string(), "not prefixed".to_string()];
        let result = normalize_patch_content(patch_content, &prefix_lines);
        let expected = "❯ already prefixed\n❯ not prefixed\n";
        assert_eq!(
            result, expected,
            "already-prefixed lines should not get double prefix"
        );
    }

    #[test]
    fn normalize_patch_content_empty_prefix_lines_passthrough() {
        let patch_content = "some line\nanother line\n";
        let result = normalize_patch_content(patch_content, &[]);
        assert_eq!(
            result, patch_content,
            "empty prefix_lines should leave content unchanged"
        );
    }

    #[test]
    fn normalize_patch_content_non_matching_lines_unchanged() {
        let patch_content = "agent response line\n### heading\n";
        let prefix_lines = vec!["user line".to_string()];
        let result = normalize_patch_content(patch_content, &prefix_lines);
        assert_eq!(
            result, patch_content,
            "non-matching lines should pass through unchanged"
        );
    }

    #[test]
    fn normalize_prefix_lines_skipped_for_replace_mode_components() {
        // Regression: normalize_patch_content was applied to ALL patches including agent:pending.
        // When a line from the exchange user_added set also appeared in a pending patch, it would
        // incorrectly receive the ❯  prefix. The fix gates normalization on is_append_mode_component.
        let pending_content =
            "- [ ] Build Gutenberg replacement HTML for home page\n- [ ] Update page content\n";
        let prefix_lines = vec!["- [ ] Build Gutenberg replacement HTML for home page".to_string()];
        // Simulate the guard: only apply normalize_patch_content for exchange (append-mode) components.
        // For pending (replace-mode), content must pass through unchanged.
        let is_pending = !is_append_mode_component("pending");
        assert!(is_pending, "pending should not be an append-mode component");
        // If the guard is respected, pending content is not normalized.
        let result = if is_append_mode_component("pending") {
            normalize_patch_content(pending_content, &prefix_lines)
        } else {
            pending_content.to_string()
        };
        assert_eq!(
            result, pending_content,
            "agent:pending content must NOT receive ❯  prefix"
        );
        assert!(
            !result.contains("❯ "),
            "no ❯  prefix should appear in pending patches"
        );
    }

    #[test]
    fn normalize_user_prompts_no_exchange_passthrough() {
        let content = "No exchange here.\n";
        let baseline = "No exchange here.\n";
        let snapshot = "";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert_eq!(
            result, content,
            "document without exchange should pass through unchanged"
        );
    }

    #[test]
    fn normalize_user_prompts_restores_prefix_lost_in_file() {
        // Regression: snapshot has ❯ do but the editor file (baseline) has do without prefix.
        // This happens when the IPC normalization fails to update the editor file.
        // The binary must restore ❯  so the snapshot stays correct and the
        // next IPC write delivers fullContent with the correct prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ done\n❯ do\n- [ ] task\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- agent:boundary:abc123:doc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ do"),
            "❯  prefix must be restored when snapshot had it but file lost it: {}",
            result
        );
        assert!(
            !result.contains("\ndo\n"),
            "bare do line must not remain without prefix: {}",
            result
        );
        // ❯ done must not be double-prefixed
        assert!(!result.contains("❯ ❯"), "no double-prefix: {}", result);
    }

    #[test]
    fn normalize_user_prompts_heading_replacement_does_not_swallow_next_prompt() {
        // Regression: commit-time `(HEAD)` churn replaces an existing response heading,
        // which shows up as Delete+Insert in snapshot→baseline. That replacement must
        // not reopen an agent block and suppress ❯ prefixing for the following user line.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4\nAgent answer.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4 (HEAD)\nAgent answer.\nfix #vedj. add spec + tests. build + install for local testing\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4 (HEAD)\nAgent answer.\nfix #vedj. add spec + tests. build + install for local testing\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("### Re: topic — gpt-5.4 (HEAD)"),
            "replacement heading should be preserved: {}",
            result
        );
        assert!(
            result.contains("Agent answer."),
            "existing agent body should be preserved: {}",
            result
        );
        assert!(
            result.contains("❯ fix #vedj. add spec + tests. build + install for local testing"),
            "new user prompt should get prefix despite heading replacement: {}",
            result
        );
        assert!(
            !result.contains("❯ Agent answer."),
            "existing agent body should not be prefixed: {}",
            result
        );
        assert!(
            !result.contains("❯ ### Re: topic"),
            "replacement heading should not be prefixed: {}",
            result
        );
    }

    // ── agent-response-block tracking ────────────────────────────────────────

    #[test]
    fn normalize_user_prompts_agent_table_rows_not_prefixed() {
        // Core bug: stale snapshot causes agent response table rows (inside ### Re: blocks)
        // to appear as Insert lines and incorrectly receive ❯ prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Question\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Question\n### Re: analysis — opus-4-6\n| model | score |\n|-------|-------|\n| gpt-4 | 85.0 |\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Question\n### Re: analysis — opus-4-6\n| model | score |\n|-------|-------|\n| gpt-4 | 85.0 |\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ |"),
            "table rows inside agent response should NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("| model | score |"),
            "table content should be preserved: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_agent_subheadings_not_prefixed() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n### Re: topic\nSome text.\n#### Details\nMore text.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### Re: topic\nSome text.\n#### Details\nMore text.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ "),
            "no lines should get prefix — all are agent content: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_user_text_after_equal_heading() {
        // Heading is Equal (in snapshot), user adds text after it. User text gets ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\nNew user input\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\nNew user input\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ New user input"),
            "user text after Equal heading should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_agent_block_ends_at_prompt() {
        // Agent block (Insert heading) ends when ❯-prefixed line appears.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n### Re: answer\nAgent text.\n❯ New question\nFollow-up text.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### Re: answer\nAgent text.\n❯ New question\nFollow-up text.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ Agent text"),
            "agent text should NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ New question"),
            "already-prefixed line should be preserved: {}",
            result
        );
        assert!(
            result.contains("❯ Follow-up text."),
            "user text after ❯ should get prefix: {}",
            result
        );
    }

    #[test]
    fn normalize_user_prompts_heading_in_fence_not_agent_block() {
        // A heading inside a code fence is code, not an agent response marker.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nBefore.\n```md\n### Not a real heading\nSome code.\n```\nAfter.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nBefore.\n```md\n### Not a real heading\nSome code.\n```\nAfter.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Before."),
            "text before fence should get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ After."),
            "text after fence should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "heading inside fence should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Some code"),
            "code inside fence should not get prefix: {}",
            result
        );
    }

    // ── safety rail: normalize_user_prompts_in_exchange_safe ────────────────

    #[test]
    fn normalize_safe_passes_through_under_threshold() {
        // Small diff (1 user-added line) — should behave exactly like the pure function.
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();

        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("❯ Hello"),
            "under threshold, ❯ prefix should still be applied: {result}"
        );
    }

    #[test]
    fn normalize_safe_bails_over_threshold() {
        // Construct a baseline with >50 unique "user-added" lines relative to the snapshot.
        // The safety rail should refuse to apply ❯ prefix and return content unchanged.
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();

        let mut baseline_lines = String::new();
        let mut content_lines = String::new();
        for i in 0..60 {
            baseline_lines.push_str(&format!("user line {i}\n"));
            content_lines.push_str(&format!("user line {i}\n"));
        }
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = format!(
            "<!-- agent:exchange patch=append -->\n{baseline_lines}<!-- /agent:exchange -->\n"
        );
        let content = format!(
            "<!-- agent:exchange patch=append -->\n{content_lines}<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n"
        );

        let result = normalize_user_prompts_in_exchange_safe(&content, &baseline, snapshot, &file);
        // No ❯ prefix should be applied — content should be returned unchanged.
        assert_eq!(
            result, content,
            "over threshold, content should pass through unchanged"
        );
        assert!(
            !result.contains("❯ user line"),
            "no ❯ prefix should be applied when threshold exceeded"
        );
    }

    #[test]
    fn normalize_safe_threshold_exact_boundary() {
        // Exactly 50 lines — at threshold, still applies prefix (strictly greater-than check).
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();

        let mut lines = String::new();
        for i in 0..50 {
            lines.push_str(&format!("line {i}\n"));
        }
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            format!("<!-- agent:exchange patch=append -->\n{lines}<!-- /agent:exchange -->\n");
        let content = format!(
            "<!-- agent:exchange patch=append -->\n{lines}<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n"
        );

        let result = normalize_user_prompts_in_exchange_safe(&content, &baseline, snapshot, &file);
        // At exactly 50, prefix should be applied (> is strict).
        assert!(
            result.contains("❯ line 0"),
            "at threshold, first line should get prefix: {result}"
        );
        assert!(
            result.contains("❯ line 49"),
            "at threshold, last line should get prefix: {result}"
        );
    }

    // --- exchange shrink guard tests ---

    #[test]
    fn shrink_guard_blocks_truncation() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        let long_exchange = "a]".repeat(250); // 500 bytes
        let old = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            long_exchange
        );
        let new = "<!-- agent:exchange -->\n.\n<!-- /agent:exchange -->\n";

        let result = check_exchange_shrink_guard(&old, new, &doc);
        assert!(
            result.is_err(),
            "shrink guard should block truncation from 500 to ~1 byte"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("shrink"), "error should mention shrink: {msg}");
    }

    #[test]
    fn shrink_guard_allows_normal_write() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        let old_text = "x".repeat(200);
        let new_text = "y".repeat(100); // 50% — well above 10%
        let old = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            old_text
        );
        let new = format!(
            "<!-- agent:exchange -->\n{}\n<!-- /agent:exchange -->\n",
            new_text
        );

        let result = check_exchange_shrink_guard(&old, &new, &doc);
        assert!(
            result.is_ok(),
            "shrink guard should allow 50% reduction: {:?}",
            result.err()
        );
    }

    #[test]
    fn shrink_guard_skips_small_exchange() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        // Old exchange is only 50 bytes — below SHRINK_GUARD_MIN_BYTES
        let old =
            "<!-- agent:exchange -->\nSmall content here, not much.\n<!-- /agent:exchange -->\n";
        let new = "<!-- agent:exchange -->\n.\n<!-- /agent:exchange -->\n";

        let result = check_exchange_shrink_guard(old, new, &doc);
        assert!(
            result.is_ok(),
            "shrink guard should skip small exchanges: {:?}",
            result.err()
        );
    }

    #[test]
    fn shrink_guard_passes_no_exchange() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");

        // No exchange component at all
        let old = "# Just a heading\nSome content.\n";
        let new = "# Just a heading\n.\n";

        let result = check_exchange_shrink_guard(old, new, &doc);
        assert!(
            result.is_ok(),
            "shrink guard should pass when no exchange component exists"
        );
    }

    #[test]
    fn extract_exchange_content_len_works() {
        let doc = "<!-- agent:exchange -->\nHello world\n<!-- /agent:exchange -->\n";
        assert_eq!(extract_exchange_content_len(doc), "Hello world".len());

        let empty = "<!-- agent:exchange -->\n\n<!-- /agent:exchange -->\n";
        assert_eq!(extract_exchange_content_len(empty), 0);

        let no_exchange = "Just text.";
        assert_eq!(extract_exchange_content_len(no_exchange), 0);
    }

    #[test]
    fn splice_pending_replaces_content_when_both_have_pending() {
        // target has stale/empty pending (built from pre-mutation baseline)
        let target = "\
<!-- agent:exchange -->
response content
<!-- /agent:exchange -->
<!-- agent:pending -->
- [ ] [#aaaa] old item
<!-- /agent:pending -->
";
        // source is the current on-disk file with a pending-done mutation applied
        let source = "\
<!-- agent:exchange -->
original content
<!-- /agent:exchange -->
<!-- agent:pending -->
- [x] [#aaaa] old item
<!-- /agent:pending -->
";
        let result = splice_pending_component(target, source);
        // exchange content from target is preserved
        assert!(
            result.contains("response content"),
            "exchange content should come from target"
        );
        // pending content from source (with [x]) is used
        assert!(
            result.contains("- [x] [#aaaa] old item"),
            "pending done state should come from source"
        );
        // old pending from target is gone
        assert!(
            !result.contains("- [ ] [#aaaa] old item"),
            "stale open pending should be replaced"
        );
    }

    #[test]
    fn splice_pending_noop_when_source_has_no_pending() {
        let target = "\
<!-- agent:exchange -->
response
<!-- /agent:exchange -->
<!-- agent:pending -->
- [ ] [#bbbb] task
<!-- /agent:pending -->
";
        let source = "\
<!-- agent:exchange -->
original
<!-- /agent:exchange -->
";
        let result = splice_pending_component(target, source);
        assert_eq!(
            result, target,
            "target should be returned unchanged when source has no pending"
        );
    }

    #[test]
    fn splice_pending_warns_when_target_missing_pending() {
        // target has no pending component; source does — should return target unchanged
        let target = "\
<!-- agent:exchange -->
response
<!-- /agent:exchange -->
";
        let source = "\
<!-- agent:exchange -->
original
<!-- /agent:exchange -->
<!-- agent:pending -->
- [x] [#cccc] done item
<!-- /agent:pending -->
";
        let result = splice_pending_component(target, source);
        assert_eq!(
            result, target,
            "target should be returned unchanged when target has no pending"
        );
    }
}

#[cfg(test)]
mod ack_content_snapshot_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_ack_content_sidecar_read() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "test-patch-abc123";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();
        let sidecar = ack_dir.join(format!("{patch_id}.md"));
        std::fs::write(&sidecar, "applied content from plugin").unwrap();

        let result = read_ack_content_sidecar(&project_root, patch_id).unwrap();
        assert_eq!(result, Some("applied content from plugin".to_string()));
        assert!(!sidecar.exists(), "sidecar should be deleted after read");
    }

    #[test]
    fn test_poll_sidecar_present_immediately() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-immediate";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();
        std::fs::write(ack_dir.join(format!("{patch_id}.md")), "immediate content").unwrap();

        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("immediate content".to_string()));
    }

    #[test]
    fn test_poll_sidecar_appears_after_delay() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-delayed";

        let ack_dir = project_root.join(".agent-doc/ack-content");
        std::fs::create_dir_all(&ack_dir).unwrap();

        // Spawn a thread that writes the sidecar after 50ms using atomic
        // rename to avoid the poll reading a partially-written file.
        let sidecar_path = ack_dir.join(format!("{patch_id}.md"));
        let tmp_path = ack_dir.join(format!("{patch_id}.md.tmp"));
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::write(&tmp_path, "delayed content").unwrap();
            std::fs::rename(&tmp_path, &sidecar_path).unwrap();
        });

        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(500),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert_eq!(result, Some("delayed content".to_string()));
    }

    #[test]
    fn test_poll_sidecar_timeout() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().to_path_buf();
        let patch_id = "poll-timeout";

        // Don't create the sidecar — poll should timeout
        std::fs::create_dir_all(project_root.join(".agent-doc/ack-content")).unwrap();

        let start = std::time::Instant::now();
        let result = poll_ack_content_sidecar(
            &project_root,
            patch_id,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(25),
        )
        .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, None);
        assert!(
            elapsed >= std::time::Duration::from_millis(100),
            "should wait at least the timeout"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(300),
            "should not wait much longer than timeout"
        );
    }

    #[test]
    fn normalization_fallback_uses_content_ours_when_sidecar_missing_prefix() {
        // When the sidecar is missing a ❯ prefix expected by normalize_prefix_lines,
        // try_ipc must fall back to content_ours for the snapshot (#jbpfx2).
        // Simulates the IntelliJ exact-match failure: plugin wrote sidecar without
        // the ❯ prefix, so content_ours (binary's authoritative state) is used.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\n<!-- agent:boundary:test-bnd-001 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");

        // content_ours has the ❯ prefix — binary's authoritative state
        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n❯ do #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
        let normalize_prefix_lines = vec!["do #jbpfx2".to_string()];

        // Simulate plugin: reads patch_id, writes sidecar WITHOUT prefix (bug), ACKs
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path) {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(pid) = json.get("patch_id").and_then(|v| v.as_str()) {
                                    // Write sidecar WITHOUT ❯ prefix (plugin failure)
                                    let bad_sidecar = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
                                    let _ = std::fs::write(
                                        ack_dir.join(format!("{pid}.md")),
                                        bad_sidecar,
                                    );
                                }
                            }
                        }
                        let _ = std::fs::remove_file(&path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),
            Some(content_ours),
            Some(normalize_prefix_lines.as_slice()),
            None,
        )
        .unwrap();
        assert!(
            result.success,
            "IPC should succeed when plugin consumes patch"
        );

        // Snapshot must use content_ours (has ❯ prefix), NOT the sidecar
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do #jbpfx2"),
            "snapshot must use content_ours with ❯ prefix; got: {}",
            snap
        );
    }
}

#[cfg(test)]
mod submodule_patch_routing_tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper: run a git command in `dir` with isolated user.name/email so the
    /// command works in CI environments that lack global git config. Asserts
    /// the command succeeds and prints stderr on failure.
    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "protocol.file.allow=always",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git command failed to spawn");
        assert!(
            out.status.success(),
            "git {:?} failed: stderr={}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn resolve_ipc_project_root_uses_nearest_agent_doc_for_submodule_file() {
        // Build a parent+submodule layout. Verify that a document inside the
        // submodule resolves to the SUBMODULE's .agent-doc/ root, not the
        // superproject. This matches the IDE plugin's resolveRootFor logic so
        // ack-content paths agree between Rust and Kotlin.
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        // Bootstrap a "remote" submodule repo with one committed file.
        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        // Bootstrap parent repo and add the submodule under src/submodule.
        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );

        // Submodule has its own .agent-doc — the IDE plugin registers it as a root.
        let submodule_root = parent.join("src/submodule");
        std::fs::create_dir_all(submodule_root.join(".agent-doc/patches")).unwrap();

        // Place a document inside the submodule.
        let doc = submodule_root.join("test.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let canonical = doc.canonicalize().unwrap();
        let project_root = resolve_ipc_project_root(&canonical);

        assert_eq!(
            project_root, submodule_root,
            "submodule file must resolve to submodule root (nearest .agent-doc/) to match IDE plugin routing"
        );

        // The superproject must NOT be returned — ack-content would diverge.
        assert_ne!(
            project_root, parent,
            "must not return the superproject — ack-content written at submodule root would not be found"
        );
    }

    // Note: a "not in git repo" fallback test is intentionally omitted because
    // /tmp tempdirs are typically nested inside the developer's checkout (the
    // agent-doc workspace itself is a git repo), so `git rev-parse
    // --show-toplevel` from `/tmp/...` walks up into the source tree. The
    // fallback path is exercised in production by non-git workspaces.

    /// Helper: start a fake socket listener that ACKs every message.
    /// Returns a handle that keeps the listener alive until dropped.
    fn start_fake_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let root_clone = root.clone();
            let _ = crate::ipc_socket::start_listener(&root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = v
                    .get("patch_id")
                    .and_then(|p| p.as_str())
                    .unwrap_or("unknown");
                // Write ack-content sidecar so poll_ack_content_sidecar succeeds
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                let file_path = v.get("file").and_then(|f| f.as_str()).unwrap_or("");
                let content = if !file_path.is_empty() {
                    std::fs::read_to_string(file_path).unwrap_or_default()
                } else {
                    String::new()
                };
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    /// Helper: wait for the socket listener to become connectable (up to 1s).
    fn wait_for_listener(project_root: &Path) {
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }

    #[test]
    fn try_ipc_routes_to_submodule_root_not_superproject() {
        // Verify that try_ipc routes patches to the SUBMODULE's own .agent-doc/
        // root, not the superproject. The submodule has its own .agent-doc/ so
        // the IDE plugin's resolveRootFor and Rust's find_project_root both
        // return the submodule root, keeping ack-content paths in sync.
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        // Bootstrap "remote" submodule repo with one commit.
        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

        // Bootstrap parent repo and add the submodule.
        git(&parent, &["init"]);
        std::fs::write(parent.join("README.md"), "parent").unwrap();
        git(&parent, &["add", "README.md"]);
        git(&parent, &["commit", "-m", "init"]);
        git(
            &parent,
            &[
                "submodule",
                "add",
                sub_src.to_string_lossy().as_ref(),
                "src/submodule",
            ],
        );

        // Submodule has its own .agent-doc/ — mirrors the real boost-client layout.
        let submodule_root = parent.join("src/submodule");
        std::fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(submodule_root.join(".agent-doc/crdt")).unwrap();

        // Start a fake socket listener on the SUBMODULE root (not the parent).
        let _listener = start_fake_listener(&submodule_root);
        wait_for_listener(&submodule_root);

        // Place a document inside the submodule.
        let doc = submodule_root.join("test.md");
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->content<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "test response");

        // try_ipc should route to the submodule's socket listener and succeed.
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "try_ipc should succeed via socket IPC routed to the submodule root"
        );

        // Verify the parent did NOT get the patch file.
        let parent_patches = parent.join(".agent-doc/patches");
        assert!(
            !parent_patches.exists(),
            "parent should NOT receive patch files — submodule routes to its own .agent-doc/"
        );
    }

    #[test]
    fn try_ipc_routes_to_git_toplevel_for_non_submodule() {
        // Verify that try_ipc routes patches to the git toplevel (not a
        // superproject) when the document lives in a plain git repo. This
        // exercises the git_toplevel_at path (step 2 in resolve_ipc_project_root).
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();

        // Initialize a plain git repo (not a submodule of anything).
        git(&root, &["init"]);
        std::fs::write(root.join("README.md"), "root").unwrap();
        git(&root, &["add", "README.md"]);
        git(&root, &["commit", "-m", "init"]);

        // Create .agent-doc structure.
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/crdt")).unwrap();

        // Start a fake socket listener.
        let _listener = start_fake_listener(&root);
        wait_for_listener(&root);

        // Create a document in a subdirectory.
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let doc = root.join("tasks/test.md");
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n<!-- agent:exchange -->content<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "response");

        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            result.success,
            "try_ipc should succeed via socket IPC routed to the git toplevel"
        );
    }

    #[test]
    fn cleanup_legacy_ipc_degraded_removes_marker() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let marker = root.join(".agent-doc/ipc-degraded");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::write(&marker, "").unwrap();
        assert!(marker.exists());
        cleanup_legacy_ipc_degraded(root);
        assert!(!marker.exists(), "legacy marker should be removed");
    }

    #[test]
    fn response_already_in_current_detects_plugin_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        // Plugin applied the response AND user added an edit
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User added this line.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "should detect plugin-applied response"
        );
    }

    #[test]
    fn response_already_in_current_false_when_not_applied() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — opus-4-6
This is the agent response.
With multiple lines.
<!-- /agent:exchange -->
";
        // Plugin did NOT apply — only user edits present
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
User typed something new.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should not detect when plugin hasn't applied"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_exchange() {
        let base = "No components here.";
        let content_ours = "No components here either.";
        let content_current = "Still no components.";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "should return false when no exchange components"
        );
    }

    #[test]
    fn response_already_in_current_false_when_no_changes() {
        let base = "\
<!-- agent:exchange patch=append -->
Same content.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, base, base),
            "should return false when ours equals base"
        );
    }
}

#[cfg(test)]
mod future_work_signal_tests {
    use super::*;

    #[test]
    fn detects_worth_revisiting() {
        let result =
            check_future_work_signals("This design is fine. Worth revisiting after v2.", false);
        assert_eq!(result, Some("worth revisiting"));
    }

    #[test]
    fn detects_future_work() {
        let result = check_future_work_signals("This is future work for the next release.", false);
        assert_eq!(result, Some("future work"));
    }

    #[test]
    fn detects_follow_up_needed() {
        let result = check_future_work_signals("Follow-up needed on the auth migration.", false);
        assert_eq!(result, Some("follow-up needed"));
    }

    #[test]
    fn no_warning_when_pending_add_provided() {
        let result = check_future_work_signals("Worth revisiting later.", true);
        assert_eq!(result, None);
    }

    #[test]
    fn no_warning_without_signals() {
        let result = check_future_work_signals("Everything is complete and working.", false);
        assert_eq!(result, None);
    }

    #[test]
    fn case_insensitive_detection() {
        let result = check_future_work_signals("WORTH REVISITING this approach.", false);
        assert_eq!(result, Some("worth revisiting"));
    }

    #[test]
    fn imperative_contract_rejects_status_only_response() {
        let file = Path::new("session.md");
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do #6zyp. run tests. build + install. commit + push\n";
        let err = enforce_imperative_response_contract_for_diff(
            file,
            diff,
            "### Re: task — gpt-5\nIn progress. Continuing now.",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("imperative document directive requires concrete execution evidence or a concrete blocker"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn imperative_contract_allows_concrete_blocker() {
        let file = Path::new("session.md");
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do #6zyp. run tests. build + install. commit + push\n";
        enforce_imperative_response_contract_for_diff(
            file,
            diff,
            "### Re: blocked — gpt-5\nBlocked by missing `OPENROUTER_API_KEY`; build cannot proceed.",
        )
        .expect("blocker response should be accepted");
    }

    #[test]
    fn imperative_contract_allows_execution_evidence() {
        let file = Path::new("session.md");
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+go\n";
        enforce_imperative_response_contract_for_diff(
            file,
            diff,
            "### Re: done — gpt-5\nVerification:\n- `cargo test --manifest-path src/agent-doc/Cargo.toml`\nCommit / push:\n- `abc1234`\n",
        )
        .expect("evidence response should be accepted");
    }

    #[test]
    fn lift_pending_nested_inside_exchange() {
        let doc = "\
<!-- agent:exchange patch=append -->
some exchange content
<!-- agent:pending -->
- [ ] [#abc1] task one
<!-- /agent:pending -->
<!-- /agent:exchange -->
";
        let result = lift_pending_from_exchange(doc).unwrap();
        // pending should be after exchange close, not inside it
        let ex_close = result.find("<!-- /agent:exchange -->").unwrap();
        let pend_open = result.find("<!-- agent:pending").unwrap();
        assert!(
            pend_open > ex_close,
            "pending (at {}) should be after exchange close (at {})",
            pend_open,
            ex_close
        );
        // exchange content preserved
        assert!(result.contains("some exchange content"));
        // pending content preserved
        assert!(result.contains("- [ ] [#abc1] task one"));
    }

    #[test]
    fn lift_pending_already_sibling_returns_none() {
        let doc = "\
<!-- agent:exchange patch=append -->
exchange content
<!-- /agent:exchange -->

<!-- agent:pending -->
- [ ] [#abc1] task
<!-- /agent:pending -->
";
        assert!(lift_pending_from_exchange(doc).is_none());
    }

    #[test]
    fn lift_pending_no_exchange_returns_none() {
        let doc = "\
<!-- agent:pending -->
- [ ] [#abc1] task
<!-- /agent:pending -->
";
        assert!(lift_pending_from_exchange(doc).is_none());
    }

    #[test]
    fn lift_pending_no_pending_returns_none() {
        let doc = "\
<!-- agent:exchange patch=append -->
exchange content
<!-- /agent:exchange -->
";
        assert!(lift_pending_from_exchange(doc).is_none());
    }

    #[test]
    fn lift_pending_preserves_surrounding_content() {
        let doc = "\
---
title: test
---

<!-- agent:exchange patch=append -->
response here
<!-- agent:pending -->
- [ ] [#x1] item
<!-- /agent:pending -->
<!-- /agent:exchange -->

## Footer
";
        let result = lift_pending_from_exchange(doc).unwrap();
        assert!(result.contains("---\ntitle: test\n---"));
        assert!(result.contains("response here"));
        assert!(result.contains("## Footer"));
        // Verify ordering
        let ex_close = result.find("<!-- /agent:exchange -->").unwrap();
        let pend_open = result.find("<!-- agent:pending").unwrap();
        let footer = result.find("## Footer").unwrap();
        assert!(pend_open > ex_close, "pending after exchange close");
        assert!(footer > pend_open, "footer after pending");
    }
}

#[cfg(test)]
mod verify_sidecar_normalization_tests {
    use super::{enforce_orchestrate_template_patch_contract, verify_sidecar_normalization};

    #[test]
    fn empty_targets_always_passes() {
        assert!(verify_sidecar_normalization("anything", &[]));
    }

    #[test]
    fn all_targets_prefixed() {
        let sidecar = "some line\n❯ do #task1\n❯ do #task2\nother line";
        let targets = vec!["do #task1".to_string(), "do #task2".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn missing_prefix_detected() {
        let sidecar = "some line\n❯ do #task1\ndo #task2\nother line";
        let targets = vec!["do #task1".to_string(), "do #task2".to_string()];
        assert!(!verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn trailing_whitespace_mismatch_tolerated() {
        let sidecar = "❯ do #task1\n❯ do #task2  \n";
        let targets = vec!["do #task1  ".to_string(), "do #task2".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn blank_targets_skipped() {
        let sidecar = "❯ do #task1\nother";
        let targets = vec!["do #task1".to_string(), "".to_string(), "   ".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn target_at_start_of_sidecar() {
        let sidecar = "❯ first line\nrest";
        let targets = vec!["first line".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn target_not_in_sidecar_at_all() {
        let sidecar = "line one\nline two\n";
        let targets = vec!["nonexistent line".to_string()];
        assert!(!verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn sidecar_missing_prefix_when_target_has_trailing_whitespace() {
        // Simulates the IntelliJ trailing-space bug: binary sent "do the thing "
        // (trailing space), IntelliJ stripped to "do the thing" in the buffer,
        // plugin's original exact-match failed silently, sidecar has no prefix.
        // verify_sidecar_normalization must detect this.
        let sidecar = "some other line\ndo the thing\nmore content";
        let targets = vec!["do the thing ".to_string()];
        assert!(
            !verify_sidecar_normalization(sidecar, &targets),
            "missing prefix must be detected even when target has trailing whitespace"
        );
    }

    #[test]
    fn orchestrate_contract_requires_exchange_patch() {
        let patches = vec![crate::template::PatchBlock::new("status", "updated")];
        let err = enforce_orchestrate_template_patch_contract(Some("orchestrate"), &patches, "")
            .unwrap_err();
        assert!(err.to_string().contains("patch:exchange"));
    }

    #[test]
    fn orchestrate_contract_rejects_unmatched_transcript() {
        let patches = vec![crate::template::PatchBlock::new("exchange", "ok")];
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &patches,
            "### Re: raw transcript — gpt-5",
        )
        .unwrap_err();
        assert!(err.to_string().contains("raw unmatched content"));
    }

    #[test]
    fn orchestrate_contract_allows_exchange_only_patch() {
        let patches = vec![crate::template::PatchBlock::new("exchange", "ok")];
        enforce_orchestrate_template_patch_contract(Some("orchestrate"), &patches, "")
            .expect("exchange-only orchestrate patch should be accepted");
    }
}

#[cfg(test)]
mod precommit_pending_capture_tests {
    use std::fs;

    fn setup_precommit(
        root: &std::path::Path,
        frontmatter: &str,
        response: &str,
        had_pending_mutations: bool,
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = format!("{frontmatter}## Exchange\n\nHello\n");
        fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        if had_pending_mutations {
            crate::cycle_state::mark_pending_mutations(&doc).unwrap();
        }
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(&content),
            Some(&content),
        )
        .unwrap();
        doc
    }

    fn setup_precommit_with_pending(
        root: &std::path::Path,
        frontmatter: &str,
        response: &str,
        pending_body: &str,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let content = format!(
            "{frontmatter}<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n{pending_body}<!-- /agent:pending -->\n"
        );
        fs::write(&doc, &content).unwrap();
        crate::snapshot::save(&doc, &content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&content), Some(&content)).unwrap();
        crate::capture::capture_response(&doc, response).unwrap();
        if !pending_done_ids.is_empty() {
            crate::cycle_state::record_pending_done_ids(
                &doc,
                &pending_done_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(&content),
            Some(&content),
        )
        .unwrap();
        doc
    }

    #[test]
    fn precommit_blocks_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("[finalize] pre-commit gate"));
        assert!(err.to_string().contains("--pending-add"));
    }

    #[test]
    fn precommit_passes_with_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            true,
        );

        super::precommit_pending_capture_check(&doc)
            .expect("should pass when pending mutations were recorded");
    }

    #[test]
    fn precommit_inactive_in_warn_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        super::precommit_pending_capture_check(&doc)
            .expect("should pass in warn mode — only post-commit session-check fires");
    }

    #[test]
    fn precommit_inactive_in_default_mode() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        super::precommit_pending_capture_check(&doc).expect("should pass in default (warn) mode");
    }

    #[test]
    fn precommit_respects_suppression_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: recommendations — opus-4-6\n\n<!-- no-pending-capture -->\n## Recommendations\n1. Add regression coverage\n2. Update the spec\n",
            false,
        );

        super::precommit_pending_capture_check(&doc)
            .expect("should pass when suppression marker present");
    }

    #[test]
    fn precommit_blocks_single_unresolved_bug_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: tmux pane closure — opus-4-6\n\nBecause that session was still hitting the older tmux route/sync cleanup bug that #4qgx was meant to close.\n",
            false,
        );

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("[finalize] pre-commit gate"));
        assert!(err.to_string().contains("--pending-add"));
    }

    #[test]
    fn precommit_pending_done_blocks_by_default_for_session_docs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        let err = super::precommit_pending_done_check(&doc).unwrap_err();
        assert!(err.to_string().contains("[finalize] pre-commit gate"));
        assert!(err.to_string().contains("#4qja"));
        assert!(err.to_string().contains("--pending-done 4qja"));
    }

    #[test]
    fn precommit_pending_done_passes_when_id_was_recorded() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &["4qja"],
        );

        super::precommit_pending_done_check(&doc)
            .expect("should pass when matching pending-done was recorded");
    }

    #[test]
    fn precommit_pending_done_warn_mode_skips_precommit_block() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\npending_done_guard: warn\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        super::precommit_pending_done_check(&doc)
            .expect("warn mode should defer to post-commit session-check");
    }

    #[test]
    fn precommit_pending_done_respects_suppression_marker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\n<!-- no-pending-done-guard -->\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        super::precommit_pending_done_check(&doc)
            .expect("suppression marker should disable the pre-commit pending-done gate");
    }
}

#[cfg(test)]
mod pending_patch_normalization_tests {
    use super::normalize_backlog_patch_response;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn doc_with_backlog(root: &TempDir, backlog_body: &str) -> (PathBuf, String) {
        let doc = root.path().join("doc.md");
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n{backlog_body}<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, &content).unwrap();
        (doc, content)
    }

    #[test]
    fn normalize_pending_patch_repairs_lone_bare_placeholder() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#] repair placeholder\n",
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("lone bare placeholder should be normalized");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("repair placeholder"));
        assert!(rewritten.contains("- [ ] [#"));
        assert!(
            !rewritten.contains("- [ ] [#] repair placeholder"),
            "bare placeholder must not persist: {}",
            rewritten
        );
    }

    #[test]
    fn normalize_pending_patch_rejects_stacked_leading_id_prefixes() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#] [#ship1] release checklist\n",
        )];

        let err =
            match normalize_backlog_patch_response(&doc, &content, patches, String::new(), false) {
                Ok(_) => panic!("stacked leading id prefixes should be rejected"),
                Err(err) => err,
            };
        let msg = err.to_string();
        assert!(
            msg.contains("pending/backlog patch"),
            "unexpected error: {}",
            msg
        );
        assert!(
            msg.contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn write_flags_allow_replace_bypasses_enforcement() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "- [ ] [#aaaa] existing\n");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#zzzz] new\n",
        )];
        normalize_backlog_patch_response(&doc, &content, patches.clone(), String::new(), true)
            .expect("allow_replace=true should bypass enforcement");
        super::enforce_no_replace_pending(&patches, true)
            .expect("allow=true should bypass enforcement");
    }

    #[test]
    fn write_flags_default_rejects_replace_pending() {
        let tmp = TempDir::new().unwrap();
        let (_doc, _content) = doc_with_backlog(&tmp, "- [ ] [#aaaa] existing\n");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#zzzz] new\n",
        )];
        super::enforce_no_replace_pending(&patches, false)
            .expect_err("allow=false should reject backlog replacement");
    }
}
