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
//!   `template::apply_patches`, then performs the same lock/merge/atomic-write
//!   cycle as `run`.
//!
//! - `run_stream`: CRDT stream-flush mode. Like `run_template` but uses
//!   `merge::merge_contents_crdt` for conflict-free merge. Saves both a text
//!   snapshot and a CRDT state snapshot after every flush. Supports IPC-first
//!   writes: when `.agent-doc/patches/` exists and `--force-disk` is not set,
//!   tries `try_ipc` first; on timeout (exit code 75 / `EX_TEMPFAIL`) writes
//!   locally and removes the fallback patch file after a successful local
//!   commit so an editor watcher cannot replay it later.
//!
//! - `run_ipc`: explicit IPC-only mode. Serialises patches as JSON to
//!   `.agent-doc/patches/<hash>.json`, polls for the plugin to delete the file
//!   as ACK (2 s timeout), then falls back to the direct CRDT disk path.
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
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;
use similar::{ChangeTag, TextDiff};

use crate::snapshot::find_project_root;
use crate::{
    component, component::is_backlog_component, frontmatter, merge, repair, sessions, snapshot,
    template,
};
use crate::{
    flow::document_mutation::{TemplateStructureGuardReason, log_template_structure_guard_event},
    flow::types::FlowOutcome,
};

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
    pub allow_replace_pending: bool,
    pub pending_only: bool,
    pub status: Option<String>,
    /// Optional CLI override for the agent-doc lint gate. `None` means
    /// "no CLI override; use frontmatter/config/default precedence".
    pub lint_override: Option<crate::lint_gate::LintCliMode>,
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
    pub pending_kept_open_ids: Vec<String>,
    pub strict_closeout: bool,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapshotPersistMode {
    FinalContent,
    ContentOurs,
}

fn snapshot_persist_mode(
    baseline: Option<&str>,
    content_ours: &str,
    final_content: &str,
) -> SnapshotPersistMode {
    if baseline.is_none() {
        return SnapshotPersistMode::FinalContent;
    }

    let ours_norm = strip_boundary_for_dedup(content_ours);
    let final_norm = strip_boundary_for_dedup(final_content);
    if ours_norm == final_norm {
        return SnapshotPersistMode::FinalContent;
    }

    if crate::session_check::detect_bypassed_response_write_between(&ours_norm, &final_norm)
        .is_some()
    {
        return SnapshotPersistMode::FinalContent;
    }

    let ours_prompt_norm = crate::diff::strip_comments(&ours_norm);
    let final_prompt_norm = crate::diff::strip_comments(&final_norm);
    let Some(diff_text) =
        crate::diff::unified_diff_from_contents(&ours_prompt_norm, &final_prompt_norm)
    else {
        return SnapshotPersistMode::FinalContent;
    };
    let has_prompt_bearing_user_drift = crate::diff::classify_prompt_bearing_changes(&diff_text)
        .iter()
        .any(|change| {
            matches!(
                change.kind,
                crate::diff::PromptBearingChangeKind::PromptTarget
                    | crate::diff::PromptBearingChangeKind::ContentEdit
            )
        });

    if has_prompt_bearing_user_drift {
        SnapshotPersistMode::ContentOurs
    } else {
        SnapshotPersistMode::FinalContent
    }
}

fn snapshot_persist_mode_with_current(
    baseline: Option<&str>,
    base: &str,
    content_current: &str,
    content_ours: &str,
    final_content: &str,
) -> SnapshotPersistMode {
    if baseline.is_some()
        && strip_boundary_for_dedup(base) != strip_boundary_for_dedup(content_current)
        && (has_prompt_bearing_user_drift(base, content_current)
            || non_exchange_drift_carries_directive(base, content_current))
    {
        return SnapshotPersistMode::ContentOurs;
    }

    snapshot_persist_mode(baseline, content_ours, final_content)
}

/// `#fintol2` — whether the non-`exchange` user drift between `base` and `current`
/// carries a next-cycle directive that must be CARRIED FORWARD rather than
/// forward-merged. Returns true only when an outside-`exchange` change adds a
/// carry-forward signal line (a prompt / question / `dispatch` / `do #` directive
/// / `#tag`). A PLAIN outside-`exchange` content edit (e.g. editing a parked
/// comment-note's prose) returns false, so `snapshot_persist_mode_with_current`
/// falls through to `FinalContent` and the edit is forward-merged into the
/// commit. Unlike `has_prompt_bearing_user_drift` this does NOT strip comments,
/// so a `dispatch #…` directive typed inside a `<!-- … -->` scratch block is still
/// recognized as carry-forward (the scratch-directive integration tests).
fn non_exchange_drift_carries_directive(base: &str, current: &str) -> bool {
    let base_norm = strip_boundary_for_dedup(base);
    let current_norm = strip_boundary_for_dedup(current);
    if base_norm == current_norm {
        return false;
    }
    if !outside_component_content_changed(&base_norm, &current_norm, "exchange") {
        return false;
    }
    added_nonblank_lines(&base_norm, &current_norm)
        .iter()
        .any(|line| line_is_carry_forward_signal(line))
}

fn outside_component_content_changed(left: &str, right: &str, component_name: &str) -> bool {
    let left_component = match component::parse(left) {
        Ok(components) => components.into_iter().find(|c| c.name == component_name),
        Err(_) => return left != right,
    };
    let right_component = match component::parse(right) {
        Ok(components) => components.into_iter().find(|c| c.name == component_name),
        Err(_) => return left != right,
    };

    let Some(left_component) = left_component else {
        return left != right;
    };
    let Some(right_component) = right_component else {
        return true;
    };

    left[..left_component.open_end] != right[..right_component.open_end]
        || left[left_component.close_start..] != right[right_component.close_start..]
}

fn has_prompt_bearing_user_drift(base: &str, current: &str) -> bool {
    !prompt_bearing_user_changes_between(base, current).is_empty()
}

fn prompt_bearing_user_changes_between(
    base: &str,
    current: &str,
) -> Vec<crate::diff::PromptBearingChange> {
    let base_norm = strip_boundary_for_dedup(base);
    let current_norm = strip_boundary_for_dedup(current);
    let base_prompt_norm = crate::diff::strip_comments(&base_norm);
    let current_prompt_norm = crate::diff::strip_comments(&current_norm);
    let Some(diff_text) =
        crate::diff::unified_diff_from_contents(&base_prompt_norm, &current_prompt_norm)
    else {
        return Vec::new();
    };
    let mut changes: Vec<_> = crate::diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                crate::diff::PromptBearingChangeKind::PromptTarget
                    | crate::diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if diff_text.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if line.starts_with("+++") {
            return false;
        }
        let trimmed = added.trim();
        trimmed.starts_with('❯') || crate::diff::text_line_looks_like_prompt_target(trimmed)
    }) {
        for line in diff_text.lines() {
            let Some(added) = line.strip_prefix('+') else {
                continue;
            };
            if line.starts_with("+++") {
                continue;
            }
            let trimmed = added.trim();
            if trimmed.starts_with('❯') || crate::diff::text_line_looks_like_prompt_target(trimmed)
            {
                let text = trimmed
                    .strip_prefix('❯')
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string();
                if !changes.iter().any(|change| {
                    change.kind == crate::diff::PromptBearingChangeKind::PromptTarget
                        && change.text.trim() == text
                }) {
                    changes.push(crate::diff::PromptBearingChange {
                        kind: crate::diff::PromptBearingChangeKind::PromptTarget,
                        text,
                    });
                }
            }
        }
    }
    changes
}

fn prompt_bearing_change_owned_by_content_ours(
    change: &crate::diff::PromptBearingChange,
    owned_changes: &[crate::diff::PromptBearingChange],
) -> bool {
    let text = normalized_prompt_line(&change.text);
    owned_changes
        .iter()
        .any(|owned| owned.kind == change.kind && normalized_prompt_line(&owned.text) == text)
}

pub fn ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
    baseline: &str,
    snapshot_candidate: &str,
    content_ours: &str,
) -> bool {
    let baseline_norm = strip_boundary_for_dedup(baseline);
    let candidate_norm = strip_boundary_for_dedup(snapshot_candidate);
    let ours_norm = strip_boundary_for_dedup(content_ours);
    if outside_component_content_changed(&baseline_norm, &candidate_norm, "exchange")
        && outside_component_content_changed(&ours_norm, &candidate_norm, "exchange")
    {
        return true;
    }

    let candidate_changes = prompt_bearing_user_changes_between(baseline, snapshot_candidate);
    if candidate_changes.is_empty() {
        return false;
    }
    let owned_changes = prompt_bearing_user_changes_between(baseline, content_ours);
    candidate_changes
        .iter()
        .any(|change| !prompt_bearing_change_owned_by_content_ours(change, &owned_changes))
}

/// Non-blank, trimmed lines present in `candidate` but not in `baseline` — the
/// content a side added relative to the common ancestor. Set-based (order- and
/// count-insensitive) so it stays a conservative coverage check: a line that
/// appears in both is never counted as "added", which can only make
/// [`response_target_disjoint_from_user_edit`] return `false` (fail closed),
/// never a false `true`.
fn added_nonblank_lines(baseline: &str, candidate: &str) -> Vec<String> {
    let base: std::collections::HashSet<&str> = baseline
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    candidate
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !base.contains(l))
        .map(|l| l.to_string())
        .collect()
}

/// True when a user-added line is a next-cycle instruction that `#fintol2` must
/// carry forward rather than fold into the commit: a prompt target (`❯ …`,
/// `do #…`), a question, a `dispatch`/`fix #` directive, a `spec-test` build
/// directive, or an inline / leading `#tag`. Mirrors the post-commit directive
/// matcher in `git.rs` and the gate's own prompt detection, kept deliberately
/// broad — carry-forward is the safe default, so a false positive only defers an
/// edit to the next cycle, never commits a directive prematurely.
pub(crate) fn line_is_carry_forward_signal(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if crate::diff::text_line_looks_like_prompt_target(trimmed) {
        return true;
    }
    let lower = trimmed
        .trim_start_matches('❯')
        .trim_start()
        .to_ascii_lowercase();
    trimmed.starts_with('❯')
        || trimmed.ends_with('?')
        || trimmed.starts_with('#')
        || trimmed.contains(" #")
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("dispatch")
        || lower.starts_with("fix #")
        || lower.contains("spec-test")
        || lower.contains("spec test")
}

