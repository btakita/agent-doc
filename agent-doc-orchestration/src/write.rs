//! # Module: write
//!
//! All write paths for agent responses: inline append, template patch, stream
//! (CRDT), IPC-to-IDE-plugin, and recovery helpers. Each path follows the same
//! invariant: save pending → acquire lock → compute `content_ours` (baseline +
//! response) → merge with any concurrent user edits → atomic write → save a
//! snapshot that is usually `final_content` (the actual post-merge disk state),
//! but preserves `content_ours` when an explicit baseline is active and
//! concurrent user edits must remain visible next cycle → clear pending.
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
//!   `template_io::apply_patches`, then performs the same lock/merge/atomic-write
//!   cycle as `run`.
//!
//! - `run_stream`: CRDT stream-flush mode. Like `run_template` but uses
//!   `merge::merge_contents_crdt` for conflict-free merge. Saves both a text
//!   snapshot and a CRDT state snapshot after every flush. Supports IPC-first
//!   writes: when `.agent-doc/patches/` exists and `--force-disk` is not set,
//!   tries `try_ipc` first; on timeout or missing proof, retains the pending
//!   response/queued patch and fails closed so the operator retries through the
//!   editor path instead of writing behind the active buffer.
//!
//! - `run_ipc`: explicit IPC-only mode. Serialises patches as JSON to
//!   `.agent-doc/patches/<hash>.json`, polls for the plugin to delete the file
//!   as ACK (2 s timeout), then fails closed without direct disk fallback when
//!   the write is unproven.
//! - `run_command(options, commit_mode)`: shared CLI entrypoint for `write` and
//!   `finalize`. `finalize` is always strict. `write --commit` stays
//!   best-effort for non-session documents and `--pending-only`, but upgrades to
//!   the same strict commit-boundary contract as `finalize` when the target file
//!   is a real session document (`agent_doc_session` / legacy `session`) and the
//!   command is writing a response. A bare session-document `write` may still
//!   preserve the response body/capture for recovery, but it must fail closed
//!   instead of returning success while the cycle remains open at
//!   `response_captured` / `write_applied`.
//!
//! - `try_ipc`: low-level IPC helper used by `run_stream`. Writes a JSON patch
//!   file (component patches + optional frontmatter + `reposition_boundary`
//!   flag) and polls for ACK. Returns `Ok(true)` on success, `Ok(false)` on
//!   timeout. Safe to call unconditionally — returns `false` immediately when
//!   `.agent-doc/patches/` does not exist. Synthesises a boundary-aware
//!   exchange patch when no explicit patches exist but unmatched content and a
//!   boundary marker are present.
//!
//! - `try_ipc_full_content`: disabled full-document editor replacement path.
//!   It preserves the terminal committed-cycle cleanup guard, rejects
//!   template/component scope for diagnostics, then returns `false` before
//!   emitting any socket/file payload. Callers fall back to the guarded
//!   disk/snapshot repair path.
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
//! - `agent_doc_turn::heuristics::future_work_signal`: detects deferred-work
//!   phrases in responses while `run_stream` owns only the warning side effect
//!   when the caller did not provide `--pending-add`.
//!
//! - `enforce_imperative_response_contract(file, baseline, current, response)`:
//!   when the current document diff contains imperative user directives
//!   (`do #id`, `run tests`, `build + install`, `commit + push`, or approval
//!   words like `go`), rejects status-only/meta responses using
//!   `agent_doc_turn::response_text::response_satisfies_imperative_contract`.
//!   The write path keeps diff inspection, ops-log emission, and rejection
//!   formatting as the binary-side backstop for the executable-directive
//!   contract.
//!
//! - `agent_doc_template::sanitize`: escapes `<!-- agent:NAME -->` and
//!   `<!-- /agent:NAME -->` markers appearing in patch content before the
//!   component parser can treat them as real delimiters.
//!
//! - `agent_doc_turn::response_text`: strips leading `## Assistant` and
//!   trailing `## User` headings from append responses before this module adds
//!   the canonical transcript headings.
//!
//! - `atomic_write_pub`: public thin wrapper around the internal `atomic_write`
//!   (write to temp file + rename). Used by `compact` and other modules.
//!
//! ## Agentic Contracts
//!
//! - Snapshot invariant: the snapshot saved after every write normally contains
//!   `final_content` (the actual post-merge disk state), not `content_ours`.
//!   This eliminates ghost diffs caused by stale baselines (e.g. streaming
//!   checkpoints with an outdated baseline). Narrow exception: when the caller
//!   supplied an explicit baseline and concurrent prompt-bearing or
//!   non-`agent:exchange` user edits changed the merged disk state, the snapshot
//!   stays at `content_ours` so those late user edits remain visible to the next
//!   diff cycle instead of being folded into the just-finished turn.
//! - Once a response survives strict pre-write closeout gates, it is saved to
//!   the pending store before any document mutation and cleared only after a
//!   successful write, so an interrupted write is recoverable.
//! - Pre-response snapshot is captured from the live document state while the
//!   advisory doc lock is held, so `undo` restores the exact on-disk content
//!   that existed immediately before the local write path applied the response.
//! - All writes are atomic (temp file + rename). Partial writes never corrupt
//!   the document.
//! - Advisory file lock (`flock`) serialises concurrent writes to the same
//!   document; the lock is dropped immediately after `atomic_write`.
//! - `try_ipc` returns `false` immediately (no I/O wait) when
//!   `.agent-doc/patches/` does not exist. `try_ipc_full_content` always
//!   returns `false` after cleanup/diagnostic guards because whole-document IPC
//!   is disabled.
//! - IPC writes include `reposition_boundary: true` so the plugin moves the
//!   boundary marker to end-of-exchange in the same Document API transaction as
//!   the patch, avoiding a second round-trip.
//! - CRDT snapshots are saved from the merged state (not from `content_ours`)
//!   so subsequent merges use the correct shared ancestor, preventing
//!   character-level duplication across cycles.
//! - `agent_doc_template::sanitize` is applied to every patch block before any
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
//! - `try_ipc_full_content_returns_false`: full-content IPC is disabled and
//!   returns `false` without emitting socket/file payloads.
//! - `sanitize_escapes_open_agent_tag`: `<!-- agent:exchange -->` inside patch
//!   content is escaped to `&lt;!-- agent:exchange --&gt;`.
//! - *(aspirational)* `run_stream_crdt_merge`: concurrent user keystroke during
//!   stream flush → CRDT merge produces text containing both agent response and
//!   user addition without character interleaving.
//! - *(aspirational)* `ipc_fallback_on_timeout`: `run_stream` with IPC timeout
//!   exits with code 75 and leaves a patch file for deferred plugin pickup.
//! - `detects_worth_revisiting`: response with "Worth revisiting" and no
//!   pending-add → returns `Some("worth revisiting")`.
//! - `future_work_signal`: response with "future work" and no pending-add state
//!   → warns from the write path.
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
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use agent_doc_document::write_normalization::{
    SplicePendingComponentWarning, cleanup_resolved_backlog_prompts_after_response,
    count_code_fence_openings, latest_response_block_missing_from_current,
    lift_pending_from_exchange, splice_pending_component,
    splice_response_block_into_current_exchange, strip_boundary_for_dedup,
};
use agent_doc_document_realtime::write_policy::response_already_in_current;
use agent_doc_element::element::{self, is_backlog_component};
use agent_doc_element_exchange::{
    exchange_has_live_user_edit, exchange_prompt_prefix_count, exchange_prompt_text_duplicated,
    repair_response_precedes_prompt_in_exchange as repair_response_prompt_order_in_exchange,
    response_precedes_prompt_in_exchange, strip_prompt_prefix_from_response_body_first_lines,
};
use agent_doc_fs::find_project_root;
use agent_doc_queue::queue_consume::{
    queue_consumption_allowed_for_response, queue_targeted_completion_id_for_current_head,
};
use agent_doc_queue::queue_prompt_drift::{
    dropped_queue_prompt_lines_after_content_ours, preserve_content_ours_over_live_queue_deletions,
};
use agent_doc_workflow::session_cycle::{
    FinalizeRerunCommand, compact_command_hint, finalize_rerun_command_base,
    group_pending_add_targets, parse_id_order, parse_tracked_work_edits,
    pending_kept_open_ids_from_mutations,
};

use crate::flow::document_mutation::{
    TemplateStructureGuardReason, log_template_structure_guard_event,
};
use agent_doc_flow::types::FlowOutcome;
use agent_doc_frontmatter::frontmatter;
use agent_doc_template as template;

use crate::{merge, repair, sessions, snapshot};
use agent_doc_template::stale_baseline::{
    exchange_append_patch_can_rebase_to_head, is_append_mode_component, is_stale_baseline,
    patch_touches_exchange,
};
use agent_doc_turn::response_replay::{dedupe_responses, response_materialized_in_content};

thread_local! {
    static RESPONSE_STDIN_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

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
    pub pending_add_to: Vec<String>,
    pub pending_add_gated: Vec<String>,
    /// `#ah0s`: repeated `<id> <text>` pairs — insert after the anchor id.
    pub pending_add_after: Vec<String>,
    /// `#ah0s`: repeated `<id> <text>` pairs — insert before the anchor id.
    pub pending_add_before: Vec<String>,
    /// `#ah0s`: tail-insert items (`--pending-add-back` / `--pending-append`).
    pub pending_add_back: Vec<String>,
    pub icebox_add: Vec<String>,
    /// Repeated `<id> <text>` pairs — insert after the anchor id in `agent:icebox`.
    pub icebox_add_after: Vec<String>,
    /// Repeated `<id> <text>` pairs — insert before the anchor id in `agent:icebox`.
    pub icebox_add_before: Vec<String>,
    /// Tail-insert items into `agent:icebox`.
    pub icebox_add_back: Vec<String>,
    /// Edit an icebox item: `id=new text` (repeatable).
    pub icebox_edit: Vec<String>,
    /// Clear all icebox items.
    pub icebox_clear: bool,
    /// Reorder icebox items by comma-separated hash ids.
    pub icebox_reorder: Option<String>,
    pub pending_done: Vec<String>,
    pub pending_edit: Vec<String>,
    pub pending_clear: bool,
    pub pending_reorder: Option<String>,
    pub pending_gate: Vec<String>,
    pub pending_ungate: Vec<String>,
    pub pending_resolve_gate: Vec<String>,
    pub pending_set_gate_type: Vec<String>,
    pub pending_set_verify: Vec<String>,
    pub review_add: Vec<String>,
    pub review_edit: Vec<String>,
    /// `#reviewrm`: ids to delete from `agent:review` (clears stale/duplicate
    /// entries, including same-id collisions, without an ambiguous edit-by-id).
    pub review_remove: Vec<String>,
    /// `#reviewrm`: ids to resolve out of `agent:review` into `agent:done`.
    pub review_resolve: Vec<String>,
    /// Queue-head completion ids proven by the response/route but not backed by
    /// a `pending` mutation such as `--done`.
    pub queue_completion_ids: Vec<String>,
    pub allow_replace_pending: bool,
    pub pending_only: bool,
    pub status: Option<String>,
    /// Optional CLI override for the agent-doc lint gate. `None` means
    /// "no CLI override; use frontmatter/config/default precedence".
    pub lint_override: Option<agent_doc_frontmatter::lint::LintCliMode>,
    /// Cross-repo sibling commits to run after a successful session-doc commit.
    /// Must align positionally with `commit_sibling_message`. Empty vector means
    /// "no sibling commits".
    pub commit_sibling: Vec<PathBuf>,
    /// Commit message for each `commit_sibling` entry (same length, same order).
    pub commit_sibling_message: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct WriteFlags {
    pub allow_replace_pending: bool,
    pub has_pending_add: bool,
    pub has_pending_done: bool,
    pub has_pending_mutation: bool,
    pub pending_done_ids: Vec<String>,
    pub queue_completion_ids: Vec<String>,
    pub pending_kept_open_ids: Vec<String>,
    pub strict_closeout: bool,
    pub force_disk: bool,
    pub rerun_command_base: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitMode {
    None,
    BestEffort,
    Required,
}

pub fn run_command_with_response(
    options: CommandOptions,
    commit_mode: CommitMode,
    response: String,
) -> Result<()> {
    let previous = RESPONSE_STDIN_OVERRIDE.with(|slot| slot.replace(Some(response)));
    let result = run_command(options, commit_mode);
    RESPONSE_STDIN_OVERRIDE.with(|slot| {
        slot.replace(previous);
    });
    result
}

fn read_response_input() -> Result<String> {
    if let Some(response) = RESPONSE_STDIN_OVERRIDE.with(|slot| slot.borrow_mut().take()) {
        return Ok(response);
    }

    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;
    Ok(response)
}

fn log_resolved_backlog_prompt_cleanup(file: &Path, removed_total: usize) {
    crate::ops_log::log_op(
        file,
        &format!(
            "cleanup_resolved_backlog_prompts file={} removed={}",
            file.display(),
            removed_total
        ),
    );
    eprintln!(
        "[write] removed {} resolved prompt target(s) from backlog component(s)",
        removed_total
    );
}

fn log_splice_pending_component_warning(warning: &SplicePendingComponentWarning) {
    match warning {
        SplicePendingComponentWarning::SourceParseFailed(err) => {
            eprintln!(
                "[write] WARNING: splice_pending: failed to parse source components: {}",
                err
            );
        }
        SplicePendingComponentWarning::TargetParseFailed(err) => {
            eprintln!(
                "[write] WARNING: splice_pending: failed to parse target components: {}",
                err
            );
        }
        SplicePendingComponentWarning::TargetMissingBacklogComponent => {
            eprintln!(
                "[write] WARNING: splice_pending: source has tracked backlog content but target does not - pending mutations may be lost on IPC fallback"
            );
        }
    }
}

/// Resolve the merge baseline (the common ancestor handed to the finalize merge).
///
/// `#mps` Rung 3 (flip): when the model-projected-baseline cutover is enabled
/// (`AGENT_DOC_MPS=1`), source the base by projecting the model overlay pinned at
/// preflight (`snapshot::load_baseline_model`), cross-checking against — and
/// falling back to — the legacy `.md` baseline. The `.md` read stays the fail-safe
/// (and, with the flag on, the derived cross-check cache; Rung 4). With the flag
/// off this is byte-for-byte the legacy `.md` path.
fn read_explicit_baseline(file: &Path, baseline_file: Option<&Path>) -> Result<Option<String>> {
    let md_content = read_explicit_baseline_md(file, baseline_file)?;

    if crate::snapshot::mps_enabled() {
        match crate::snapshot::load_baseline_model(file, md_content.as_deref()) {
            Ok(Some(projection)) => return Ok(Some(projection)),
            Ok(None) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "mps_baseline_resolve source=md_fallback reason=no_model file={}",
                        file.display()
                    ),
                );
            }
            Err(e) => {
                // Fail-safe: a model-baseline error must never break finalize —
                // fall back to the legacy `.md` baseline and log loudly.
                eprintln!("[write] #mps baseline model resolve failed, using .md baseline: {e}");
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "mps_baseline_resolve source=md_fallback reason=model_error file={}",
                        file.display()
                    ),
                );
            }
        }
    }

    Ok(md_content)
}

/// Legacy `.md` baseline read (the pre-`#mps` behavior). See [`read_explicit_baseline`].
fn read_explicit_baseline_md(file: &Path, baseline_file: Option<&Path>) -> Result<Option<String>> {
    let Some(path) = baseline_file else {
        return Ok(None);
    };

    match std::fs::read_to_string(path) {
        Ok(content) => return Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e)
                .with_context(|| format!("failed to read baseline file {}", path.display()));
        }
    }

    if let Err(e) = crate::snapshot::try_migrate_renamed(file) {
        eprintln!("[write] warning: rename migration before baseline fallback failed: {e}");
    }

    let migrated_path = agent_doc_fs::baseline_path_for(file).with_context(|| {
        format!(
            "failed to resolve migrated baseline path for {}",
            file.display()
        )
    })?;
    if migrated_path == path {
        anyhow::bail!(
            "failed to read baseline file {}: file not found",
            path.display()
        );
    }

    match std::fs::read_to_string(&migrated_path) {
        Ok(content) => {
            eprintln!(
                "[write] baseline file {} was missing; using migrated baseline {}",
                path.display(),
                migrated_path.display()
            );
            Ok(Some(content))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "failed to read baseline file {}: file not found; migrated baseline {} was also missing",
                path.display(),
                migrated_path.display()
            )
        }
        Err(e) => Err(e).with_context(|| {
            format!(
                "failed to read migrated baseline file {} after {} was missing",
                migrated_path.display(),
                path.display()
            )
        }),
    }
}