/// `#fintol1` — conflict-scope primitive driving the `#fintol2` finalize
/// tolerance. True when the concurrent user edit carried by `candidate` (the live
/// buffer / ack content at finalize) is a DISJOINT, plain content edit that can
/// be forward-merged into THIS cycle's commit (the operator-confirmed behavior:
/// "allow the user to type unrelated changes / compaction without being
/// rejected — agent response lands, user edit preserved") instead of being
/// carried forward to the next cycle.
///
/// Returns `true` only for the narrow, provably-safe case and stays conservative
/// everywhere else (a false `true` would commit a bad merge), requiring ALL of:
/// 1. the user edit is confined OUTSIDE the `exchange` component (the response's
///    own target), so a new prompt, a response-body rewrite, or a re-typed answer
///    can never be spliced — those stay on the carry-forward / fail-closed path;
/// 2. none of the user's added lines (the candidate-added lines the response did
///    not write) is a carry-forward signal — a prompt, question, `dispatch` /
///    `do #` directive, or `#tag`. Those are next-cycle instructions and are
///    preserved as a next-cycle diff, never folded into the commit (this keeps
///    the `EFS_LIVE_PROMPT` / scratch-directive / `#next-steps` integration tests
///    carrying their content forward);
/// 3. a 3-way merge (base = `baseline`, ours = `content_ours`, theirs =
///    `candidate`) produces no git conflict markers and preserves every non-blank
///    line the response added AND every non-blank line the user added — `git
///    merge-file --diff3` conflicts only when both sides changed the same region.
///
/// A merge error, a dropped line, a pure deletion, or no user edit also returns
/// `false`. A plain content edit outside `exchange` (e.g. editing a parked
/// comment-note's prose) is the case that forward-merges.
pub fn response_target_disjoint_from_user_edit(
    baseline: &str,
    content_ours: &str,
    candidate: &str,
) -> bool {
    // No concurrent user edit relative to the response → nothing to forward-merge.
    if strip_boundary_for_dedup(candidate) == strip_boundary_for_dedup(content_ours) {
        return false;
    }
    let user_added = added_nonblank_lines(baseline, candidate);
    // A pure deletion (no added lines) is not forward-merged — fail closed.
    if user_added.is_empty() {
        return false;
    }
    // The agent response always targets the `exchange` component. Confine the
    // forward-merge to user edits OUTSIDE `exchange` so a new prompt, a
    // response-body rewrite, or a re-typed answer (all inside `exchange`) is
    // never spliced — those stay on the established carry-forward / fail-closed
    // path. The candidate's exchange normally also carries the agent's own
    // response (the IPC write landed in the live buffer); subtract those
    // response lines so only genuine user exchange edits disqualify the merge.
    // This also defeats the `git merge-file --diff3` append-resolution that would
    // otherwise fold a user rewrite of the response body into a duplicated union.
    let baseline_ex = exchange_component_text(baseline);
    let candidate_ex = exchange_component_text(candidate);
    let ours_ex = exchange_component_text(content_ours);
    let response_ex_added: std::collections::HashSet<String> =
        added_nonblank_lines(&baseline_ex, &ours_ex).into_iter().collect();
    let user_ex_added = added_nonblank_lines(&baseline_ex, &candidate_ex)
        .into_iter()
        .any(|line| !response_ex_added.contains(&line));
    if user_ex_added {
        return false;
    }
    // The lines the USER added (candidate-added minus the response's own added
    // lines). A prompt / question / `dispatch` / `do #` directive / `#tag` among
    // them is a next-cycle instruction that must be carried forward, not folded
    // into the commit, so it disqualifies the forward-merge.
    let response_added_set: std::collections::HashSet<String> =
        added_nonblank_lines(baseline, content_ours).into_iter().collect();
    let user_carries_directive = user_added
        .iter()
        .filter(|line| !response_added_set.contains(*line))
        .any(|line| line_is_carry_forward_signal(line));
    if user_carries_directive {
        return false;
    }
    // Region independence outside `exchange` is then PROVEN by a clean 3-way
    // merge that preserves both the response and the user edit. A conflict marker
    // (both sides changed the same region) or a dropped line returns false.
    let Ok(merged) = crate::merge::merge_contents(baseline, content_ours, candidate) else {
        return false;
    };
    if merged.contains("<<<<<<<") || merged.contains(">>>>>>>") {
        return false;
    }
    let merged_lines: std::collections::HashSet<&str> = merged
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let response_added = added_nonblank_lines(baseline, content_ours);
    response_added
        .iter()
        .all(|l| merged_lines.contains(l.as_str()))
        && user_added.iter().all(|l| merged_lines.contains(l.as_str()))
}

/// The raw text of the `agent:exchange` component, or empty if absent. Used to
/// scope dropped-prompt detection to user-authored exchange content only —
/// queue / backlog / scratch-comment drift is legitimately preserved in the
/// working tree for the next cycle and is not a data-loss case.
fn exchange_component_text(doc: &str) -> String {
    let Ok(components) = crate::component::parse(doc) else {
        return String::new();
    };
    components
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(doc).to_string())
        .unwrap_or_default()
}

/// `#exchange-prompt-dropped-on-merge`: the user-authored prompt-bearing lines
/// present in the IPC `candidate` (disk / ack sidecar) `agent:exchange` that
/// `content_ours` does not own — i.e. the exchange prompt lines that would be
/// lost when `content_ours` is adopted. Scoped to the `agent:exchange`
/// component so queue / backlog / scratch drift (preserved for the next cycle)
/// is not misread as a dropped prompt. Recorded so `session-check` can fail
/// closed on the data-loss class.
fn dropped_prompt_lines_after_content_ours(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> Vec<String> {
    let baseline_ex = exchange_component_text(baseline);
    let candidate_ex = exchange_component_text(candidate);
    let content_ours_ex = exchange_component_text(content_ours);

    let candidate_changes = prompt_bearing_user_changes_between(&baseline_ex, &candidate_ex);
    if candidate_changes.is_empty() {
        return Vec::new();
    }
    let owned_changes = prompt_bearing_user_changes_between(&baseline_ex, &content_ours_ex);
    candidate_changes
        .into_iter()
        // Only a new prompt target (a `do #id` / `❯ ...` / prompt-shaped line) is
        // an unambiguously dropped user prompt. Multi-line content edits are
        // noisy diff context, not a discrete prompt to recover.
        .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
        .filter(|change| !prompt_bearing_change_owned_by_content_ours(change, &owned_changes))
        .map(|change| change.text.trim().to_string())
        .filter(|text| !text.is_empty() && !text.contains('\n'))
        .collect()
}

fn queue_component_text(doc: &str) -> String {
    let Ok(components) = crate::component::parse(doc) else {
        return String::new();
    };
    components
        .iter()
        .find(|c| c.name == "queue")
        .map(|c| c.content(doc).to_string())
        .unwrap_or_default()
}

fn queue_prompt_texts(body: &str) -> Vec<String> {
    let Ok(entries) = crate::queue::parse(body) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) if !prompt.multiline => {
                let text = prompt.text.trim().to_string();
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .collect()
}

/// Active + consumed (struck) queue prompt texts. A queue item struck this cycle
/// (`QueueEntry::Completed`) was CONSUMED, not dropped, so it must count toward
/// `content_ours` coverage when deciding whether adopting `content_ours` would
/// drop a user-added candidate prompt (`#dropqueue-consumed-falsecount`).
/// `queue_prompt_texts` returns only active `Prompt` entries, which made a
/// consumed item read as a dropped user edit and tripped the
/// `#queue-user-edit-overwrite` guard on a correct closeout.
fn queue_prompt_texts_including_consumed(body: &str) -> Vec<String> {
    let Ok(entries) = crate::queue::parse(body) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt)
            | crate::queue::QueueEntry::Completed(prompt)
                if !prompt.multiline =>
            {
                let text = prompt.text.trim().to_string();
                (!text.is_empty()).then_some(text)
            }
            _ => None,
        })
        .collect()
}

fn queue_prompt_counts(prompts: &[String]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for prompt in prompts {
        *counts.entry(prompt.clone()).or_insert(0) += 1;
    }
    counts
}

fn queue_prompt_count(counts: &HashMap<String, usize>, prompt: &str) -> usize {
    counts.get(prompt).copied().unwrap_or(0)
}

/// `#queue-user-edit-overwrite`: the user-authored queue prompt line(s) present
/// in the IPC `candidate` (disk / ack sidecar) `agent:queue` that `content_ours`
/// does not own; these would be silently deleted when `content_ours` is adopted.
/// Scoped to `agent:queue` so exchange / backlog drift is not misread. Recorded
/// so `session-check` can fail closed on the silent-queue-deletion class instead
/// of letting convergence drop a user-added queue item the current response never
/// consumed.
fn dropped_queue_prompt_lines_after_content_ours(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> Vec<String> {
    let baseline_q = queue_component_text(baseline);
    let candidate_q = queue_component_text(candidate);
    let content_ours_q = queue_component_text(content_ours);

    let baseline_prompts = queue_prompt_texts(&baseline_q);
    let candidate_prompts = queue_prompt_texts(&candidate_q);
    // #dropqueue-consumed-falsecount: count items content_ours CONSUMED (struck)
    // this cycle as covered, not dropped — a struck queue item is answered, not
    // silently deleted.
    let content_ours_prompts = queue_prompt_texts_including_consumed(&content_ours_q);
    if candidate_prompts.is_empty() {
        return Vec::new();
    }

    let baseline_counts = queue_prompt_counts(&baseline_prompts);
    let content_ours_counts = queue_prompt_counts(&content_ours_prompts);
    let mut candidate_seen = HashMap::new();
    let mut dropped = Vec::new();

    for prompt in candidate_prompts {
        let seen = candidate_seen.entry(prompt.clone()).or_insert(0);
        *seen += 1;

        let baseline_count = queue_prompt_count(&baseline_counts, &prompt);
        if *seen <= baseline_count {
            continue;
        }

        let candidate_added_index = *seen - baseline_count;
        let content_ours_added_count =
            queue_prompt_count(&content_ours_counts, &prompt).saturating_sub(baseline_count);
        if candidate_added_index > content_ours_added_count {
            dropped.push(prompt);
        }
    }

    dropped
}

/// Preserve live queue deletions while still adopting the agent-owned
/// `content_ours` response snapshot.
///
/// Live queue additions are intentionally *not* folded into `content_ours`; they
/// stay as visible next-cycle work and are covered by `dropped_queue_prompts` if
/// an editor overwrite loses them. Deletions are different: if a prompt existed
/// at baseline and the live IPC candidate removed it, raw `content_ours` would
/// resurrect that deleted queue item in the response commit. Remove those
/// baseline-owned deleted prompts from the `content_ours` queue only.
fn apply_live_queue_deletions_to_content_ours(
    baseline: &str,
    live_candidate: &str,
    content_ours: &str,
) -> String {
    let baseline_prompts = queue_prompt_texts(&queue_component_text(baseline));
    if baseline_prompts.is_empty() {
        return content_ours.to_string();
    }
    let candidate_prompts = queue_prompt_texts(&queue_component_text(live_candidate));
    let baseline_counts = queue_prompt_counts(&baseline_prompts);
    let candidate_counts = queue_prompt_counts(&candidate_prompts);
    let deleted_counts: HashMap<String, usize> = baseline_counts
        .iter()
        .filter_map(|(prompt, baseline_count)| {
            let candidate_count = queue_prompt_count(&candidate_counts, prompt);
            let deleted = baseline_count.saturating_sub(candidate_count);
            (deleted > 0).then(|| (prompt.clone(), deleted))
        })
        .collect();
    if deleted_counts.is_empty() {
        return content_ours.to_string();
    }

    let target_comps = match component::parse(content_ours) {
        Ok(comps) => comps,
        Err(_) => return content_ours.to_string(),
    };
    let Some(target_queue) = target_comps.iter().find(|c| c.name == "queue") else {
        return content_ours.to_string();
    };
    let target_body = &content_ours[target_queue.open_end..target_queue.close_start];
    let Ok(target_entries) = crate::queue::parse(target_body) else {
        return content_ours.to_string();
    };

    let mut removed_counts: HashMap<String, usize> = HashMap::new();
    let mut changed = false;
    let mut kept = Vec::with_capacity(target_entries.len());
    for entry in target_entries {
        let remove = match &entry {
            crate::queue::QueueEntry::Prompt(prompt) if !prompt.multiline => {
                let text = prompt.text.trim().to_string();
                let deleted_count = queue_prompt_count(&deleted_counts, &text);
                if deleted_count == 0 {
                    false
                } else {
                    let removed = removed_counts.entry(text).or_insert(0);
                    if *removed < deleted_count {
                        *removed += 1;
                        true
                    } else {
                        false
                    }
                }
            }
            _ => false,
        };
        if remove {
            changed = true;
        } else {
            kept.push(entry);
        }
    }

    if !changed {
        return content_ours.to_string();
    }

    let new_body = crate::queue::render(&kept);
    let out = target_queue.replace_content(content_ours, &new_body);
    if crate::queue::prompts(&kept).is_empty() {
        frontmatter::merge_queue_state(&out, false).unwrap_or(out)
    } else {
        out
    }
}

fn snapshot_content_to_persist<'a>(
    mode: SnapshotPersistMode,
    content_ours: &'a str,
    final_content: &'a str,
) -> &'a str {
    match mode {
        SnapshotPersistMode::FinalContent => final_content,
        SnapshotPersistMode::ContentOurs => content_ours,
    }
}

fn normalized_prompt_line(line: &str) -> String {
    line.trim()
        .strip_prefix('❯')
        .unwrap_or_else(|| line.trim())
        .trim()
        .to_string()
}

fn prompt_target_lines(target: &str) -> Vec<String> {
    target
        .lines()
        .map(normalized_prompt_line)
        .filter(|line| !line.is_empty())
        .collect()
}

fn prompt_target_matches_at(
    segments: &[&str],
    removed: &[bool],
    start: usize,
    target: &[String],
) -> bool {
    if start + target.len() > segments.len() {
        return false;
    }
    target.iter().enumerate().all(|(offset, expected)| {
        let idx = start + offset;
        !removed[idx] && normalized_prompt_line(segments[idx].trim_end_matches('\n')) == *expected
    })
}