/// Pre-write gate for an explicit-baseline finalize against an already-`committed`
/// cycle (`#finalize-stale-baseline-reopen-friction`).
///
/// The cycle phase is `Committed` whenever the prior finalize already closed and
/// no fresh `preflight` reopened the cycle (for example a post-commit retry or a
/// second response in the same turn). Historically this always failed
/// closed with "run `agent-doc preflight` and retry", forcing a manual reopen even
/// for a legitimately new response.
///
/// Returns:
/// - `Ok(None)` — no gate applies (open cycle, non-finalize mode, or no baseline);
///   the caller reads the explicit baseline normally.
/// - `Ok(Some(fresh_baseline))` — a genuinely new response was supplied after the
///   commit, so the cycle is auto-reopened from `HEAD` and the caller must diff the
///   new response against this `HEAD` baseline (the stale explicit baseline is
///   discarded). This is exactly what a manual `preflight` reopen would do.
/// - `Err(..)` — fail closed. A true replay (the incoming response is already
///   materialized in `HEAD`) must not be re-applied (duplicate-block risk); an
///   empty/repair response or a non-git document cannot be safely auto-reopened.
fn guard_no_explicit_baseline_replay_after_committed_cycle(
    file: &Path,
    commit_mode: CommitMode,
    baseline_file: Option<&Path>,
) -> Result<Option<String>> {
    if commit_mode != CommitMode::Required || baseline_file.is_none() {
        return Ok(None);
    }

    let Some(cycle_id) = cycle_already_committed(file) else {
        return Ok(None);
    };

    // Read the incoming response now and re-stash it so the downstream write path
    // (which calls `read_response_input` once for the resolved mode) still sees it.
    let response = read_response_input()?;
    RESPONSE_STDIN_OVERRIDE.with(|slot| {
        slot.borrow_mut().replace(response.clone());
    });

    let head = crate::git::show_head(file).ok().flatten();

    let reject = |reason: &str| -> anyhow::Error {
        crate::ops_log::log_op(
            file,
            &format!(
                "explicit_baseline_replay_rejected file={} cycle_id={} reason={reason}",
                file.display(),
                cycle_id
            ),
        );
        anyhow::anyhow!(
            "[finalize] pre-write gate: the latest agent-doc cycle `{}` for {} is already `committed`; refusing to apply an explicit-baseline response without reopening the binary-owned write/commit path. Run `agent-doc preflight {}` and retry with the new baseline_file.",
            cycle_id,
            file.display(),
            file.display()
        )
    };

    // True replay: the incoming response is already committed in HEAD. Re-applying
    // it risks a duplicate block, and the work is already durable — fail closed.
    if let Some(head) = head.as_deref()
        && response_materialized_in_content(&response, head)
    {
        return Err(reject("response_already_in_head"));
    }

    // Empty/repair response (pending-only closeout) — keep failing closed; the
    // auto-reopen path is for genuinely new assistant responses only.
    if response.trim().is_empty() {
        return Err(reject("empty_response"));
    }

    // No HEAD (non-git or empty repo) — cannot mint a safe HEAD baseline.
    let Some(head) = head else {
        return Err(reject("no_head_baseline"));
    };

    // Genuinely new response after a committed cycle: auto-reopen a fresh
    // `preflight_started` cycle from HEAD instead of forcing a manual preflight,
    // and hand the caller the HEAD baseline so the new response diffs against the
    // actual committed state (the stale explicit baseline is discarded).
    crate::cycle_state::start_preflight(file, Some(&head), Some(&head))?;
    crate::ops_log::log_op(
        file,
        &format!(
            "explicit_baseline_replay_auto_reopened file={} cycle_id={}",
            file.display(),
            cycle_id
        ),
    );
    eprintln!(
        "[finalize] cycle `{}` was already committed; auto-reopened a fresh cycle for the new response (baseline refreshed to HEAD) instead of requiring a manual preflight.",
        cycle_id
    );
    Ok(Some(head))
}

fn ensure_pending_add_target(target: &Path) -> Result<()> {
    if !target.exists() {
        anyhow::bail!(
            "--backlog-add-to target file not found (also --pending-add-to target file not found): {}",
            target.display()
        );
    }
    let content = std::fs::read_to_string(target).with_context(|| {
        format!(
            "failed to read --backlog-add-to target {}",
            target.display()
        )
    })?;
    let components = agent_doc_element::element::parse(&content).with_context(|| {
        format!(
            "failed to parse --backlog-add-to target {}",
            target.display()
        )
    })?;
    if !components
        .iter()
        .any(|component| agent_doc_element::element::is_backlog_component(&component.name))
    {
        anyhow::bail!(
            "--backlog-add-to target {} has no agent:backlog/agent:pending component",
            target.display()
        );
    }
    Ok(())
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

fn enforce_review_done_guard(file: &Path, id: &str) -> Result<()> {
    let mode = crate::session_check::resolve_review_done_guard_mode(file)?;
    if mode == agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(());
    }
    let Some(component_name) = crate::backlog_cmd::open_item_component_name(file, id)? else {
        return Ok(());
    };
    if agent_doc_element::element::is_review_component(&component_name) {
        return Ok(());
    }

    let normalized = agent_doc_element_backlog::backlog::normalize_pending_id(id);
    let message = format!(
        "review_done_guard: --done #{} resolved from agent:{} instead of agent:review; run --backlog-gate {} first or set review_done_guard = \"off\"",
        normalized, component_name, normalized
    );
    match mode {
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Warn => {
            eprintln!("[write] warning: {}", message);
            Ok(())
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Strict => {
            log_closeout_guard(
                file,
                agent_doc_flow::types::FlowStage::PreWriteGuard,
                agent_doc_flow::types::FlowOutcome::Blocked,
                agent_doc_turn::closeout_guard::CloseoutGuardReason::ReviewDoneSourceNotReviewed,
            );
            anyhow::bail!("{}", message)
        }
        agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode::Off => Ok(()),
    }
}

pub fn guard_no_exchange_compaction_request_for_diff(file: &Path, diff_text: &str) -> Result<()> {
    if agent_doc_diff::detect_exchange_compaction_request(diff_text) {
        anyhow::bail!(
            "bare `compact exchange` directive detected in the current diff; close this turn \
             through the binary compaction path instead: `{}` \
             (optionally add `--message ...` for a custom checkpoint summary)",
            compact_command_hint(file)
        );
    }
    Ok(())
}

fn guard_no_exchange_compaction_request_between(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
) -> Result<()> {
    let baseline_owned = baseline
        .map(ToOwned::to_owned)
        .or_else(|| snapshot::load(file).ok().flatten());
    let Some(base) = baseline_owned.as_deref() else {
        return Ok(());
    };
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(base, current_content) else {
        return Ok(());
    };
    guard_no_exchange_compaction_request_for_diff(file, &diff_text)
}

/// `#bare-write-captured-uncommitted`: true when a bare `agent-doc write` placed
/// or preserved an assistant response body this cycle that HEAD does not yet have.
/// Such a write must cross the same commit boundary as `write --commit` instead of
/// stranding the visible response outside the binary-owned closeout.
///
/// Two confirmations, both required:
/// 1. The cycle is still open at `response_captured`/`write_applied` — a response
///    placement phase reached by this write (terminal/preflight-only states are
///    excluded). This covers both the captured path and the synthetic-cycle stream
///    path that only marks `write_applied` without a durable capture.
/// 2. The working tree carries an assistant response heading that HEAD does not —
///    the content-level proof that a response is genuinely uncommitted, so a
///    pending/status-only bare write (no response placed) is never force-committed.
fn bare_write_placed_response_body(file: &Path) -> Result<bool> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(false);
    };
    if !state.is_open() {
        return Ok(false);
    }
    if !matches!(
        state.phase,
        agent_doc_turn::CyclePhase::ResponseCaptured | agent_doc_turn::CyclePhase::WriteApplied
    ) {
        return Ok(false);
    }
    let current = std::fs::read_to_string(file)
        .context("failed to read document for bare-write response-body detection")?;
    let Some(head) = crate::git::show_head(file)? else {
        return Ok(false);
    };
    Ok(
        agent_doc_turn::document_drift::detect_bypassed_response_write_between(&head, &current)
            .is_some(),
    )
}

fn consume_queue_prompts_for_done_ids_closeout(
    file: &Path,
    done_ids: &[String],
    force_disk: bool,
) -> Result<Option<QueueConsumptionOutcome>> {
    if force_disk {
        consume_queue_prompts_with_outcome(file, done_ids, true)
    } else {
        consume_queue_prompts_for_done_ids_with_outcome(file, done_ids)
    }
}

pub fn run_command(options: CommandOptions, commit_mode: CommitMode) -> Result<()> {
    let file = options.file.as_path();

    if let Some(ref origin) = options.origin {
        crate::ops_log::log_op(
            file,
            &format!("write_origin file={} origin={}", file.display(), origin),
        );
    }
    // #jb-tsift-pane-sync diagnostic: capture a write/commit to `file` that is
    // executing inside a tmux pane owning a different document (the
    // cross-document contamination vector — e.g. a tsift.md-owned pane
    // committing agent-doc-bugs2.md's response).
    crate::sync::log_cross_document_execution_context(file, "write");

    // #manual-queue-head-loss: extend the `#queue-clear-unrun-items` removal-proof
    // anchor to user queue heads inserted AFTER preflight (for example a
    // `do [#id]` typed into `agent:queue` during a stalled / busy-pane dispatch
    // attempt). Read the live working-tree document here — before any pending
    // mutation or queue convergence mutates it — and union its directive heads
    // into the recorded set so closeout cannot silently drop a runnable manual
    // head whose backlog item is still open. Best-effort: an absent cycle state
    // is a no-op, and a read failure is logged (the real write path below reads
    // the document again and surfaces any genuine I/O error).
    match std::fs::read_to_string(file) {
        Ok(live_doc) => {
            if let Err(err) = crate::cycle_state::observe_live_queue_heads(file, &live_doc) {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "observe_live_queue_heads_failed file={} err={}",
                        file.display(),
                        err
                    ),
                );
            }
        }
        Err(err) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "observe_live_queue_heads_read_failed file={} err={}",
                    file.display(),
                    err
                ),
            );
        }
    }

    let has_pending_ops = !options.pending_add.is_empty()
        || !options.pending_add_to.is_empty()
        || !options.pending_add_gated.is_empty()
        || !options.pending_add_after.is_empty()
        || !options.pending_add_before.is_empty()
        || !options.pending_add_back.is_empty()
        || !options.icebox_add.is_empty()
        || !options.icebox_add_after.is_empty()
        || !options.icebox_add_before.is_empty()
        || !options.icebox_add_back.is_empty()
        || !options.icebox_edit.is_empty()
        || options.icebox_clear
        || options.icebox_reorder.is_some()
        || !options.pending_done.is_empty()
        || !options.pending_edit.is_empty()
        || options.pending_clear
        || options.pending_reorder.is_some()
        || !options.pending_gate.is_empty()
        || !options.pending_ungate.is_empty()
        || !options.pending_resolve_gate.is_empty()
        || !options.pending_set_gate_type.is_empty()
        || !options.pending_set_verify.is_empty()
        || !options.review_add.is_empty()
        || !options.review_edit.is_empty()
        || !options.review_remove.is_empty()
        || !options.review_resolve.is_empty();

    if options.pending_only && !has_pending_ops {
        anyhow::bail!("--backlog-only requires at least one backlog/icebox/review mutation flag");
    }
    if options.pending_only && (options.is_template || options.is_stream || options.is_ipc) {
        anyhow::bail!("--backlog-only cannot be combined with --template, --stream, or --ipc");
    }
    if options.pending_only && commit_mode == CommitMode::Required {
        anyhow::bail!("finalize does not support --backlog-only");
    }
    if !options.pending_add_to.len().is_multiple_of(2) {
        anyhow::bail!("--backlog-add-to expects repeated FILE TEXT pairs");
    }
    if options.commit_sibling.len() != options.commit_sibling_message.len() {
        anyhow::bail!(
            "--commit-sibling and --commit-sibling-message must be repeated the same number of times in positional pairs (got {} sibling(s), {} message(s))",
            options.commit_sibling.len(),
            options.commit_sibling_message.len()
        );
    }
    if !options.commit_sibling.is_empty() && commit_mode == CommitMode::None {
        anyhow::bail!(
            "--commit-sibling requires --commit (or `agent-doc finalize`); the sibling trailer URL needs the session-document commit sha"
        );
    }
    let pending_kept_open_ids = pending_kept_open_ids_from_mutations(
        &options.pending_edit,
        &options.pending_gate,
        &options.pending_ungate,
        &options.pending_set_gate_type,
        &options.pending_set_verify,
        &options.review_edit,
        options.pending_reorder.as_deref(),
    );
    let mut commit_mode = resolve_commit_mode(file, commit_mode, options.pending_only)?;
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

    if has_pending_ops || options.status.is_some() {
        let current_content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let snapshot_doc = snapshot::load(file).ok().flatten();
        guard_no_stale_snapshot_reset_drift(
            file,
            snapshot_doc.as_deref(),
            &current_content,
            "pre-pending write",
        )?;
    }

    if has_pending_ops {
        crate::backlog_cmd::with_force_disk_pending_writes(options.force_disk, || {
            if options.pending_clear {
                crate::backlog_cmd::clear(file)?;
            }
            if options.icebox_clear {
                crate::backlog_cmd::icebox_clear(file)?;
            }
            // `#opsproof-samecycle-add`: track ids added this cycle so post-commit
            // ops-proof auto-completion never reaps a brand-new same-cycle add.
            let mut same_cycle_added_ids: Vec<String> =
                crate::backlog_cmd::add_many(file, &options.pending_add, false)?;
            let pending_add_targets = group_pending_add_targets(&options.pending_add_to)?;
            for (target, items) in &pending_add_targets {
                ensure_pending_add_target(target)?;
                crate::backlog_cmd::add_many(target, items, false).with_context(|| {
                    format!(
                        "failed to apply --backlog-add-to target {}",
                        target.display()
                    )
                })?;
            }
            same_cycle_added_ids.extend(crate::backlog_cmd::add_many(
                file,
                &options.pending_add_gated,
                true,
            )?);
            // #ah0s: explicit-position adds (after/before <id>, tail). Applied after
            // the front-insert default so anchor ids added this same cycle resolve.
            for pair in options.pending_add_after.chunks(2) {
                if let [anchor, text] = pair {
                    let id = crate::backlog_cmd::add_after(file, anchor, text)
                        .with_context(|| format!("failed to apply --backlog-add-after {anchor}"))?;
                    same_cycle_added_ids.push(id);
                } else {
                    anyhow::bail!("--backlog-add-after expects repeated ID TEXT pairs");
                }
            }
            for pair in options.pending_add_before.chunks(2) {
                if let [anchor, text] = pair {
                    let id =
                        crate::backlog_cmd::add_before(file, anchor, text).with_context(|| {
                            format!("failed to apply --backlog-add-before {anchor}")
                        })?;
                    same_cycle_added_ids.push(id);
                } else {
                    anyhow::bail!("--backlog-add-before expects repeated ID TEXT pairs");
                }
            }
            for text in &options.pending_add_back {
                same_cycle_added_ids.push(crate::backlog_cmd::add_back(file, text)?);
            }
            same_cycle_added_ids.extend(crate::backlog_cmd::icebox_add_many(
                file,
                &options.icebox_add,
            )?);
            for pair in options.icebox_add_after.chunks(2) {
                if let [anchor, text] = pair {
                    let id = crate::backlog_cmd::icebox_add_after(file, anchor, text)
                        .with_context(|| format!("failed to apply --icebox-add-after {anchor}"))?;
                    same_cycle_added_ids.push(id);
                } else {
                    anyhow::bail!("--icebox-add-after expects repeated ID TEXT pairs");
                }
            }
            for pair in options.icebox_add_before.chunks(2) {
                if let [anchor, text] = pair {
                    let id = crate::backlog_cmd::icebox_add_before(file, anchor, text)
                        .with_context(|| format!("failed to apply --icebox-add-before {anchor}"))?;
                    same_cycle_added_ids.push(id);
                } else {
                    anyhow::bail!("--icebox-add-before expects repeated ID TEXT pairs");
                }
            }
            for text in &options.icebox_add_back {
                same_cycle_added_ids.push(crate::backlog_cmd::icebox_add_back(file, text)?);
            }
            if !options.pending_add.is_empty()
                || !options.pending_add_to.is_empty()
                || !options.pending_add_gated.is_empty()
                || !options.pending_add_after.is_empty()
                || !options.pending_add_before.is_empty()
                || !options.pending_add_back.is_empty()
                || !options.icebox_add.is_empty()
                || !options.icebox_add_after.is_empty()
                || !options.icebox_add_before.is_empty()
                || !options.icebox_add_back.is_empty()
            {
                crate::cycle_state::mark_pending_mutations(file)?;
                crate::cycle_state::mark_pending_added(file)?;
            }
            if !same_cycle_added_ids.is_empty() {
                crate::cycle_state::record_pending_added_ids(file, &same_cycle_added_ids)?;
            }
            if !options.pending_edit.is_empty() {
                let edits = parse_tracked_work_edits(&options.pending_edit, "--backlog-edit")?;
                crate::backlog_cmd::edit_many(file, &edits)?;
            }
            if !options.icebox_edit.is_empty() {
                let edits = parse_tracked_work_edits(&options.icebox_edit, "--icebox-edit")?;
                crate::backlog_cmd::icebox_edit_many(file, &edits)?;
            }
            for id in &options.pending_gate {
                crate::backlog_cmd::gate(file, id)?;
            }
            if !options.pending_gate.is_empty() {
                crate::cycle_state::record_pending_gated_ids(file, &options.pending_gate)?;
            }
            for pair in &options.pending_set_gate_type {
                let (id, gt) = pair.split_once('=').with_context(|| {
                    format!("--backlog-set-gate-type expects 'id=type', got: {}", pair)
                })?;
                crate::backlog_cmd::set_gate_type(file, id, gt)?;
            }
            for pair in &options.pending_set_verify {
                let (id, spec) = pair.split_once('=').with_context(|| {
                    format!(
                        "--backlog-set-verify expects 'id=<verify/disproof predicate spec>', got: {}",
                        pair
                    )
                })?;
                crate::backlog_cmd::set_gate_verify(file, id, spec)?;
            }
            let mut review_added_ids: Vec<String> = Vec::new();
            for value in &options.review_add {
                if let Some(id) = crate::backlog_cmd::review_add(file, value)? {
                    review_added_ids.push(id);
                }
            }
            if !review_added_ids.is_empty() {
                // `#opsproof-samecycle-add`: a freshly added gated review item must
                // not be ops-proof auto-completed on the cycle it first appears.
                crate::cycle_state::record_pending_added_ids(file, &review_added_ids)?;
            }
            for pair in &options.review_edit {
                let (id, text) = pair
                    .split_once('=')
                    .with_context(|| format!("--review-edit expects 'id=text', got: {}", pair))?;
                crate::backlog_cmd::review_edit(file, id, text)?;
            }
            for id in &options.review_resolve {
                crate::backlog_cmd::review_resolve(file, id)?;
            }
            for id in &options.review_remove {
                crate::backlog_cmd::review_remove(file, id)?;
            }
            for id in &options.pending_ungate {
                crate::backlog_cmd::ungate(file, id)?;
            }
            for gt in &options.pending_resolve_gate {
                crate::backlog_cmd::resolve_gate(file, gt)?;
            }
            for id in &options.pending_done {
                enforce_review_done_guard(file, id)?;
                crate::backlog_cmd::done(file, id)?;
            }
            if !options.pending_done.is_empty() {
                crate::cycle_state::record_pending_done_ids(file, &options.pending_done)?;
                crate::cycle_state::mark_pending_mutations(file)?;
            }
            if let Some(ref order) = options.pending_reorder {
                let ids = parse_id_order(order);
                crate::backlog_cmd::reorder(file, &ids)?;
            }
            if let Some(ref order) = options.icebox_reorder {
                let ids = parse_id_order(order);
                crate::backlog_cmd::icebox_reorder(file, &ids)?;
            }
            if !pending_kept_open_ids.is_empty() {
                crate::cycle_state::record_pending_kept_open_ids(file, &pending_kept_open_ids)?;
            }
            crate::cycle_state::mark_pending_mutations(file)?;
            Ok(())
        })?;
    }

    if let Some(ref status_text) = options.status {
        crate::status_cmd::set_with_options(file, status_text, options.force_disk)?;
    }

    if options.pending_only {
        run_closeout_pending_maintenance(file, commit_mode, options.force_disk)?;
        if commit_mode != CommitMode::None {
            crate::lint_gate::run(file, options.lint_override)?;
        }
        return finalize_commit(file, commit_mode);
    }

    let write_flags = WriteFlags {
        allow_replace_pending: options.allow_replace_pending,
        has_pending_add: !options.pending_add.is_empty()
            || !options.pending_add_to.is_empty()
            || !options.pending_add_gated.is_empty()
            || !options.pending_add_after.is_empty()
            || !options.pending_add_before.is_empty()
            || !options.pending_add_back.is_empty()
            || !options.icebox_add.is_empty()
            || !options.icebox_add_after.is_empty()
            || !options.icebox_add_before.is_empty()
            || !options.icebox_add_back.is_empty()
            || !options.review_add.is_empty(),
        has_pending_done: !options.pending_done.is_empty(),
        has_pending_mutation: has_pending_ops,
        pending_done_ids: options.pending_done.clone(),
        queue_completion_ids: options.queue_completion_ids.clone(),
        pending_kept_open_ids: pending_kept_open_ids.clone(),
        strict_closeout: commit_mode == CommitMode::Required,
        force_disk: options.force_disk,
        rerun_command_base: finalize_rerun_command_base(FinalizeRerunCommand {
            required_commit: commit_mode == CommitMode::Required,
            file,
            baseline_file: options.baseline_file.as_deref(),
            is_template: options.is_template,
            is_stream: options.is_stream,
            is_ipc: options.is_ipc,
            force_disk: options.force_disk,
            origin: options.origin.as_deref(),
            pending_add: &options.pending_add,
            pending_add_to: &options.pending_add_to,
            pending_add_gated: &options.pending_add_gated,
            pending_add_after: &options.pending_add_after,
            pending_add_before: &options.pending_add_before,
            pending_add_back: &options.pending_add_back,
            icebox_add: &options.icebox_add,
            icebox_add_after: &options.icebox_add_after,
            icebox_add_before: &options.icebox_add_before,
            icebox_add_back: &options.icebox_add_back,
            pending_done: &options.pending_done,
            pending_edit: &options.pending_edit,
            pending_clear: options.pending_clear,
            pending_reorder: options.pending_reorder.as_deref(),
            pending_gate: &options.pending_gate,
            pending_ungate: &options.pending_ungate,
            pending_resolve_gate: &options.pending_resolve_gate,
            pending_set_gate_type: &options.pending_set_gate_type,
            pending_set_verify: &options.pending_set_verify,
            review_add: &options.review_add,
            review_edit: &options.review_edit,
            allow_replace_pending: options.allow_replace_pending,
            pending_only: options.pending_only,
            status: options.status.as_deref(),
        }),
    };

    let baseline = match guard_no_explicit_baseline_replay_after_committed_cycle(
        file,
        commit_mode,
        options.baseline_file.as_deref(),
    )? {
        // Auto-reopened a committed cycle for a genuinely new response: diff against
        // the fresh HEAD baseline, not the stale explicit baseline file.
        Some(fresh_head_baseline) => Some(fresh_head_baseline),
        None => read_explicit_baseline(file, options.baseline_file.as_deref())?,
    };

    let current_content =
        std::fs::read_to_string(file).context("failed to read document for pre-write guards")?;
    guard_no_exchange_compaction_request_between(file, baseline.as_deref(), &current_content)?;
    let current_resolved_mode = if options.is_template || (!options.is_ipc && !options.is_stream) {
        let (fm, _) = frontmatter::parse(&current_content)?;
        Some(fm.resolve_mode())
    } else {
        None
    };
    let template_flag_on_crdt_doc = options.is_template
        && current_resolved_mode
            .as_ref()
            .is_some_and(|mode| mode.is_crdt());

    let write_result = if options.is_ipc {
        run_ipc(file, baseline.as_deref(), write_flags)
    } else if options.is_stream || template_flag_on_crdt_doc {
        if template_flag_on_crdt_doc && !options.is_stream {
            eprintln!(
                "[write] CRDT document received --template; routing through stream/CRDT write path"
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "template_flag_crdt_routed_to_stream file={} recovery=retry_crdt_instead",
                    file.display()
                ),
            );
        }
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
        if current_resolved_mode.is_some_and(|mode| mode.is_crdt()) {
            run_stream(
                file,
                baseline.as_deref(),
                options.force_disk,
                options.origin.as_deref(),
                write_flags,
            )
        } else {
            run(file, baseline.as_deref(), write_flags)
        }
    };

    // #bare-write-captured-uncommitted: a bare `agent-doc write` (CommitMode::None)
    // that placed or preserved an assistant response body on a session document must
    // finish the same write+commit boundary as `write --commit`. IPC `already_applied`
    // proves content placement, not closeout — the response is now visible, so escalate
    // to the commit boundary BEFORE the commit-mode-gated phases (queue consumption,
    // pending gates, commit) instead of stranding the cycle at `response_captured`.
    // `recover_missing_commit_boundary` only repairs responses already in HEAD; a
    // genuinely-uncommitted placed body has no other recovery path, so we must commit.
    if write_result.is_ok()
        && commit_mode == CommitMode::None
        && is_session_document(file)?
        && bare_write_placed_response_body(file)?
    {
        if crate::git::is_in_git_repo(file) {
            eprintln!(
                "[write] bare write placed a response body on session document {}; escalating to the commit boundary (#bare-write-captured-uncommitted)",
                file.display()
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "bare_write_escalated_to_commit file={} reason=response_body_placed",
                    file.display()
                ),
            );
            commit_mode = CommitMode::Required;
        } else {
            anyhow::bail!(
                "bare `agent-doc write` placed a response body on {} but the document is not in a git repository, so the cycle cannot reach a committed state. Move it into a git repo and rerun with `agent-doc write --commit {}`.",
                file.display(),
                file.display()
            );
        }
    }

    if write_result.is_ok() {
        run_closeout_pending_maintenance(file, commit_mode, options.force_disk)?;
    }

    // Phase 3b: pre-commit pending closeout gates (strict mode only).
    if write_result.is_ok() && commit_mode == CommitMode::Required {
        precommit_pending_capture_check(file)?;
        precommit_pending_done_check_with_options(
            file,
            PendingDoneCheckOptions {
                force_disk: options.force_disk,
            },
        )?;
    }

    // Phase 3b.1: tagpath agent-doc lint gate. Runs on the final file
    // state after the response/pending edits have merged, before the
    // snapshot/commit boundary. Errors fail the cycle closed so malformed
    // directives (for example `<!-- agent:done archive PATH -->` missing
    // `=`) cannot reach a committed state. Mode resolution: CLI override
    // > frontmatter `agent_doc_lint_dialect` > workspace `.agent-doc/
    // config.toml` `[lint] dialect` > default (`warn`).
    if write_result.is_ok() && commit_mode != CommitMode::None {
        crate::lint_gate::run(file, options.lint_override)?;
    }

    // Phase 3c: consume queue prompt after all other strict closeout gates
    // have passed so a rejected closeout cannot advance the queue early. The
    // layered completion signals — explicit `do queue`/prompt-target/`--done`
    // triggers, explicit `--done`/`--pending-gate`/`--review-resolve`/
    // `--pending-edit` completion of an id-backed head, a synthetic/preset
    // heading-id match, and a free-text head answered by this cycle's response —
    // all resolve through
    // `queue_consumption_allowed_for_response` so every successful closeout uses
    // an identical decision. Unproven IPC retries fail before this phase and do
    // not advance the queue.
    if write_result.is_ok() {
        let response_body = crate::capture::load_active(file)?
            .map(|capture| capture.response_body)
            .unwrap_or_default();
        let mut queue_completion_ids = agent_doc_queue::queue_heads::explicit_queue_completion_ids(
            &options.pending_done,
            &options.pending_gate,
            &options.pending_edit,
            &options.review_resolve,
        );
        queue_completion_ids.extend(options.queue_completion_ids.iter().cloned());
        let queue_consumption_allowed = queue_consumption_allowed_for_response(
            file,
            baseline.as_deref(),
            &current_content,
            &response_body,
            &queue_completion_ids,
        )?;
        if queue_consumption_allowed
            && let Some(head_id) = queue_targeted_completion_id_for_current_head(
                file,
                baseline.as_deref(),
                &current_content,
                &response_body,
                &options.pending_done,
            )?
            && !queue_completion_ids
                .iter()
                .any(|id| agent_doc_queue::queue_response::normalize_done_id(id) == head_id)
        {
            queue_completion_ids.push(head_id);
        }
        match commit_mode {
            CommitMode::None => {}
            CommitMode::BestEffort => {
                if queue_consumption_allowed {
                    if let Err(e) = consume_queue_prompts_for_done_ids_closeout(
                        file,
                        &queue_completion_ids,
                        options.force_disk,
                    ) {
                        eprintln!("[queue] warning: consumption failed: {}", e);
                    }
                    if let Err(e) = mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_completion_ids,
                        options.force_disk,
                    ) {
                        eprintln!("[queue] warning: done-id marking failed: {}", e);
                    }
                } else {
                    match mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_completion_ids,
                        options.force_disk,
                    ) {
                        Ok(0) => eprintln!("{}", queue_skip_diagnostic_for_file(file)?),
                        Ok(_) => {}
                        Err(e) => eprintln!("[queue] warning: done-id marking failed: {}", e),
                    }
                }
            }
            CommitMode::Required => {
                if queue_consumption_allowed {
                    consume_queue_prompts_for_done_ids_closeout(
                        file,
                        &queue_completion_ids,
                        options.force_disk,
                    )?;
                    mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_completion_ids,
                        options.force_disk,
                    )?;
                } else {
                    let marked = mark_completed_queue_prompts_for_done_ids(
                        file,
                        &queue_completion_ids,
                        options.force_disk,
                    )?;
                    if marked == 0 {
                        eprintln!("{}", queue_skip_diagnostic_for_file(file)?);
                    }
                }
            }
        }

        // `#ftstrike`: strike any free-text queue head this cycle's response
        // answered, regardless of position. The leading-head consume above only
        // strikes a contiguous leading run and stops at an id-backed head, so a
        // free-text report sitting BEHIND an unfinished `do [#id]` head (the
        // common case while a backlog directive head is still draining) was never
        // struck even after the response addressed it — the operator could not
        // tell which typed reports were answered. This runs independent of
        // `queue_consumption_allowed` (which governs the leading head only) and is
        // best-effort: a missed strike must never fail an otherwise-clean closeout.
        if commit_mode != CommitMode::None {
            match strike_answered_free_text_queue_heads(file, &response_body, options.force_disk) {
                Ok(0) => {}
                Ok(n) => eprintln!("[queue] struck {n} answered free-text head(s) (#ftstrike)"),
                Err(e) => eprintln!("[queue] warning: free-text head strike failed: {e}"),
            }
        }
    }

    // `#pendingaddqueuesync`: `--pending-add*` mutations are applied during
    // write/finalize, after preflight's backlog→queue sync has already run.
    // Once the current head has been consumed, append same-cycle pending adds
    // into active go-mode backlog queues so captured follow-up work is not
    // stranded outside the drain.
    if write_result.is_ok() && commit_mode != CommitMode::None {
        match commit_mode {
            CommitMode::None => {}
            CommitMode::BestEffort => {
                if let Err(e) = crate::preflight::sync_same_cycle_pending_adds_into_go_queue(file) {
                    eprintln!(
                        "[queue] warning: same-cycle pending-add queue sync failed: {}",
                        e
                    );
                }
            }
            CommitMode::Required => {
                crate::preflight::sync_same_cycle_pending_adds_into_go_queue(file)?;
            }
        }
    }

    let commit_result = if write_result.is_ok() {
        let primary = finalize_commit(file, commit_mode);
        if primary.is_ok() && !options.commit_sibling.is_empty() {
            let pairs: Vec<(std::path::PathBuf, String)> = options
                .commit_sibling
                .iter()
                .cloned()
                .zip(options.commit_sibling_message.iter().cloned())
                .collect();
            agent_doc_git_io::sibling::commit_siblings_for_session_doc(file, &pairs)?;
        }
        primary
    } else {
        Ok(())
    };
    // A response-bearing bare write on a session document is escalated to the commit
    // boundary above (#bare-write-captured-uncommitted), so reaching here with
    // CommitMode::None means the write placed no response body. Any open cycle now is
    // a pre-existing interrupted closeout, not content this write stranded.
    let bare_session_write_result =
        if write_result.is_ok() && commit_mode == CommitMode::None && is_session_document(file)? {
            crate::session_check::enforce_clean_closeout(file).context(
                "bare `agent-doc write` did not place a response body, but the session \
             document still has an open cycle outside the commit boundary",
            )
        } else {
            Ok(())
        };

    match (write_result, commit_result, bare_session_write_result) {
        (Ok(()), Ok(()), Ok(())) => Ok(()),
        (Err(write_err), Ok(()), Ok(())) => Err(write_err),
        (Ok(()), Err(commit_err), Ok(())) => Err(commit_err),
        (Ok(()), Ok(()), Err(boundary_err)) => Err(boundary_err),
        (Err(write_err), Err(commit_err), Ok(())) => Err(write_err.context(commit_err.to_string())),
        (Err(write_err), Ok(()), Err(boundary_err)) => {
            Err(write_err.context(boundary_err.to_string()))
        }
        (Ok(()), Err(commit_err), Err(boundary_err)) => {
            Err(boundary_err.context(commit_err.to_string()))
        }
        (Err(write_err), Err(commit_err), Err(boundary_err)) => {
            Err(write_err.context(format!("{commit_err}\n{boundary_err}")))
        }
    }
}

fn finalize_commit(file: &Path, commit_mode: CommitMode) -> Result<()> {
    match commit_mode {
        CommitMode::None => Ok(()),
        CommitMode::BestEffort => {
            if crate::git::is_in_git_repo(file) {
                let session_document = is_session_document(file)?;
                // `#crdtauth4` — authority-gated commit barrier (plan phase 4).
                // No-op under `GitAuthoritative` (Detached); under `MultiReplica`
                // flushes live editor replicas to a consistent cut before commit.
                let barrier_ready = crate::crdt_relay_host::commit_barrier_for_file(file);
                if !barrier_ready {
                    log_closeout_guard(
                        file,
                        agent_doc_flow::types::FlowStage::PreCommitGuard,
                        agent_doc_flow::types::FlowOutcome::Blocked,
                        agent_doc_turn::closeout_guard::CloseoutGuardReason::ReplicaDeliveryPending,
                    );
                    eprintln!(
                        "[commit] skipped: live editor replica delivery is still pending for {}",
                        file.display()
                    );
                    if session_document {
                        anyhow::bail!(
                            "live editor replica delivery is still pending for {}; retry after the editor buffer reaches disk",
                            file.display()
                        );
                    }
                    return Ok(());
                }
                match crate::git::commit(file) {
                    // `#staleinmem` — record what we just committed so a later
                    // out-of-band disk correction is detectable at the next barrier.
                    Ok(_) => crate::crdt_relay_host::record_committed_baseline_for_file(file),
                    Err(e) => {
                        eprintln!("[commit] warning: {}", e);
                        if session_document {
                            return Err(e.context(format!(
                                "session-document best-effort commit failed for {}",
                                file.display()
                            )));
                        }
                    }
                }
                crate::session_check::enforce_clean_closeout(file)?;
            } else {
                eprintln!("[commit] skipped (not in git repo)");
            }
            Ok(())
        }
        CommitMode::Required => complete_required_closeout(file).map(|_| ()),
    }
}

pub fn complete_required_closeout(file: &Path) -> Result<bool> {
    crate::flow::closeout::complete_required_closeout(file)
}

fn log_closeout_guard(
    file: &Path,
    stage: agent_doc_flow::types::FlowStage,
    outcome: agent_doc_flow::types::FlowOutcome,
    reason: agent_doc_turn::closeout_guard::CloseoutGuardReason,
) {
    crate::flow::closeout::log_closeout_guard_event(file, stage, outcome, reason);
}

fn recover_empty_response_for_strict_closeout(file: &Path, flags: &WriteFlags) -> Result<bool> {
    if flags.strict_closeout {
        let outcome = repair::run(file)?;
        if recover_missing_committed_head_response(file)? {
            return Ok(true);
        }
        if outcome.repaired() {
            eprintln!(
                "[write] empty response stdin; recovered existing agent-doc response state with {:?}",
                outcome
            );
            return Ok(true);
        }
        if recover_dedupe_only_drift(file)? {
            return Ok(true);
        }
    }
    if flags.has_pending_mutation {
        eprintln!(
            "[write] empty response stdin; committing pending mutations without a response body"
        );
        return Ok(true);
    }
    Ok(false)
}

fn recover_missing_committed_head_response(file: &Path) -> Result<bool> {
    let Some(head_content) = crate::git::show_head(file)? else {
        return Ok(false);
    };
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let Some(response_block) = latest_response_block_missing_from_current(&head_content, &current)
    else {
        return Ok(false);
    };
    let Some(recovered) = splice_response_block_into_current_exchange(&current, &response_block)
    else {
        return Ok(false);
    };
    if recovered == current {
        return Ok(false);
    }
    eprintln!(
        "[write] empty response stdin; merged latest committed HEAD response back into visible document for {}",
        file.display()
    );
    guard_visible_write_idle_and_current(file, "recover_committed_head_response", &current)?;
    atomic_write(file, &recovered)?;
    crate::snapshot::save(file, &recovered)?;
    crate::git::commit(file)?;
    Ok(true)
}

/// When `agent-doc write --commit <FILE>` runs with empty stdin and the
/// working tree differs from HEAD only by the deletions a fresh
/// `agent-doc dedupe <FILE>` would produce against HEAD, accept that as a
/// recoverable closeout: align the snapshot to the current file and commit
/// through the normal binary path.
///
/// Why: when a finalize cycle produces a duplicate response (for example the
/// IPC retry / file-IPC fallback path adopting a response already in the live
/// file), the user runs `agent-doc dedupe` to remove the duplicate. The
/// follow-up `agent-doc write --commit` (no stdin) used to bail with
/// "empty response — nothing to write", forcing a manual `git commit` and
/// defeating the binary-owned closeout contract. This branch keeps the
/// closeout binary-owned by recognizing the dedupe-only drift signature.
fn recover_dedupe_only_drift(file: &Path) -> Result<bool> {
    let Some(head_content) = crate::git::show_head(file)? else {
        return Ok(false);
    };
    let current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    if current == head_content {
        return Ok(false);
    }
    let dedupe_of_head = dedupe_responses(&head_content);
    if dedupe_of_head == head_content {
        // HEAD has no duplicates — drift is something else, not a dedupe outcome.
        return Ok(false);
    }
    if dedupe_of_head != current {
        return Ok(false);
    }
    eprintln!(
        "[write] empty response stdin; current file matches dedupe(HEAD) for {} — committing dedupe-only working-tree drift through the binary closeout path",
        file.display()
    );
    crate::snapshot::save(file, &current)?;
    crate::git::commit(file)?;
    Ok(true)
}