fn remove_prompt_target_blocks_from_body(body: &str, targets: &[String]) -> (String, usize) {
    let segments: Vec<&str> = body.split_inclusive('\n').collect();
    if segments.is_empty() || targets.is_empty() {
        return (body.to_string(), 0);
    }

    let mut removed = vec![false; segments.len()];
    let mut removed_count = 0usize;
    let target_lines: Vec<Vec<String>> = targets
        .iter()
        .map(|target| prompt_target_lines(target))
        .filter(|lines| !lines.is_empty())
        .collect();

    for target in &target_lines {
        if let Some(start) = (0..segments.len())
            .rev()
            .find(|&idx| prompt_target_matches_at(&segments, &removed, idx, target))
        {
            for slot in removed.iter_mut().skip(start).take(target.len()) {
                *slot = true;
            }
            removed_count += 1;
        }
    }

    if removed_count == 0 {
        return (body.to_string(), 0);
    }

    let mut cleaned = String::with_capacity(body.len());
    for (idx, segment) in segments.iter().enumerate() {
        if !removed[idx] {
            cleaned.push_str(segment);
        }
    }
    (cleaned, removed_count)
}

fn prompt_targets_added_to_backlog(
    base: &str,
    current: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    let base_components = component::parse(base).context("failed to parse baseline components")?;
    let current_components =
        component::parse(current).context("failed to parse current components")?;
    let mut targets = Vec::new();

    for current_component in current_components
        .iter()
        .filter(|component| is_backlog_component(&component.name))
    {
        let base_body = base_components
            .iter()
            .find(|component| component.name == current_component.name)
            .map(|component| component.content(base))
            .unwrap_or("");
        let current_body = current_component.content(current);
        let Some(diff_text) = crate::diff::unified_diff_from_contents(base_body, current_body)
        else {
            continue;
        };
        let component_targets: Vec<String> =
            crate::diff::classify_prompt_bearing_changes(&diff_text)
                .into_iter()
                .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
                .map(|change| change.text)
                .collect();
        if !component_targets.is_empty() {
            targets.push((current_component.name.clone(), component_targets));
        }
    }

    Ok(targets)
}

fn cleanup_resolved_backlog_prompts_after_response(
    file: &Path,
    base: &str,
    current: &str,
    final_content: &str,
) -> Result<Option<String>> {
    let targets = prompt_targets_added_to_backlog(base, current)?;
    if targets.is_empty() {
        return Ok(None);
    }

    let mut result = final_content.to_string();
    let mut removed_total = 0usize;
    for (component_name, component_targets) in targets {
        let components = component::parse(&result)
            .with_context(|| format!("failed to parse final components in {}", file.display()))?;
        let Some(component) = components
            .iter()
            .find(|component| component.name == component_name)
        else {
            continue;
        };
        let body = component.content(&result);
        let (cleaned_body, removed_count) =
            remove_prompt_target_blocks_from_body(body, &component_targets);
        if removed_count == 0 {
            continue;
        }
        result = component.replace_content(&result, &cleaned_body);
        removed_total += removed_count;
    }

    if removed_total == 0 {
        return Ok(None);
    }

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
    Ok(Some(result))
}

fn shell_quote_cli_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\"'\"'"))
}