mod pending_checks;
pub use pending_checks::*;

/// Consume the first queue prompt after a successful write cycle.
///
/// Called between the write step and the commit step so the consumption
/// is included in the same git commit as the response (atomic).
///
/// Reads frontmatter for `queue_active: true`; if the queue is not active
/// this is a no-op. On consumption, the first prompt is removed from both
/// the file and the snapshot. When the queue drains to empty, `auto` is
/// stripped and `queue_active` is cleared.
mod queue_consume;
pub use queue_consume::*;

/// Enforcement: reject full-replacement blocks targeting the `pending` component
/// unless the caller explicitly opts in.
///
/// Canonical form: `<!-- replace:pending -->...<!-- /replace:pending -->` with
/// `--allow-replace-pending` (or `AGENT_DOC_ALLOW_REPLACE_PENDING=1`).
///
/// The pending system requires mutations via granular flags
/// (`--pending-add/done/edit/clear/reorder`); a full-replace block on a list the
/// user concurrently edits enables silent-data-loss via concurrent-edit clobber
/// and hash instability.
///
/// Phase 3 inversion (2026-04-14): the default is now reject. Library callers
/// (FFI, tests, future SDK consumers) must opt in explicitly.
pub fn enforce_no_replace_pending(patches: &[template::PatchBlock], allow: bool) -> Result<()> {
    if allow {
        return Ok(());
    }
    let allow_canonical = std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false);
    if allow_canonical {
        return Ok(());
    }
    if patches.iter().any(|p| {
        is_backlog_component(&p.name) || agent_doc_element::element::is_review_component(&p.name)
    }) {
        anyhow::bail!(
            "ERR: replace:pending/review block forbidden — use --pending-add/done/edit/clear/reorder or --review-add/edit. \
             See specs/pending-system.md."
        );
    }
    Ok(())
}

mod materialize;
pub use materialize::*;

/// Resolve the IPC project root for `canonical` (an already-canonicalized file
/// path). Uses the nearest `.agent-doc/` directory to match the IDE plugin's
/// `resolveRootFor` logic — submodule documents use the submodule's own
/// `.agent-doc/`, not the superproject's. Falls back to git toplevel for
/// plain git repos without `.agent-doc/`, then the file's parent directory.
fn resolve_ipc_project_root(canonical: &Path) -> std::path::PathBuf {
    let parent = canonical.parent().unwrap_or(Path::new("/"));
    let git_toplevel = agent_doc_git_io::dirs::git_toplevel_at(parent);
    // 1. Nearest .agent-doc/ root — mirrors IDE plugin's resolveRootFor.
    //    Submodule files resolve to the submodule root, not the superproject,
    //    so ack-content and patch paths agree between Rust and Kotlin.
    if let Some(p) = find_project_root(canonical)
        && git_toplevel
            .as_ref()
            .is_none_or(|toplevel| p.starts_with(toplevel))
    {
        return p;
    }
    // 2. Plain git repo without .agent-doc: use the toplevel.
    if let Some(toplevel) = git_toplevel {
        return toplevel;
    }
    // 3. Last resort: file's parent directory.
    parent.to_path_buf()
}

/// Public accessor for `resolve_ipc_project_root` (used by git.rs).
pub fn resolve_ipc_project_root_pub(canonical: &Path) -> std::path::PathBuf {
    resolve_ipc_project_root(canonical)
}

pub(crate) fn ipc_direct_disk_degraded_for_file(project_root: &Path, file: &Path) -> Result<bool> {
    ipc::ipc_direct_disk_degraded(project_root, file)
}

mod normalize;
pub use normalize::*;

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

fn dedupe_consecutive_response_blocks(content: &str, file: &Path) -> String {
    let deduped = dedupe_responses(content);
    if deduped != content {
        eprintln!(
            "[write] dedup: removed consecutive duplicate response block(s) from {} before closeout",
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "dedupe_consecutive_response_blocks file={} before_closeout=true",
                file.display()
            ),
        );
    }
    deduped
}

#[derive(Clone, Copy, Debug)]
struct DuplicatePromptRepairOptions<'a> {
    source: &'a str,
    before: Option<&'a str>,
    preserve_doc: Option<&'a str>,
    preserve_current_doc: Option<&'a str>,
    enforce_residue_guard: bool,
}

impl<'a> DuplicatePromptRepairOptions<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            before: None,
            preserve_doc: None,
            preserve_current_doc: None,
            enforce_residue_guard: true,
        }
    }

    fn with_before(mut self, before: Option<&'a str>) -> Self {
        self.before = before;
        self
    }

    fn preserving(mut self, preserve_doc: Option<&'a str>) -> Self {
        self.preserve_doc = preserve_doc;
        self
    }

    fn preserving_current(mut self, preserve_current_doc: Option<&'a str>) -> Self {
        self.preserve_current_doc = preserve_current_doc;
        self
    }

    fn without_residue_guard(mut self) -> Self {
        self.enforce_residue_guard = false;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DuplicatePromptRepairReport {
    response_blocks: bool,
    answered_tail: bool,
    post_exchange_comments: bool,
    prompt_lines_against_before: bool,
    live_prefix_variants: bool,
}

impl DuplicatePromptRepairReport {
    fn changed(self) -> bool {
        self.response_blocks
            || self.answered_tail
            || self.post_exchange_comments
            || self.prompt_lines_against_before
            || self.live_prefix_variants
    }
}

fn repair_duplicate_prompt_artifacts(
    content: &str,
    file: &Path,
    options: DuplicatePromptRepairOptions<'_>,
) -> Result<(String, DuplicatePromptRepairReport)> {
    let mut repaired = content.to_string();
    let mut report = DuplicatePromptRepairReport::default();

    let response_deduped = dedupe_consecutive_response_blocks(&repaired, file);
    if response_deduped != repaired {
        repaired = response_deduped;
        report.response_blocks = true;
    }

    if let Some(answered_tail_deduped) =
        agent_doc_template::remove_duplicate_answered_exchange_prompt_tail(&repaired)
    {
        repaired = answered_tail_deduped;
        report.answered_tail = true;
        crate::ops_log::log_op(
            file,
            &format!(
                "duplicate_answered_exchange_prompt_tail_removed file={} source={} before_commit=true",
                file.display(),
                options.source
            ),
        );
    }

    let (comment_deduped, comment_changed) =
        remove_post_exchange_duplicate_prompt_comments_with_log(
            &repaired,
            file,
            options.source,
            options.preserve_doc,
            options.preserve_current_doc,
        );
    if comment_changed {
        repaired = comment_deduped;
        report.post_exchange_comments = true;
    }

    if let Some(before) = options.before {
        let (prompt_deduped, prompt_changed) =
            dedupe_prompt_lines_against_before(before, &repaired, file);
        if prompt_changed {
            repaired = prompt_deduped;
            report.prompt_lines_against_before = true;
        }
    }

    let (adjacent_prefix_deduped, adjacent_prefix_changed) =
        dedupe_adjacent_prompt_prefix_duplicates(&repaired, file);
    if adjacent_prefix_changed {
        repaired = adjacent_prefix_deduped;
        report.live_prefix_variants = true;
    }

    let (prefix_deduped, prefix_changed) =
        dedupe_live_prompt_prefix_variants_in_tail(&repaired, file);
    if prefix_changed {
        repaired = prefix_deduped;
        report.live_prefix_variants = true;
    }

    if options.enforce_residue_guard {
        enforce_no_duplicate_prompt_residue(file, &repaired, options.source)?;
    }

    if report.changed() {
        crate::ops_log::log_op(
            file,
            &format!(
                "duplicate_prompt_artifact_repair file={} source={} response_blocks={} answered_tail={} post_exchange_comments={} prompt_lines_against_before={} live_prefix_variants={} before_commit=true",
                file.display(),
                options.source,
                report.response_blocks,
                report.answered_tail,
                report.post_exchange_comments,
                report.prompt_lines_against_before,
                report.live_prefix_variants
            ),
        );
    }

    Ok((repaired, report))
}

pub fn repair_commit_prompt_artifacts_against_snapshot(
    file: &Path,
    snapshot: &str,
    current: &str,
) -> Option<String> {
    let mut repaired = current.to_string();
    let mut report = DuplicatePromptRepairReport::default();

    let (prompt_deduped, prompt_changed) =
        dedupe_prompt_lines_against_before(snapshot, &repaired, file);
    if prompt_changed {
        repaired = prompt_deduped;
        report.prompt_lines_against_before = true;
    }

    let (adjacent_prefix_deduped, adjacent_prefix_changed) =
        dedupe_adjacent_prompt_prefix_duplicates(&repaired, file);
    if adjacent_prefix_changed {
        repaired = adjacent_prefix_deduped;
        report.live_prefix_variants = true;
    }

    let (prefix_deduped, prefix_changed) =
        dedupe_live_prompt_prefix_variants_in_tail(&repaired, file);
    if prefix_changed {
        repaired = prefix_deduped;
        report.live_prefix_variants = true;
    }

    if report.changed() {
        crate::ops_log::log_op(
            file,
            &format!(
                "duplicate_prompt_artifact_repair file={} source=commit-pre-stage response_blocks=false answered_tail=false post_exchange_comments=false prompt_lines_against_before={} live_prefix_variants={} before_commit=true",
                file.display(),
                report.prompt_lines_against_before,
                report.live_prefix_variants
            ),
        );
        Some(repaired)
    } else {
        None
    }
}

fn enforce_no_duplicate_prompt_residue(file: &Path, content: &str, context: &str) -> Result<()> {
    match agent_doc_template::guard_no_duplicate_prompt_residue_outside_exchange(content) {
        Ok(()) => Ok(()),
        Err(err) => {
            log_template_structure_guard_event(
                file,
                TemplateStructureGuardReason::DuplicatePromptResidue,
                FlowOutcome::FailedClosed,
            );
            Err(err).with_context(|| {
                format!(
                    "duplicate prompt residue check failed for {} ({context})",
                    file.display()
                )
            })
        }
    }
}

pub fn normalize_template_structure_or_fail(content: &str, file: &Path) -> Result<String> {
    normalize_template_structure_or_fail_preserving(content, file, None)
}

pub fn normalize_template_structure_or_fail_preserving(
    content: &str,
    file: &Path,
    preserve_doc: Option<&str>,
) -> Result<String> {
    let lifted = lift_pending_from_exchange_safe(content, file);
    // Defense-in-depth: merge any duplicate exchange openers that may have
    // survived the patch application phase (e.g., via CRDT/git merge).
    let deduped_openers = {
        let mut result = lifted;
        while let Some(merged) = agent_doc_template::repair_duplicate_exchange_opener(&result)? {
            eprintln!("[write] normalize_template_structure: merged duplicate exchange opener");
            result = merged;
        }
        result
    };
    let (normalized, _) = repair_duplicate_prompt_artifacts(
        &agent_doc_element::element::strip_backlog_patch_attr(&deduped_openers),
        file,
        DuplicatePromptRepairOptions::new("structure").preserving(preserve_doc),
    )?;
    match agent_doc_template::guard_no_conversation_tail_outside_exchange(&normalized) {
        Ok(()) => Ok(normalized),
        Err(err)
            if err.chain().any(|cause| {
                cause
                    .to_string()
                    .contains("closing marker <!-- /agent:exchange --> without matching open")
            }) =>
        {
            if let Some(repaired) =
                agent_doc_template::repair_duplicate_exchange_close_scaffold(&normalized)?
            {
                log_template_structure_guard_event(
                    file,
                    TemplateStructureGuardReason::DuplicateScaffoldDropped,
                    FlowOutcome::Completed,
                );
                let (repaired, _) = repair_duplicate_prompt_artifacts(
                    &repaired,
                    file,
                    DuplicatePromptRepairOptions::new("duplicate-scaffold repair")
                        .preserving(preserve_doc),
                )?;
                agent_doc_template::guard_no_conversation_tail_outside_exchange(&repaired)
                    .context(format!(
                        "template structure guard failed for {} after duplicate-scaffold repair",
                        file.display()
                    ))?;
                return Ok(repaired);
            }
            if agent_doc_template::repair_duplicate_exchange_close_mixed_scaffold_tail(&normalized)?
                .is_some()
            {
                log_template_structure_guard_event(
                    file,
                    TemplateStructureGuardReason::MixedDuplicateScaffoldTail,
                    FlowOutcome::FailedClosed,
                );
                anyhow::bail!(
                    "mixed duplicate scaffold tail for {}: live conversation text is interleaved with duplicated template scaffold; refusing automatic closeout repair",
                    file.display()
                );
            }
            if let Some(repaired) =
                agent_doc_template::repair_duplicate_exchange_close_tail(&normalized)?
            {
                log_template_structure_guard_event(
                    file,
                    TemplateStructureGuardReason::DuplicateCloseTailMoved,
                    FlowOutcome::Completed,
                );
                let (repaired, _) = repair_duplicate_prompt_artifacts(
                    &repaired,
                    file,
                    DuplicatePromptRepairOptions::new("duplicate-close repair")
                        .preserving(preserve_doc),
                )?;
                agent_doc_template::guard_no_conversation_tail_outside_exchange(&repaired)
                    .context(format!(
                        "template structure guard failed for {} after duplicate-close repair",
                        file.display()
                    ))?;
                return Ok(repaired);
            }
            Err(err)
                .with_context(|| format!("template structure guard failed for {}", file.display()))
        }
        Err(err) => Err(err)
            .with_context(|| format!("template structure guard failed for {}", file.display())),
    }
}

/// Minimum byte count for exchange content before the shrink guard triggers.
/// Below this threshold the exchange is too small to be worth protecting.
const SHRINK_GUARD_MIN_BYTES: usize = 100;

/// Maximum ratio (new / old) that the shrink guard allows without `--force`.
/// If the new exchange content is less than this fraction of the old, refuse.
const SHRINK_GUARD_MAX_RATIO: f64 = 0.10;

const VISIBLE_WRITE_TYPING_DEBOUNCE_MS: u64 = 500;
const VISIBLE_WRITE_TYPING_TIMEOUT_MS: u64 = 5_000;

/// Max re-merge attempts when reconciling the visible-write guard with a
/// foreign disk write that landed after the merge was computed
/// (#ipc-drift-visbuf-reconcile). After this many drifting re-reads, fall back
/// to the fail-closed guard so the operator retries.
const VISIBLE_WRITE_RECONCILE_MAX_ATTEMPTS: usize = 3;

pub fn guard_visible_write_idle(file: &Path, source: &str) -> Result<()> {
    guard_visible_write_idle_with_budget(
        file,
        source,
        VISIBLE_WRITE_TYPING_DEBOUNCE_MS,
        VISIBLE_WRITE_TYPING_TIMEOUT_MS,
    )
}

fn guard_visible_write_idle_with_budget(
    file: &Path,
    source: &str,
    debounce_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    let idle_reached =
        agent_doc_debounce::await_idle_via_file(&indicator_path, debounce_ms, timeout_ms);
    let facts = agent_doc_document_realtime::write_policy::VisibleWriteTypingFacts {
        idle_reached,
        timeout_ms,
    };
    let decision =
        agent_doc_document_realtime::write_policy::decide_visible_write_after_typing(facts);
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::document_mutation::visible_write_guard_event(decision, source),
    );
    if decision == agent_doc_document_realtime::write_policy::VisibleWriteDecision::Apply {
        return Ok(());
    }

    crate::ops_log::log_op(
        file,
        &format!(
            "visible_write_deferred_active_typing file={} source={} debounce_ms={} timeout_ms={}",
            file.display(),
            source,
            debounce_ms,
            timeout_ms
        ),
    );
    anyhow::bail!(
        "visible document write for {} deferred: editor typing did not settle within {}ms; retry after typing stops",
        file.display(),
        timeout_ms
    )
}

/// Outcome of reconciling the visible-write guard with the on-disk state.
///
/// Distinguishes a *pending user edit in the editor* (which must still fail
/// closed) from a *foreign agent-doc disk write* that landed after the response
/// merge was computed (which is CRDT-reconcilable via a re-merge instead of
/// stranding the captured response outside HEAD — `#ipc-drift-visbuf-reconcile`).
pub(crate) enum VisibleWriteReconcile {
    /// Disk and the live editor buffer agree with the expected content; the
    /// caller may write its computed `final_content`.
    Clean,
    /// The on-disk file drifted to a foreign agent-doc write *after* the
    /// response merge was computed, but the live editor buffer did NOT diverge
    /// (no pending user edit). Carries the fresh disk content so the caller can
    /// re-merge the captured response against it and retry.
    DiskDrifted { fresh_current: String },
}

pub(crate) fn guard_visible_write_idle_and_current(
    file: &Path,
    source: &str,
    expected_current: &str,
) -> Result<()> {
    guard_visible_write_idle_current_or_target(file, source, expected_current, None)
}

pub(crate) fn guard_visible_write_idle_current_or_target(
    file: &Path,
    source: &str,
    expected_current: &str,
    target_content: Option<&str>,
) -> Result<()> {
    match guard_visible_write_reconcile_with_target(file, source, expected_current, target_content)?
    {
        VisibleWriteReconcile::Clean => Ok(()),
        VisibleWriteReconcile::DiskDrifted { fresh_current } => {
            crate::flow::proof::log_flow_event(
                file,
                crate::flow::document_mutation::visible_write_current_changed_event(source),
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "visible_write_deferred_current_changed file={} source={} expected_hash={} current_hash={}",
                    file.display(),
                    source,
                    agent_doc_hash::content_hash(expected_current),
                    agent_doc_hash::content_hash(&fresh_current)
                ),
            );
            anyhow::bail!(
                "visible document write for {} deferred: document changed after the response merge was computed; retry after typing stops",
                file.display()
            )
        }
    }
}

/// Like [`guard_visible_write_idle_and_current`] but, instead of failing closed
/// when the on-disk file drifted *after* the merge was computed, reports the
/// fresh disk content so a CRDT-merge caller can re-merge the captured response
/// and retry. A genuine live editor-buffer divergence (pending user edit) still
/// fails closed — only a clean foreign disk write is reported as reconcilable.
fn guard_visible_write_reconcile_with_target(
    file: &Path,
    source: &str,
    expected_current: &str,
    target_content: Option<&str>,
) -> Result<VisibleWriteReconcile> {
    guard_visible_write_idle(file, source)?;
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    let actual_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;
    if let Some(live) =
        agent_doc_debounce::live_buffer_diverges_from_content(&indicator_path, expected_current)
    {
        // #nm1x provenance suppression: when the editor-visible buffer matches the
        // current on-disk content, the editor holds no unsaved edits *ahead* of
        // disk. The divergence is then disk-vs-expected — an independent / foreign
        // document edit, the reconcilable `DiskDrifted` case below — not a pending
        // user edit. Only a genuine unsaved editor buffer ahead of disk fails
        // closed. This replaces the coarse "any live-buffer divergence blocks
        // finalize" gate with an actor-aware check (the live-buffer actor is not
        // diverging when it already equals disk).
        let disk_hash = agent_doc_hash::content_hash(&actual_current);
        let editor_matches_disk =
            live.len == actual_current.len() && live.hash.eq_ignore_ascii_case(&disk_hash);
        if editor_matches_disk {
            let expected_hash = agent_doc_hash::content_hash(expected_current);
            crate::ops_log::log_op(
                file,
                &format!(
                    "visible_write_live_buffer_matches_disk file={} source={} expected_len={} expected_hash={} disk_len={} disk_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    expected_hash,
                    actual_current.len(),
                    disk_hash,
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
        } else if let Some(target) = target_content
            && actual_current == expected_current
            && live_buffer_snapshot_matches_content(&live, target)
        {
            crate::ops_log::log_op(
                file,
                &format!(
                    "visible_write_live_buffer_matches_target file={} source={} expected_len={} expected_hash={} target_len={} target_hash={} disk_len={} disk_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    agent_doc_hash::content_hash(expected_current),
                    target.len(),
                    agent_doc_hash::content_hash(target),
                    actual_current.len(),
                    disk_hash,
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
        } else {
            crate::flow::proof::log_flow_event(
                file,
                crate::flow::document_mutation::visible_write_current_changed_event(source),
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "visible_write_deferred_live_buffer_changed file={} source={} expected_len={} expected_hash={} live_len={} live_hash={} live_ts={}",
                    file.display(),
                    source,
                    expected_current.len(),
                    agent_doc_hash::content_hash(expected_current),
                    live.len,
                    live.hash,
                    live.timestamp_ms
                ),
            );
            anyhow::bail!(
                "visible editor buffer for {} differs from the expected disk state; save or discard the editor buffer, then retry",
                file.display()
            );
        }
    }
    if actual_current == expected_current {
        return Ok(VisibleWriteReconcile::Clean);
    }

    // Disk drifted but the live editor buffer did not diverge: a foreign
    // agent-doc supervisor appended to the same document mid-generation. This
    // is reconcilable — report the fresh disk content so the caller re-merges
    // instead of failing closed and stranding the captured response.
    crate::ops_log::log_op(
        file,
        &format!(
            "visible_write_disk_drift_reconcilable file={} source={} expected_hash={} current_hash={}",
            file.display(),
            source,
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(&actual_current)
        ),
    );
    Ok(VisibleWriteReconcile::DiskDrifted {
        fresh_current: actual_current,
    })
}

fn live_buffer_snapshot_matches_content(
    snapshot: &agent_doc_debounce::LiveBufferSnapshot,
    content: &str,
) -> bool {
    snapshot.len == content.len()
        && snapshot
            .hash
            .eq_ignore_ascii_case(&agent_doc_hash::content_hash(content))
}

/// Drive the visible-write reconcile loop (#ipc-drift-visbuf-reconcile).
///
/// Starting from `initial_current`/`initial_payload` (the merge computed against
/// the first disk read), repeatedly consult `guard`. On a [`VisibleWriteReconcile::Clean`]
/// outcome the merge is safe to write and `(current, payload)` is returned. On a
/// [`VisibleWriteReconcile::DiskDrifted`] outcome — a foreign agent-doc write that
/// landed after the merge — re-merge via `recompute` against the fresh disk content
/// and retry, up to `max_attempts`. If the document keeps drifting past that bound,
/// fall back to `fail_closed` (which fails the cycle so the operator retries).
///
/// Factored out so the loop logic is unit-testable with injected guard/recompute
/// closures, without needing a mid-write disk mutation race.
fn reconcile_visible_write<T>(
    file: &Path,
    initial_current: String,
    initial_payload: T,
    max_attempts: usize,
    mut guard: impl FnMut(&Path, &str, &T) -> Result<VisibleWriteReconcile>,
    mut recompute: impl FnMut(&str) -> Result<T>,
    fail_closed: impl FnOnce(&Path, &str, &T) -> Result<()>,
) -> Result<(String, T)> {
    let mut current = initial_current;
    let mut payload = initial_payload;
    for _ in 0..max_attempts {
        match guard(file, &current, &payload)? {
            VisibleWriteReconcile::Clean => return Ok((current, payload)),
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                current = fresh_current;
                payload = recompute(&current)?;
            }
        }
    }
    // Document kept changing under us across every reconcile attempt; fall back
    // to the fail-closed guard so the operator retries.
    fail_closed(file, &current, &payload)?;
    Ok((current, payload))
}

mod converge;
pub use converge::*;

mod exchange_reconcile;
pub(crate) use exchange_reconcile::*;

struct TemplatePatchApplicationBase<'a, 'b> {
    file: &'b Path,
    baseline: Option<&'a str>,
    content_at_start: &'a str,
    patches: &'b [template::PatchBlock],
    unmatched: &'b str,
    mode_overrides: &'b std::collections::HashMap<String, String>,
    source: &'b str,
    strict_closeout: bool,
}

fn template_patch_application_base<'a>(
    input: TemplatePatchApplicationBase<'a, '_>,
) -> Result<std::borrow::Cow<'a, str>> {
    let Some(base) = input.baseline else {
        return Ok(std::borrow::Cow::Borrowed(input.content_at_start));
    };
    if !input.strict_closeout
        || !exchange_append_patch_can_rebase_to_head(
            input.patches,
            input.unmatched,
            input.mode_overrides,
        )
    {
        return Ok(std::borrow::Cow::Borrowed(base));
    }

    let Some(head) = crate::git::show_head(input.file)? else {
        return Ok(std::borrow::Cow::Borrowed(base));
    };
    if !is_stale_baseline(base, &head) {
        return Ok(std::borrow::Cow::Borrowed(base));
    }

    eprintln!(
        "[write] explicit baseline is missing committed exchange content — using HEAD as {} patch base",
        input.source
    );
    crate::ops_log::log_op(
        input.file,
        &format!(
            "explicit_baseline_rebased_to_head file={} source={} base_len={} head_len={} patches={} unmatched_len={}",
            input.file.display(),
            input.source,
            base.len(),
            head.len(),
            input.patches.len(),
            input.unmatched.trim().len()
        ),
    );
    Ok(std::borrow::Cow::Owned(head))
}

#[allow(clippy::too_many_arguments)]
fn log_exchange_write_diagnostic(
    file: &Path,
    source: &str,
    write_mode: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    before: &str,
    after: &str,
    patches: &[template::PatchBlock],
    unmatched: &str,
) {
    let before_exchange = agent_doc_element_exchange::exchange_content(before);
    let after_exchange = agent_doc_element_exchange::exchange_content(after);
    let touches_exchange =
        before_exchange != after_exchange || patch_touches_exchange(patches, unmatched);
    if !touches_exchange {
        return;
    }

    let before_hash = agent_doc_hash::content_hash(before);
    let after_hash = agent_doc_hash::content_hash(after);
    let live_exchange_edited = exchange_has_live_user_edit(baseline, before);
    let prompt_text_duplicated = exchange_prompt_text_duplicated(before, after);
    let before_prefix_count = before_exchange
        .map(exchange_prompt_prefix_count)
        .unwrap_or(0);
    let after_prefix_count = after_exchange
        .map(exchange_prompt_prefix_count)
        .unwrap_or(0);
    let normalized_prefix_delta = after_prefix_count.saturating_sub(before_prefix_count);
    let prompt_text_normalized = normalized_prefix_delta > 0;
    let cycle_id = crate::cycle_state::load(file)
        .ok()
        .flatten()
        .map(|state| state.cycle_id)
        .unwrap_or_else(|| "-".to_string());
    let writer_pid = std::process::id();
    let writer_exe = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "-".to_string());

    crate::ops_log::log_op(
        file,
        &format!(
            "exchange_write_diagnostic file={} writer_pid={} writer_exe={} source={} write_mode={} patch_id={} cycle_id={} before_hash={} after_hash={} live_exchange_edited={} prompt_text_duplicated={} prompt_text_normalized={} normalized_prefix_delta={} patches={} unmatched_len={}",
            file.display(),
            writer_pid,
            writer_exe,
            source,
            write_mode,
            patch_id.unwrap_or("-"),
            cycle_id,
            before_hash,
            after_hash,
            live_exchange_edited,
            prompt_text_duplicated,
            prompt_text_normalized,
            normalized_prefix_delta,
            patches.len(),
            unmatched.trim().len()
        ),
    );
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
        let ts = agent_doc_log_time::format_log_timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
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

mod run_entry;
pub use run_entry::*;

mod ipc;
pub use ipc::*;
// ---------------------------------------------------------------------------
// Internal helpers (same patterns as submit.rs)
// ---------------------------------------------------------------------------

fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = agent_doc_fs::state_lock_path_for(path)?;
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

fn capture_locked_pre_response(path: &Path) -> Result<(std::fs::File, String)> {
    let doc_lock = acquire_doc_lock(path)?;
    let content_at_start = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    snapshot::save_pre_response(path, &content_at_start)?;
    Ok((doc_lock, content_at_start))
}

/// When the current file already contains the response block, prefer adopting
/// that current content and re-running transcript normalization instead of
/// CRDT-merging the same response a second time.
fn adopt_current_response_without_duplication(
    file: &Path,
    base: &str,
    content_ours: &str,
    content_current: &str,
    snapshot: Option<&str>,
    response: &str,
) -> Result<Option<String>> {
    if !agent_doc_turn::response_replay::response_already_applied(content_current, response)
        && !response_already_in_current(base, content_ours, content_current)
    {
        return Ok(None);
    }

    let mut repaired = content_current.to_string();
    if let Some(snapshot_doc) = snapshot {
        repaired = normalize_user_prompts_in_exchange_safe(&repaired, base, snapshot_doc, file);
    }
    repaired =
        normalize_template_structure_or_fail_preserving(&repaired, file, Some(content_current))?;
    Ok(Some(repaired))
}

fn normalize_final_template_content(
    file: &Path,
    base: &str,
    snapshot: Option<&str>,
    before_current: Option<&str>,
    content: &str,
    response: Option<&str>,
) -> Result<String> {
    let mut normalized = content.to_string();
    if let Some(snapshot_doc) = snapshot {
        normalized = normalize_user_prompts_in_exchange_safe(&normalized, base, snapshot_doc, file);
    }
    if let Some(stripped) = strip_prompt_prefix_from_response_body_first_lines(&normalized) {
        crate::ops_log::log_op(
            file,
            &format!(
                "flow=document_mutation stage=crdt_post_merge_guard reason=response_body_prompt_prefix_leak file={}",
                file.display()
            ),
        );
        normalized = stripped;
    }
    let preserve_current_or_base = before_current.or(Some(base));
    normalized = normalize_template_structure_or_fail_preserving(
        &normalized,
        file,
        preserve_current_or_base,
    )?;
    if let Some(before) = before_current {
        let (deduped, report) = repair_duplicate_prompt_artifacts(
            &normalized,
            file,
            DuplicatePromptRepairOptions::new("final-template")
                .with_before(Some(before))
                .preserving(Some(base))
                .preserving_current(Some(before)),
        )?;
        if report.changed() {
            normalized = normalize_template_structure_or_fail_preserving(
                &deduped,
                file,
                preserve_current_or_base,
            )?;
        }
    }
    if let Some(repaired) =
        repair_response_prompt_order_for_file(&normalized, response, file, Some(base))?
    {
        normalized = repaired;
        normalized = normalize_template_structure_or_fail_preserving(
            &normalized,
            file,
            preserve_current_or_base,
        )?;
    }
    if response_precedes_prompt_in_exchange(&normalized, response, Some(base)) {
        crate::ops_log::log_op(
            file,
            &format!(
                "response_prompt_order_rejected file={} reason=response_precedes_prompt",
                file.display()
            ),
        );
        anyhow::bail!(
            "response patchback is still positioned before prompt-bearing exchange text; refusing to commit mis-ordered closeout"
        );
    }
    Ok(normalized)
}

pub(crate) fn repair_response_prompt_order_for_file(
    doc: &str,
    response: Option<&str>,
    file: &Path,
    prompt_must_exist_in: Option<&str>,
) -> Result<Option<String>> {
    let repaired = repair_response_prompt_order_in_exchange(doc, response, prompt_must_exist_in)
        .with_context(|| {
            format!(
                "failed to parse {} for response/prompt order repair",
                file.display()
            )
        })?;
    if repaired.is_some() {
        crate::ops_log::log_op(
            file,
            &format!(
                "response_prompt_order_repaired file={} before_commit=true",
                file.display()
            ),
        );
    }
    Ok(repaired)
}

/// Atomic write: write to temp file then rename. Public for use by compact.
pub fn atomic_write_pub(path: &Path, content: &str) -> Result<()> {
    atomic_write(path, content)
}

/// Atomic write guarded by the same visible-buffer proof used by response writes.
pub fn atomic_write_if_current_pub(
    path: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
) -> Result<()> {
    guard_visible_write_idle_and_current(path, source, expected_current)?;
    atomic_write(path, content)
}

/// `#codefence-strip`: best-effort detection log for code-fence loss during
/// agent-doc document writes. Reads the existing file before the write, counts
/// opening fence lines (``` or ~~~ at line start after optional whitespace),
/// and logs an `ops.log` marker when the new content carries strictly fewer
/// fence openings than the old content. The marker is a detection signal for
/// the operator, not a hard assertion — a deliberate edit that removes a code
/// block also fires it. Fails open silently on any IO error.
fn log_fence_count_drop_if_any(path: &Path, new_content: &str) {
    let Some(old_content) = std::fs::read_to_string(path).ok() else {
        return;
    };
    let old_fences = count_code_fence_openings(&old_content);
    let new_fences = count_code_fence_openings(new_content);
    if new_fences < old_fences {
        crate::ops_log::log_op(
            path,
            &format!(
                "fence_count_dropped file={} old_fences={} new_fences={} old_len={} new_len={}",
                path.display(),
                old_fences,
                new_fences,
                old_content.len(),
                new_content.len(),
            ),
        );
    }
}

/// Atomic write through the 08b document write-authority end state
/// ([`crate::write_authority`]). Every editor-visible document `.md` write
/// serializes through the session actor's single ordered write queue
/// ([`crate::write_queue`]), so a supervisor write and an agent-finalize write
/// for the same document can never interleave. This was the `#pcpc5cut` migration
/// (gated `off → shadow → dual-write → authority → removed`); the cutover is
/// complete and the `AGENT_DOC_WRITE_AUTHORITY` flag + bare-write bypass were
/// removed, so routing is now unconditional.
///
/// `.agent-doc/` sidecar/snapshot writes and writes already executing on the
/// session-actor owner thread take the raw path directly (the latter prevents a
/// re-entrant mailbox deadlock).
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    if agent_doc_document_realtime::write_authority::is_visible_document(path)
        && !agent_doc_document_realtime::write_authority::within_owner_scope()
    {
        log_fence_count_drop_if_any(path, content);
        let base_dir = agent_doc_fs::find_project_root(path)
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
        let file = path.to_string_lossy().to_string();
        let result = crate::write_queue::serialized_atomic_write(&base_dir, &file, path, content);
        if result.is_ok() {
            // Log after the write lands so the document path canonicalizes
            // (ops.log root resolution requires the file to exist).
            crate::ops_log::log_op(
                path,
                &format!(
                    "write_authority action=routed transport=write_queue len={} hash={}",
                    content.len(),
                    agent_doc_hash::content_hash(content)
                ),
            );
        }
        return result;
    }

    atomic_write_raw(path, content)
}

/// The raw atomic disk write: write to a temp file then rename, recording
/// write-provenance for editor-visible documents. This is the gate ladder's
/// `off` path and the path taken inside the ordered write queue.
fn atomic_write_raw(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| "failed to write temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    record_document_write_provenance(path, content);
    Ok(())
}