fn build_rerun_command_base(options: &CommandOptions, commit_mode: CommitMode) -> Option<String> {
    if commit_mode != CommitMode::Required {
        return None;
    }

    let mut args = vec!["agent-doc".to_string(), "finalize".to_string()];
    args.push(options.file.display().to_string());
    if let Some(path) = &options.baseline_file {
        args.push("--baseline-file".to_string());
        args.push(path.display().to_string());
    }
    if options.is_template {
        args.push("--template".to_string());
    }
    if options.is_stream {
        args.push("--stream".to_string());
    }
    if options.is_ipc {
        args.push("--ipc".to_string());
    }
    if options.force_disk {
        args.push("--force-disk".to_string());
    }
    if let Some(origin) = &options.origin {
        args.push("--origin".to_string());
        args.push(origin.clone());
    }
    for value in &options.pending_add {
        args.push("--pending-add".to_string());
        args.push(value.clone());
    }
    for pair in options.pending_add_to.chunks(2) {
        if let [target, value] = pair {
            args.push("--pending-add-to".to_string());
            args.push(target.clone());
            args.push(value.clone());
        }
    }
    for value in &options.pending_add_gated {
        args.push("--pending-add-gated".to_string());
        args.push(value.clone());
    }
    for pair in options.pending_add_after.chunks(2) {
        if let [anchor, value] = pair {
            args.push("--pending-add-after".to_string());
            args.push(anchor.clone());
            args.push(value.clone());
        }
    }
    for pair in options.pending_add_before.chunks(2) {
        if let [anchor, value] = pair {
            args.push("--pending-add-before".to_string());
            args.push(anchor.clone());
            args.push(value.clone());
        }
    }
    for value in &options.pending_add_back {
        args.push("--pending-add-back".to_string());
        args.push(value.clone());
    }
    for value in &options.pending_done {
        args.push("--done".to_string());
        args.push(value.clone());
    }
    for value in &options.pending_edit {
        args.push("--pending-edit".to_string());
        args.push(value.clone());
    }
    if options.pending_clear {
        args.push("--pending-clear".to_string());
    }
    if let Some(value) = &options.pending_reorder {
        args.push("--pending-reorder".to_string());
        args.push(value.clone());
    }
    for value in &options.pending_gate {
        args.push("--pending-gate".to_string());
        args.push(value.clone());
    }
    for value in &options.pending_ungate {
        args.push("--pending-ungate".to_string());
        args.push(value.clone());
    }
    for value in &options.pending_resolve_gate {
        args.push("--pending-resolve-gate".to_string());
        args.push(value.clone());
    }
    for value in &options.pending_set_gate_type {
        args.push("--pending-set-gate-type".to_string());
        args.push(value.clone());
    }
    for value in &options.pending_set_verify {
        args.push("--pending-set-verify".to_string());
        args.push(value.clone());
    }
    for value in &options.review_add {
        args.push("--review-add".to_string());
        args.push(value.clone());
    }
    for value in &options.review_edit {
        args.push("--review-edit".to_string());
        args.push(value.clone());
    }
    if options.allow_replace_pending {
        args.push("--allow-replace-pending".to_string());
    }
    if options.pending_only {
        args.push("--pending-only".to_string());
    }
    if let Some(status) = &options.status {
        args.push("--status".to_string());
        args.push(status.clone());
    }
    Some(
        args.into_iter()
            .map(|arg| shell_quote_cli_arg(&arg))
            .collect::<Vec<_>>()
            .join(" "),
    )
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

    let migrated_path = crate::snapshot::baseline_path_for(file).with_context(|| {
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
/// no fresh `preflight` reopened the cycle (for example an exit-75 IPC-timeout
/// retry, or a second response in the same turn). Historically this always failed
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

fn grouped_pending_add_to(raw: &[String]) -> Result<Vec<(PathBuf, Vec<String>)>> {
    if !raw.len().is_multiple_of(2) {
        anyhow::bail!("--pending-add-to expects repeated FILE TEXT pairs");
    }

    let mut grouped: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for pair in raw.chunks(2) {
        let target = PathBuf::from(&pair[0]);
        let text = pair[1].clone();
        if let Some((_, items)) = grouped.iter_mut().find(|(existing, _)| existing == &target) {
            items.push(text);
        } else {
            grouped.push((target, vec![text]));
        }
    }
    Ok(grouped)
}

fn ensure_pending_add_target(target: &Path) -> Result<()> {
    if !target.exists() {
        anyhow::bail!(
            "--pending-add-to target file not found: {}",
            target.display()
        );
    }
    let content = std::fs::read_to_string(target).with_context(|| {
        format!(
            "failed to read --pending-add-to target {}",
            target.display()
        )
    })?;
    let components = crate::component::parse(&content).with_context(|| {
        format!(
            "failed to parse --pending-add-to target {}",
            target.display()
        )
    })?;
    if !components
        .iter()
        .any(|component| crate::component::is_backlog_component(&component.name))
    {
        anyhow::bail!(
            "--pending-add-to target {} has no agent:backlog/agent:pending component",
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

fn compact_command_hint(file: &Path) -> String {
    format!("agent-doc compact {} --commit", file.display())
}

fn pending_kept_open_ids_from_options(options: &CommandOptions) -> Vec<String> {
    let mut ids = Vec::new();

    for pair in &options.pending_edit {
        if let Some((id, _)) = pair.split_once('=') {
            ids.push(id.to_string());
        }
    }
    ids.extend(options.pending_gate.iter().cloned());
    ids.extend(options.pending_ungate.iter().cloned());
    for pair in &options.pending_set_gate_type {
        if let Some((id, _)) = pair.split_once('=') {
            ids.push(id.to_string());
        }
    }
    for pair in &options.pending_set_verify {
        if let Some((id, _)) = pair.split_once('=') {
            ids.push(id.to_string());
        }
    }
    for pair in &options.review_edit {
        if let Some((id, _)) = pair.split_once('=') {
            ids.push(id.to_string());
        }
    }
    if let Some(order) = &options.pending_reorder {
        ids.extend(
            order
                .split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty()),
        );
    }

    ids
}

fn enforce_review_done_guard(file: &Path, id: &str) -> Result<()> {
    let mode = crate::session_check::resolve_review_done_guard_mode(file)?;
    if mode == crate::frontmatter::PendingCaptureGuardMode::Off {
        return Ok(());
    }
    let Some(component_name) = crate::pending_cmd::open_item_component_name(file, id)? else {
        return Ok(());
    };
    if crate::component::is_review_component(&component_name) {
        return Ok(());
    }

    let normalized = crate::pending::normalize_pending_id(id);
    let message = format!(
        "review_done_guard: --done #{} resolved from agent:{} instead of agent:review; run --pending-gate {} first or set review_done_guard = \"off\"",
        normalized, component_name, normalized
    );
    match mode {
        crate::frontmatter::PendingCaptureGuardMode::Warn => {
            eprintln!("[write] warning: {}", message);
            Ok(())
        }
        crate::frontmatter::PendingCaptureGuardMode::Strict => {
            log_closeout_guard(
                file,
                crate::flow::types::FlowStage::PreWriteGuard,
                crate::flow::types::FlowOutcome::Blocked,
                crate::flow::closeout::CloseoutGuardReason::ReviewDoneSourceNotReviewed,
            );
            anyhow::bail!("{}", message)
        }
        crate::frontmatter::PendingCaptureGuardMode::Off => Ok(()),
    }
}

pub fn guard_no_exchange_compaction_request_for_diff(file: &Path, diff_text: &str) -> Result<()> {
    if crate::diff::detect_exchange_compaction_request(diff_text) {
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
    let Some(diff_text) = crate::diff::unified_diff_from_contents(base, current_content) else {
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
        crate::cycle_state::CyclePhase::ResponseCaptured
            | crate::cycle_state::CyclePhase::WriteApplied
    ) {
        return Ok(false);
    }
    let current = std::fs::read_to_string(file)
        .context("failed to read document for bare-write response-body detection")?;
    let Some(head) = crate::git::show_head(file)? else {
        return Ok(false);
    };
    Ok(crate::session_check::detect_bypassed_response_write_between(&head, &current).is_some())
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
        anyhow::bail!("--pending-only requires at least one --pending-* flag");
    }
    if options.pending_only && (options.is_template || options.is_stream || options.is_ipc) {
        anyhow::bail!("--pending-only cannot be combined with --template, --stream, or --ipc");
    }
    if options.pending_only && commit_mode == CommitMode::Required {
        anyhow::bail!("finalize does not support --pending-only");
    }
    if !options.pending_add_to.len().is_multiple_of(2) {
        anyhow::bail!("--pending-add-to expects repeated FILE TEXT pairs");
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

    if has_pending_ops {
        let pending_kept_open_ids = pending_kept_open_ids_from_options(&options);
        if options.pending_clear {
            crate::pending_cmd::clear(file)?;
        }
        // `#opsproof-samecycle-add`: track ids added this cycle so post-commit
        // ops-proof auto-completion never reaps a brand-new same-cycle add.
        let mut same_cycle_added_ids: Vec<String> =
            crate::pending_cmd::add_many(file, &options.pending_add, false)?;
        let pending_add_targets = grouped_pending_add_to(&options.pending_add_to)?;
        for (target, items) in &pending_add_targets {
            ensure_pending_add_target(target)?;
            crate::pending_cmd::add_many(target, items, false).with_context(|| {
                format!(
                    "failed to apply --pending-add-to target {}",
                    target.display()
                )
            })?;
        }
        same_cycle_added_ids.extend(crate::pending_cmd::add_many(
            file,
            &options.pending_add_gated,
            true,
        )?);
        // #ah0s: explicit-position adds (after/before <id>, tail). Applied after
        // the front-insert default so anchor ids added this same cycle resolve.
        for pair in options.pending_add_after.chunks(2) {
            if let [anchor, text] = pair {
                crate::pending_cmd::add_after(file, anchor, text)
                    .with_context(|| format!("failed to apply --pending-add-after {anchor}"))?;
            } else {
                anyhow::bail!("--pending-add-after expects repeated ID TEXT pairs");
            }
        }
        for pair in options.pending_add_before.chunks(2) {
            if let [anchor, text] = pair {
                crate::pending_cmd::add_before(file, anchor, text)
                    .with_context(|| format!("failed to apply --pending-add-before {anchor}"))?;
            } else {
                anyhow::bail!("--pending-add-before expects repeated ID TEXT pairs");
            }
        }
        for text in &options.pending_add_back {
            crate::pending_cmd::add_back(file, text)?;
        }
        if !options.pending_add.is_empty()
            || !options.pending_add_to.is_empty()
            || !options.pending_add_gated.is_empty()
            || !options.pending_add_after.is_empty()
            || !options.pending_add_before.is_empty()
            || !options.pending_add_back.is_empty()
        {
            crate::cycle_state::mark_pending_mutations(file)?;
            crate::cycle_state::mark_pending_added(file)?;
        }
        if !same_cycle_added_ids.is_empty() {
            crate::cycle_state::record_pending_added_ids(file, &same_cycle_added_ids)?;
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
        if !options.pending_gate.is_empty() {
            crate::cycle_state::record_pending_gated_ids(file, &options.pending_gate)?;
        }
        for pair in &options.pending_set_gate_type {
            let (id, gt) = pair.split_once('=').with_context(|| {
                format!("--pending-set-gate-type expects 'id=type', got: {}", pair)
            })?;
            crate::pending_cmd::set_gate_type(file, id, gt)?;
        }
        for pair in &options.pending_set_verify {
            let (id, spec) = pair.split_once('=').with_context(|| {
                format!(
                    "--pending-set-verify expects 'id=<verify/disproof predicate spec>', got: {}",
                    pair
                )
            })?;
            crate::pending_cmd::set_gate_verify(file, id, spec)?;
        }
        let mut review_added_ids: Vec<String> = Vec::new();
        for value in &options.review_add {
            review_added_ids.push(crate::pending_cmd::review_add(file, value)?);
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
            crate::pending_cmd::review_edit(file, id, text)?;
        }
        for id in &options.review_resolve {
            crate::pending_cmd::review_resolve(file, id)?;
        }
        for id in &options.review_remove {
            crate::pending_cmd::review_remove(file, id)?;
        }
        for id in &options.pending_ungate {
            crate::pending_cmd::ungate(file, id)?;
        }
        for gt in &options.pending_resolve_gate {
            crate::pending_cmd::resolve_gate(file, gt)?;
        }
        for id in &options.pending_done {
            enforce_review_done_guard(file, id)?;
            crate::pending_cmd::done(file, id)?;
        }
        if !options.pending_done.is_empty() {
            crate::cycle_state::record_pending_done_ids(file, &options.pending_done)?;
            crate::cycle_state::mark_pending_mutations(file)?;
        }
        if let Some(ref order) = options.pending_reorder {
            let ids: Vec<String> = order
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            crate::pending_cmd::reorder(file, &ids)?;
        }
        if !pending_kept_open_ids.is_empty() {
            crate::cycle_state::record_pending_kept_open_ids(file, &pending_kept_open_ids)?;
        }
        crate::cycle_state::mark_pending_mutations(file)?;
    }

    if let Some(ref status_text) = options.status {
        crate::status_cmd::set(file, status_text)?;
    }

    if options.pending_only {
        run_closeout_pending_maintenance(file, commit_mode)?;
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
            || !options.review_add.is_empty(),
        has_pending_done: !options.pending_done.is_empty(),
        has_pending_mutation: has_pending_ops,
        pending_done_ids: options.pending_done.clone(),
        pending_kept_open_ids: pending_kept_open_ids_from_options(&options),
        strict_closeout: commit_mode == CommitMode::Required,
        rerun_command_base: build_rerun_command_base(&options, commit_mode),
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
        run_closeout_pending_maintenance(file, commit_mode)?;
    }

    // Phase 3b: pre-commit pending closeout gates (strict mode only).
    if write_result.is_ok() && commit_mode == CommitMode::Required {
        precommit_pending_capture_check(file)?;
        precommit_pending_done_check(file)?;
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
    // triggers, explicit `--done`/`--pending-gate`/`--pending-edit` completion of
    // an id-backed head, a synthetic/preset heading-id match, and a free-text head
    // answered by this cycle's response — all resolve through
    // `queue_consumption_allowed_for_response` so every closeout, including the
    // stream IPC-timeout `exit(75)` path, uses an identical decision
    // (#queue-consume-on-stream-ipc-timeout).
    if write_result.is_ok() {
        let response_body = crate::capture::load_active(file)?
            .map(|capture| capture.response_body)
            .unwrap_or_default();
        let queue_consumption_allowed = queue_consumption_allowed_for_response(
            file,
            baseline.as_deref(),
            &current_content,
            &response_body,
            &options.pending_done,
            &options.pending_gate,
            &options.pending_edit,
        )?;
        match commit_mode {
            CommitMode::None => {}
            CommitMode::BestEffort => {
                if queue_consumption_allowed {
                    if let Err(e) =
                        consume_queue_prompts_for_done_ids_with_outcome(file, &options.pending_done)
                    {
                        eprintln!("[queue] warning: consumption failed: {}", e);
                    }
                    if let Err(e) = mark_completed_queue_prompts_for_done_ids(
                        file,
                        &options.pending_done,
                        false,
                    ) {
                        eprintln!("[queue] warning: done-id marking failed: {}", e);
                    }
                } else {
                    match mark_completed_queue_prompts_for_done_ids(
                        file,
                        &options.pending_done,
                        false,
                    ) {
                        Ok(0) => eprintln!("{}", queue_skip_diagnostic_for_file(file)?),
                        Ok(_) => {}
                        Err(e) => eprintln!("[queue] warning: done-id marking failed: {}", e),
                    }
                }
            }
            CommitMode::Required => {
                if queue_consumption_allowed {
                    consume_queue_prompts_for_done_ids_with_outcome(file, &options.pending_done)?;
                    mark_completed_queue_prompts_for_done_ids(file, &options.pending_done, false)?;
                } else {
                    let marked = mark_completed_queue_prompts_for_done_ids(
                        file,
                        &options.pending_done,
                        false,
                    )?;
                    if marked == 0 {
                        eprintln!("{}", queue_skip_diagnostic_for_file(file)?);
                    }
                }
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
            crate::git_sibling::commit_siblings_for_session_doc(file, &pairs)?;
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
                if let Err(e) = crate::git::commit(file) {
                    eprintln!("[commit] warning: {}", e);
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
    stage: crate::flow::types::FlowStage,
    outcome: crate::flow::types::FlowOutcome,
    reason: crate::flow::closeout::CloseoutGuardReason,
) {
    crate::flow::closeout::log_closeout_guard_event(file, stage, outcome, reason);
}

fn recover_empty_response_for_strict_closeout(file: &Path, flags: &WriteFlags) -> Result<bool> {
    if flags.strict_closeout {
        let outcome = repair::run(file)?;
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
    let dedupe_of_head = crate::dedupe::dedupe_responses(&head_content);
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
pub fn enforce_no_replace_pending(patches: &[template::PatchBlock], allow: bool) -> Result<()> {
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
    if patches
        .iter()
        .any(|p| is_backlog_component(&p.name) || crate::component::is_review_component(&p.name))
    {
        anyhow::bail!(
            "ERR: replace:pending/review block forbidden — use --pending-add/done/edit/clear/reorder or --review-add/edit. \
             See specs/pending-system.md."
        );
    }
    Ok(())
}

fn count_markdown_checklist_items(body: &str) -> usize {
    body.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| trimmed.strip_prefix("+ "))
                .or_else(|| {
                    let digit_run = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
                    if digit_run > 0 {
                        trimmed[digit_run..].strip_prefix(". ")
                    } else {
                        None
                    }
                })
            else {
                return false;
            };

            rest.starts_with("[ ] ") || rest.starts_with("[x] ") || rest.starts_with("[/] ")
        })
        .count()
}

fn todo_component_checklist_count(current_content: &str) -> Result<Option<usize>> {
    let components = component::parse(current_content)
        .context("failed to parse components for todo patch validation")?;
    Ok(components
        .iter()
        .find(|component| component.name == "todo")
        .map(|component| count_markdown_checklist_items(component.content(current_content))))
}

pub fn enforce_no_destructive_todo_patch(
    current_content: &str,
    patches: &[template::PatchBlock],
) -> Result<()> {
    let Some(todo_patch) = patches.iter().rev().find(|patch| patch.name == "todo") else {
        return Ok(());
    };
    let Some(current_count) = todo_component_checklist_count(current_content)? else {
        return Ok(());
    };
    if current_count == 0 {
        return Ok(());
    }

    let patched_count = count_markdown_checklist_items(&todo_patch.content);
    if patched_count < current_count {
        anyhow::bail!(
            "ERR: patch:todo would reduce total checklist item count from {} to {} and is forbidden because it can silently delete untouched todo entries. Rewrite the full todo component or edit the document directly.",
            current_count,
            patched_count
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
    let git_toplevel = crate::git::git_toplevel_at(parent);
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

/// Helper: extract boundary_id for a named component from the document.
///
/// Searches for `<!-- agent:boundary:UUID -->` inside the component's content,
/// skipping matches inside fenced code blocks and inline code spans.
pub fn find_boundary_id(doc: &str, component_name: &str) -> Option<String> {
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

mod normalize;
pub use normalize::*;

fn enforce_orchestrate_template_patch_contract(
    origin: Option<&str>,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
) -> Result<()> {
    crate::flow::document_mutation::enforce_orchestrate_patchback_contract(
        origin, patches, unmatched,
    )
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

fn dedupe_consecutive_response_blocks(content: &str, file: &Path) -> String {
    let deduped = crate::dedupe::dedupe_responses(content);
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
        crate::template::remove_duplicate_answered_exchange_prompt_tail(&repaired)
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
    match crate::template::guard_no_duplicate_prompt_residue_outside_exchange(content) {
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
        while let Some(merged) = crate::template::repair_duplicate_exchange_opener(&result)? {
            eprintln!("[write] normalize_template_structure: merged duplicate exchange opener");
            result = merged;
        }
        result
    };
    let (normalized, _) = repair_duplicate_prompt_artifacts(
        &crate::component::strip_backlog_patch_attr(&deduped_openers),
        file,
        DuplicatePromptRepairOptions::new("structure").preserving(preserve_doc),
    )?;
    match crate::template::guard_no_conversation_tail_outside_exchange(&normalized) {
        Ok(()) => Ok(normalized),
        Err(err)
            if err.chain().any(|cause| {
                cause
                    .to_string()
                    .contains("closing marker <!-- /agent:exchange --> without matching open")
            }) =>
        {
            if let Some(repaired) =
                crate::template::repair_duplicate_exchange_close_scaffold(&normalized)?
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
                crate::template::guard_no_conversation_tail_outside_exchange(&repaired).context(
                    format!(
                        "template structure guard failed for {} after duplicate-scaffold repair",
                        file.display()
                    ),
                )?;
                return Ok(repaired);
            }
            if crate::template::repair_duplicate_exchange_close_mixed_scaffold_tail(&normalized)?
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
                crate::template::repair_duplicate_exchange_close_tail(&normalized)?
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
                crate::template::guard_no_conversation_tail_outside_exchange(&repaired).context(
                    format!(
                        "template structure guard failed for {} after duplicate-close repair",
                        file.display()
                    ),
                )?;
                return Ok(repaired);
            }
            Err(err)
                .with_context(|| format!("template structure guard failed for {}", file.display()))
        }
        Err(err) => Err(err)
            .with_context(|| format!("template structure guard failed for {}", file.display())),
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

/// Minimum size delta before stale snapshot reset drift is considered dangerous.
const STALE_SNAPSHOT_RESET_DRIFT_MIN_BYTES: usize = 100;

/// Maximum current/snapshot size ratio for reset-drift detection.
const STALE_SNAPSHOT_RESET_DRIFT_MAX_RATIO: f64 = 0.90;

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
        crate::debounce::await_idle_via_file(&indicator_path, debounce_ms, timeout_ms);
    let facts = crate::flow::document_mutation::VisibleWriteTypingFacts {
        idle_reached,
        timeout_ms,
    };
    let decision = crate::flow::document_mutation::decide_visible_write_after_typing(facts);
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::document_mutation::visible_write_guard_event(decision, source),
    );
    if decision == crate::flow::document_mutation::VisibleWriteDecision::Apply {
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

fn guard_visible_write_idle_and_current(
    file: &Path,
    source: &str,
    expected_current: &str,
) -> Result<()> {
    match guard_visible_write_reconcile(file, source, expected_current)? {
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
                    crate::ops_log::content_hash(expected_current),
                    crate::ops_log::content_hash(&fresh_current)
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
fn guard_visible_write_reconcile(
    file: &Path,
    source: &str,
    expected_current: &str,
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
        crate::debounce::live_buffer_diverges_from_content(&indicator_path, expected_current)
    {
        // #nm1x provenance suppression: when the editor-visible buffer matches the
        // current on-disk content, the editor holds no unsaved edits *ahead* of
        // disk. The divergence is then disk-vs-expected — an independent / foreign
        // document edit, the reconcilable `DiskDrifted` case below — not a pending
        // user edit. Only a genuine unsaved editor buffer ahead of disk fails
        // closed. This replaces the coarse "any live-buffer divergence blocks
        // finalize" gate with an actor-aware check (the live-buffer actor is not
        // diverging when it already equals disk).
        let disk_hash = crate::ops_log::content_hash(&actual_current);
        let editor_matches_disk =
            live.len == actual_current.len() && live.hash.eq_ignore_ascii_case(&disk_hash);
        if editor_matches_disk {
            let expected_hash = crate::ops_log::content_hash(expected_current);
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
                    crate::ops_log::content_hash(expected_current),
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
            crate::ops_log::content_hash(expected_current),
            crate::ops_log::content_hash(&actual_current)
        ),
    );
    Ok(VisibleWriteReconcile::DiskDrifted {
        fresh_current: actual_current,
    })
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
    mut guard: impl FnMut(&Path, &str) -> Result<VisibleWriteReconcile>,
    mut recompute: impl FnMut(&str) -> Result<T>,
    fail_closed: impl FnOnce(&Path, &str) -> Result<()>,
) -> Result<(String, T)> {
    let mut current = initial_current;
    let mut payload = initial_payload;
    for _ in 0..max_attempts {
        match guard(file, &current)? {
            VisibleWriteReconcile::Clean => return Ok((current, payload)),
            VisibleWriteReconcile::DiskDrifted { fresh_current } => {
                current = fresh_current;
                payload = recompute(&current)?;
            }
        }
    }
    // Document kept changing under us across every reconcile attempt; fall back
    // to the fail-closed guard so the operator retries.
    fail_closed(file, &current)?;
    Ok((current, payload))
}

mod converge;
pub use converge::*;

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

fn exchange_content(doc: &str) -> Option<&str> {
    component::parse(doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "exchange")
        .map(|component| component.content(doc))
}

fn normalized_prompt_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("## User")
        || is_markdown_heading_line(trimmed)
    {
        return None;
    }
    Some(
        trimmed
            .strip_prefix('❯')
            .unwrap_or(trimmed)
            .trim()
            .to_string(),
    )
}

fn is_markdown_heading_line(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ')
}

fn normalized_prompt_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in exchange.lines() {
        if let Some(text) = normalized_prompt_text(line) {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

fn response_aware_user_prompt_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for info in exchange_prompt_reconciliation_infos(exchange, None) {
        if let Some(text) = info.normalized {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

fn user_prompt_count_growth(reference: &str, candidate: &str) -> usize {
    let (Some(reference_exchange), Some(candidate_exchange)) =
        (exchange_content(reference), exchange_content(candidate))
    else {
        return 0;
    };
    let reference_counts = response_aware_user_prompt_counts(reference_exchange);
    let candidate_counts = response_aware_user_prompt_counts(candidate_exchange);
    candidate_counts
        .iter()
        .map(|(line, candidate_count)| {
            let reference_count = reference_counts.get(line).copied().unwrap_or(0);
            candidate_count.saturating_sub(reference_count)
        })
        .sum()
}

fn exchange_has_live_user_edit(baseline: Option<&str>, before: &str) -> bool {
    let Some(base) = baseline else {
        return false;
    };
    let Some(base_exchange) = exchange_content(base) else {
        return false;
    };
    let Some(before_exchange) = exchange_content(before) else {
        return false;
    };
    strip_boundary_for_dedup(base_exchange) != strip_boundary_for_dedup(before_exchange)
}

fn file_ipc_consumed_without_live_exchange_ack(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    before: Option<&str>,
    after: &str,
    ack_content_proven: bool,
) -> bool {
    if ack_content_proven {
        return false;
    }
    let Some(before) = before else {
        return false;
    };
    if !exchange_has_live_user_edit(baseline, before) {
        return false;
    }
    let (Some(before_exchange), Some(after_exchange)) =
        (exchange_content(before), exchange_content(after))
    else {
        return false;
    };
    if strip_boundary_for_dedup(before_exchange) != strip_boundary_for_dedup(after_exchange) {
        return false;
    }

    let before_hash = crate::ops_log::content_hash(before);
    let after_hash = crate::ops_log::content_hash(after);
    eprintln!(
        "[write] file IPC consumed for {} with live exchange edits but no ack-content proof and no exchange materialization — falling back before snapshot/commit",
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "file_ipc_live_exchange_unacknowledged file={} source={} patch_id={} before_hash={} after_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            before_hash,
            after_hash
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "live_exchange_without_ack_content",
        "direct_write_fallback",
        &format!("before_hash={} after_hash={}", before_hash, after_hash),
    );
    true
}

fn exchange_prompt_prefix_count(exchange: &str) -> usize {
    exchange
        .lines()
        .filter(|line| line.trim_start().starts_with("❯ "))
        .count()
}

fn exchange_prompt_text_duplicated(before: &str, after: &str) -> bool {
    let Some(before_exchange) = exchange_content(before) else {
        return false;
    };
    let Some(after_exchange) = exchange_content(after) else {
        return false;
    };
    let before_counts = normalized_prompt_counts(before_exchange);
    let after_counts = normalized_prompt_counts(after_exchange);
    after_counts.iter().any(|(line, after_count)| {
        let before_count = before_counts.get(line).copied().unwrap_or(0);
        before_count > 0 && *after_count > before_count
    })
}

#[derive(Clone, Debug)]
struct PromptLineInfo {
    segment: String,
    normalized: Option<String>,
    prefixed: bool,
    remove: bool,
}

fn split_line_segment(segment: &str) -> (&str, &str) {
    segment
        .strip_suffix('\n')
        .map(|line| (line, "\n"))
        .unwrap_or((segment, ""))
}

fn exchange_prompt_reconciliation_infos(
    exchange: &str,
    target_counts: Option<&HashMap<String, usize>>,
) -> Vec<PromptLineInfo> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;
    let mut in_code_fence = false;
    let mut infos = Vec::new();

    for segment in exchange.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        let trimmed = line.trim();
        let is_fence = is_exchange_code_fence_delimiter(trimmed);
        let was_in_code_fence = in_code_fence;
        let mut eligible = !(was_in_code_fence || is_fence);
        if eligible {
            if trimmed.starts_with(boundary_prefix) {
                in_response_block = false;
                response_heading_was_prefixed = false;
                eligible = false;
            } else if is_exchange_response_heading_for_prefix_repair(trimmed) {
                in_response_block = true;
                response_heading_was_prefixed =
                    is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
                eligible = false;
            } else if in_response_block {
                let is_target = target_counts
                    .is_some_and(|counts| normalization_target_matches_line(line, counts));
                if starts_targeted_or_prefixed_prompt_repair_after_response(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                ) {
                    in_response_block = false;
                    response_heading_was_prefixed = false;
                } else {
                    eligible = false;
                }
            }
        }

        let normalized = if eligible {
            normalized_prompt_text(line)
        } else {
            None
        };
        infos.push(PromptLineInfo {
            segment: segment.to_string(),
            normalized,
            prefixed: trimmed.starts_with("❯ "),
            remove: false,
        });
        if is_fence {
            in_code_fence = !in_code_fence;
        }
    }

    infos
}

fn prompt_reconciliation_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for info in exchange_prompt_reconciliation_infos(exchange, None) {
        if let Some(text) = info.normalized {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

fn last_exchange_boundary_tail_start(exchange: &str) -> Option<usize> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut offset = 0usize;
    let mut tail_start = None;
    for segment in exchange.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        if line.trim().starts_with(boundary_prefix) {
            tail_start = Some(offset + segment.len());
        }
        offset += segment.len();
    }
    tail_start
}

fn probable_live_prompt_prefix_variant(shorter: &str, longer: &str) -> bool {
    let shorter = shorter.trim();
    let longer = longer.trim();
    if shorter.len() < 16 || longer.len() <= shorter.len() + 2 {
        return false;
    }
    if !longer.starts_with(shorter) || !longer.is_char_boundary(shorter.len()) {
        return false;
    }
    if matches!(
        shorter.chars().last(),
        Some('.' | '!' | '?' | ':' | ';' | ')' | ']')
    ) {
        return false;
    }
    true
}

fn is_exchange_code_fence_delimiter(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if first != '`' && first != '~' {
        return false;
    }
    trimmed.chars().take_while(|ch| *ch == first).count() >= 3
}

fn dedupe_live_prompt_prefix_variants_in_tail(content: &str, file: &Path) -> (String, bool) {
    let Ok(components) = component::parse(content) else {
        return (content.to_string(), false);
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return (content.to_string(), false);
    };
    let exchange_content = exchange.content(content);
    let Some(tail_start) = last_exchange_boundary_tail_start(exchange_content) else {
        return (content.to_string(), false);
    };
    let tail = &exchange_content[tail_start..];
    if tail.trim().is_empty() {
        return (content.to_string(), false);
    }

    #[derive(Clone, Debug)]
    struct TailLine {
        segment: String,
        normalized: Option<String>,
        remove: bool,
    }

    let mut in_fence = false;
    let mut lines = Vec::<TailLine>::new();
    for segment in tail.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        let trimmed = line.trim();
        let is_fence = is_exchange_code_fence_delimiter(trimmed);
        let normalized = if !in_fence && !is_fence {
            normalized_prompt_text(line)
        } else {
            None
        };
        lines.push(TailLine {
            segment: segment.to_string(),
            normalized,
            remove: false,
        });
        if is_fence {
            in_fence = !in_fence;
        }
    }
    if !tail.ends_with('\n') && !tail.is_empty() {
        let consumed: usize = lines.iter().map(|line| line.segment.len()).sum();
        if consumed < tail.len() {
            let rest = &tail[consumed..];
            lines.push(TailLine {
                segment: rest.to_string(),
                normalized: normalized_prompt_text(rest),
                remove: false,
            });
        }
    }

    let mut changed = false;
    for idx in 0..lines.len().saturating_sub(1) {
        if lines[idx].remove || lines[idx + 1].remove {
            continue;
        }
        let Some(left) = lines[idx].normalized.as_deref() else {
            continue;
        };
        let Some(right) = lines[idx + 1].normalized.as_deref() else {
            continue;
        };
        let left_prefixed = lines[idx].segment.trim_start().starts_with("❯ ");
        let right_prefixed = lines[idx + 1].segment.trim_start().starts_with("❯ ");
        if left == right && left_prefixed != right_prefixed {
            if left_prefixed {
                lines[idx + 1].remove = true;
            } else {
                lines[idx].remove = true;
            }
            changed = true;
        } else if probable_live_prompt_prefix_variant(left, right) {
            lines[idx].remove = true;
            changed = true;
        } else if probable_live_prompt_prefix_variant(right, left) {
            lines[idx + 1].remove = true;
            changed = true;
        }
    }

    if !changed {
        return (content.to_string(), false);
    }

    let repaired_tail = lines
        .into_iter()
        .filter(|line| !line.remove)
        .map(|line| line.segment)
        .collect::<String>();
    let repaired_exchange = format!("{}{}", &exchange_content[..tail_start], repaired_tail);
    let repaired = exchange.replace_content(content, &repaired_exchange);
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

fn dedupe_adjacent_prompt_prefix_duplicates(content: &str, file: &Path) -> (String, bool) {
    let Ok(components) = component::parse(content) else {
        return (content.to_string(), false);
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return (content.to_string(), false);
    };
    let exchange_content = exchange.content(content);
    let mut lines = exchange_prompt_reconciliation_infos(exchange_content, None);
    let mut changed = false;

    for idx in 0..lines.len().saturating_sub(1) {
        if lines[idx].remove || lines[idx + 1].remove {
            continue;
        }
        let Some(left) = lines[idx].normalized.as_deref() else {
            continue;
        };
        let Some(right) = lines[idx + 1].normalized.as_deref() else {
            continue;
        };
        if left == right && lines[idx].prefixed != lines[idx + 1].prefixed {
            if lines[idx].prefixed {
                lines[idx + 1].remove = true;
            } else {
                lines[idx].remove = true;
            }
            changed = true;
        }
    }

    if !changed {
        return (content.to_string(), false);
    }

    let repaired_exchange = lines
        .into_iter()
        .filter(|line| !line.remove)
        .map(|line| line.segment)
        .collect::<String>();
    let repaired = exchange.replace_content(content, &repaired_exchange);
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

fn dedupe_prompt_lines_against_before(before: &str, after: &str, file: &Path) -> (String, bool) {
    let Some(before_exchange) = exchange_content(before) else {
        return (after.to_string(), false);
    };
    let Ok(components) = component::parse(after) else {
        return (after.to_string(), false);
    };
    let Some(after_exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return (after.to_string(), false);
    };

    let before_counts = prompt_reconciliation_counts(before_exchange);
    if before_counts.is_empty() {
        return (after.to_string(), false);
    }
    let mut lines: Vec<PromptLineInfo> =
        exchange_prompt_reconciliation_infos(after_exchange.content(after), Some(&before_counts));

    let mut by_text: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, line) in lines.iter().enumerate() {
        if let Some(text) = line.normalized.as_ref() {
            by_text.entry(text.clone()).or_default().push(idx);
        }
    }

    let mut changed = false;
    for (text, indexes) in by_text {
        let allowed = before_counts.get(&text).copied().unwrap_or(0);
        if allowed == 0 || indexes.len() <= allowed {
            continue;
        }

        let mut excess = indexes.len() - allowed;
        if indexes.iter().any(|idx| lines[*idx].prefixed) {
            let unprefixed_indexes: Vec<usize> = indexes
                .iter()
                .copied()
                .filter(|idx| !lines[*idx].prefixed)
                .collect();
            for idx in unprefixed_indexes {
                if excess == 0 {
                    break;
                }
                lines[idx].remove = true;
                excess -= 1;
                changed = true;
            }
        }
        if excess > 0 {
            for idx in indexes.iter().rev().copied() {
                if excess == 0 {
                    break;
                }
                if lines[idx].remove {
                    continue;
                }
                lines[idx].remove = true;
                excess -= 1;
                changed = true;
            }
        }
    }

    if !changed {
        return (after.to_string(), false);
    }

    let repaired_exchange = lines
        .into_iter()
        .filter(|line| !line.remove)
        .map(|line| line.segment)
        .collect::<String>();
    let repaired = after_exchange.replace_content(after, &repaired_exchange);
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_prompt_duplicate_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

fn remove_post_exchange_duplicate_prompt_comments_with_log(
    content: &str,
    file: &Path,
    source: &str,
    preserve_doc: Option<&str>,
    preserve_current_doc: Option<&str>,
) -> (String, bool) {
    let preserve_docs = [preserve_doc, preserve_current_doc]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let Some(cleaned) =
        crate::template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
            content,
            &preserve_docs,
        )
    else {
        return (content.to_string(), false);
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "post_exchange_duplicate_prompt_comment_removed file={} source={} before_commit=true",
            file.display(),
            source
        ),
    );
    (cleaned, true)
}

fn patch_touches_exchange(patches: &[template::PatchBlock], unmatched: &str) -> bool {
    patches.iter().any(|patch| patch.name == "exchange") || !unmatched.trim().is_empty()
}

fn exchange_append_patch_can_rebase_to_head(
    patches: &[template::PatchBlock],
    unmatched: &str,
    mode_overrides: &std::collections::HashMap<String, String>,
) -> bool {
    if mode_overrides
        .get("exchange")
        .is_some_and(|mode| mode == "replace")
    {
        return false;
    }
    patch_touches_exchange(patches, unmatched)
}

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
    let before_exchange = exchange_content(before);
    let after_exchange = exchange_content(after);
    let touches_exchange =
        before_exchange != after_exchange || patch_touches_exchange(patches, unmatched);
    if !touches_exchange {
        return;
    }

    let before_hash = crate::ops_log::content_hash(before);
    let after_hash = crate::ops_log::content_hash(after);
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

    // `#fcc0`: template (non-CRDT) mode writes the merged document straight to
    // disk — the only response path with no prior IPC attempt. Converge through
    // the editor when a JB listener is active (no `File Cache Conflict` dialog);
    // the guard already ran above, so the no-listener fallback is the bare disk
    // write rather than the double-guarded converger entry point.
    if !try_editor_converge(file, &final_content, &content_current, "write_template")? {
        atomic_write(file, &final_content)?;
    }

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
/// tries IPC first. On IPC timeout, writes locally, commits when possible,
/// removes the queued fallback patch after the local closeout, and exits with
/// code 75 (EX_TEMPFAIL).
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
    let snapshot_doc = snapshot::load(file).ok().flatten();
    guard_no_stale_snapshot_reset_drift(
        file,
        snapshot_doc.as_deref(),
        &current_content,
        "stream write",
    )?;
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
        // write happens only as the last-resort IPC-timeout path below.
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
            // IPC timeout — patch file was already cleaned up by try_ipc,
            // but we want to leave a NEW patch file in place for the plugin
            // to pick up later. Re-write it with the SAME patch_id so the
            // plugin can deduplicate if the original IPC delivery was late.
            // Guard: if the cycle was already committed (e.g., a concurrent
            // closeout succeeded), skip the re-write to prevent re-dirtying.
            if let Some(ref committed_id) = cycle_already_committed(file) {
                eprintln!(
                    "[write] run_stream IPC timeout: cycle {} already committed — skipping fallback patch re-write",
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
                        "run_stream_ipc_timeout_skip_fallback file={} cycle_id={} reason=already_committed",
                        file.display(),
                        committed_id
                    ),
                );
                cleanup_fallback_patch_files(file);
                drop(doc_lock);
                repair::clear_pending(file)?;
                return Ok(());
            }
            let hash = snapshot::doc_hash(file)?;
            let patch_file = patches_dir.join(format!("{}.json", hash));

            // Use shared helper for synthesis (same boundary-aware logic as try_ipc)
            let norm_lines_for_timeout = if normalize_prefix_lines.is_empty() {
                None
            } else {
                Some(normalize_prefix_lines.as_slice())
            };
            // Reuse the same boundary seed (patch_id) as the try_ipc build so the
            // fallback patch carries an IDENTICAL boundary, not a fresh random one
            // — the plugin then dedups/replaces instead of appending a second copy
            // (#finalize-visible-buffer-ipc-timeout-race).
            let ipc_patches = build_ipc_patches_json(
                file,
                &patches,
                &unmatched,
                norm_lines_for_timeout,
                Some(&ipc_result.patch_id),
            )?;
            let ipc_node_patches = build_ipc_node_patches_json(Some(base), Some(&content_ours));

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
                    "node_patches": ipc_node_patches,
                    "unmatched": effective_unmatched,
                    "baseline": ipc_baseline.unwrap_or(""),
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
            let (final_content, crdt_state) = if let Some(repaired_current) =
                adopt_current_response_without_duplication(
                    file,
                    base,
                    &content_ours,
                    &content_current,
                    snapshot_doc.as_deref(),
                    &response,
                )? {
                // Plugin already applied the response before the sidecar ack
                // arrived. Re-normalize the current transcript so a retry can
                // still restore missing `❯ ` prefixes without duplicating the
                // response via CRDT merge.
                eprintln!(
                    "[write] IPC timeout path: response already in current file; adopting normalized current content"
                );
                crate::ops_log::log_op(
                    file,
                    "ipc_timeout_plugin_already_applied: adopting normalized current content",
                );
                let doc = crate::crdt::CrdtDoc::from_text(&repaired_current);
                (repaired_current, doc.encode_state())
            } else if content_current == base {
                let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
                (content_ours.clone(), doc.encode_state())
            } else {
                eprintln!("[write] IPC timeout path: file modified, CRDT merging...");
                let base_state = snapshot::crdt_merge_base_state(file, base)?.state;
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
            let final_content = normalize_final_template_content(
                file,
                base,
                snapshot_doc.as_deref(),
                Some(&content_current),
                &final_content,
                Some(&response),
            )?;
            let snapshot_mode = snapshot_persist_mode_with_current(
                baseline,
                base,
                &content_current,
                &content_ours,
                &final_content,
            );
            let snapshot_content =
                snapshot_content_to_persist(snapshot_mode, &content_ours, &final_content);
            let snapshot_crdt_state = match snapshot_mode {
                SnapshotPersistMode::FinalContent => crdt_state.clone(),
                SnapshotPersistMode::ContentOurs => {
                    crate::crdt::CrdtDoc::from_text(&content_ours).encode_state()
                }
            };
            // Snapshot saved BEFORE document write (#wcf5).
            if let Err(e) = guard_visible_write_idle_and_current(
                file,
                "run_stream_ipc_timeout",
                &content_current,
            ) {
                eprintln!(
                    "[write] WARNING: visible write deferred before exit(75): {}",
                    e
                );
                std::process::exit(75);
            }
            if let Err(e) = snapshot::save(file, snapshot_content) {
                eprintln!(
                    "[write] WARNING: snapshot save before exit(75) failed: {}",
                    e
                );
            }
            if let Err(e) =
                snapshot::save_document_crdt(file, &snapshot_crdt_state, snapshot_content)
            {
                eprintln!(
                    "[write] WARNING: CRDT state save before exit(75) failed: {}",
                    e
                );
            }
            let local_write_applied = match atomic_write(file, &final_content) {
                Ok(_) => true,
                Err(e) => {
                    eprintln!(
                        "[write] WARNING: failed to write to working tree before exit(75): {}",
                        e
                    );
                    false
                }
            };
            // The snapshot + disk write above are the only work that needs the
            // pre-response doc lock; release it now so the queue-consume below
            // (and any other re-entrant `acquire_doc_lock` caller) does not
            // flock-deadlock against our own still-held guard. `acquire_doc_lock`
            // uses an exclusive flock that conflicts across separate opens within
            // the same process, so holding `doc_lock` across
            // `consume_queue_prompts_with_outcome` self-deadlocks
            // (#queue-consume-on-stream-ipc-timeout-deadlock).
            drop(doc_lock);
            if local_write_applied {
                log_exchange_write_diagnostic(
                    file,
                    "run_stream_ipc_timeout",
                    "stream_ipc_timeout_disk",
                    Some(&ipc_result.patch_id),
                    baseline,
                    &content_current,
                    &final_content,
                    &patches,
                    &unmatched,
                );
                write_claimed_patch_sentinel(&project_root, &ipc_result.patch_id);
                // #queue-consume-on-stream-ipc-timeout: this closeout commits the
                // response and `exit(75)`s WITHOUT returning to the Phase 3c queue
                // consume in `write_with_options`. Consume the answered head here
                // (force-disk — the IPC/editor that just timed out is dead, so the
                // visible-write guard would only stall again) before committing, so
                // the struck head lands in the same commit. Otherwise a finalized
                // response leaves an unstruck head that re-serves the
                // already-answered prompt and treadmills the auto-loop on the next
                // preflight. The decision matches the strict closeout exactly.
                match queue_consumption_allowed_for_response(
                    file,
                    baseline,
                    &content_current,
                    &response,
                    &flags.pending_done_ids,
                    &flags.pending_kept_open_ids,
                    &[],
                ) {
                    Ok(true) => {
                        if let Err(e) =
                            consume_queue_prompts_with_outcome(file, &flags.pending_done_ids, true)
                        {
                            eprintln!(
                                "[queue] warning: consume on stream IPC-timeout failed: {}",
                                e
                            );
                        }
                        if let Err(e) = mark_completed_queue_prompts_for_done_ids(
                            file,
                            &flags.pending_done_ids,
                            true,
                        ) {
                            eprintln!(
                                "[queue] warning: done-id marking on stream IPC-timeout failed: {}",
                                e
                            );
                        }
                    }
                    Ok(false) => {
                        let marked = mark_completed_queue_prompts_for_done_ids(
                            file,
                            &flags.pending_done_ids,
                            true,
                        )
                        .unwrap_or_else(|e| {
                            eprintln!(
                                "[queue] warning: done-id marking on stream IPC-timeout failed: {}",
                                e
                            );
                            0
                        });
                        if marked == 0
                            && let Ok(diag) = queue_skip_diagnostic_for_file(file)
                        {
                            eprintln!("{}", diag);
                        }
                    }
                    Err(e) => eprintln!(
                        "[queue] warning: queue consume decision on stream IPC-timeout failed: {}",
                        e
                    ),
                }
            }
            // #exit75-done-reap-not-atomic: this stream IPC-timeout closeout
            // commits and `exit(75)`s WITHOUT returning to `complete_required_closeout`,
            // so reap the `[x]` items the --done flags just marked HERE, before the
            // commit, so the reap lands in the same exit-75 commit instead of
            // stranding a completed item for a recovery preflight (which also
            // strands a fresh `preflight_started` cycle). `run_pending_maintenance`
            // writes the reaped/archived doc + snapshot (no commit); the commit
            // below stages it. Idempotent (no-op when nothing is `[x]`) + non-fatal.
            if let Err(e) = crate::preflight::run_pending_maintenance(file) {
                eprintln!(
                    "[commit] stream IPC-timeout pending-reap maintenance failed (non-fatal): {}",
                    e
                );
            }
            if crate::git::is_in_git_repo(file) {
                match crate::git::commit(file) {
                    Ok(_) => cleanup_fallback_patch_files(file),
                    Err(e) => eprintln!("[commit] warning: commit before exit(75) failed: {}", e),
                }
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
            match merge::merge_contents_crdt(Some(&base_state), &content_ours, content_current) {
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
    let (content_current, (final_content, crdt_state, cleaned_resolved_backlog_prompts_applied)) =
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
    let snapshot_content =
        snapshot_content_to_persist(snapshot_mode, &content_ours, &final_content);
    let snapshot_crdt_state = match snapshot_mode {
        SnapshotPersistMode::FinalContent => crdt_state,
        SnapshotPersistMode::ContentOurs => {
            crate::crdt::CrdtDoc::from_text(&content_ours).encode_state()
        }
    };

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
    snapshot::save(file, snapshot_content)?;
    snapshot::save_document_crdt(file, &snapshot_crdt_state, snapshot_content)?;

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
                repair_partial_response_materialization_before_fallback(
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

    // Timeout — fall back to direct stream write
    if consumed_without_materialization {
        eprintln!(
            "[write] IPC patch was consumed without materializing the response — falling back to direct write"
        );
    } else {
        eprintln!(
            "[write] IPC timeout ({}s) — falling back to direct write",
            timeout.as_secs()
        );
        log_ipc_proof_failure(
            file,
            "explicit_file_ipc",
            Some(&patch_id),
            "no_ack",
            "direct_write_fallback",
            &format!(
                "timeout_secs={} patch_file={}",
                timeout.as_secs(),
                patch_file.display()
            ),
        );
    }
    // Clean up the unconsumed patch file
    let _ = std::fs::remove_file(&patch_file);

    // Guard: if the cycle was already committed by a concurrent closeout,
    // skip the fallback disk write to prevent re-dirtying the document.
    if let Some(ref committed_id) = cycle_already_committed(file) {
        eprintln!(
            "[write] run_ipc timeout fallback: cycle {} already committed — skipping disk write",
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
        return Ok(());
    }

    // Fall back to stream write logic
    let mut content_ours = template::apply_patches_with_overrides_with_context(
        base,
        &patches,
        &unmatched,
        file,
        &mode_overrides,
        Some(&rc),
    )
    .context("failed to apply template patches")?;
    content_ours =
        normalize_template_structure_or_fail_preserving(&content_ours, file, Some(base))?;

    // Apply frontmatter patch if present
    if let Some(ref yaml) = frontmatter_yaml {
        content_ours = crate::frontmatter::merge_fields(&content_ours, yaml)
            .context("failed to apply frontmatter patch")?;
    }
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;
    let (final_content, crdt_state) = if content_current == base {
        let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
        (content_ours.clone(), doc.encode_state())
    } else if let Some(repaired_current) = adopt_current_response_without_duplication(
        file,
        base,
        &content_ours,
        &content_current,
        snapshot_doc.as_deref(),
        &response,
    )? {
        eprintln!(
            "[write] IPC fallback: response already in current file; adopting normalized current content"
        );
        let doc = crate::crdt::CrdtDoc::from_text(&repaired_current);
        (repaired_current, doc.encode_state())
    } else {
        eprintln!("[write] File was modified during response generation. CRDT merging...");
        // Match the normal stream write path: the explicit/locked baseline is
        // the common ancestor for this response cycle. The persisted `.yrs`
        // sidecar may be stale relative to that baseline when the editor timed
        // out while the user was typing, and using it here can replay old
        // document content as a fresh concurrent insertion.
        let base_state = snapshot::crdt_merge_base_state(file, base)?.state;
        match merge::merge_contents_crdt(Some(&base_state), &content_ours, &content_current) {
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
    let final_content = normalize_final_template_content(
        file,
        base,
        snapshot_doc.as_deref(),
        Some(&content_current),
        &final_content,
        Some(&response),
    )?;
    log_exchange_write_diagnostic(
        file,
        "run_ipc_timeout_fallback",
        "ipc_timeout_disk",
        None,
        baseline,
        &content_current,
        &final_content,
        &patches,
        &unmatched,
    );
    guard_visible_write_idle_and_current(file, "run_ipc_timeout_fallback", &content_current)?;
    atomic_write(file, &final_content)?;
    snapshot::save(file, &final_content)?;
    snapshot::save_document_crdt(file, &crdt_state, &final_content)?;
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

    guard_visible_write_idle_and_current(file, "apply_append_from_string", &content_current)?;
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
    } else if content_current == content {
        content_ours.clone()
    } else {
        merge::merge_contents(&content, &content_ours, &content_current)?
    };
    let final_content = normalize_final_template_content(
        file,
        &content,
        snapshot_doc.as_deref(),
        Some(&content_current),
        &final_content,
        Some(response.as_str()),
    )?;

    guard_visible_write_idle_and_current(file, "apply_template_from_string", &content_current)?;
    // `#fcc0`: this repair recovery applies template (component) patches straight
    // to disk with no prior IPC attempt — the same direct-disk-no-IPC class as
    // queue consume. Converge through the editor when a JB listener is active (no
    // `File Cache Conflict` dialog); the guard already ran above, so the
    // no-listener fallback is the bare disk write.
    if !try_editor_converge(file, &final_content, &content_current, "apply_template")? {
        atomic_write(file, &final_content)?;
    }
    // Save snapshot as the repaired/merged final content.
    snapshot::save(file, &final_content)?;
    drop(doc_lock);
    eprintln!("[write] Template patches applied to {}", file.display());
    Ok(())
}

mod ipc;
pub use ipc::*;
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
pub fn sanitize_patches(patches: &mut [template::PatchBlock]) {
    for patch in patches.iter_mut() {
        patch.content = sanitize_component_tags(&patch.content);
    }
}

/// Sanitize unmatched (non-patch) response text so agent-generated
/// `<!-- agent:NAME -->` markers cannot create duplicate component blocks
/// when appended to the exchange component.
pub fn sanitize_unmatched(unmatched: &mut String) {
    *unmatched = sanitize_component_tags(unmatched);
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

fn capture_locked_pre_response(path: &Path) -> Result<(std::fs::File, String)> {
    let doc_lock = acquire_doc_lock(path)?;
    let content_at_start = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    snapshot::save_pre_response(path, &content_at_start)?;
    Ok((doc_lock, content_at_start))
}

fn normalize_component_content_for_delta(content: &str) -> String {
    crate::diff::strip_comments(&strip_boundary_for_dedup(content))
}

fn containment_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn base_prompt_prefix_equivalents(base: &str) -> HashSet<String> {
    base.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            Some(
                trimmed
                    .strip_prefix('❯')
                    .unwrap_or(trimmed)
                    .trim()
                    .to_string(),
            )
        })
        .collect()
}

fn inserted_delta_hunks(base: &str, ours: &str) -> Vec<Vec<String>> {
    let base_prefix_equivalents = base_prompt_prefix_equivalents(base);
    let base_lines = base
        .lines()
        .filter_map(containment_line)
        .collect::<HashSet<_>>();
    let diff = TextDiff::from_lines(base, ours);
    let mut hunks = Vec::<Vec<String>>::new();
    let mut current = Vec::<String>::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => {
                let line = change.to_string();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if base_lines.contains(trimmed) {
                    continue;
                }
                if let Some(unprefixed) = trimmed.strip_prefix('❯') {
                    let unprefixed = unprefixed.trim();
                    if base_prefix_equivalents.contains(unprefixed) {
                        continue;
                    }
                }
                current.push(trimmed.to_string());
            }
            ChangeTag::Delete | ChangeTag::Equal => {
                if !current.is_empty() {
                    hunks.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if !current.is_empty() {
        hunks.push(current);
    }

    hunks
        .into_iter()
        .filter(|hunk| response_delta_hunk_is_actionable(hunk))
        .collect()
}

fn response_delta_hunk_is_actionable(hunk: &[String]) -> bool {
    hunk.iter().any(|line| {
        line.starts_with("### Re:")
            || line.starts_with("## Assistant")
            || line.starts_with("## User")
    }) || hunk.len() >= 2
}

fn contains_contiguous_hunk(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Detect whether the plugin has already applied the agent's response patches.
///
/// On IPC sidecar ack timeout, the socket delivery may have succeeded but the
/// confirmation did not arrive in time. If the plugin applied the patches, the
/// exchange component in `content_current` already contains the response delta
/// from `content_ours`. CRDT merging in this state would duplicate the response.
///
/// Detection: compute normalized insertion hunks from `base -> content_ours` in
/// `agent:exchange`, ignore boundary/comment churn and prompt-prefix-only
/// normalization lines, and require each actionable response hunk to appear
/// contiguously in `content_current`. This is intentionally stricter than a
/// line-overlap count so short responses do not adopt current content from a
/// coincidental shared body line.
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

    let base_content = normalize_component_content_for_delta(base_e.content(base));
    let ours_content = normalize_component_content_for_delta(ours_e.content(content_ours));
    let current_content = normalize_component_content_for_delta(current_e.content(content_current));

    // No changes to exchange — nothing to detect
    if ours_content.trim() == base_content.trim() {
        return false;
    }

    let response_hunks = inserted_delta_hunks(&base_content, &ours_content);
    if response_hunks.is_empty() {
        return false;
    }

    let current_lines = current_content
        .lines()
        .filter_map(containment_line)
        .collect::<Vec<_>>();
    let detected = response_hunks
        .iter()
        .all(|hunk| contains_contiguous_hunk(&current_lines, hunk));

    if detected {
        eprintln!(
            "[write] plugin-applied detection: {} normalized response delta hunk(s) already in current",
            response_hunks.len()
        );
    }

    detected
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
    if !crate::repair::response_already_applied(content_current, response)
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

/// Strip leaked harness user-prompt markers (`❯ `) from the leading response
/// body lines of every `### Re: ...` response block inside `agent:exchange`.
///
/// Background: when finalize falls through to the CRDT merge path while the
/// live document had a `❯ ` user input at the same column position as the
/// incoming response body, or when repair adopts an already-visible response
/// while the snapshot only contains the response heading, prompt normalization
/// can leave the response paragraphs prefixed with `❯ `. `session-check` then
/// classifies the corrupted block as a prompt-only closeout tail or the next
/// closeout replays the same response.
///
/// Real response bodies do not use `❯ ` as a paragraph marker. Strip a leading
/// run of prefixed response-body lines until the first unprefixed body line; any
/// later `❯ ` text is preserved as quoted/user-visible prose. Returns
/// `Some(repaired)` when any prefix was stripped, `None` when the document is
/// clean. See `tasks/agent-doc/plan-crdt-merge-prompt-prefix-leaks-into-response-body.md`.
pub fn strip_prompt_prefix_from_response_body_first_lines(content: &str) -> Option<String> {
    let components = component::parse(content).ok()?;
    let exchange = components.iter().find(|c| c.name == "exchange")?;
    let exchange_body = exchange.content(content);

    let mut repaired_lines: Vec<String> = Vec::with_capacity(exchange_body.lines().count());
    let mut in_response_block = false;
    let mut saw_unprefixed_response_body_line = false;
    let mut stripped_any = false;
    for line in exchange_body.lines() {
        let trimmed_start = line.trim_start();
        let is_response_heading = trimmed_start.starts_with("### Re:");
        let is_other_heading = trimmed_start.starts_with("###") && !is_response_heading
            || trimmed_start.starts_with("## ")
            || trimmed_start.starts_with("# ");
        let is_exchange_marker = trimmed_start.starts_with("<!-- agent:")
            || trimmed_start.starts_with("<!-- /agent:")
            || trimmed_start.starts_with("<!-- agent:boundary:");

        if is_response_heading {
            in_response_block = true;
            saw_unprefixed_response_body_line = false;
            repaired_lines.push(line.to_string());
            continue;
        }
        if is_other_heading || is_exchange_marker {
            in_response_block = false;
            saw_unprefixed_response_body_line = false;
            repaired_lines.push(line.to_string());
            continue;
        }
        if in_response_block && !line.trim().is_empty() {
            if !saw_unprefixed_response_body_line
                && starts_prompt_run_after_response(trimmed_start, false)
            {
                in_response_block = false;
                saw_unprefixed_response_body_line = false;
                repaired_lines.push(line.to_string());
                continue;
            }
            if let Some(rest) = line.strip_prefix("❯ ")
                && !saw_unprefixed_response_body_line
            {
                stripped_any = true;
                repaired_lines.push(rest.to_string());
                continue;
            }
            if line.trim_start() == "❯" && !saw_unprefixed_response_body_line {
                stripped_any = true;
                repaired_lines.push(String::new());
                continue;
            }
            saw_unprefixed_response_body_line = true;
        }
        repaired_lines.push(line.to_string());
    }
    if !stripped_any {
        return None;
    }
    let mut repaired_body = repaired_lines.join("\n");
    if exchange_body.ends_with('\n') && !repaired_body.ends_with('\n') {
        repaired_body.push('\n');
    }
    Some(exchange.replace_content(content, &repaired_body))
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
        repair_response_precedes_prompt_in_exchange(&normalized, response, file, Some(base))?
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

#[derive(Clone, Debug)]
struct ExchangeLineSegment {
    segment: String,
    line: String,
}

fn split_exchange_line_segments(content: &str) -> Vec<ExchangeLineSegment> {
    content
        .split_inclusive('\n')
        .map(|segment| {
            let line = segment
                .strip_suffix('\n')
                .map(str::to_string)
                .unwrap_or_else(|| segment.to_string());
            ExchangeLineSegment {
                segment: segment.to_string(),
                line,
            }
        })
        .collect()
}

fn line_is_exchange_boundary(trimmed: &str) -> bool {
    trimmed.starts_with("<!-- agent:boundary:")
}

fn normalized_response_signature_lines(
    response: Option<&str>,
) -> std::collections::HashSet<String> {
    response
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("<!--"))
        .filter(|line| *line != "Done.")
        .map(|line| line.trim_start_matches('❯').trim().to_string())
        .collect()
}

fn exchange_response_block_matches_signature(
    segments: &[ExchangeLineSegment],
    heading_idx: usize,
    prompt_idx: usize,
    signature: &std::collections::HashSet<String>,
    response: Option<&str>,
) -> bool {
    let Some(response) = response else {
        return false;
    };
    if response.trim().is_empty() {
        return false;
    }
    let heading = segments[heading_idx].line.trim();
    if response.contains(heading) {
        return true;
    }
    if signature.is_empty() {
        return false;
    }
    segments[heading_idx..prompt_idx].iter().any(|segment| {
        let normalized = segment
            .line
            .trim()
            .trim_start_matches('❯')
            .trim()
            .to_string();
        !normalized.is_empty() && signature.contains(&normalized)
    })
}

fn find_response_precedes_prompt_candidate(
    exchange_content: &str,
    response: Option<&str>,
) -> Option<(usize, usize, usize)> {
    let segments = split_exchange_line_segments(exchange_content);
    let signature = normalized_response_signature_lines(response);

    for heading_idx in 0..segments.len() {
        let heading = segments[heading_idx].line.trim();
        if !is_exchange_response_heading_for_prefix_repair(heading) {
            continue;
        }
        let mut saw_boundary_after_heading = false;
        for idx in (heading_idx + 1)..segments.len() {
            let trimmed = segments[idx].line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if line_is_exchange_boundary(trimmed) {
                saw_boundary_after_heading = true;
                continue;
            }
            if trimmed.starts_with("<!--") {
                continue;
            }
            if is_exchange_response_heading_for_prefix_repair(trimmed) {
                break;
            }
            let normalized = trimmed.trim_start_matches('❯').trim();
            let is_target = signature.contains(normalized);
            if saw_boundary_after_heading
                && starts_prompt_run_after_response(trimmed, is_target)
                && exchange_response_block_matches_signature(
                    &segments,
                    heading_idx,
                    idx,
                    &signature,
                    response,
                )
            {
                let mut prompt_end = segments.len();
                for (next_idx, next) in segments.iter().enumerate().skip(idx + 1) {
                    if is_exchange_response_heading_for_prefix_repair(next.line.trim()) {
                        prompt_end = next_idx;
                        break;
                    }
                }
                return Some((heading_idx, idx, prompt_end));
            }
        }
    }
    None
}

fn response_precedes_prompt_in_exchange(
    doc: &str,
    response: Option<&str>,
    prompt_must_exist_in: Option<&str>,
) -> bool {
    let Ok(components) = component::parse(doc) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    let exchange_content = exchange.content(doc);
    let Some((_, prompt_idx, prompt_end)) =
        find_response_precedes_prompt_candidate(exchange_content, response)
    else {
        return false;
    };
    if let Some(required_doc) = prompt_must_exist_in {
        let segments = split_exchange_line_segments(exchange_content);
        let prompt_lines =
            normalized_non_boundary_exchange_lines(&segments[prompt_idx..prompt_end]);
        return exchange_contains_normalized_line_sequence(required_doc, &prompt_lines);
    }
    true
}

pub fn repair_response_precedes_prompt_in_exchange(
    doc: &str,
    response: Option<&str>,
    file: &Path,
    prompt_must_exist_in: Option<&str>,
) -> Result<Option<String>> {
    let components = component::parse(doc).with_context(|| {
        format!(
            "failed to parse {} for response/prompt order repair",
            file.display()
        )
    })?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let exchange_content = exchange.content(doc);
    let Some((heading_idx, prompt_idx, prompt_end)) =
        find_response_precedes_prompt_candidate(exchange_content, response)
    else {
        return Ok(None);
    };
    let segments = split_exchange_line_segments(exchange_content);
    if let Some(required_doc) = prompt_must_exist_in {
        let prompt_lines =
            normalized_non_boundary_exchange_lines(&segments[prompt_idx..prompt_end]);
        if !exchange_contains_normalized_line_sequence(required_doc, &prompt_lines) {
            return Ok(None);
        }
    }
    let boundary_id = crate::boundary::find_boundary_id_in_component(doc, exchange);
    let boundary_marker = boundary_id
        .as_deref()
        .map(crate::boundary::format_marker)
        .unwrap_or_else(|| crate::boundary::format_marker(&crate::boundary::new_id()));

    let keep_non_boundary =
        |segment: &ExchangeLineSegment| !line_is_exchange_boundary(segment.line.trim());
    let prefix = segments[..heading_idx]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();
    let response_block = segments[heading_idx..prompt_idx]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();
    let prompt_block = segments[prompt_idx..prompt_end]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();
    let suffix = segments[prompt_end..]
        .iter()
        .filter(|segment| keep_non_boundary(segment))
        .map(|segment| segment.segment.as_str())
        .collect::<String>();

    let mut repaired_exchange = String::new();
    repaired_exchange.push_str(&prefix);
    if !repaired_exchange.is_empty()
        && !repaired_exchange.ends_with('\n')
        && !prompt_block.is_empty()
    {
        repaired_exchange.push('\n');
    }
    repaired_exchange.push_str(&prompt_block);
    if !repaired_exchange.is_empty()
        && !repaired_exchange.ends_with('\n')
        && !response_block.is_empty()
    {
        repaired_exchange.push('\n');
    }
    repaired_exchange.push_str(&response_block);
    if !repaired_exchange.ends_with('\n') {
        repaired_exchange.push('\n');
    }
    repaired_exchange.push_str(&boundary_marker);
    repaired_exchange.push('\n');
    repaired_exchange.push_str(&suffix);

    let repaired = exchange.replace_content(doc, &repaired_exchange);
    if repaired == doc {
        return Ok(None);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "response_prompt_order_repaired file={} before_commit=true",
            file.display()
        ),
    );
    Ok(Some(repaired))
}

fn normalized_non_boundary_exchange_lines(segments: &[ExchangeLineSegment]) -> Vec<String> {
    segments
        .iter()
        .filter_map(|segment| {
            let trimmed = segment.line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("<!--")
                || line_is_exchange_boundary(trimmed)
            {
                return None;
            }
            Some(trimmed.trim_start_matches('❯').trim().to_string())
        })
        .collect()
}

fn exchange_contains_normalized_line_sequence(doc: &str, needle: &[String]) -> bool {
    if needle.is_empty() {
        return false;
    }
    let Ok(components) = component::parse(doc) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    let haystack = split_exchange_line_segments(exchange.content(doc));
    let haystack = normalized_non_boundary_exchange_lines(&haystack);
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Transfer the tracked backlog/pending component content from `source` into
/// `target`.
///
/// When the on-disk file has pending mutations applied (e.g. `--done`)
/// that are not reflected in `content_ours` (which was built from a pre-mutation
/// baseline), this function preserves those mutations by splicing the tracked
/// backlog component from `source` into `target`.
///
/// Behaviour:
/// - If both `source` and `target` have a tracked backlog component: replaces the
///   content between the markers in `target` with the content from `source`.
/// - If `source` has no tracked backlog component: returns `target` unchanged.
/// - If `source` has tracked backlog content but `target` does not: logs a
///   warning and returns `target` unchanged (can't locate insertion point
///   without knowing document structure).
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
                "[write] WARNING: splice_pending: source has tracked backlog content but target does not — \
                 pending mutations may be lost on IPC fallback"
            );
            target.to_string()
        }
    }
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
    if crate::write_authority::is_visible_document(path)
        && !crate::write_authority::within_owner_scope()
    {
        let base_dir = crate::fs_util::find_project_root(path)
            .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")).to_path_buf());
        let file = path.to_string_lossy().to_string();
        let result = crate::write_queue::serialized_atomic_write(&base_dir, &file, path, content);
        if result.is_ok() {
            // Log after the write lands so the document path canonicalizes
            // (ops.log root resolution requires the file to exist).
            crate::ops_log::log_op(
                path,
                &format!(
                    "write_authority action=routed len={} hash={}",
                    content.len(),
                    crate::ops_log::content_hash(content)
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
    if !crate::write_authority::is_visible_document(path) {
        return;
    }
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    let write_id = uuid::Uuid::new_v4().to_string();
    let hash = crate::debounce::content_hash(content);
    if let Err(e) = crate::debounce::record_write_provenance(
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
mod tests;
#[cfg(test)]
mod post_commit_prefix_repair_tests;
#[cfg(test)]
mod ack_content_snapshot_tests;
#[cfg(test)]
mod submodule_patch_routing_tests;
#[cfg(test)]
mod queue_prompt_echo_summary_tests;
#[cfg(test)]
mod future_work_signal_tests;
#[cfg(test)]
mod verify_sidecar_normalization_tests;
#[cfg(test)]
mod precommit_pending_capture_tests;
#[cfg(test)]
mod pending_patch_normalization_tests;
#[cfg(test)]
mod late_fallback_patch_guard_tests;