/// Record write-provenance for agent-doc's own disk write to a session document
/// (#pcp2 / #ipc-drift-writeprovenance). Skips `.agent-doc/` sidecar/snapshot
/// writes — provenance is only meaningful for the editor-visible document. The
/// path is canonicalized to match the lookup key used by the visible-write
/// reconcile guard. Best-effort: never fails the write.
///
/// Shared by every agent-doc document-write path (the IPC/finalize `write.rs`
/// `atomic_write` and the direct-run `run.rs` `atomic_write`) so a foreign-looking
/// disk change from any agent-doc writer is positively attributed instead of
/// inferred from the `LIVE_BUFFER_STALE_SKEW_MS` mtime heuristic.
pub(crate) fn record_document_write_provenance(path: &Path, content: &str) {
    if !agent_doc_document_realtime::write_authority::is_visible_document(path) {
        return;
    }
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let write_id = uuid::Uuid::new_v4().to_string();
    let hash = agent_doc_hash::content_hash(content);
    if let Err(e) = agent_doc_debounce::record_write_provenance(
        &canonical,
        content.len(),
        &hash,
        &write_id,
        "agent",
    ) {
        eprintln!(
            "[write] WARNING: failed to record write provenance for {}: {}",
            path.display(),
            e
        );
    }
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

    /// #pcp2: a document disk write records write-provenance, but `.agent-doc/`
    /// sidecar/snapshot writes do not (provenance is only meaningful for the
    /// editor-visible document).
    #[test]
    fn atomic_write_records_provenance_for_document_not_sidecar() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("live-buffer")).unwrap();

        let doc = tmp.path().join("prov-doc.md");
        atomic_write(&doc, "hello document").unwrap();
        let doc_key = doc
            .canonicalize()
            .unwrap_or(doc.clone())
            .to_string_lossy()
            .to_string();
        let prov = agent_doc_debounce::write_provenance(&doc_key)
            .expect("document write should record provenance");
        assert_eq!(prov.len, "hello document".len());
        assert_eq!(prov.hash, agent_doc_hash::content_hash("hello document"));
        assert_eq!(prov.actor, "agent");
        assert!(!prov.write_id.is_empty());

        // A write under .agent-doc/ (sidecar/snapshot) must NOT record provenance.
        let sidecar = tmp.path().join(".agent-doc").join("snapshots").join("s.md");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        atomic_write(&sidecar, "snapshot bytes").unwrap();
        let sidecar_key = sidecar
            .canonicalize()
            .unwrap_or(sidecar.clone())
            .to_string_lossy()
            .to_string();
        assert!(
            agent_doc_debounce::write_provenance(&sidecar_key).is_none(),
            "an .agent-doc/ sidecar write must not record document provenance"
        );
    }

    #[test]
    fn best_effort_session_commit_fails_closed_when_live_buffer_is_ahead_of_disk() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/live-buffer")).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let doc = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous\n\n",
            "previous response\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        let editor_visible = format!("{committed}\neditor-only mutation\n");
        agent_doc_debounce::document_changed_with_content_for_editor(
            &doc.display().to_string(),
            &editor_visible,
            Some("jetbrains:test"),
        );

        let err = finalize_commit(&doc, CommitMode::BestEffort)
            .expect_err("session best-effort commit must fail closed on an unflushed live buffer");
        assert!(
            err.to_string()
                .contains("session-document best-effort commit failed")
                || err.to_string().contains("live editor buffer"),
            "error should identify the unresolved session closeout:\n{err}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::PreflightStarted,
            "best-effort session commit must not mark the turn committed"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_blocked_live_buffer_ahead_of_disk file="),
            "blocked live-buffer commit should be logged:\n{log}"
        );
    }

    /// `#codefence-strip`: detection-log regression — a write that drops a
    /// triple-backtick fence opening must surface a `fence_count_dropped`
    /// ops.log marker so the operator can grep for the incident.
    #[test]
    fn fence_count_drop_is_logged_for_visible_document_write() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("fence-doc.md");
        let fenced = "intro\n```js\nconst x = 1;\n```\ntail\n";
        atomic_write(&doc, fenced).unwrap();
        // First write has no prior file, so no drop log expected.
        let log_path = tmp.path().join(".agent-doc").join("logs").join("ops.log");
        let log_before = fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            !log_before.contains("fence_count_dropped"),
            "first write (no prior file) must not log a fence drop"
        );
        // Second write removes the fence — must log the drop.
        fs::write(&log_path, "").unwrap();
        atomic_write(&doc, "intro\nconst x = 1;\ntail\n").unwrap();
        let log_after = fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            log_after.contains("fence_count_dropped") && log_after.contains("old_fences=2"),
            "a write that drops a fence must log fence_count_dropped; got: {}",
            log_after
        );
    }

    /// `#codefence-strip`: the fence-opening counter recognizes both backtick
    /// and tilde fences, ignores longer backtick runs that are not fence
    /// openings (e.g. inline ``````), and treats indented fences as openings.
    #[test]
    fn count_code_fence_openings_handles_backtick_and_tilde() {
        assert_eq!(count_code_fence_openings("```\ncode\n```\n"), 2);
        assert_eq!(count_code_fence_openings("~~~\ncode\n~~~\n"), 2);
        assert_eq!(
            count_code_fence_openings("  ```js\nconst x = 1;\n  ```\n"),
            2
        );
        assert_eq!(count_code_fence_openings("no fences here"), 0);
        assert_eq!(count_code_fence_openings("```python\nprint('hi')\n```"), 2);
        assert_eq!(
            count_code_fence_openings("``````\nnot a fence open by CommonMark\n``````\n"),
            0
        );
    }

    /// 08b end state: a routed visible-document write still records write
    /// provenance, because the queue job runs `atomic_write` on the owner thread
    /// where the owner-scope guard takes the raw path (`atomic_write_raw`), and
    /// that raw path is what records provenance.
    #[test]
    fn write_authority_routed_write_records_provenance() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("routed-doc.md");
        atomic_write(&doc, "routed content").unwrap();
        assert_eq!(fs::read_to_string(&doc).unwrap(), "routed content");
        let key = doc
            .canonicalize()
            .unwrap_or(doc.clone())
            .to_string_lossy()
            .to_string();
        assert!(
            agent_doc_debounce::write_provenance(&key).is_some(),
            "the routed write's inner raw path records provenance"
        );
    }
    /// 08b end state: every editor-visible document write is routed through the
    /// session actor's ordered write queue (no flag). The routed write executes
    /// `atomic_write` again on the owner thread; the owner-scope re-entrancy
    /// guard keeps that inner write on the raw path, so this must not deadlock
    /// and the content must land.
    #[test]
    fn write_authority_visible_write_routes_through_queue_without_deadlock() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc").join("logs")).unwrap();
        let doc = tmp.path().join("routed-doc2.md");
        atomic_write(&doc, "routed content").unwrap();
        assert_eq!(fs::read_to_string(&doc).unwrap(), "routed content");
        let ops = fs::read_to_string(tmp.path().join(".agent-doc").join("logs").join("ops.log"))
            .unwrap_or_default();
        assert!(
            ops.contains("write_authority action=routed transport=write_queue"),
            "a visible-document write must report the routed decision to ops.log: {ops:?}"
        );
    }
    /// `.agent-doc/` sidecar writes are never routed — they always take the raw
    /// path directly.
    #[test]
    fn write_authority_never_routes_agent_doc_sidecars() {
        let tmp = TempDir::new().unwrap();
        let sidecar = tmp.path().join(".agent-doc").join("snapshots").join("s.md");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        atomic_write(&sidecar, "sidecar bytes").unwrap();
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "sidecar bytes");
    }
    #[test]
    fn queued_file_reposition_patch_carries_generation_token() {
        // #late-ipc-patch-duplicate-stall: the durable file reposition patch must
        // carry the cycle id + a baseline content hash so a LATE applier can
        // fence a superseded patch (drop instead of re-materialize a duplicate
        // response block). Reposition-only body invariant must hold too.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        let doc = root.join("plan.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — opus\nDone.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, content).unwrap();
        let cs = crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let result = queue_file_ipc_reposition_boundary(&doc, Some("abc123"), &[]).unwrap();
        assert!(matches!(result, FileIpcRepositionResult::Queued));

        let hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let patch_file = root.join(".agent-doc/patches").join(format!("{hash}.json"));
        let payload: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&patch_file).unwrap()).unwrap();

        assert_eq!(
            payload["cycle_id"].as_str(),
            Some(cs.cycle_id.as_str()),
            "queued reposition patch must tag the originating cycle id"
        );
        assert_eq!(
            payload["baseline_hash"].as_str(),
            Some(agent_doc_hash::content_hash(content).as_str()),
            "queued reposition patch must tag the baseline content hash it targets"
        );
        // Reposition-only invariant: no response body re-materialized.
        assert_eq!(payload["patches"], serde_json::json!([]));
        assert_eq!(payload["unmatched"], serde_json::json!(""));
        assert_eq!(payload["reposition_boundary"], serde_json::json!(true));
        assert!(agent_doc_ipc_protocol::existing_patch_is_reposition_only(
            &payload
        ));
    }
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
        // so we verify the pattern works via agent_doc_fs::snapshot_path_for + direct I/O.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();

        // Verify snapshot path computation works
        let snap_path = agent_doc_fs::snapshot_path_for(&doc).unwrap();
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
    fn visible_write_guard_blocks_when_editor_typing_active() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/typing")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "body\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);
        for _ in 0..50 {
            if agent_doc_debounce::is_typing_via_file(&doc_str, 60_000) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let err = guard_visible_write_idle_with_budget(&doc, "test_visible_write", 60_000, 0)
            .unwrap_err();

        assert!(err.to_string().contains("editor typing did not settle"));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_typing_defer_active_typing:test_visible_write"));
        assert!(log.contains("visible_write_deferred_active_typing"));
    }
    #[test]
    fn force_disk_closeout_queue_consume_bypasses_active_listener() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do the fix — opus-4-8\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the fix\n",
            "<!-- /agent:queue -->\n",
        )
        .to_string();
        fs::write(&doc, &source).unwrap();
        snapshot::save(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        // `#6b5h`: a real editor is attached — seed a live plugin-owner lease so the
        // non-force consume fails closed (protects the buffer) rather than taking
        // an unproven editor-delivery disk fallback.
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = consume_queue_prompts_for_done_ids_closeout(&doc, &[], false).unwrap_err();
        let err = format!("{err:?}");
        assert!(
            err.contains("refused direct disk write"),
            "non-force queue consume must fail closed with an active listener: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "non-force closeout must not write behind an active listener"
        );

        let outcome = consume_queue_prompts_for_done_ids_closeout(&doc, &[], true)
            .expect("force-disk closeout queue consume should write directly")
            .expect("force-disk closeout should consume the answered head");

        assert_eq!(outcome.consumed_count, 1);
        let result = fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("queue: stop") && !result.contains("queue_active: true"),
            "drained queue consume should clear the active queue flag:\n{result}"
        );
        assert!(
            !result.contains("- do the fix"),
            "force-disk recovery must remove the answered queue head:\n{result}"
        );
        assert!(
            result.contains("> do the fix"),
            "force-disk recovery must retain the answered prompt in the response quote:\n{result}"
        );
    }

    #[test]
    fn force_disk_closeout_pending_maintenance_bypasses_active_listener() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Reap me\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n",
        )
        .to_string();
        fs::write(&doc, &source).unwrap();
        snapshot::save(&doc, &source).unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = run_closeout_pending_maintenance(&doc, CommitMode::Required, false).unwrap_err();
        let err = format!("{err:?}");
        assert!(
            err.contains("refused direct disk write"),
            "non-force closeout pending maintenance must protect the active listener: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "non-force closeout must not write behind an active listener"
        );

        run_closeout_pending_maintenance(&doc, CommitMode::Required, true)
            .expect("force-disk closeout pending maintenance should write directly");

        let result = fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| agent_doc_element::element::is_backlog_component(&component.name))
            .unwrap()
            .content(&result)
            .to_string();
        assert!(
            !backlog_after.contains("[#done1]"),
            "force-disk maintenance must reap the completed item:\n{result}"
        );
        assert!(
            result.contains("[#keep1]"),
            "force-disk maintenance must retain unrelated active items:\n{result}"
        );
        assert!(
            result.contains("## Completed / Reaped") && result.contains("[#done1] Reap me"),
            "force-disk maintenance must archive the reaped item:\n{result}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        let reason_marker = ["reason", "force_disk"].join("=");
        assert!(
            log.contains("pending_maintenance_writeback")
                && log.contains("transport=disk_force")
                && log.contains(&reason_marker),
            "force-disk maintenance should leave an attributable transport log:\n{log}"
        );
    }

    #[test]
    fn closeout_pending_maintenance_defers_before_ipc_when_live_buffer_has_operator_edit() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Reap me\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n",
        )
        .to_string();
        fs::write(&doc, &source).unwrap();
        snapshot::save(&doc, &source).unwrap();

        let live_operator_buffer = source.replace(
            "<!-- /agent:backlog -->",
            "- [ ] [#operator] unsaved operator edit\n<!-- /agent:backlog -->",
        );
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &live_operator_buffer,
            "test-editor",
            "test",
            "1",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        let err = run_closeout_pending_maintenance(&doc, CommitMode::Required, false).unwrap_err();
        let err = format!("{err:?}");
        assert!(
            err.contains("visible editor buffer"),
            "pending maintenance should defer on unsaved operator buffer before IPC: {err}"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "deferred maintenance must leave the visible disk document untouched"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_deferred_live_buffer_changed")
                && log.contains("source=pending_maintenance"),
            "defer should be attributed to the visible-write guard:\n{log}"
        );
        assert!(
            !log.contains("pending_maintenance_editor_convergence_attempt"),
            "pending maintenance must not send an editor patch while operator edits are ahead of disk:\n{log}"
        );
    }

    #[test]
    fn closeout_pending_maintenance_skips_status_only_housekeeping_with_active_listener() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        let source = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Ready. Top backlog item: #old.\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#new] Keep me\n",
            "<!-- /agent:backlog -->\n",
        )
        .to_string();
        fs::write(&doc, &source).unwrap();
        snapshot::save(&doc, &source).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&source), Some(&source)).unwrap();
        crate::capture::capture_response(&doc, "Done.").unwrap();

        let _listener = crate::test_support::start_ack_without_content_listener(dir.path());
        crate::test_support::wait_for_live_prompt_drift_listener(dir.path());
        crate::test_support::seed_live_plugin_owner_lease(doc.to_str().unwrap());

        run_closeout_pending_maintenance(&doc, CommitMode::Required, false)
            .expect("status-only housekeeping should not block response closeout");

        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "closeout skip must leave visible operator-owned document text untouched"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_pending_maintenance_skipped")
                && log.contains("basis=no_tracked_work_closeout"),
            "skip should leave an attributable ops-log entry:\n{log}"
        );
    }

    #[test]
    fn visible_write_guard_blocks_when_current_changed_after_merge() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

<!--
scratch
-->
";
        fs::write(&doc, expected).unwrap();
        fs::write(
            &doc,
            expected.replace("scratch", "scratch\nstill typing this line"),
        )
        .unwrap();

        let err = guard_visible_write_idle_and_current(&doc, "test_current_changed", expected)
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("document changed after the response merge was computed")
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_current_changed:test_current_changed"));
        assert!(log.contains("visible_write_deferred_current_changed"));
    }
    #[test]
    fn visible_write_guard_blocks_when_idle_editor_buffer_differs_from_disk() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        let live_buffer = expected.replace(
            "<!-- /agent:exchange -->",
            "prompt typed but not saved\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            live_buffer.len(),
            &agent_doc_hash::content_hash(&live_buffer),
        )
        .unwrap();

        let err = guard_visible_write_idle_and_current(&doc, "test_live_buffer_changed", expected)
            .unwrap_err();

        assert!(
            err.to_string().contains("visible editor buffer"),
            "expected live-buffer guard error: {err}"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), expected);
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_current_changed:test_live_buffer_changed"));
        assert!(log.contains("visible_write_deferred_live_buffer_changed"));
    }
    #[test]
    fn visible_write_reconcile_treats_editor_matching_disk_as_reconcilable_drift() {
        // #nm1x: the editor reported a buffer that diverges from `expected` but
        // *matches the current on-disk content* (an independent document edit the
        // editor already saved). That is not a pending unsaved user edit, so the
        // guard must not fail closed — it reports the reconcilable DiskDrifted case
        // instead, letting the response re-merge against the fresh disk content.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
### Re: old
<!-- /agent:exchange -->
";
        // Disk + editor both carry an independent queue edit not present in
        // `expected`; the editor digest equals disk (saved, no pending edit).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n- do [#sibling]\n<!-- /agent:queue -->",
        );
        fs::write(&doc, &drifted).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            drifted.len(),
            &agent_doc_hash::content_hash(&drifted),
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_editor_matches_disk",
            expected,
            None,
        )
        .unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_live_buffer_matches_disk"),
            "expected provenance-suppression log: {log}"
        );
        assert!(
            log.contains("source=test_editor_matches_disk"),
            "marker must identify the write source: {log}"
        );
        assert!(
            log.contains(&format!("expected_len={}", expected.len())),
            "marker must carry expected length: {log}"
        );
        assert!(
            log.contains(&format!(
                "expected_hash={}",
                agent_doc_hash::content_hash(expected)
            )),
            "marker must carry expected hash: {log}"
        );
        assert!(
            log.contains(&format!("disk_len={}", drifted.len())),
            "marker must carry disk length: {log}"
        );
        assert!(
            log.contains(&format!(
                "disk_hash={}",
                agent_doc_hash::content_hash(&drifted)
            )),
            "marker must carry disk hash: {log}"
        );
        assert!(
            log.contains(&format!("live_len={}", drifted.len())),
            "marker must carry live-buffer length: {log}"
        );
        assert!(
            log.contains(&format!(
                "live_hash={}",
                agent_doc_hash::content_hash(&drifted)
            )),
            "marker must carry live-buffer hash: {log}"
        );
        assert!(
            log.contains("live_ts="),
            "marker must carry live timestamp: {log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "must not record a fail-closed live-buffer block: {log}"
        );
    }
    #[test]
    fn visible_write_reconcile_reports_clean_when_disk_matches() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        fs::write(&doc, expected).unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_clean", expected, None).unwrap();
        assert!(matches!(outcome, VisibleWriteReconcile::Clean));
    }
    #[test]
    fn visible_write_reconcile_accepts_live_buffer_matching_target() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/live-buffer")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected = "\
<!-- agent:exchange patch=append -->
❯ Fix the repair retry
<!-- /agent:exchange -->
";
        let target = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: Fix the repair retry - gpt-5\n\nDone.\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, expected).unwrap();
        let doc_str = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &doc_str,
            target.len(),
            &agent_doc_hash::content_hash(&target),
        )
        .unwrap();

        let outcome = guard_visible_write_reconcile_with_target(
            &doc,
            "test_live_buffer_matches_target",
            expected,
            Some(&target),
        )
        .unwrap();

        assert!(matches!(outcome, VisibleWriteReconcile::Clean));
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            expected,
            "the guard only classifies proof; the caller still owns the disk write"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("visible_write_live_buffer_matches_target")
                && log.contains("source=test_live_buffer_matches_target"),
            "target-matching live-buffer proof should be logged:\n{log}"
        );
        assert!(
            !log.contains("visible_write_deferred_live_buffer_changed"),
            "a target-matching live buffer must not trip the drift guard:\n{log}"
        );
    }
    #[test]
    fn visible_write_reconcile_reports_disk_drift_without_live_buffer_edit() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        let expected =
            "<!-- agent:exchange patch=append -->\n### Re: x\n<!-- /agent:exchange -->\n";
        // Disk grew under us with a foreign agent-doc append (no live editor buffer
        // sidecar = no pending user edit), so the guard must report it as a
        // reconcilable drift rather than failing closed (#ipc-drift-visbuf-reconcile).
        let drifted = expected.replace(
            "<!-- /agent:exchange -->",
            "### Re: foreign\n<!-- /agent:exchange -->",
        );
        fs::write(&doc, &drifted).unwrap();

        let outcome =
            guard_visible_write_reconcile_with_target(&doc, "test_drift", expected, None).unwrap();
        match outcome {
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                assert_eq!(fresh_current, drifted);
            }
            VisibleWriteReconcile::Clean => panic!("expected DiskDrifted, got Clean"),
        }
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("visible_write_disk_drift_reconcilable"));
    }
    #[test]
    fn reconcile_visible_write_remerges_foreign_append_then_lands_clean() {
        // The first guard call sees a foreign disk append; the loop must re-merge
        // the captured response against the fresh disk content and then succeed
        // without failing closed and stranding the response (#ipc-drift-visbuf-reconcile).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed\n").unwrap();

        let base = "BASE";
        let foreign = "BASE+FOREIGN";
        let guard_calls = std::cell::RefCell::new(0usize);
        let recompute_calls = std::cell::RefCell::new(0usize);

        let guard =
            |_f: &Path, expected: &str, _payload: &String| -> Result<VisibleWriteReconcile> {
                let mut n = guard_calls.borrow_mut();
                *n += 1;
                if *n == 1 {
                    assert_eq!(expected, base);
                    Ok(VisibleWriteReconcile::DiskDrifted {
                        fresh_current: foreign.to_string(),
                    })
                } else {
                    assert_eq!(expected, foreign);
                    Ok(VisibleWriteReconcile::Clean)
                }
            };
        let recompute = |current: &str| -> Result<String> {
            *recompute_calls.borrow_mut() += 1;
            // The re-merge incorporates the foreign disk content + the response.
            Ok(format!("{current}+RESPONSE"))
        };
        let fail_closed = |_f: &Path, _c: &str, _payload: &String| -> Result<()> {
            panic!("must not fail closed on a reconcilable foreign append");
        };

        let (current, payload) = reconcile_visible_write(
            &doc,
            base.to_string(),
            format!("{base}+RESPONSE"),
            3,
            guard,
            recompute,
            fail_closed,
        )
        .unwrap();

        assert_eq!(current, foreign);
        assert_eq!(payload, "BASE+FOREIGN+RESPONSE");
        assert_eq!(*guard_calls.borrow(), 2);
        assert_eq!(*recompute_calls.borrow(), 1);
    }
    #[test]
    fn reconcile_visible_write_falls_back_to_fail_closed_when_drift_never_settles() {
        // A document that keeps drifting past the attempt bound must fall back to
        // the fail-closed guard so the operator retries instead of looping forever.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed\n").unwrap();

        let counter = std::cell::RefCell::new(0usize);
        let guard = |_f: &Path, _e: &str, _payload: &String| -> Result<VisibleWriteReconcile> {
            let mut n = counter.borrow_mut();
            *n += 1;
            Ok(VisibleWriteReconcile::DiskDrifted {
                fresh_current: format!("drift-{n}"),
            })
        };
        let recompute = |current: &str| -> Result<String> { Ok(current.to_string()) };
        let fail_closed = |_f: &Path, _c: &str, _payload: &String| -> Result<()> {
            anyhow::bail!("document still changing");
        };

        let err = reconcile_visible_write(
            &doc,
            "start".to_string(),
            "start".to_string(),
            3,
            guard,
            recompute,
            fail_closed,
        )
        .unwrap_err();
        assert!(err.to_string().contains("document still changing"));
        assert_eq!(*counter.borrow(), 3);
    }
    #[test]
    fn capture_locked_pre_response_reads_live_content_after_lock_wait() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "original\n").unwrap();

        let lock_path = agent_doc_fs::state_lock_path_for(&doc).unwrap();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let held_lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        held_lock.lock_exclusive().unwrap();

        let doc_for_thread = doc.clone();
        let capture = std::thread::spawn(move || capture_locked_pre_response(&doc_for_thread));

        std::thread::sleep(Duration::from_millis(100));
        fs::write(&doc, "updated while waiting\n").unwrap();
        drop(held_lock);

        let (captured_lock, captured_content) = capture.join().unwrap().unwrap();
        drop(captured_lock);

        assert_eq!(captured_content, "updated while waiting\n");
        assert_eq!(
            snapshot::load_pre_response(&doc).unwrap().unwrap(),
            "updated while waiting\n"
        );
    }
    #[test]
    fn missing_explicit_baseline_reads_migrated_baseline_after_document_move() {
        let dir = TempDir::new().unwrap();
        for subdir in [
            "snapshots",
            "baselines",
            "locks",
            "pending",
            "crdt",
            "pre-response",
        ] {
            fs::create_dir_all(dir.path().join(".agent-doc").join(subdir)).unwrap();
        }

        let session_uuid = "moved-baseline-session";
        let old_doc = dir.path().join("old.md");
        let doc_content = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\nBody\n",
            session_uuid
        );
        fs::write(&old_doc, &doc_content).unwrap();

        let old_hash = agent_doc_fs::document_state_hash(&old_doc).unwrap();
        let old_snapshot = dir
            .path()
            .join(".agent-doc/snapshots")
            .join(format!("{}.md", old_hash));
        fs::write(&old_snapshot, &doc_content).unwrap();
        let old_baseline = dir
            .path()
            .join(".agent-doc/baselines")
            .join(format!("{}.md", old_hash));
        fs::write(&old_baseline, "preflight baseline\n").unwrap();

        let new_doc = dir.path().join("new.md");
        fs::rename(&old_doc, &new_doc).unwrap();

        assert!(snapshot::try_migrate_renamed(&new_doc).unwrap());
        assert!(!old_baseline.exists());
        let migrated_baseline = agent_doc_fs::baseline_path_for(&new_doc).unwrap();
        assert!(migrated_baseline.exists());

        let baseline = read_explicit_baseline(&new_doc, Some(&old_baseline))
            .unwrap()
            .expect("baseline should be recovered from migrated hash");
        assert_eq!(baseline, "preflight baseline\n");
    }
    #[test]
    fn guard_rejects_normal_write_when_diff_requests_compact_exchange() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Old progress.\n\n",
            "compact exchange\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, current).unwrap();
        snapshot::save(&doc, baseline).unwrap();

        let err = guard_no_exchange_compaction_request_between(&doc, Some(baseline), current)
            .expect_err("ordinary response write should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("compact exchange"));
        assert!(msg.contains("agent-doc compact"));
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
    fn try_ipc_file_fallback_skips_when_patches_already_applied_to_live_buffer() {
        // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
        // `[#ipcfilehashskip]` defense-in-depth dedupe gate.
        //
        // When the live file already contains the response body (e.g. via a
        // prior socket-IPC retry whose sidecar ack arrived late), the file-IPC
        // fallback must hash-compare patch outcome vs current and skip the
        // write so it does not stack a duplicate `### Re:` heading.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let already_applied_content = concat!(
            "---\nsession: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic — gpt-5\n\n",
            "Implemented.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, already_applied_content).unwrap();

        // Build a patch whose application against the current file is a no-op
        // (replace exchange with the same content it already has).
        let exchange_body = "### Re: topic — gpt-5\n\nImplemented.\n";
        let patch = agent_doc_template::PatchBlock::new("exchange", exchange_body);

        let started = std::time::Instant::now();
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        let elapsed = started.elapsed();

        assert!(
            result.success,
            "already-applied file-IPC fallback must short-circuit as success"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "skip path must not block on the 2s IPC timeout: elapsed={:?}",
            elapsed
        );

        let patches_dir = agent_doc_dir.join("patches");
        let leftover: Vec<_> = fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            leftover.is_empty(),
            "skip path must clean up any fallback patch files left around"
        );

        let live_after = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            live_after, already_applied_content,
            "skip path must not mutate the live file"
        );

        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("file_ipc_fallback_skip_already_applied"),
            "skip event must be logged for audit:\n{ops_log}"
        );
    }
    #[test]
    fn try_ipc_returns_false_when_no_patches_dir() {
        // Without .agent-doc/patches/, IPC should return false immediately
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
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
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();

        let patch = agent_doc_template::PatchBlock::new("exchange", "new content");

        // This will timeout after 2s — patch file is written but never consumed
        let result = try_ipc(&doc, &[patch], "", None, None, None, None, None).unwrap();
        assert!(
            !result.success,
            "should return false on timeout (no plugin)"
        );

        // Patch file should remain queued for an editor-owned retry.
        let patches_dir = agent_doc_dir.join("patches");
        let entries: Vec<_> = fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !entries.is_empty(),
            "patch file should remain queued after timeout"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=no_ack")
                && log.contains("recovery=retry_without_disk_write"),
            "IPC timeout should log the failed invariant and recovery path:\n{log}"
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

        let patch = agent_doc_template::PatchBlock::new("exchange", "new content");

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
    fn try_ipc_rejects_consumed_partial_response_materialization() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let original = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, original).unwrap();

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: missed patchback - gpt-5\n\nRecovered answer.",
        );
        let partial = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "content\n",
            "### Re: missed patchback - gpt-5\n",
            "<!-- /agent:exchange -->\n"
        );

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let partial_for_watcher = partial.to_string();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let _ = fs::write(&doc_for_watcher, &partial_for_watcher);
                            let _ =
                                fs::write(ack_dir.join(format!("{pid}.md")), &partial_for_watcher);
                        }
                        let _ = fs::remove_file(path);
                        return;
                    }
                }
            }
        });

        let result = try_ipc(&doc, &[patch], "", None, Some(original), None, None, None).unwrap();
        assert!(
            !result.success,
            "IPC consume without the full response body must fall back instead of saving a successful snapshot"
        );

        let snap = snapshot::load(&doc).unwrap();
        assert!(
            snap.as_deref()
                .is_none_or(|content| !content.contains("Recovered answer.")),
            "partial IPC materialization must not become the committed snapshot: {snap:?}"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_materialization_missing_response") && log.contains("source=file_ipc"),
            "missing response materialization should be logged for operator repair:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=missing_response_probe")
                && log.contains("recovery=retry_without_disk_write"),
            "missing response materialization should name its invariant and recovery:\n{log}"
        );
    }
    #[test]
    fn file_ipc_consumed_with_live_exchange_edit_requires_ack_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[],
            "",
            None,
            Some(baseline),
            None,
            None,
            Some("patch-live-edit"),
        )
        .unwrap();
        watcher.join().unwrap();

        assert!(
            !result.success,
            "file IPC consumption with live exchange edits and unchanged disk content must fall back"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "unacknowledged live-edit IPC must not become the saved snapshot"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("file_ipc_live_exchange_unacknowledged")
                && log.contains("patch_id=patch-live-edit"),
            "unacknowledged live-edit IPC should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=live_exchange_without_ack_content")
                && log.contains("recovery=retry_without_disk_write"),
            "unacknowledged live-edit IPC should name its invariant and recovery:\n{log}"
        );
    }
    #[test]
    fn file_ipc_accepts_ack_content_sidecar_when_disk_lags_live_exchange() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let ack_content = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "### Re: live prompt — gpt-5\n\n",
            "Handled.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains-test-editor";
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            before,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let ack_for_watcher = ack_content.to_string();
        let doc_for_watcher = doc_str.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    if let Ok(text) = fs::read_to_string(entry.path())
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                    {
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                        let _ = agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_for_watcher,
                            &ack_for_watcher,
                            editor_id,
                            "jetbrains",
                            "test",
                            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        );
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: live prompt — gpt-5\n\nHandled.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            None,
            None,
            Some("patch-live-edit-ack"),
        )
        .unwrap();
        watcher.join().unwrap();

        assert!(
            result.success,
            "ack-content proof should let file IPC accept an applied response even when disk still shows the pre-ack live exchange edit"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(ack_content),
            "snapshot must use the authoritative ack-content sidecar"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            ack_content,
            "proven ack-content must be written through so stale disk cannot overwrite the editor-visible response"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("file_ipc_live_exchange_unacknowledged"),
            "ack-content proof must bypass the unacknowledged live-edit fallback:\n{log}"
        );
        assert!(
            log.contains("ack_content_disk_write_through"),
            "ack-content disk write-through should be auditable:\n{log}"
        );
    }
    #[test]
    fn file_ipc_ack_content_live_prompt_drift_requires_visible_repair() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ New prompt typed during closeout\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let ack_content = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "❯ New prompt typed during closeout\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let ack_for_watcher = ack_content.to_string();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    if let Ok(text) = fs::read_to_string(entry.path())
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                    {
                        let _ = fs::write(&doc_for_watcher, &ack_for_watcher);
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("patch-live-prompt-drift"),
        );
        watcher.join().unwrap();

        let result = result.unwrap();
        assert!(
            !result.success,
            "live prompt drift without response materialization must not close out successfully"
        );
        assert_ne!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "snapshot must not silently advance to content_ours when the visible editor/worktree still holds the drift candidate"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            ack_content,
            "visible live prompt should remain in the working tree for the next cycle"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("snapshot_saved_file_ipc"),
            "unsafe snapshot adoption must not be saved after an unmaterialized response:\n{log}"
        );
    }

    #[test]
    fn file_ipc_ack_content_partial_exchange_word_requires_visible_repair() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "operator-partial-wo\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let ack_content = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "operator-partial-wo\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let ack_for_watcher = ack_content.to_string();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    if let Ok(text) = fs::read_to_string(entry.path())
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                    {
                        let _ = fs::write(&doc_for_watcher, &ack_for_watcher);
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("patch-partial-exchange-word"),
        );
        watcher.join().unwrap();

        let result = result.unwrap();
        assert!(
            !result.success,
            "partial operator text without response materialization must not close out successfully"
        );
        assert_ne!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "snapshot must not silently advance to content_ours when the visible editor/worktree still holds a partial operator word"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            ack_content,
            "visible partial operator word should remain in the working tree for the next cycle"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("snapshot_saved_file_ipc"),
            "unsafe snapshot adoption must not be saved after an unmaterialized response:\n{log}"
        );
    }
    // #exch-intermix-falsedrop: a queue item consumed (struck) this cycle is
    // recorded as "dropped" by the drift-time candidate-vs-content_ours heuristic,
    // but it survives struck in the adopted snapshot, so auto-recovery must STILL
    // fire (it is a false-positive drop record, not real user-content loss). This
    // is the exact live wedge from agent-doc-bugs2 #opsproof-falsepos closeout.
    #[test]
    fn file_ipc_ack_content_post_exchange_comment_drift_uses_content_ours_snapshot() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Typing a new prompt below exchange during closeout. #next-steps\n",
            "-->\n"
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n"
        );
        let ack_content = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Typing a new prompt below exchange during closeout. #next-steps\n",
            "-->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let ack_for_watcher = ack_content.to_string();
        let watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    if let Ok(text) = fs::read_to_string(entry.path())
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                        && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                    {
                        let _ = fs::write(&doc_for_watcher, &ack_for_watcher);
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: Please reply — gpt-5\n\nAnswered.",
        );
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("patch-post-exchange-comment-drift"),
        )
        .unwrap();
        watcher.join().unwrap();

        assert!(
            result.success,
            "IPC delivery itself should remain successful"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "snapshot must not absorb post-exchange comment text typed after preflight"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            ack_content,
            "visible post-exchange comment text should remain in the working tree for the next cycle"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("flow=document_mutation")
                && log.contains("stage=ipc_snapshot_adoption")
                && log.contains("reason=live_prompt_drift_after_preflight")
                && log.contains("ipc_snapshot_adoption_blocked"),
            "unsafe post-exchange drift adoption should be logged:\n{log}"
        );
    }
    #[test]
    fn file_ipc_post_dedupe_unchanged_exchange_requires_ack_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "<!-- /agent:exchange -->\n"
        );
        let before = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let plugin_after = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "content\n",
            "live prompt\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(mut entries) = fs::read_dir(&watcher_dir) else {
                    continue;
                };
                if let Some(Ok(entry)) = entries.next() {
                    let _ = fs::write(&doc_for_watcher, plugin_after);
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch = agent_doc_template::PatchBlock::new("exchange", "live prompt\n");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            None,
            None,
            Some("patch-post-dedupe"),
        )
        .unwrap();

        assert!(
            !result.success,
            "file IPC must fall back when final deduped exchange is unchanged without ack-content proof"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "unacknowledged post-dedupe no-op IPC must not become the saved snapshot"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("file_ipc_live_exchange_unacknowledged")
                && log.contains("patch_id=patch-post-dedupe"),
            "post-dedupe unacknowledged live-edit IPC should be logged:\n{log}"
        );
        assert!(
            !log.contains("snapshot_saved_file_ipc"),
            "post-dedupe unacknowledged live-edit IPC must not save a file-IPC snapshot:\n{log}"
        );
    }
    #[test]
    fn try_ipc_full_content_returns_false_when_disabled() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let result = try_ipc_full_content(&doc, "new content").unwrap();
        assert!(
            !result,
            "full-content IPC is disabled and should return false"
        );
    }
    #[test]
    fn apply_patches_sanitize_unmatched_prevents_duplicate_exchange_block() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Existing answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        let unmatched = "### Re: deploy — gpt-5\n\nDeployed.\n\n<!-- agent:exchange -->\nLeaked content\n<!-- /agent:exchange -->\n";
        let mut sanitized_unmatched = unmatched.to_string();
        agent_doc_template::sanitize::sanitize_unmatched(&mut sanitized_unmatched);

        let result =
            crate::template_io::apply_patches(doc, &[], &sanitized_unmatched, &file).unwrap();

        let exchange_opens = result.matches("<!-- agent:exchange").count();
        assert_eq!(
            exchange_opens, 1,
            "must have exactly one exchange opener, got {exchange_opens}:\n{result}"
        );
        assert!(
            !result.contains("<!-- agent:exchange -->\nLeaked content"),
            "leaked exchange markers must be escaped, not create a second block"
        );
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "escaped markers should appear in result"
        );
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

        let patch = agent_doc_template::PatchBlock::new("exchange", "agent response content");

        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\n<!-- /agent:exchange -->\n";

        // Simulate the plugin applying the patch and performing a safe boundary
        // reposition in the same editor-visible write.
        let after_plugin_write = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\n<!-- agent:boundary:plugin-boundary -->\n<!-- /agent:exchange -->\n";

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
        assert!(snap.contains("plugin-boundary"));
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
    #[test]
    fn build_ipc_node_patches_json_tracks_strike_and_insert_by_node_key() {
        let before = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let after = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- ~~do [#beta]~~
- do [#gamma]
<!-- /agent:queue -->
";

        let patches = build_ipc_node_patches_json(Some(before), Some(after));

        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:beta:0"
                && patch["op"] == "strike"
                && patch["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("- ~~do [#beta]~~"))
                && patch["expected_content"].as_str() == Some("- do [#beta]\n")
                && patch["expected_content_hash"].as_str().is_some()
        }));
        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:gamma:0"
                && patch["op"] == "insert"
                && patch["after"] == "queue:0:beta:0"
                && patch["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("- do [#gamma]"))
        }));
    }
    #[test]
    fn build_ipc_node_patches_json_tracks_reorder_without_text_matching() {
        let before = "\
<!-- agent:queue priority go -->
- do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let after = "\
<!-- agent:queue priority go -->
- do [#beta]
- do [#alpha]
<!-- /agent:queue -->
";

        let patches = build_ipc_node_patches_json(Some(before), Some(after));

        assert!(patches.iter().any(|patch| {
            patch["component"] == "queue"
                && patch["node_key"] == "queue:0:beta:0"
                && patch["op"] == "move"
                && patch["before"] == "queue:0:alpha:0"
        }));
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
        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
        // Unmatched content is identical to what's already in the exchange
        let result = build_ipc_patches_json(&doc, &patches, existing, None, None).unwrap();

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

        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
        let new_content = "Completely new agent response.";
        let result = build_ipc_patches_json(&doc, &patches, new_content, None, None).unwrap();

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
    fn build_ipc_patches_json_preserves_leading_code_fence_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ show fenced prompt\n",
            "```\n",
            "prompt body\n",
            "```\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, doc_content).unwrap();

        let patches = vec![agent_doc_template::PatchBlock::new(
            "exchange",
            "```\nresponse body\n```\n",
        )];
        let result = build_ipc_patches_json(&doc, &patches, "", None, None).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["component"].as_str().unwrap(), "exchange");
        assert_eq!(result[0]["op"].as_str().unwrap(), "append");
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            "```\nresponse body\n```\n",
            "IPC payload must keep a leading code fence byte-for-byte"
        );
    }
    #[test]
    fn synthesis_normalizes_prefix_lines_for_unmatched_exchange_content() {
        // Regression for the JB-plugin bare `do #expatch...` shape: when IPC
        // synthesizes an exchange patch from unmatched content, it must bake the
        // computed `normalize_prefix_lines` into that synthesized patch because
        // the plugin normalizes before applying patches.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nPrevious response.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
        let unmatched = "do #expatch. spec-test-build-install-commit-push\n### Re: #expatch — gpt-5\n\nImplemented.\n";
        let prefix_lines = vec!["do #expatch. spec-test-build-install-commit-push".to_string()];
        let result = build_ipc_patches_json(
            &doc,
            &patches,
            unmatched,
            Some(prefix_lines.as_slice()),
            None,
        )
        .unwrap();

        assert_eq!(
            result.len(),
            1,
            "synthesis should still produce one exchange patch"
        );
        assert_eq!(
            result[0]["component"].as_str().unwrap(),
            "exchange",
            "synthesized patch should target exchange"
        );
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            "❯ do #expatch. spec-test-build-install-commit-push\n### Re: #expatch — gpt-5\n\nImplemented.",
            "synthesized unmatched exchange content must carry the prefixed prompt line"
        );
    }
    #[test]
    fn file_ipc_synthesized_exchange_patch_omits_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let prompt = "do #ipcfull. spec-test-build-install-commit-push";
        let original = format!(
            "---\nagent_doc_format: template\n---\n\n\
<!-- agent:exchange patch=append -->\n{prompt}\n<!-- /agent:exchange -->\n"
        );
        let unmatched = "### Re: ipc full-content guard - gpt-5\n\nDone.";
        let after_plugin_write = format!(
            "---\nagent_doc_format: template\n---\n\n\
<!-- agent:exchange patch=append -->\n❯ {prompt}\n{unmatched}\n<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, &original).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains-test-editor";
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &original,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let seen_payload = std::sync::Arc::new(std::sync::Mutex::new(None));
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let doc_str_for_watcher = doc_str.clone();
        let seen_for_watcher = seen_payload.clone();
        let after_for_watcher = after_plugin_write.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = fs::read_to_string(&path)
                            && let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text)
                        {
                            if let Some(pid) = payload.get("patch_id").and_then(|v| v.as_str()) {
                                let _ = fs::write(
                                    ack_dir.join(format!("{pid}.md")),
                                    &after_for_watcher,
                                );
                            }
                            *seen_for_watcher.lock().unwrap() = Some(payload);
                        }
                        let _ = fs::write(&doc_for_watcher, &after_for_watcher);
                        let _ = agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                            &doc_str_for_watcher,
                            &after_for_watcher,
                            editor_id,
                            "jetbrains",
                            "test",
                            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                        );
                        let _ = fs::remove_file(path);
                        return;
                    }
                }
            }
        });

        let prefix_lines = vec![prompt.to_string()];
        let result = try_ipc(
            &doc,
            &[],
            unmatched,
            None,
            Some(&original),
            Some(&after_plugin_write),
            Some(prefix_lines.as_slice()),
            Some("patch-synth-no-full-content"),
        )
        .unwrap();

        assert!(
            result.success,
            "file IPC should accept the synthesized exchange patch"
        );
        let payload = seen_payload
            .lock()
            .unwrap()
            .clone()
            .expect("watcher should capture the IPC payload");
        assert!(
            payload.get("fullContent").is_none(),
            "template response IPC with a synthesized component patch must not send fullContent: {payload}"
        );
        assert_eq!(
            payload["unmatched"], "",
            "synthesized exchange patch must consume unmatched text instead of sending it twice"
        );
        let patches = payload["patches"]
            .as_array()
            .expect("payload patches should be an array");
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(patches[0]["content"], unmatched);
        assert_eq!(payload["normalize_prefix_lines"][0], prompt);
    }
    #[test]
    fn template_normalization_only_file_ipc_omits_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let prompt = "do #norm-only. spec-test-build-install-commit-push";
        let original = format!(
            "---\nagent_doc_format: template\n---\n\n\
<!-- agent:exchange patch=append -->\n{prompt}\n<!-- agent:boundary:test -->\n<!-- /agent:exchange -->\n"
        );
        let normalized = original.replace(prompt, &format!("❯ {prompt}"));
        fs::write(&doc, &original).unwrap();
        let doc_str = doc.to_string_lossy().to_string();
        let editor_id = "jetbrains-test-editor";
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            &original,
            editor_id,
            "jetbrains",
            "test",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let seen_payload = std::sync::Arc::new(std::sync::Mutex::new(None));
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let doc_str_for_watcher = doc_str.clone();
        let normalized_for_watcher = normalized.clone();
        let seen_for_watcher = seen_payload.clone();
        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(3) {
                let Ok(entries) = fs::read_dir(&patches_dir) else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("json") {
                        continue;
                    }
                    let text = fs::read_to_string(&path).unwrap();
                    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
                    let patch_id = payload
                        .get("patch_id")
                        .and_then(|value| value.as_str())
                        .unwrap()
                        .to_string();
                    fs::write(&doc_for_watcher, &normalized_for_watcher).unwrap();
                    agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
                        &doc_str_for_watcher,
                        &normalized_for_watcher,
                        editor_id,
                        "jetbrains",
                        "test",
                        &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
                    )
                    .unwrap();
                    fs::write(
                        ack_dir.join(format!("{patch_id}.md")),
                        &normalized_for_watcher,
                    )
                    .unwrap();
                    *seen_for_watcher.lock().unwrap() = Some(payload);
                    fs::remove_file(path).unwrap();
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            false
        });

        let prefix_lines = vec![prompt.to_string()];
        let result = try_ipc(
            &doc,
            &[],
            "",
            None,
            Some(&original),
            Some(&normalized),
            Some(prefix_lines.as_slice()),
            Some("patch-template-norm-only-no-full-content"),
        )
        .unwrap();

        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");
        assert!(
            result.success,
            "normalization-only template IPC should accept a narrow payload"
        );
        let payload = seen_payload
            .lock()
            .unwrap()
            .clone()
            .expect("watcher should capture the IPC payload");
        assert!(
            payload.get("fullContent").is_none(),
            "template normalization-only IPC must not send fullContent: {payload}"
        );
        assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
        assert_eq!(payload["unmatched"], "");
        assert_eq!(payload["normalize_prefix_lines"][0], prompt);

        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("scope=template_frontmatter"),
            "template fullContent rejection should be logged:\n{ops_log}"
        );
    }
    #[test]
    fn effective_unmatched_cleared_when_synthesis_consumes_content() {
        // When synthesis consumes the unmatched content (patches input was empty,
        // ipc_patches output is non-empty), effective_unmatched should be "".
        // This prevents the plugin from applying the content twice (IPC duplicate bug).
        let patches: Vec<agent_doc_template::PatchBlock> = vec![];
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
        let explicit_patch = agent_doc_template::PatchBlock::new("exchange", "response");
        let patches_explicit = [explicit_patch];
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
    fn exchange_write_diagnostic_logs_live_edit_provenance() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content.\n<!-- /agent:exchange -->\n";
        let before = "<!-- agent:exchange patch=append -->\nOld content.\nlive prompt\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\nOld content.\n❯ live prompt\nlive prompt\n### Re: response\nDone.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, before).unwrap();
        let patches = vec![template::PatchBlock::new(
            "exchange",
            "### Re: response\nDone.\n",
        )];

        log_exchange_write_diagnostic(
            &doc,
            "test_source",
            "test_mode",
            Some("patch-123"),
            Some(baseline),
            before,
            after,
            &patches,
            "",
        );

        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("exchange_write_diagnostic"));
        assert!(log.contains("source=test_source"));
        assert!(log.contains("write_mode=test_mode"));
        assert!(log.contains("patch_id=patch-123"));
        assert!(log.contains("live_exchange_edited=true"));
        assert!(log.contains("prompt_text_duplicated=true"));
        assert!(log.contains("prompt_text_normalized=true"));
        assert!(log.contains("normalized_prefix_delta=1"));
        assert!(log.contains("before_hash="));
        assert!(log.contains("after_hash="));
        assert!(log.contains("writer_pid="));
    }
    #[test]
    fn prompt_dedupe_skips_assistant_response_quotes() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:old -->\n",
            "quote this exact line\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ quote this exact line\n",
            "### Re: response — gpt-5\n\n",
            "quote this exact line\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), after, "test_ipc").unwrap();

        assert!(
            !changed,
            "assistant response quotes must not be treated as duplicate prompt text"
        );
        assert_eq!(repaired.matches("quote this exact line").count(), 2);
    }
    #[test]
    fn normalize_template_structure_repairs_live_prompt_prefix_variant_after_boundary() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert_eq!(repaired.matches("sent + re \n").count(), 0);
        assert_eq!(repaired.matches("sent + received \n").count(), 1);
    }
    #[test]
    fn normalize_template_structure_rejects_mixed_duplicate_scaffold_prompt_prefix_variant() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        fs::write(&doc, content).unwrap();

        let err = normalize_template_structure_or_fail(content, &doc).unwrap_err();

        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=mixed_duplicate_scaffold_tail"));
    }
    #[test]
    fn normalize_template_structure_keeps_prefix_variant_inside_response_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: example — gpt-5\n\n",
            "This sentence is a prefix\n",
            "This sentence is a prefix variant in assistant prose\n",
            "<!-- agent:boundary:613974fd -->\n",
            "<!-- /agent:exchange -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(repaired.contains("This sentence is a prefix\n"));
        assert!(repaired.contains("This sentence is a prefix variant in assistant prose\n"));
    }
    #[test]
    fn ipc_snapshot_dedupes_live_prompt_prefix_variant() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "<!-- /agent:exchange -->\n"
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:613974fd -->\n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + re \n",
            "agent-doc on corky.md running opencode, the arrow key functionality works at first. But once a turn starts, the arrow keys are corrupted. Is there a way to log the key that is sent + received \n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), after, "test_ipc").unwrap();

        assert!(changed);
        assert_eq!(repaired.matches("sent + re \n").count(), 0);
        assert_eq!(repaired.matches("sent + received \n").count(), 1);
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("live_prompt_prefix_variant_repaired"));
        assert!(log.contains("ipc_snapshot_deduped"));
    }
    #[test]
    fn ipc_snapshot_scrubs_post_exchange_duplicate_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = before
            .replace(
                "<!-- /agent:exchange -->",
                "### Re: duplicate prompt cleanup — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
            )
            .replace(
                "<!-- agent:backlog -->",
                "<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n-->\n\n<!-- agent:backlog -->",
            );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), &after, "test_ipc").unwrap();

        assert!(changed);
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "IPC ack-content dedupe must scrub duplicate post-exchange prompt text:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "IPC ack-content dedupe must preserve the ordinary HTML comment shell:\n{repaired}"
        );
        assert!(repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("post_exchange_duplicate_prompt_comment_removed"));
        assert!(log.contains("duplicate_prompt_artifact_repair"));
        assert!(log.contains("post_exchange_comments=true"));
    }
    #[test]
    fn ipc_snapshot_preserves_owned_post_exchange_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "The post-exchange IPC handoff scratch comment should not be deleted. #spec-test-build-install-commit-push";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Keep this owned scratch note visible.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let after = before.replace(
        "<!-- /agent:exchange -->",
        "### Re: IPC handoff — gpt-5\n\nHandled.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
    );
        fs::write(&doc, &before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(&before), &after, "test_ipc").unwrap();

        assert!(
            !changed,
            "owned post-exchange comments should not force IPC snapshot repair"
        );
        assert!(
        repaired.contains(&format!(
            "<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nKeep this owned scratch note visible.\n-->"
        )),
        "IPC ack-content dedupe must preserve owned mixed scratch comments:\n{repaired}"
    );
    }
    #[test]
    fn ipc_snapshot_rejects_plain_markdown_duplicate_prompt_residue() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let after = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: response — gpt-5\n\n",
                "Done.\n",
                "<!-- agent:boundary:new -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );
        fs::write(&doc, &before).unwrap();

        let err = dedupe_ipc_snapshot_content(&doc, Some(&before), &after, "test_ipc").unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue"),
            "IPC snapshot dedupe must fail closed on duplicate prompt Markdown residue: {err}"
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
        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );
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
        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );
        let expected = "❯ already prefixed\n❯ not prefixed\n";
        assert_eq!(
            result, expected,
            "already-prefixed lines should not get double prefix"
        );
    }
    #[test]
    fn normalize_patch_content_empty_prefix_lines_passthrough() {
        let patch_content = "some line\nanother line\n";
        let result =
            agent_doc_document_realtime::write_policy::normalize_patch_content(patch_content, &[]);
        assert_eq!(
            result, patch_content,
            "empty prefix_lines should leave content unchanged"
        );
    }
    #[test]
    fn normalize_patch_content_non_matching_lines_unchanged() {
        let patch_content = "agent response line\n### heading\n";
        let prefix_lines = vec!["user line".to_string()];
        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );
        assert_eq!(
            result, patch_content,
            "non-matching lines should pass through unchanged"
        );
    }
    #[test]
    fn normalize_patch_content_counts_duplicate_targets() {
        let patch_content = "spec-test-build-install-commit-push\nspec-test-build-install-commit-push\nspec-test-build-install-commit-push\n";
        let prefix_lines = vec![
            "spec-test-build-install-commit-push".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        let result = agent_doc_document_realtime::write_policy::normalize_patch_content(
            patch_content,
            &prefix_lines,
        );

        assert_eq!(
            result,
            "❯ spec-test-build-install-commit-push\n❯ spec-test-build-install-commit-push\nspec-test-build-install-commit-push\n"
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
            agent_doc_document_realtime::write_policy::normalize_patch_content(
                pending_content,
                &prefix_lines,
            )
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
    fn empty_response_recovery_merges_missing_committed_head_response() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n\n",
            "### Re: latest committed — gpt-5\n\n",
            "Latest body.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        git(&["add", "doc.md"]);
        git(&["commit", "-m", "commit response"]);

        let visible = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "restart/recycle your supervisor\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n",
            "<!-- agent:boundary:working -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, visible).unwrap();
        crate::snapshot::save(&doc, visible).unwrap();

        assert!(
            recover_missing_committed_head_response(&doc).unwrap(),
            "missing committed response should be recovered"
        );

        let recovered = fs::read_to_string(&doc).unwrap();
        assert!(
            recovered.contains("restart/recycle your supervisor"),
            "operator/current status edit must be preserved:\n{recovered}"
        );
        assert!(
            recovered.contains("### Re: latest committed — gpt-5")
                && recovered.contains("Latest body."),
            "latest committed response must be merged back into the visible file:\n{recovered}"
        );
        let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(snapshot, recovered);

        let head_after = std::process::Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:doc.md"])
            .output()
            .unwrap();
        assert!(head_after.status.success());
        let head_after = String::from_utf8(head_after.stdout).unwrap();
        assert_eq!(
            head_after, recovered,
            "recovery should commit the merged visible document"
        );
    }

    #[test]
    fn strict_empty_response_recovery_continues_after_stale_preflight_repair() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);

        let doc = root.join("doc.md");
        let committed = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n\n",
            "### Re: latest committed — gpt-5\n\n",
            "Latest body.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, committed).unwrap();
        crate::snapshot::save(&doc, committed).unwrap();
        git(&["add", "doc.md"]);
        git(&["commit", "-m", "commit response"]);

        let visible = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n\n",
            "restart/recycle your supervisor\n",
            "done\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Prior body.\n",
            "<!-- agent:boundary:working -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, visible).unwrap();
        crate::snapshot::save(&doc, visible).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(visible), Some(visible)).unwrap();

        let strict = WriteFlags {
            strict_closeout: true,
            ..Default::default()
        };
        assert!(
            recover_empty_response_for_strict_closeout(&doc, &strict).unwrap(),
            "strict empty response recovery should continue past stale preflight repair"
        );

        let recovered = fs::read_to_string(&doc).unwrap();
        assert!(
            recovered.contains("restart/recycle your supervisor")
                && recovered.contains("### Re: latest committed — gpt-5"),
            "strict recovery must preserve current edits and restore committed response:\n{recovered}"
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        let head_after = crate::git::show_head(&doc).unwrap().unwrap();
        assert_eq!(head_after, recovered);
    }

    // ── agent-response-block tracking ────────────────────────────────────────
    // ── safety rail: normalize_user_prompts_in_exchange_safe ────────────────
    // --- exchange shrink guard tests ---
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
        let result = splice_pending_component(target, source).content;
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
        let result = splice_pending_component(target, source).content;
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
        let result = splice_pending_component(target, source).content;
        assert_eq!(
            result, target,
            "target should be returned unchanged when target has no pending"
        );
    }
}
