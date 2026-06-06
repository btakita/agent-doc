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
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

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
    pub review_add: Vec<String>,
    pub review_edit: Vec<String>,
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
            || has_non_exchange_user_drift(base, content_current))
    {
        return SnapshotPersistMode::ContentOurs;
    }

    snapshot_persist_mode(baseline, content_ours, final_content)
}

fn has_non_exchange_user_drift(base: &str, current: &str) -> bool {
    let base_norm = strip_boundary_for_dedup(base);
    let current_norm = strip_boundary_for_dedup(current);
    if base_norm == current_norm {
        return false;
    }

    outside_component_content_changed(&base_norm, &current_norm, "exchange")
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

/// `#queue-user-edit-overwrite`: the user-authored `do [#id]` queue line(s)
/// present in the IPC `candidate` (disk / ack sidecar) `agent:queue` that
/// `content_ours` does not own — i.e. the queue edits that would be silently
/// deleted when `content_ours` is adopted. Scoped to `agent:queue` so exchange /
/// backlog drift is not misread. Recorded so `session-check` can fail closed on
/// the silent-queue-deletion class instead of letting convergence drop a
/// user-added queue head the current response never consumed.
fn dropped_queue_prompt_lines_after_content_ours(
    baseline: &str,
    candidate: &str,
    content_ours: &str,
) -> Vec<String> {
    let baseline_q = queue_component_text(baseline);
    let candidate_q = queue_component_text(candidate);
    let content_ours_q = queue_component_text(content_ours);

    let candidate_changes = prompt_bearing_user_changes_between(&baseline_q, &candidate_q);
    if candidate_changes.is_empty() {
        return Vec::new();
    }
    let owned_changes = prompt_bearing_user_changes_between(&baseline_q, &content_ours_q);
    candidate_changes
        .into_iter()
        // Only a discrete prompt target (a `do #id` / prompt-shaped queue line)
        // is an unambiguous user queue edit. Multi-line content edits are noisy
        // diff context, not a discrete queued prompt to recover.
        .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
        .filter(|change| !prompt_bearing_change_owned_by_content_ours(change, &owned_changes))
        .map(|change| change.text.trim().to_string())
        .filter(|text| !text.is_empty() && !text.contains('\n'))
        .collect()
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

fn read_explicit_baseline(file: &Path, baseline_file: Option<&Path>) -> Result<Option<String>> {
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
        || !options.review_add.is_empty()
        || !options.review_edit.is_empty();

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
        crate::pending_cmd::add_many(file, &options.pending_add_gated, true)?;
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
        for value in &options.review_add {
            crate::pending_cmd::review_add(file, value)?;
        }
        for pair in &options.review_edit {
            let (id, text) = pair
                .split_once('=')
                .with_context(|| format!("--review-edit expects 'id=text', got: {}", pair))?;
            crate::pending_cmd::review_edit(file, id, text)?;
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

    let baseline = read_explicit_baseline(file, options.baseline_file.as_deref())?;

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
                } else {
                    eprintln!("{}", queue_skip_diagnostic_for_file(file)?);
                }
            }
            CommitMode::Required => {
                if queue_consumption_allowed {
                    consume_queue_prompts_for_done_ids_with_outcome(file, &options.pending_done)?;
                } else {
                    eprintln!("{}", queue_skip_diagnostic_for_file(file)?);
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

pub fn unresolved_backlog_capture_targets(
    file: &Path,
    state: &crate::cycle_state::CycleState,
) -> Vec<String> {
    let current = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());

    state
        .required_backlog_targets
        .iter()
        .filter(|target| {
            let target_path = Path::new(&target.path);
            let normalized_target =
                std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
            if normalized_target == current {
                return !state.had_pending_mutations;
            }

            let Ok(Some(content)) = std::fs::read_to_string(&normalized_target).map(Some) else {
                return true;
            };
            let Ok(components) = crate::component::parse(&content) else {
                return true;
            };
            let component = target
                .component
                .as_deref()
                .and_then(|name| components.iter().find(|component| component.name == name))
                .or_else(|| {
                    components
                        .iter()
                        .find(|component| crate::component::is_backlog_component(&component.name))
                })
                .or_else(|| {
                    components.iter().find(|component| {
                        crate::component::is_tracked_work_component(&component.name)
                    })
                });
            let current_hash = component
                .map(|component| crate::ops_log::content_hash(component.content(&content)));
            match (&target.baseline_hash, current_hash) {
                (Some(expected), Some(current)) => current == *expected,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => true,
            }
        })
        .map(|target| target.path.clone())
        .collect()
}

fn normalize_pending_id(id: &str) -> String {
    id.trim().trim_start_matches('#').to_ascii_lowercase()
}

fn tracked_work_ids_from_component_body(body: &str) -> HashSet<String> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .into_iter()
        .filter(|item| !item.is_done())
        .map(|item| normalize_pending_id(&item.id))
        .filter(|id| !id.is_empty())
        .collect()
}

fn tracked_work_ids_for_target(
    content: &str,
    preferred_component: Option<&str>,
) -> Result<HashSet<String>> {
    let components = crate::component::parse(content)?;
    let component = preferred_component
        .and_then(|name| components.iter().find(|component| component.name == name))
        .or_else(|| {
            components
                .iter()
                .find(|component| crate::component::is_backlog_component(&component.name))
        })
        .or_else(|| {
            components
                .iter()
                .find(|component| crate::component::is_tracked_work_component(&component.name))
        });
    Ok(component
        .map(|component| tracked_work_ids_from_component_body(component.content(content)))
        .unwrap_or_default())
}

fn promised_backlog_item_ids_from_response(
    response_text: &str,
    state: &crate::cycle_state::CycleState,
) -> Vec<String> {
    let baseline_ids: HashSet<String> = state
        .required_backlog_targets
        .iter()
        .flat_map(|target| target.baseline_item_ids.iter())
        .map(|id| normalize_pending_id(id))
        .collect();
    let (_, items, _) = crate::pending::parse_items(response_text);
    let mut promised = Vec::new();
    for item in items.into_iter().filter(|item| !item.is_done()) {
        let id = normalize_pending_id(&item.id);
        if id.is_empty()
            || baseline_ids.contains(&id)
            || promised.iter().any(|existing| existing == &id)
        {
            continue;
        }
        promised.push(id);
    }
    promised
}

pub fn promised_backlog_item_inventory_shortfall(
    state: &crate::cycle_state::CycleState,
    response_text: &str,
) -> Option<(usize, usize)> {
    if state.required_backlog_targets.is_empty() || state.required_explicit_backlog_item_count == 0
    {
        return None;
    }

    let promised_count = promised_backlog_item_ids_from_response(response_text, state).len();
    if promised_count >= state.required_explicit_backlog_item_count {
        None
    } else {
        Some((state.required_explicit_backlog_item_count, promised_count))
    }
}

fn promised_plan_reference_paths(file: &Path, response_text: &str) -> Vec<String> {
    let mut promised = Vec::new();
    for raw_line in response_text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") || trimmed.starts_with('>') {
            continue;
        }
        if !trimmed.to_ascii_lowercase().contains("plan") {
            continue;
        }
        let Some(path) = crate::security::referenced_markdown_path(file, trimmed) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !file_name.contains("plan") {
            continue;
        }
        let normalized = std::fs::canonicalize(&path)
            .unwrap_or(path)
            .display()
            .to_string();
        if !promised.iter().any(|existing| existing == &normalized) {
            promised.push(normalized);
        }
    }
    promised
}

pub fn promised_plan_reference_shortfall(
    file: &Path,
    state: &crate::cycle_state::CycleState,
    response_text: &str,
) -> Option<(usize, usize)> {
    if state.required_plan_reference_count == 0 {
        return None;
    }

    let promised_count = promised_plan_reference_paths(file, response_text).len();
    if promised_count >= state.required_plan_reference_count {
        None
    } else {
        Some((state.required_plan_reference_count, promised_count))
    }
}

pub fn unresolved_promised_backlog_item_ids(
    file: &Path,
    state: &crate::cycle_state::CycleState,
    response_text: &str,
) -> Vec<String> {
    if state.required_backlog_targets.is_empty() {
        return Vec::new();
    }

    let promised_ids = promised_backlog_item_ids_from_response(response_text, state);
    if promised_ids.is_empty() {
        return Vec::new();
    }

    let current = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut current_target_ids = HashSet::new();
    for target in &state.required_backlog_targets {
        let target_path = Path::new(&target.path);
        let normalized_target =
            std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
        let content = if normalized_target == current {
            match std::fs::read_to_string(file) {
                Ok(content) => content,
                Err(_) => continue,
            }
        } else {
            match std::fs::read_to_string(&normalized_target) {
                Ok(content) => content,
                Err(_) => continue,
            }
        };
        let Ok(ids) = tracked_work_ids_for_target(&content, target.component.as_deref()) else {
            continue;
        };
        current_target_ids.extend(ids);
    }

    promised_ids
        .into_iter()
        .filter(|id| !current_target_ids.contains(id))
        .map(|id| format!("#{}", id))
        .collect()
}

fn precommit_pending_capture_check(file: &Path) -> Result<()> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(());
    };
    if state.had_pending_mutations && state.required_backlog_targets.is_empty() {
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
    let missing_targets = unresolved_backlog_capture_targets(file, &state);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureTargetMissing,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active prompt required backlog capture in {} \
             but those tracked-work surfaces did not change this cycle\n\
             [finalize] hint: update those backlog targets before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_targets.join(", ")
        );
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_backlog_item_inventory_shortfall(&state, &response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureInventoryShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active #agent-doc-bug contract described at least {} distinct issue(s), \
             but the response only enumerated {} explicit backlog item(s) for target(s) {}\n\
             [finalize] hint: enumerate each transferred bug as a tracked backlog item in the response \
             (for example `- [ ] [#id] ...`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count,
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) =
            promised_plan_reference_shortfall(file, &state, &response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePlanShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active #agent-doc-bug contract required at least {} explicit plan reference(s), \
             but the response only cited {} existing plan path(s)\n\
             [finalize] hint: create each plan file and cite it in the response \
             (for example `Plan: tasks/agent-doc/plan-foo.md`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count
        );
    }
    let missing_ids = unresolved_promised_backlog_item_ids(file, &state, &response_text);
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePromisedIdsMissing,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: response promised new tracked item(s) {} \
             for explicit backlog target(s) {}, but those ids are still missing after this cycle\n\
             [finalize] hint: transfer every listed item into the explicit target backlog, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_ids.join(", "),
            state
                .required_backlog_targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if state.requires_backlog_capture
        && state.required_backlog_targets.is_empty()
        && !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureRequired,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: active prompt requested backlog capture \
             but no backlog mutations were recorded this cycle\n\
             [finalize] hint: re-run finalize with --pending-add flags, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures"
        );
    }

    if state.had_pending_mutations {
        return Ok(());
    }

    let mode = crate::session_check::resolve_pending_capture_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
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

    log_closeout_guard(
        file,
        crate::flow::types::FlowStage::PreCommitGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingCaptureRecommendations,
    );
    anyhow::bail!(
        "[finalize] pre-commit gate: response contains ~{} recommendation-like items \
         but no --pending-add flags were used this cycle\n\
         [finalize] hint: re-run finalize with --pending-add flags, \
         add <!-- no-pending-capture --> to suppress, \
         or set pending_capture_guard = \"warn\" to downgrade",
        signal.estimated_count
    );
}

fn prewrite_pending_capture_check(
    file: &Path,
    response_body: &str,
    flags: &WriteFlags,
) -> Result<()> {
    if !flags.strict_closeout {
        return Ok(());
    }

    let state = crate::cycle_state::load(file)?;
    let has_explicit_targets = state
        .as_ref()
        .is_some_and(|state| !state.required_backlog_targets.is_empty());
    if !has_explicit_targets
        && (state
            .as_ref()
            .is_some_and(|state| state.had_pending_mutations)
            || flags.has_pending_add
            || flags.has_pending_done)
    {
        return Ok(());
    }
    if response_body.contains("<!-- no-pending-capture -->") {
        return Ok(());
    }

    let response_text = crate::session_check::response_text_for_guards(response_body);
    if response_text.trim().is_empty() {
        return Ok(());
    }
    let missing_targets = state
        .as_ref()
        .map(|state| unresolved_backlog_capture_targets(file, state))
        .unwrap_or_default();
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_targets.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureTargetMissing,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active prompt required backlog capture in {} \
             but those tracked-work surfaces did not change this cycle\n\
             [finalize] hint: update those backlog targets before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_targets.join(", ")
        );
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) = state
            .as_ref()
            .and_then(|state| promised_backlog_item_inventory_shortfall(state, &response_text))
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureInventoryShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active #agent-doc-bug contract described at least {} distinct issue(s), \
             but the response only enumerated {} explicit backlog item(s) for target(s) {}\n\
             [finalize] hint: enumerate each transferred bug as a tracked backlog item in the response \
             (for example `- [ ] [#id] ...`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count,
            state
                .as_ref()
                .map(|state| {
                    state
                        .required_backlog_targets
                        .iter()
                        .map(|target| target.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        );
    }
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && let Some((expected_count, promised_count)) = state
            .as_ref()
            .and_then(|state| promised_plan_reference_shortfall(file, state, &response_text))
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePlanShortfall,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active #agent-doc-bug contract required at least {} explicit plan reference(s), \
             but the response only cited {} existing plan path(s)\n\
             [finalize] hint: create each plan file and cite it in the response \
             (for example `Plan: tasks/agent-doc/plan-foo.md`) before finalize, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            expected_count,
            promised_count
        );
    }
    let missing_ids = state
        .as_ref()
        .map(|state| unresolved_promised_backlog_item_ids(file, state, &response_text))
        .unwrap_or_default();
    if !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
        && !missing_ids.is_empty()
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCapturePromisedIdsMissing,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: response promised new tracked item(s) {} \
             for explicit backlog target(s) {}, but those ids are still missing after this cycle\n\
             [finalize] hint: transfer every listed item into the explicit target backlog, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures",
            missing_ids.join(", "),
            state
                .as_ref()
                .map(|state| {
                    state
                        .required_backlog_targets
                        .iter()
                        .map(|target| target.path.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        );
    }
    if state.as_ref().is_some_and(|state| {
        state.requires_backlog_capture && state.required_backlog_targets.is_empty()
    }) && !crate::prompt_contract::response_explicitly_has_no_followups(&response_text)
    {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingCaptureRequired,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: active prompt requested backlog capture \
             but no backlog mutations were recorded this cycle\n\
             [finalize] hint: re-run finalize with --pending-add flags, \
             explicitly state that there were no actionable follow-up items, \
             add <!-- no-pending-capture --> to suppress, \
             or set pending_capture_guard = \"warn\" to downgrade heuristic-only captures"
        );
    }

    if state
        .as_ref()
        .is_some_and(|state| state.had_pending_mutations)
        || flags.has_pending_add
        || flags.has_pending_done
    {
        return Ok(());
    }

    let mode = crate::session_check::resolve_pending_capture_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
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

    log_closeout_guard(
        file,
        crate::flow::types::FlowStage::PreWriteGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingCaptureRecommendations,
    );
    anyhow::bail!(
        "[finalize] pre-write gate: response contains ~{} recommendation-like items \
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
    let malformed = crate::session_check::malformed_tracked_item_refs(file, Some(&response_text))?;
    if !malformed.is_empty() {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreCommitGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingDoneMalformedTrackedItem,
        );
        anyhow::bail!(
            "[finalize] pre-commit gate: {}",
            crate::session_check::malformed_tracked_item_message(&malformed)
        );
    }
    let missing = crate::session_check::detect_missing_pending_done_ids(
        file,
        &response_text,
        &state.pending_done_ids,
        &state.pending_kept_open_ids,
    )?;
    if missing.is_empty() {
        return Ok(());
    }

    if crate::session_check::resolve_auto_done(file)? {
        for id in &missing {
            auto_apply_pending_done_id(file, id)?;
        }
        crate::cycle_state::record_pending_done_ids(file, &missing)?;
        crate::cycle_state::mark_pending_mutations(file)?;
        eprintln!(
            "[finalize] auto_done: recorded {}",
            missing
                .iter()
                .map(|id| format!("--done {}", id))
                .collect::<Vec<_>>()
                .join(" ")
        );
        return Ok(());
    }

    let ids = missing
        .iter()
        .map(|id| format!("#{}", id))
        .collect::<Vec<_>>()
        .join(", ");
    let hint = missing
        .iter()
        .map(|id| format!("--done {}", id))
        .collect::<Vec<_>>()
        .join(" ");

    log_closeout_guard(
        file,
        crate::flow::types::FlowStage::PreCommitGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingDoneMissing,
    );
    anyhow::bail!(
        "[finalize] pre-commit gate: response appears to complete existing pending {} \
         but no matching `--done` was recorded this cycle\n\
         [finalize] hint: re-run finalize with {}, \
         add <!-- no-pending-done-guard --> to suppress, \
         or set pending_done_guard = \"warn\" to downgrade",
        ids,
        hint
    );
}

fn prewrite_pending_done_check(file: &Path, response_body: &str, flags: &WriteFlags) -> Result<()> {
    if !flags.strict_closeout {
        return Ok(());
    }

    let mode = crate::session_check::resolve_pending_done_guard_mode(file)?;
    if mode != crate::frontmatter::PendingCaptureGuardMode::Strict {
        return Ok(());
    }

    let state = crate::cycle_state::load(file)?;
    let mut recorded_done_ids = state
        .as_ref()
        .map(|state| state.pending_done_ids.clone())
        .unwrap_or_default();
    recorded_done_ids.extend(flags.pending_done_ids.clone());
    let mut kept_open_ids = state
        .as_ref()
        .map(|state| state.pending_kept_open_ids.clone())
        .unwrap_or_default();
    kept_open_ids.extend(flags.pending_kept_open_ids.clone());
    if response_body.contains("<!-- no-pending-done-guard -->") {
        return Ok(());
    }

    let response_text = crate::session_check::response_text_for_guards(response_body);
    let malformed = crate::session_check::malformed_tracked_item_refs(file, Some(&response_text))?;
    if !malformed.is_empty() {
        log_closeout_guard(
            file,
            crate::flow::types::FlowStage::PreWriteGuard,
            crate::flow::types::FlowOutcome::Blocked,
            crate::flow::closeout::CloseoutGuardReason::PendingDoneMalformedTrackedItem,
        );
        anyhow::bail!(
            "[finalize] pre-write gate: {}",
            crate::session_check::malformed_tracked_item_message(&malformed)
        );
    }
    let missing = crate::session_check::detect_missing_pending_done_ids(
        file,
        &response_text,
        &recorded_done_ids,
        &kept_open_ids,
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
        .map(|id| format!("--done {}", id))
        .collect::<Vec<_>>()
        .join(" ");
    let recovery = flags
        .rerun_command_base
        .as_ref()
        .map(|base| {
            format!(
                "\n[finalize] recovery: re-run the same response with {} {}",
                base, hint
            )
        })
        .unwrap_or_default();

    log_closeout_guard(
        file,
        crate::flow::types::FlowStage::PreWriteGuard,
        crate::flow::types::FlowOutcome::Blocked,
        crate::flow::closeout::CloseoutGuardReason::PendingDoneMissing,
    );
    anyhow::bail!(
        "[finalize] pre-write gate: response appears to complete existing pending {} \
         but no matching `--done` was recorded this cycle\n\
         [finalize] hint: re-run finalize with {}, \
         add <!-- no-pending-done-guard --> to suppress, \
         or set pending_done_guard = \"warn\" to downgrade{}",
        ids,
        hint,
        recovery
    );
}

fn auto_apply_pending_done_if_enabled(
    file: &Path,
    response_body: &str,
    flags: &WriteFlags,
    current_content: &mut String,
) -> Result<()> {
    if !flags.strict_closeout || !crate::session_check::resolve_auto_done(file)? {
        return Ok(());
    }
    if response_body.contains("<!-- no-pending-done-guard -->") {
        return Ok(());
    }

    let state = crate::cycle_state::load(file)?;
    let mut recorded_done_ids = state
        .as_ref()
        .map(|state| state.pending_done_ids.clone())
        .unwrap_or_default();
    recorded_done_ids.extend(flags.pending_done_ids.clone());
    let mut kept_open_ids = state
        .as_ref()
        .map(|state| state.pending_kept_open_ids.clone())
        .unwrap_or_default();
    kept_open_ids.extend(flags.pending_kept_open_ids.clone());

    let response_text = crate::session_check::response_text_for_guards(response_body);
    let missing = crate::session_check::detect_missing_pending_done_ids(
        file,
        &response_text,
        &recorded_done_ids,
        &kept_open_ids,
    )?;
    if missing.is_empty() {
        return Ok(());
    }

    for id in &missing {
        auto_apply_pending_done_id(file, id)?;
    }
    crate::cycle_state::record_pending_done_ids(file, &missing)?;
    crate::cycle_state::mark_pending_mutations(file)?;
    *current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {} after auto_done", file.display()))?;
    eprintln!(
        "[finalize] auto_done: recorded {}",
        missing
            .iter()
            .map(|id| format!("--done {}", id))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(())
}

fn auto_apply_pending_done_id(file: &Path, id: &str) -> Result<()> {
    if let Some(component) = crate::pending_cmd::open_item_component_name(file, id)?
        && crate::component::is_backlog_component(&component)
    {
        crate::pending_cmd::gate(file, id)?;
    }
    enforce_review_done_guard(file, id)?;
    crate::pending_cmd::done(file, id)
}

fn run_closeout_pending_maintenance(file: &Path, commit_mode: CommitMode) -> Result<()> {
    if commit_mode != CommitMode::Required {
        return Ok(());
    }
    crate::preflight::run_pending_maintenance(file).map(|_| ())
}

/// Consume the first queue prompt after a successful write cycle.
///
/// Called between the write step and the commit step so the consumption
/// is included in the same git commit as the response (atomic).
///
/// Reads frontmatter for `queue_active: true`; if the queue is not active
/// this is a no-op. On consumption, the first prompt is removed from both
/// the file and the snapshot. When the queue drains to empty, `auto` is
/// stripped and `queue_active` is cleared.
struct QueueConsumptionPlan {
    consumed_text: String,
    consumed_texts: Vec<String>,
    remaining: usize,
    drained: bool,
    auto: bool,
    new_document: String,
    new_snapshot: String,
    save_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueConsumptionOutcome {
    pub consumed_text: String,
    pub consumed_count: usize,
    pub remaining: usize,
    pub drained: bool,
    pub auto: bool,
}

#[allow(dead_code)]
pub fn consume_queue_prompt(file: &Path) -> Result<bool> {
    Ok(consume_queue_prompt_with_outcome(file)?.is_some())
}

pub fn consume_queue_prompt_with_outcome(file: &Path) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_outcome(file, &[], false)
}

pub fn consume_queue_prompts_for_done_ids_with_outcome(
    file: &Path,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_outcome(file, done_ids, false)
}

/// Strike the active queue head, **skipping the visible-write idle guard**, for
/// the repair recovery path (`#repair-strike-consumed-head`). Repair already
/// writes the recovered response straight to disk (bypassing IPC/IDE), so the
/// matching head strike must also bypass the guard — otherwise a live IDE buffer
/// would block the strike and leave the answered free-text head live for
/// preflight to re-present. Callers must scope this to heads the recovered
/// response actually answered.
pub fn consume_queue_prompt_force_disk(file: &Path) -> Result<Option<QueueConsumptionOutcome>> {
    consume_queue_prompts_with_outcome(file, &[], true)
}

fn consume_queue_prompts_with_outcome(
    file: &Path,
    done_ids: &[String],
    skip_visible_guard: bool,
) -> Result<Option<QueueConsumptionOutcome>> {
    // Hold the document lock for the entire read-parse-write cycle to prevent
    // concurrent edits from invalidating parsed offsets (TOCTOU fix).
    let _lock = acquire_doc_lock(file)?;
    let content =
        std::fs::read_to_string(file).context("queue consume: failed to read document")?;
    let Some(plan) = plan_queue_prompt_consumption(file, &content, done_ids)? else {
        return Ok(None);
    };

    if !skip_visible_guard {
        guard_visible_write_idle(file, "queue_consume")?;
    }
    atomic_write(file, &plan.new_document).context("queue consume: failed to write document")?;
    if plan.save_snapshot {
        snapshot::save(file, &plan.new_snapshot)?;
    }

    let outcome = QueueConsumptionOutcome {
        consumed_text: plan.consumed_text.clone(),
        consumed_count: plan.consumed_texts.len(),
        remaining: plan.remaining,
        drained: plan.drained,
        auto: plan.auto,
    };
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

    // #recguard-wedge-escape: a consumed head means the loop advanced, so reset
    // any owner-pane self-invocation wedge counter. Otherwise a future re-add of
    // the same head text could inherit a stale count and halt prematurely.
    if let Err(err) = crate::recguard_wedge::clear(file) {
        eprintln!(
            "[recguard-wedge] WARNING: failed to clear wedge counter for {}: {}",
            file.display(),
            err
        );
    }

    Ok(Some(outcome))
}

pub fn should_consume_queue_prompt_for_diff(file: &Path, diff_text: Option<&str>) -> Result<bool> {
    let content =
        std::fs::read_to_string(file).context("queue consume guard: failed to read document")?;
    should_consume_queue_prompt_for_diff_content(file, &content, diff_text)
}

fn should_consume_queue_prompt_for_write(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    done_ids: &[String],
) -> Result<bool> {
    // An explicit `--done` naming the queue head authorizes consumption
    // regardless of any pending mutations bundled into the same diff
    // (#pending-add-suppresses-queue-consume). Check it FIRST so a bundled
    // `--pending-add` cannot make the diff-based check below emit a misleading
    // "active prompt differs from queue head" diagnostic for a turn that does
    // in fact complete the head.
    if queue_head_matches_done_ids(current_content, done_ids)? {
        return Ok(true);
    }
    let Some(base) = baseline else {
        return Ok(false);
    };
    let base_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let diff_text = crate::diff::unified_diff_from_contents(&base_norm, &current_norm);
    should_consume_queue_prompt_for_diff_content(file, current_content, diff_text.as_deref())
}

pub(crate) fn queue_skip_diagnostic_for_file(file: &Path) -> Result<String> {
    let content =
        std::fs::read_to_string(file).context("queue skip diagnostic: failed to read document")?;
    queue_skip_diagnostic_for_content(&content)
}

fn queue_skip_diagnostic_for_content(content: &str) -> Result<String> {
    const GENERIC: &str =
        "[queue] skipped consumption because the active prompt did not target the queue head";

    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(GENERIC.to_string());
    };
    let queue_head_display = display_queue_prompt_text(&queue_head);
    if queue_head_is_free_text_prompt(content)? {
        return Ok(format!(
            "[queue] kept free-text head `{queue_head_display}` because free-text heads are consumed by an answering `### Re:` response, not a tracked id outcome. Confirm the response targets this head (heading/topic match) so the answered-response path can strike it; otherwise it stays queued."
        ));
    }
    if let Some(id) = queue_prompt_done_id(&queue_head) {
        return Ok(format!(
            "[queue] kept head `{queue_head_display}` because the response did not record a completion outcome for #{id}. Reap it with `--done {id}`, gate it with `--pending-gate {id}`, or keep/narrow it with `--pending-edit \"{id}=...\"`. (missing proof: no done/gate/reap recorded for #{id} this cycle)"
        ));
    }
    Ok(GENERIC.to_string())
}

fn should_consume_queue_prompt_for_diff_content(
    file: &Path,
    content: &str,
    diff_text: Option<&str>,
) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(true);
    };
    let Some(diff_text) = diff_text else {
        return Ok(false);
    };
    let prompt_changes: Vec<_> = crate::diff::classify_prompt_bearing_changes(diff_text)
        .into_iter()
        .filter(|change| {
            matches!(
                change.kind,
                crate::diff::PromptBearingChangeKind::PromptTarget
                    | crate::diff::PromptBearingChangeKind::ContentEdit
            )
        })
        .collect();
    if crate::diff::detect_queue_trigger(diff_text) {
        return Ok(true);
    }
    if prompt_changes
        .iter()
        .any(|change| queue_prompt_text_matches(&change.text, &queue_head))
    {
        return Ok(true);
    }

    // Not a user-facing failure on its own: the caller still has explicit
    // completion-signal fallbacks (`--done`/`--pending-gate`/`--pending-edit`,
    // synthetic-head heading match). Only the caller's final "skipped
    // consumption" line is the authoritative skip signal, so record this detail
    // to ops_log instead of stderr to avoid a false-alarm during a turn that
    // ultimately consumes the head (#pending-add-suppresses-queue-consume).
    crate::ops_log::log_op(
        file,
        &format!(
            "queue_diff_active_prompt_differs file={} prompt_changes={:?} queue_head={:?}",
            file.display(),
            prompt_changes
                .iter()
                .map(|change| change.text.as_str())
                .collect::<Vec<_>>(),
            queue_head
        ),
    );
    Ok(false)
}

/// True when this cycle's diff introduced a prompt-bearing exchange change (a
/// new or edited user prompt) that does NOT match the active queue head — i.e.
/// the response answered *foreign* exchange work. Used to keep a free-text queue
/// head queued when the cycle was driven by an unrelated new exchange prompt
/// rather than by draining the head (#queue-head-struck-on-foreign-exchange-answer).
///
/// A legitimate free-text-head drain has no such foreign prompt-bearing change
/// (the head itself was already in the baseline queue, and the only addition is
/// this cycle's `### Re:` response, which is not classified as a prompt), so this
/// returns false and the head is allowed to drain.
fn cycle_answered_foreign_exchange_prompt(
    baseline: Option<&str>,
    current_content: &str,
    queue_head: &str,
) -> bool {
    let Some(base) = baseline else {
        return false;
    };
    let base_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(base));
    let current_norm = crate::diff::strip_comments(&strip_boundary_for_dedup(current_content));
    let Some(diff_text) = crate::diff::unified_diff_from_contents(&base_norm, &current_norm) else {
        return false;
    };
    // A foreign exchange prompt is a user-prompt line (`❯ …`) genuinely NEW this
    // cycle whose text is not the queue head. The bug shape is a foreign prompt
    // that WAS answered this cycle, and `classify_prompt_bearing_changes`
    // suppresses prompts already answered by an adjacent response — so scan the
    // raw added lines for the canonical `❯` user-prompt marker instead of the
    // suppressed classifier.
    //
    // #free-text-head-consume-genuine-not-struck: the unified diff is computed
    // against the *normalized snapshot* baseline, but `current_content` is the
    // *live* working-tree/editor buffer. The buffer preserves `❯` prompt
    // prefixes on already-answered prompts that the snapshot normalized to the
    // bare form (CLAUDE.md "committed exchange-only prompt-prefix normalization
    // on already-answered prompts"). A pure `do x` → `❯ do x` prefix flip then
    // shows as an added `+❯ …` line and was wrongly read as a NEW foreign
    // prompt, blocking the free-text head strike and stalling the auto-loop. So
    // a `❯` added line counts as foreign only when its normalized text is absent
    // from the baseline entirely — a genuine new prompt, not a prefix flip on a
    // prompt that already existed (in either `❯ X` or bare `X` form) at baseline.
    let baseline_prompt_texts: std::collections::HashSet<String> = base_norm
        .lines()
        .map(|line| normalize_queue_prompt_text(line.trim().trim_start_matches('❯').trim()))
        .filter(|text| !text.is_empty())
        .collect();
    let debug = std::env::var("AGENT_DOC_DEBUG_QUEUE_CONSUME").is_ok();
    diff_text.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if added.starts_with("++") {
            return false; // unified-diff `+++` file header, not content
        }
        let Some(prompt) = added.trim().strip_prefix('❯') else {
            return false;
        };
        let prompt = prompt.trim();
        if prompt.is_empty() || queue_prompt_text_matches(prompt, queue_head) {
            return false;
        }
        // Skip prefix-normalization artifacts: the prompt text already existed in
        // the baseline (bare or `❯`-prefixed), so it is not new this cycle.
        if baseline_prompt_texts.contains(&normalize_queue_prompt_text(prompt)) {
            if debug {
                eprintln!(
                    "[queue-consume] ❯ added line is a prefix-flip on an existing baseline prompt, not foreign: {prompt:?}"
                );
            }
            return false;
        }
        if debug {
            eprintln!(
                "[queue-consume] foreign ❯ prompt added this cycle (blocks free-text head strike): {prompt:?} (head={queue_head:?})"
            );
        }
        true
    })
}

fn active_queue_head_text(content: &str) -> Result<Option<String>> {
    let (fm, _) = frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = component::parse(content)?;
    let comp = components
        .iter()
        .find(|component| component.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume guard: queue_active is true but document has no agent:queue component"
            )
        })?;
    let body = &content[comp.open_end..comp.close_start];
    let entries =
        crate::queue::parse(body).context("queue consume guard: failed to parse document queue")?;
    Ok(crate::queue::first_prompt(&entries).map(|prompt| prompt.text.clone()))
}

/// True when a closeout flag in this cycle explicitly names the active queue
/// head's `#id` — `--done`, `--pending-gate`, or `--pending-edit "<id>=…"`.
///
/// This is the explicit completion signal that authorizes queue-head consumption
/// (#queue-strike-on-halt). A `### Re:` heading that merely mentions the head id
/// is not a completion signal — a halt/refusal response names the head to explain
/// why it is *not* being completed — so consumption is driven by an explicit
/// closeout flag, never by heading text. `--pending-edit` counts because the
/// agent rewrote the item's tracked text as part of resolving it.
fn queue_head_has_explicit_completion_signal(
    content: &str,
    pending_done: &[String],
    pending_gate: &[String],
    pending_edit: &[String],
) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    // `--done`/`--pending-gate` entries are bare ids; `--pending-edit` entries are
    // `"<id>=new text"`, so take the id segment before `=`.
    let names_head = |raw: &str| {
        let id = raw.split_once('=').map(|(id, _)| id).unwrap_or(raw);
        normalize_done_id(id) == head_id
    };
    Ok(pending_done
        .iter()
        .chain(pending_gate.iter())
        .chain(pending_edit.iter())
        .any(|raw| names_head(raw)))
}

fn queue_head_matches_done_ids(content: &str, done_ids: &[String]) -> Result<bool> {
    if done_ids.is_empty() {
        return Ok(false);
    }
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    Ok(done_ids.iter().any(|id| normalize_done_id(id) == head_id))
}

fn queue_prompt_text_matches(prompt_change: &str, queue_head: &str) -> bool {
    normalize_queue_prompt_text(prompt_change) == normalize_queue_prompt_text(queue_head)
}

pub fn response_explicitly_targets_active_queue_head(file: &Path, response: &str) -> Result<bool> {
    let content =
        std::fs::read_to_string(file).context("queue consume guard: failed to read document")?;
    let Some(queue_head) = active_queue_head_text(&content)? else {
        return Ok(false);
    };
    Ok(response_explicitly_targets_queue_head(
        response,
        &queue_head,
    ))
}

fn response_explicitly_targets_queue_head(response: &str, queue_head: &str) -> bool {
    response
        .lines()
        .filter_map(response_heading_topic)
        .any(|topic| response_topic_matches_queue_head(topic, queue_head))
}

fn response_heading_topic(line: &str) -> Option<&str> {
    let trimmed = line.trim().trim_start_matches('❯').trim();
    let topic = trimmed.strip_prefix("### Re:")?.trim();
    Some(
        topic
            .split_once(" — ")
            .map(|(topic, _)| topic)
            .unwrap_or(topic)
            .trim(),
    )
}

fn response_topic_matches_queue_head(topic: &str, queue_head: &str) -> bool {
    // Used by the Codex Stop-hook auto-close path, which has no closeout CLI flags
    // to express completion explicitly. Two completion shapes count:
    //  1. An exact topic match (`### Re: do [#foo]` vs head `do [#foo]`).
    //  2. A topic that resolves to EXACTLY the head id (`### Re: #fix1` vs head
    //     `do #fix1`) — the Codex auto-loop titles a clean completion with the
    //     head's `#id` (#queue-head-consume-on-topic-id-regression).
    // A heading topic that merely contains the head id with trailing modifiers —
    // `### Re: #id halt`, `### Re: #id deferred` — must NOT count as completion
    // (#queue-strike-on-halt); `topic_resolves_to_exact_id` rejects those.
    if queue_prompt_text_matches(topic, queue_head) {
        return true;
    }
    queue_prompt_done_id(queue_head)
        .is_some_and(|head_id| topic_resolves_to_exact_id(topic, &head_id))
}

/// True when this cycle's captured response heading targets EXACTLY the active
/// queue head's id and that head is a *synthetic/preset* prompt rather than a
/// bare `do [#id]` directive (#queue-head-consume-on-topic-id-regression).
///
/// Synthetic queue prompts — a preset expansion or a natural-language prompt
/// carrying a trailing `#preset` id — are completed by the response itself, so a
/// `### Re: #<id>` heading that resolves to exactly that id is a genuine
/// completion signal. Bare `do [#id]` directives still require an explicit
/// closeout flag (#queue-strike-on-halt) because a halt/refusal response names
/// the head to explain why it is *not* being done. A heading topic that merely
/// contains the id with trailing modifiers — `#id halt`, `#id deferred` — never
/// counts, for either head shape.
fn response_targets_synthetic_queue_head_id(file: &Path, response: &str) -> Result<bool> {
    let content =
        std::fs::read_to_string(file).context("queue consume guard: failed to read document")?;
    let Some(queue_head) = active_queue_head_text(&content)? else {
        return Ok(false);
    };
    if queue_head_is_bare_do_directive(&queue_head) {
        return Ok(false);
    }
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    Ok(response
        .lines()
        .filter_map(response_heading_topic)
        .any(|topic| topic_resolves_to_exact_id(topic, &head_id)))
}

/// A queue head that is just a `do [#id]` / `do #id` directive — the `do` verb
/// plus the id (with optional bracket sugar) and nothing else. These follow the
/// strike-on-halt explicit-flag rule rather than heading-based consumption.
fn queue_head_is_bare_do_directive(queue_head: &str) -> bool {
    let norm = normalize_queue_prompt_text(queue_head);
    let Some(rest) = norm.strip_prefix("do ") else {
        return false;
    };
    matches!(
        rest.strip_prefix('#'),
        Some(id)
            if !id.is_empty()
                && id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    )
}

/// True when the active queue head is a free-text prompt: it carries no
/// extractable `#id` (so it is neither a `do [#id]` directive nor a `#preset`
/// head) and is not a `do queue` / `run queue` activation trigger. Such a prompt
/// has no `#id`-based completion mechanism — none of the explicit-flag or
/// heading-id consumption paths can ever strike it — so it is consumed by being
/// answered: a captured response body for the cycle completes it
/// (#free-text-queue-head-consume).
fn queue_head_is_free_text_prompt(content: &str) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    // #free-text-queue-owner-consume: a head is id-backed (NOT free text, so it
    // needs an explicit `--done`/`--pending-gate`/`--pending-edit` completion
    // signal) only when the ENTIRE head resolves to a single id directive —
    // `#id`, `[#id]`, or `do [#id]`. A free-text head that merely *mentions* a
    // `#id` in prose — e.g. `Approve [#shoptiers]. What are #next-steps?` — is
    // still free text and completes on being answered. The old `queue_prompt_done_id(..).is_some()`
    // test matched any `#id` mention and wrongly left such heads un-strikable,
    // hanging the auto-queue (they have no single id to `--done`).
    if let Some(id) = queue_prompt_done_id(&queue_head)
        && topic_resolves_to_exact_id(&queue_head, &id)
    {
        return Ok(false);
    }
    if crate::diff::detect_queue_trigger(&queue_head) {
        return Ok(false);
    }
    Ok(true)
}

/// Resolve whether this cycle's committed response should consume (strike) the
/// active queue head. Single source of truth for the strict-closeout decision so
/// alternate closeouts — notably the `run_stream` IPC-timeout `exit(75)` path,
/// which never returns to the `write_with_options` Phase 3c consume — advance the
/// queue identically and never leave an answered head queued to treadmill the
/// auto-loop on the next preflight (#queue-consume-on-stream-ipc-timeout).
///
/// Mirrors the layered signals: explicit `do queue` / prompt-target / `--done`
/// triggers, explicit `--done`/`--pending-gate`/`--pending-edit` completion of an
/// id-backed head, a response heading that resolves to a synthetic/preset head
/// id, and a free-text head answered by this cycle's response (unless the cycle
/// answered a foreign `agent:exchange` prompt instead).
pub(crate) fn queue_consumption_allowed_for_response(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response_body: &str,
    pending_done: &[String],
    pending_gate: &[String],
    pending_edit: &[String],
) -> Result<bool> {
    if should_consume_queue_prompt_for_write(file, baseline, current_content, pending_done)? {
        return Ok(true);
    }
    if queue_head_has_explicit_completion_signal(
        current_content,
        pending_done,
        pending_gate,
        pending_edit,
    )? {
        return Ok(true);
    }
    let has_response = !response_body.trim().is_empty();
    if has_response && response_targets_synthetic_queue_head_id(file, response_body)? {
        return Ok(true);
    }
    if has_response
        && queue_head_is_free_text_prompt(current_content)?
        && let Some(head_text) = active_queue_head_text(current_content)?
    {
        return Ok(!cycle_answered_foreign_exchange_prompt(
            baseline,
            current_content,
            &head_text,
        ));
    }
    Ok(false)
}

/// True when `topic` resolves to exactly `#<head_id>` (optionally `do `-prefixed
/// or `[#id]` bracketed) with no trailing modifiers. Case-insensitive; `head_id`
/// is already normalized lowercase by [`queue_prompt_done_id`].
fn topic_resolves_to_exact_id(topic: &str, head_id: &str) -> bool {
    let norm = topic.trim().trim_start_matches('❯').trim();
    let norm = norm.strip_prefix("do ").unwrap_or(norm).trim();
    let inner = norm
        .strip_prefix("[#")
        .and_then(|rest| rest.strip_suffix(']'))
        .or_else(|| norm.strip_prefix('#'));
    matches!(inner, Some(id) if id.eq_ignore_ascii_case(head_id))
}

fn queue_prompt_done_id(text: &str) -> Option<String> {
    let marker = text.find('#')?;
    let tail = &text[marker + 1..];
    let id = tail
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    if id.is_empty() {
        None
    } else {
        Some(id.to_ascii_lowercase())
    }
}

fn normalize_done_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('[')
        .trim_start_matches('#')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

fn first_n_queue_prompt_texts(entries: &[crate::queue::QueueEntry], count: usize) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            crate::queue::QueueEntry::Prompt(prompt) => Some(prompt.text.clone()),
            _ => None,
        })
        .take(count)
        .collect()
}

fn queue_consume_count_for_done_ids(
    entries: &[crate::queue::QueueEntry],
    done_ids: &[String],
) -> usize {
    if done_ids.is_empty() {
        return 0;
    }
    let done_ids = done_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<std::collections::HashSet<_>>();
    let mut count = 0usize;
    for entry in entries {
        let crate::queue::QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let Some(id) = queue_prompt_done_id(&prompt.text) else {
            break;
        };
        if done_ids.contains(&id) {
            count += 1;
            continue;
        }
        break;
    }
    count
}

fn normalize_queue_prompt_text(text: &str) -> String {
    display_queue_prompt_text(text).to_ascii_lowercase()
}

fn display_queue_prompt_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('❯')
                .trim()
                .replace("[#", "#")
                .replace(']', "")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// First non-empty, trimmed line of `text`, or `None` when blank.
fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|l| !l.is_empty())
}

/// Format consumed queue prompt(s) as a labeled blockquote echo so the response
/// block records the prompt it answered (#queue-prompt-echo-in-response).
///
/// `max_chars` is the opt-in `#queue-prompt-echo-summary` threshold: when
/// `Some(n)` and a prompt exceeds `n` characters, the echo records a bounded
/// summary (first line truncated + elided-char count + a pointer to the full
/// `agent:queue` text) instead of the verbatim prompt. `None` (default)
/// preserves the verbatim copy the user asked to keep "for now".
fn format_consumed_prompt_echo(consumed_texts: &[String], max_chars: Option<usize>) -> String {
    let mut out = String::from("> **Queue prompt:**\n>\n");
    let mut first_block = true;
    for text in consumed_texts {
        if text.trim().is_empty() {
            continue;
        }
        if !first_block {
            out.push_str(">\n");
        }
        first_block = false;
        let rendered = match max_chars {
            Some(limit) if text.chars().count() > limit => summarize_consumed_prompt(text, limit),
            _ => text.clone(),
        };
        for line in rendered.lines() {
            if line.trim().is_empty() {
                out.push_str(">\n");
            } else {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// `#queue-prompt-echo-summary`: a bounded one-line summary of a long consumed
/// queue prompt — its first non-empty line truncated to `limit` characters on a
/// char boundary, plus how many characters were elided and a pointer to the full
/// text preserved in `agent:queue`.
fn summarize_consumed_prompt(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    let first = first_nonempty_line(text).unwrap_or("").trim();
    let head: String = first.chars().take(limit).collect();
    let elided = total.saturating_sub(head.chars().count());
    format!("{head}… (+{elided} more chars; full prompt retained in agent:queue)")
}

fn line_is_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

/// Normalize a prompt line for "already present in exchange" comparison:
/// trim and strip a leading `❯` prompt marker.
fn normalize_prompt_line(line: &str) -> String {
    line.trim().trim_start_matches('❯').trim().to_string()
}

/// Locate, within `region` (the exchange content), the byte offset of the line
/// where this cycle's response heading begins. Prefers the captured response's
/// first line; falls back to the last non-code `### Re:` heading. `region_base`
/// is the absolute offset of `region` within the full document, used to skip
/// matches inside fenced code blocks.
fn locate_response_heading_offset(
    region: &str,
    region_base: usize,
    response_first_line: Option<&str>,
    code_ranges: &[(usize, usize)],
) -> Option<usize> {
    let in_code = |rel: usize| {
        let abs = region_base + rel;
        code_ranges.iter().any(|&(cs, ce)| abs >= cs && abs < ce)
    };

    if let Some(target) = response_first_line.map(str::trim).filter(|t| !t.is_empty()) {
        let mut offset = 0usize;
        for line in region.split_inclusive('\n') {
            if line.trim() == target && !in_code(offset) {
                return Some(offset);
            }
            offset += line.len();
        }
    }

    let mut offset = 0usize;
    let mut found = None;
    for line in region.split_inclusive('\n') {
        if line_is_response_heading(line.trim()) && !in_code(offset) {
            found = Some(offset);
        }
        offset += line.len();
    }
    found
}

/// Embed the consumed queue prompt echo immediately after this cycle's response
/// heading inside the `exchange` component. Returns `content` unchanged (fail-safe)
/// when the exchange/heading cannot be located, the prompt is empty, or the prompt
/// already appears in the exchange (e.g. a user typed it in directly).
fn embed_consumed_prompt_in_response(
    content: &str,
    consumed_texts: &[String],
    response_first_line: Option<&str>,
) -> String {
    if consumed_texts.iter().all(|t| t.trim().is_empty()) {
        return content.to_string();
    }
    let Ok(components) = component::parse(content) else {
        return content.to_string();
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };
    let region = &content[exchange.open_end..exchange.close_start];

    // Idempotency / manual-turn dedup: if the prompt's first line already appears
    // as an exchange line (user typed it, or a prior echo exists), skip injection.
    // #queue-prompt-echo-summary: the opt-in length threshold is read from the
    // document's own frontmatter (default None = verbatim copy).
    let max_chars = frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.queue_prompt_echo_max_chars);
    let echo = format_consumed_prompt_echo(consumed_texts, max_chars);
    if region.contains(echo.trim_end()) {
        return content.to_string();
    }
    let already_present = consumed_texts
        .iter()
        .filter_map(|t| first_nonempty_line(t))
        .any(|first| {
            let needle = normalize_prompt_line(first);
            !needle.is_empty() && region.lines().any(|l| normalize_prompt_line(l) == needle)
        });
    if already_present {
        return content.to_string();
    }

    let code_ranges = component::find_code_ranges(content);
    let Some(heading_rel) = locate_response_heading_offset(
        region,
        exchange.open_end,
        response_first_line,
        &code_ranges,
    ) else {
        return content.to_string();
    };
    let Some(nl) = region[heading_rel..].find('\n') else {
        return content.to_string();
    };
    let insert_abs = exchange.open_end + heading_rel + nl + 1;

    let mut result = String::with_capacity(content.len() + echo.len() + 2);
    result.push_str(&content[..insert_abs]);
    result.push('\n');
    result.push_str(&echo);
    result.push('\n');
    result.push_str(&content[insert_abs..]);
    result
}

fn plan_queue_prompt_consumption(
    file: &Path,
    content: &str,
    done_ids: &[String],
) -> Result<Option<QueueConsumptionPlan>> {
    let (fm, _) = frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }

    let components = component::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume: queue_active is true but document has no agent:queue component"
            )
        })?;

    let body = &content[comp.open_end..comp.close_start];
    let entries =
        crate::queue::parse(body).context("queue consume: failed to parse document queue")?;

    let consume_count = queue_consume_count_for_done_ids(&entries, done_ids).max(1);
    let consumed_texts = first_n_queue_prompt_texts(&entries, consume_count);
    let consumed_text = consumed_texts.first().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "queue consume: queue_active is true but document queue has no prompt to consume"
        )
    })?;

    let has_auto = crate::queue::has_auto_attr(&comp.attrs);
    let completed_entries = crate::queue::mark_first_n_prompts_completed(&entries, consume_count);
    let remaining = crate::queue::prompts(&completed_entries).len();
    let drained = remaining == 0;
    let new_entries = if drained {
        Vec::new()
    } else {
        completed_entries
    };
    let new_body = crate::queue::render(&new_entries);
    let mut current = comp.replace_content(content, &new_body);

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
        current = frontmatter::merge_queue_state(&current, false)?;
    }

    // Update snapshot in sync. Required closeouts must be able to prove the
    // same head prompt was removed from both the file and the snapshot.
    let snap = snapshot::load(file)?.ok_or_else(|| {
        anyhow::anyhow!("queue consume: queue_active is true but snapshot is missing")
    })?;
    let snap_comps = component::parse(&snap)?;
    let snap_queue = snap_comps
        .iter()
        .find(|c| c.name == "queue")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "queue consume: queue_active is true but snapshot has no agent:queue component"
            )
        })?;
    let snap_body = &snap[snap_queue.open_end..snap_queue.close_start];
    let snap_entries =
        crate::queue::parse(snap_body).context("queue consume: failed to parse snapshot queue")?;
    let snap_has_auto = crate::queue::has_auto_attr(&snap_queue.attrs);
    let snapshot_consumed_texts = first_n_queue_prompt_texts(&snap_entries, consume_count);
    if snapshot_consumed_texts.len() != consumed_texts.len() {
        anyhow::bail!(
            "queue consume: snapshot has {} prompt(s) available but document consumed {}",
            snapshot_consumed_texts.len(),
            consumed_texts.len()
        );
    }
    if snapshot_consumed_texts != consumed_texts {
        anyhow::bail!(
            "queue consume: snapshot head prompts {:?} do not match document head prompts {:?}",
            snapshot_consumed_texts,
            consumed_texts
        );
    }
    let snap_completed_entries =
        crate::queue::mark_first_n_prompts_completed(&snap_entries, consume_count);
    let snap_remaining = crate::queue::prompts(&snap_completed_entries).len();
    let snap_new_entries = if snap_remaining == 0 {
        Vec::new()
    } else {
        snap_completed_entries
    };
    if snap_new_entries != new_entries {
        // #finalize-divergence-orphans-committed-head / IPC-CRDT resilience: the
        // document `content` here is the post-CRDT-merge result — the merge has
        // already reconciled the agent (snapshot) side against concurrent
        // user/editor edits on the disk side. The same-head proof above
        // (`snapshot_consumed_texts == consumed_texts`) already confirmed we
        // consumed the right head; this remaining-queue difference is exactly the
        // concurrent edit the CRDT merge resolved. Hard-bailing here re-rejected
        // the merge the pipeline just succeeded at, leaving an orphaned unstruck
        // head that re-serves (the divergence error hit repeatedly under live
        // editor races). Reconcile instead: the merged document queue is
        // authoritative, and the snapshot below adopts the document's `new_body`,
        // so both sides converge on the head-struck merged state. Record the
        // reconciliation for forensics rather than failing the cycle.
        let snap_remaining_prompts = crate::queue::prompts(&snap_new_entries).len();
        let doc_remaining_prompts = crate::queue::prompts(&new_entries).len();
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_consume_divergence_reconciled file={} reason=crdt_merge_authoritative consumed={} snap_remaining={} doc_remaining={}",
                file.display(),
                consume_count,
                snap_remaining_prompts,
                doc_remaining_prompts
            ),
        );
    }

    let mut new_snap = snap_queue.replace_content(&snap, &new_body);
    if drained {
        if snap_has_auto
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
        new_snap = frontmatter::merge_queue_state(&new_snap, false)?;
    }

    // #queue-prompt-echo-in-response: an auto/synthetic queue head is never typed
    // into `agent:exchange`, so a consumed queue turn would otherwise record only
    // the `### Re:` answer with no trace of the originating prompt. Embed the
    // consumed prompt text into this cycle's response block (in BOTH the document
    // and the snapshot, so the selective-commit boundary stays consistent) when
    // the prompt is not already present in the exchange. Fail-safe: any locator
    // miss leaves the content unchanged rather than risk corrupting the exchange.
    let response_first_line = crate::capture::load_active(file)
        .ok()
        .flatten()
        .and_then(|c| first_nonempty_line(&c.response_body).map(str::to_string));
    current = embed_consumed_prompt_in_response(
        &current,
        &consumed_texts,
        response_first_line.as_deref(),
    );
    new_snap = embed_consumed_prompt_in_response(
        &new_snap,
        &consumed_texts,
        response_first_line.as_deref(),
    );

    if new_snap != snap {
        return Ok(Some(QueueConsumptionPlan {
            consumed_text,
            consumed_texts,
            remaining,
            drained,
            auto: has_auto,
            new_document: current,
            new_snapshot: new_snap,
            save_snapshot: true,
        }));
    }

    Ok(Some(QueueConsumptionPlan {
        consumed_text,
        consumed_texts,
        remaining,
        drained,
        auto: has_auto,
        new_document: current,
        new_snapshot: new_snap,
        save_snapshot: false,
    }))
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

pub struct NormalizedTemplateResponse {
    pub response_for_capture: Option<String>,
    pub patches: Vec<template::PatchBlock>,
    pub unmatched: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TemplateResponseWriteProof {
    explicit_components: Vec<String>,
    unmatched_len: usize,
}

impl TemplateResponseWriteProof {
    fn has_real_body(&self) -> bool {
        !self.explicit_components.is_empty() || self.unmatched_len > 0
    }
}

fn template_response_write_proof(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> TemplateResponseWriteProof {
    TemplateResponseWriteProof {
        explicit_components: patches
            .iter()
            .filter(|patch| patch.name != "frontmatter")
            .filter(|patch| !is_backlog_component(&patch.name))
            .filter(|patch| !crate::component::is_review_component(&patch.name))
            .filter(|patch| !patch.content.trim().is_empty())
            .map(|patch| patch.name.clone())
            .collect(),
        unmatched_len: unmatched.trim().len(),
    }
}

pub fn ensure_template_response_write_proof(
    patches: &[template::PatchBlock],
    unmatched: &str,
) -> Result<()> {
    let proof = template_response_write_proof(patches, unmatched);
    if proof.has_real_body() {
        return Ok(());
    }

    anyhow::bail!(
        "template response contains no real response-body write — include at least one non-empty response patch or non-empty unmatched response body"
    );
}

fn pending_replace_escape_hatch_enabled() -> bool {
    std::env::var("AGENT_DOC_ALLOW_REPLACE_PENDING")
        .map(|v| v == "1")
        .unwrap_or(false)
        || std::env::var("AGENT_DOC_ALLOW_PATCH_PENDING")
            .map(|v| v == "1")
            .unwrap_or(false)
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
            attrs.sort_by_key(|(left, _)| *left);
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

fn response_materialization_probe(patches: &[template::PatchBlock], unmatched: &str) -> String {
    let mut selected = patches
        .iter()
        .filter(|patch| patch.name == "exchange")
        .cloned()
        .collect::<Vec<_>>();
    let selected_exchange = !selected.is_empty();
    if selected.is_empty() && unmatched.trim().is_empty() {
        selected = patches
            .iter()
            .filter(|patch| patch.name != "frontmatter")
            .filter(|patch| !is_backlog_component(&patch.name))
            .filter(|patch| !crate::component::is_review_component(&patch.name))
            .cloned()
            .collect();
    }
    let probe_unmatched = if selected_exchange { "" } else { unmatched };
    materialized_template_response(&selected, probe_unmatched)
}

fn materialized_template_response(patches: &[template::PatchBlock], unmatched: &str) -> String {
    let mut out = String::new();
    for patch in patches {
        push_materialization_segment(&mut out, &patch.content);
    }
    push_materialization_segment(&mut out, unmatched);
    out
}

fn push_materialization_segment(out: &mut String, segment: &str) {
    let segment = segment.trim_matches(|c| c == '\n' || c == '\r');
    if segment.trim().is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(segment);
    out.push('\n');
}

pub fn response_materialization_probe_from_response(response: &str) -> String {
    match template::parse_patches(response) {
        Ok((patches, unmatched)) => response_materialization_probe(&patches, &unmatched),
        Err(_) => response.to_string(),
    }
}

pub fn response_materialized_in_content(response: &str, content: &str) -> bool {
    let probe = response_materialization_probe_from_response(response);
    probe.trim().is_empty()
        || crate::repair::response_already_applied(content, &probe)
        || crate::repair::response_already_applied_after_prefix_strip(content, &probe)
}

fn reject_marker_response_with_zero_patches(marker_count: usize, patch_count: usize) -> Result<()> {
    if patch_count == 0 && marker_count > 0 {
        anyhow::bail!(
            "template patchback parsed zero patches despite {marker_count} patch marker(s); refusing to capture a malformed response"
        );
    }
    Ok(())
}

fn ipc_response_materialized_or_fallback(
    file: &Path,
    source: &str,
    response: &str,
    content: &str,
) -> bool {
    if response_materialized_in_content(response, content) {
        return true;
    }
    let response_hash = crate::ops_log::content_hash(response);
    let content_hash = crate::ops_log::content_hash(content);
    eprintln!(
        "[write] IPC {} consumed a patch for {}, but the materialized content is missing the captured response body — falling back before snapshot/commit",
        source,
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_materialization_missing_response file={} source={} response_sha256={} content_len={} content_hash={}",
            file.display(),
            source,
            response_hash,
            content.len(),
            content_hash
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        None,
        "missing_response_probe",
        "direct_write_fallback",
        &format!(
            "response_sha256={} content_len={} content_hash={}",
            response_hash,
            content.len(),
            content_hash
        ),
    );
    false
}

fn log_ipc_proof_failure(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    invariant: &str,
    recovery: &str,
    detail: &str,
) {
    eprintln!(
        "[write] IPC proof insufficient for {}: source={} patch_id={} invariant={} recovery={}{}{}",
        file.display(),
        source,
        patch_id.unwrap_or("-"),
        invariant,
        recovery,
        if detail.is_empty() { "" } else { " " },
        detail
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_proof_insufficient file={} source={} patch_id={} invariant={} recovery={}{}{}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            invariant,
            recovery,
            if detail.is_empty() { "" } else { " " },
            detail
        ),
    );
}

fn strip_partial_response_materialization_from_exchange(
    content: &str,
    response: &str,
) -> Option<String> {
    if response_materialized_in_content(response, content) {
        return None;
    }
    let headings = response
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### Re:"))
        .collect::<Vec<_>>();
    if headings.is_empty() {
        return None;
    }

    let components = component::parse(content).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let exchange_body = &content[exchange.open_end..exchange.close_start];
    let mut repaired_exchange = String::with_capacity(exchange_body.len());
    let mut removed = false;
    let mut skipping_partial = false;

    for segment in exchange_body.split_inclusive('\n') {
        let line = segment.trim_end_matches('\n');
        let trimmed = line.trim();
        let is_target_response_heading = headings.contains(&trimmed);
        let is_structural_boundary = trimmed.starts_with("<!-- agent:boundary:")
            || trimmed.starts_with("<!-- /agent:")
            || trimmed.starts_with("<!-- agent:");
        let is_other_response_heading =
            trimmed.starts_with("### Re:") && !is_target_response_heading;
        let is_user_prompt_line = trimmed.starts_with('❯');

        if skipping_partial
            && (is_structural_boundary || is_other_response_heading || is_user_prompt_line)
        {
            skipping_partial = false;
        }

        if is_target_response_heading {
            skipping_partial = true;
            removed = true;
            continue;
        }

        if skipping_partial {
            removed = true;
            continue;
        }

        repaired_exchange.push_str(segment);
    }

    if !removed {
        return None;
    }

    let mut repaired = String::with_capacity(content.len());
    repaired.push_str(&content[..exchange.open_end]);
    repaired.push_str(&repaired_exchange);
    repaired.push_str(&content[exchange.close_start..]);
    Some(repaired)
}

fn repair_partial_response_materialization_before_fallback(
    file: &Path,
    source: &str,
    response: &str,
) -> Result<()> {
    let Ok(current) = std::fs::read_to_string(file) else {
        return Ok(());
    };
    let Some(repaired) = strip_partial_response_materialization_from_exchange(&current, response)
    else {
        return Ok(());
    };
    eprintln!(
        "[write] IPC {} partial response materialization removed before fallback for {}",
        source,
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_partial_materialization_removed file={} source={} response_sha256={} before_len={} after_len={}",
            file.display(),
            source,
            crate::ops_log::content_hash(response),
            current.len(),
            repaired.len()
        ),
    );
    atomic_write(file, &repaired)?;
    Ok(())
}

fn response_materialization_probe_from_ipc_payload(payload: &serde_json::Value) -> String {
    let patches = payload
        .get("patches")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item
                        .get("component")
                        .or_else(|| item.get("name"))
                        .and_then(|value| value.as_str())?;
                    let content = item.get("content").and_then(|value| value.as_str())?;
                    Some(template::PatchBlock::new(name, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unmatched = payload
        .get("unmatched")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    response_materialization_probe(&patches, unmatched)
}

pub fn normalize_backlog_patch_response(
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
    let (_, current_items, _) = crate::pending::parse_items(current_body);
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
    let (mut target_body, _) =
        crate::pending::backfill(&patches[backlog_index].content, &doc_id, &current_ids);
    if !crate::pending::preserves_non_item_structure(current_body, &target_body) {
        if let Some(merged_body) =
            crate::pending::merge_partial_backlog_prefix(current_body, &target_body)
        {
            target_body = merged_body;
        } else {
            anyhow::bail!(
                "ERR: pending/backlog patch changed non-list content — use granular --pending-* flags instead"
            );
        }
    }
    let (_, target_items, _) = crate::pending::parse_items(&target_body);
    let rendered_target = crate::pending::canonicalize_preserving_non_item_lines(&target_body);
    if !same_ignoring_trailing_newlines(&rendered_target, &target_body) {
        anyhow::bail!(
            "ERR: pending/backlog patch could not be normalized into supported --pending-* operations"
        );
    }

    if !same_ignoring_trailing_newlines(current_body, &target_body) {
        let normalized_body = target_body.clone();
        let mut saw_pending_add = false;
        let mut pending_done_ids = Vec::new();

        for item in &target_items {
            crate::pending::ensure_no_new_leading_custom_id_prefix(
                &item.id,
                &item.text,
                &current_ids,
                "ERR: pending/backlog patch",
            )?;
            if !current_ids.contains(&item.id) {
                saw_pending_add = true;
            }
            if item.state == crate::pending::PendingState::Done
                && current_states.get(&item.id).copied() != Some(crate::pending::PendingState::Done)
            {
                pending_done_ids.push(item.id.clone());
            }
        }

        let rewritten_doc = backlog_component.replace_content(current_content, &normalized_body);
        guard_visible_write_idle(file, "normalize_pending_patch")?;
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

pub fn canonicalize_response_for_capture(file: &Path, response: &str) -> Result<String> {
    if !response.contains("<!-- patch:") {
        return Ok(response.to_string());
    }

    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for response capture", file.display()))?;
    let Ok((fm, _)) = frontmatter::parse(&current_content) else {
        return Ok(response.to_string());
    };
    if !fm.resolve_mode().is_template() {
        return Ok(response.to_string());
    }

    let Ok((mut patches, mut unmatched)) = template::parse_patches(response) else {
        return Ok(response.to_string());
    };
    if !patches
        .iter()
        .any(|patch| is_backlog_component(&patch.name))
    {
        return Ok(response.to_string());
    }

    sanitize_patches(&mut patches);
    sanitize_unmatched(&mut unmatched);
    let normalized =
        normalize_backlog_patch_response(file, &current_content, patches, unmatched, false)?;
    Ok(normalized
        .response_for_capture
        .unwrap_or_else(|| response.to_string()))
}

fn sanitize_template_patchback_response_for_write(response: &mut String) -> Result<()> {
    let Ok((patches, unmatched)) = template::parse_patches(response) else {
        return Ok(());
    };
    if unmatched.trim().is_empty() || !patches.iter().any(|patch| patch.name == "exchange") {
        return Ok(());
    }

    match crate::replay_guard::classify_replay_payload(response) {
        crate::replay_guard::ReplayPayloadClassification::Replayable(payload) => {
            let sanitized = payload.into_owned();
            if sanitized != response.trim() {
                *response = sanitized;
            }
            Ok(())
        }
        crate::replay_guard::ReplayPayloadClassification::Empty => {
            anyhow::bail!("empty response — nothing to write")
        }
        crate::replay_guard::ReplayPayloadClassification::Blocked(reason) => {
            anyhow::bail!(
                "template response contains unsafe unmatched content around patch blocks: {reason}"
            )
        }
    }
}

fn patchback_marker_count_outside_code(response: &str) -> usize {
    crate::flow::document_mutation::patchback_marker_count_outside_code(response)
}

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
    let mut targets = Vec::new();

    for (before_line, after_line) in before_exc.lines().zip(after_exc.lines()) {
        if let Some(stripped) = after_line.strip_prefix("❯ ") {
            // after has ❯ prefix; before must have the plain version at the same position
            if before_line == stripped {
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

    let diff_text = crate::diff::unified_diff_from_contents(&snap_stripped, &baseline_stripped);
    let prompt_prefix_targets = diff_text
        .as_deref()
        .map(crate::diff::prompt_prefix_normalization_targets)
        .unwrap_or_default();

    let diff = TextDiff::from_lines(snap_stripped.as_str(), baseline_stripped.as_str());
    let mut user_added = std::collections::HashSet::<String>::new();
    let mut agent_inserted = std::collections::HashSet::<String>::new();
    let mut in_baseline_fence = false;
    let mut baseline_fence_char = '`';
    let mut baseline_fence_len = 3usize;
    let mut in_agent_block = false;
    let mut saw_deleted_heading = false;
    // `#repair-orphan-prefix-bug`: track whether the scanner is inside an
    // assistant `### Re:` block and whether that block had a body line deleted.
    // A body REPLACEMENT (delete + insert) under an Equal heading is assistant
    // content; a pure append after an unchanged response stays a user prompt.
    let mut in_re_block = false;
    let mut re_block_saw_body_delete = false;
    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches('\n');
        let trimmed = line.trim();
        let is_heading = heading_level(trimmed).is_some();
        // Equal and Insert lines are present in baseline — track their fence state.
        // Capture pre-update state to correctly detect closing delimiters as fence markers.
        let was_in_fence = in_baseline_fence;
        if change.tag() == ChangeTag::Delete {
            saw_deleted_heading = !in_baseline_fence && is_heading;
            if in_re_block
                && !in_baseline_fence
                && !is_heading
                && !trimmed.is_empty()
                && !trimmed.starts_with("<!--")
                && fence_open(trimmed).is_none()
            {
                re_block_saw_body_delete = true;
            }
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
                    // Track whether we are inside an assistant `### Re:` block so
                    // that a body REPLACEMENT under an already-present (Equal)
                    // heading is recognized as assistant content rather than a
                    // user prompt (#repair-orphan-prefix-bug).
                    in_re_block = trimmed.starts_with("### Re:");
                    re_block_saw_body_delete = false;
                } else if in_agent_block && trimmed.is_empty() {
                    // Blank assistant-response lines do not prove the following
                    // prose is user input. Only explicit prompt-run starts below
                    // can return the scanner to user-owned transcript lines.
                } else if in_agent_block
                    && (starts_targeted_prompt_repair_after_response(trimmed, true)
                        || trimmed.starts_with('❯')
                        || trimmed.starts_with("<!--"))
                {
                    in_agent_block = false;
                }
                // A prompt-run start (or explicit `❯`) after the response body
                // returns the scanner to user-owned transcript lines, ending the
                // `### Re:` replacement window.
                if in_re_block
                    && (starts_targeted_prompt_repair_after_response(trimmed, true)
                        || trimmed.starts_with('❯'))
                {
                    in_re_block = false;
                    re_block_saw_body_delete = false;
                }
            }
        }
        // A line is a fence delimiter if it opens a fence (fence_open), or closes the current
        // one (was_in_fence before update, and matches close pattern).
        let is_fence_delim = fence_open(trimmed).is_some()
            || (was_in_fence && fence_close(trimmed, baseline_fence_char, baseline_fence_len));
        // Insert body lines that replace deleted body under an existing `### Re:`
        // heading are assistant content (#repair-orphan-prefix-bug), not prompts.
        let is_re_block_replacement = in_re_block && re_block_saw_body_delete;
        if change.tag() == ChangeTag::Insert
            && !in_baseline_fence
            && !in_agent_block
            && !is_re_block_replacement
            && !heading_replaces_deleted_heading
            && !trimmed.is_empty()
            && !trimmed.starts_with('❯')
            && !trimmed.starts_with("<!--")
            && !is_fence_delim
        {
            user_added.insert(line.to_string());
        } else if change.tag() == ChangeTag::Insert && (in_agent_block || is_re_block_replacement) {
            agent_inserted.insert(line.to_string());
        }
    }

    for line in prompt_prefix_targets {
        if !agent_inserted.contains(&line) {
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

fn preserve_head_exchange_prompt_prefix_state(content: &str, head: &str) -> String {
    let Ok(head_components) = component::parse(head) else {
        return content.to_string();
    };
    let Some(head_exchange) = head_components.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };
    let mut head_unprefixed = HashMap::<String, usize>::new();
    let mut head_prefixed = HashMap::<String, usize>::new();
    for line in head_exchange.content(head).lines() {
        let line = line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('❯')
            || trimmed.starts_with("<!--")
            || is_exchange_response_heading_for_prefix_repair(trimmed)
        {
            continue;
        }
        *head_unprefixed.entry(line.to_string()).or_default() += 1;
    }
    for line in exchange_prompt_prefix_eligible_lines(head_exchange.content(head), None) {
        let line = line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if let Some(stripped) = line.strip_prefix("❯ ") {
            *head_prefixed.entry(stripped.to_string()).or_default() += 1;
        }
    }
    if head_unprefixed.is_empty() && head_prefixed.is_empty() {
        return content.to_string();
    }

    let Ok(content_components) = component::parse(content) else {
        return content.to_string();
    };
    let Some(exchange) = content_components.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };
    let exchange_content = exchange.content(content);
    let mut changed = false;
    let mut rebuilt = String::with_capacity(exchange_content.len());
    let target_counts =
        normalization_target_counts(&head_prefixed.keys().cloned().collect::<Vec<String>>());
    let mut in_response_block = false;
    for segment in exchange_content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        let trimmed = line.trim();
        if trimmed.starts_with("<!-- agent:boundary:") {
            in_response_block = false;
        } else if is_exchange_response_heading_for_prefix_repair(trimmed) {
            in_response_block = true;
        }
        let is_target = target_counts
            .get(line.trim_end())
            .copied()
            .unwrap_or_default()
            > 0;
        let eligible = if in_response_block {
            starts_prompt_run_after_response(trimmed, is_target)
        } else {
            true
        };
        if let Some(unprefixed) = line.strip_prefix("❯ ")
            && let Some(remaining) = head_unprefixed.get_mut(unprefixed)
            && *remaining > 0
        {
            rebuilt.push_str(unprefixed);
            *remaining -= 1;
            changed = true;
        } else if eligible
            && !line.starts_with("❯ ")
            && let Some(remaining) = head_prefixed.get_mut(line)
            && *remaining > 0
        {
            rebuilt.push_str("❯ ");
            rebuilt.push_str(line);
            *remaining -= 1;
            changed = true;
        } else {
            rebuilt.push_str(line);
        }
        if in_response_block && eligible && starts_prompt_run_after_response(trimmed, is_target) {
            in_response_block = false;
        }
        rebuilt.push_str(newline);
    }
    if !changed {
        return content.to_string();
    }
    exchange.replace_content(content, &rebuilt)
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

pub fn enforce_imperative_response_contract(
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

pub fn enforce_imperative_response_contract_for_diff(
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
/// 3. **No broad recovery side effects** — on overrun, the content passes
///    through unchanged. The caller's typed repair/closeout path remains
///    responsible for deciding whether disk, snapshot, or editor state changes.
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
    let mut normalized = normalize_user_prompts_in_exchange(content, baseline, snapshot);
    if normalized != content
        && let Ok(Some(head)) = crate::git::show_head(file)
    {
        let preserved = preserve_head_exchange_prompt_prefix_state(&normalized, &head);
        if preserved != normalized {
            crate::ops_log::log_op(
                file,
                &format!(
                    "normalize_preserved_head_prompt_prefix_state file={}",
                    file.display()
                ),
            );
            normalized = preserved;
        }
    }

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
             suspected snapshot/baseline divergence. Skipping ❯ prefix application this cycle.",
            applied,
            MAX_NORMALIZE_USER_LINES,
            file.display()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "normalize_threshold_exceeded applied={} threshold={} action=passthrough",
                applied, MAX_NORMALIZE_USER_LINES
            ),
        );
        return content.to_string();
    }

    normalized
}

/// Verify that the sidecar content preserved the expected `❯ ` prefixes.
///
/// For each non-blank target in `normalize_prefix_lines`, checks whether the
/// exchange user region contains the required number of `❯ <target>`
/// occurrences. Duplicate targets must be preserved by occurrence, not just by
/// set membership, because prompt presets often repeat verbatim across turns.
/// Returns `true` when all expected prefixes are present (or when there are no
/// targets to check).
pub fn verify_sidecar_normalization(sidecar: &str, normalize_prefix_lines: &[String]) -> bool {
    if normalize_prefix_lines.is_empty() {
        return true;
    }

    let sidecar_exchange = component::parse(sidecar)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(sidecar).to_string())
        })
        .unwrap_or_else(|| sidecar.to_string());
    let target_counts = normalization_target_counts(normalize_prefix_lines);

    let mut prefixed_counts = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(&sidecar_exchange, Some(&target_counts)) {
        let trimmed = line.trim_end();
        if let Some(stripped) = trimmed.strip_prefix("❯ ") {
            *prefixed_counts.entry(stripped.to_string()).or_default() += 1;
        }
    }

    for (target, required) in target_counts {
        if prefixed_counts.get(&target).copied().unwrap_or(0) < required {
            return false;
        }
    }
    true
}

fn exchange_user_region(content: &str) -> &str {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut boundary_pos = content.len();
    let mut offset = 0;
    for line in content.lines() {
        if line.trim().starts_with(boundary_prefix) {
            boundary_pos = offset;
        }
        offset += line.len() + 1;
    }
    &content[..boundary_pos]
}

fn is_exchange_response_heading_for_prefix_repair(trimmed: &str) -> bool {
    let trimmed = trimmed.strip_prefix("❯ ").unwrap_or(trimmed);
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

fn is_prefixed_exchange_response_heading_for_prefix_repair(trimmed: &str) -> bool {
    let Some(stripped) = trimmed.strip_prefix("❯ ") else {
        return false;
    };
    is_exchange_response_heading_for_prefix_repair(stripped)
}

fn normalization_target_matches_line(
    line: &str,
    target_counts: &std::collections::HashMap<String, usize>,
) -> bool {
    let normalized = line.trim_end();
    target_counts.contains_key(normalized)
        || normalized
            .strip_prefix("❯ ")
            .is_some_and(|stripped| target_counts.contains_key(stripped))
}

fn starts_prompt_run_after_response(trimmed: &str, is_target: bool) -> bool {
    crate::diff::line_looks_like_prompt_prefix_repair_start(trimmed, is_target)
}

fn starts_targeted_prompt_repair_after_response(trimmed: &str, is_target: bool) -> bool {
    crate::diff::line_looks_like_targeted_prompt_prefix_repair_start(trimmed, is_target)
}

fn starts_targeted_or_prefixed_prompt_repair_after_response(
    trimmed: &str,
    is_target: bool,
) -> bool {
    starts_targeted_prompt_repair_after_response(
        trimmed,
        is_target || trimmed.trim_start().starts_with('❯'),
    )
}

fn exchange_prompt_prefix_eligible_lines<'a>(
    content: &'a str,
    target_counts: Option<&std::collections::HashMap<String, usize>>,
) -> Vec<&'a str> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut eligible = Vec::new();
    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;

    for line in exchange_user_region(content).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(boundary_prefix) {
            in_response_block = false;
            response_heading_was_prefixed = false;
            continue;
        }
        if is_exchange_response_heading_for_prefix_repair(trimmed) {
            in_response_block = true;
            response_heading_was_prefixed =
                is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
            continue;
        }
        if crate::diff::line_looks_like_markdown_list_item(trimmed) {
            continue;
        }

        let is_target =
            target_counts.is_some_and(|counts| normalization_target_matches_line(line, counts));
        if in_response_block {
            let starts_prompt = if target_counts.is_some() {
                starts_targeted_or_prefixed_prompt_repair_after_response(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                )
            } else {
                starts_prompt_run_after_response(trimmed, false)
            };
            if starts_prompt {
                in_response_block = false;
                response_heading_was_prefixed = false;
            } else {
                continue;
            }
        }

        eligible.push(line);
    }

    eligible
}

/// Compare the committed/snapshot document against the working tree and return
/// exchange user-region lines that should regain a missing `❯ ` prefix.
pub fn extract_post_commit_normalization_targets(committed: &str, working: &str) -> Vec<String> {
    let committed_comps = component::parse(committed).unwrap_or_default();
    let working_comps = component::parse(working).unwrap_or_default();

    let committed_exc = committed_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(committed))
        .unwrap_or("");
    let working_exc = working_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(working))
        .unwrap_or("");

    if committed_exc == working_exc {
        return vec![];
    }

    let mut working_prefixed = std::collections::HashMap::<String, usize>::new();
    let mut working_unprefixed = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(working_exc, None) {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("❯ ") {
            *working_prefixed.entry(stripped.to_string()).or_default() += 1;
        } else {
            *working_unprefixed.entry(trimmed.to_string()).or_default() += 1;
        }
    }

    let mut committed_prefixed = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(committed_exc, None) {
        let Some(stripped) = line.strip_prefix("❯ ") else {
            continue;
        };
        let normalized = stripped.trim_end();
        if normalized.is_empty() {
            continue;
        }
        *committed_prefixed
            .entry(normalized.to_string())
            .or_default() += 1;
    }

    let mut missing_counts = std::collections::HashMap::<String, usize>::new();
    for (line, committed_count) in committed_prefixed {
        let working_prefixed_count = working_prefixed.get(&line).copied().unwrap_or(0);
        let working_unprefixed_count = working_unprefixed.get(&line).copied().unwrap_or(0);
        let missing = committed_count.saturating_sub(working_prefixed_count);
        let repairable = missing.min(working_unprefixed_count);
        if repairable > 0 {
            missing_counts.insert(line, repairable);
        }
    }

    let mut targets = Vec::new();
    for line in exchange_prompt_prefix_eligible_lines(committed_exc, None) {
        let Some(stripped) = line.strip_prefix("❯ ") else {
            continue;
        };
        let normalized = stripped.trim_end();
        let Some(remaining) = missing_counts.get_mut(normalized) else {
            continue;
        };
        if *remaining == 0 {
            continue;
        }
        targets.push(stripped.to_string());
        *remaining -= 1;
    }

    targets
}

/// Apply `❯ ` prefix normalization to matching lines in the exchange user
/// region of a full document.
pub fn normalize_exchange_prefixes_for_targets(doc: &str, prefix_lines: &[String]) -> String {
    if prefix_lines.is_empty() {
        return doc.to_string();
    }

    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";
    let boundary_prefix = "<!-- agent:boundary:";

    let Some(open_match) = doc.find(open_tag) else {
        return doc.to_string();
    };
    let Some(close_idx) = doc[open_match..]
        .find(close_tag)
        .map(|idx| open_match + idx)
    else {
        return doc.to_string();
    };
    let Some(open_end) = doc[open_match..]
        .find("-->")
        .map(|idx| open_match + idx + 3)
    else {
        return doc.to_string();
    };

    let before_exchange = &doc[..open_end];
    let exchange_content = &doc[open_end..close_idx];
    let after_exchange = &doc[close_idx..];

    let mut user_region_end = exchange_content.len();
    let mut offset = 0;
    for line in exchange_content.lines() {
        if line.trim().starts_with(boundary_prefix) {
            user_region_end = offset;
        }
        offset += line.len() + 1;
    }
    let user_region = &exchange_content[..user_region_end];
    let agent_region = &exchange_content[user_region_end..];

    let mut remaining = normalization_target_counts(prefix_lines);
    if remaining.is_empty() {
        return doc.to_string();
    }

    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;
    let normalized_user_region = user_region
        .split('\n')
        .map(|doc_line| {
            let trimmed = doc_line.trim();
            if trimmed.starts_with(boundary_prefix) {
                in_response_block = false;
                response_heading_was_prefixed = false;
                return doc_line.to_string();
            }
            if is_exchange_response_heading_for_prefix_repair(trimmed) {
                in_response_block = true;
                response_heading_was_prefixed =
                    is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
                return doc_line.to_string();
            }
            let normalized = doc_line.trim_end();
            let is_target = normalization_target_matches_line(doc_line, &remaining);
            if in_response_block {
                if starts_targeted_or_prefixed_prompt_repair_after_response(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                ) {
                    in_response_block = false;
                    response_heading_was_prefixed = false;
                } else {
                    return doc_line.to_string();
                }
            }
            if normalized.starts_with("❯ ")
                || crate::diff::line_looks_like_plain_response_after_prompt(normalized)
            {
                return doc_line.to_string();
            }
            let Some(remaining_count) = remaining.get_mut(normalized) else {
                return doc_line.to_string();
            };
            if *remaining_count == 0 {
                return doc_line.to_string();
            }
            *remaining_count -= 1;
            format!("❯ {doc_line}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("{before_exchange}{normalized_user_region}{agent_region}{after_exchange}")
}

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

fn guard_visible_write_idle_and_current(
    file: &Path,
    source: &str,
    expected_current: &str,
) -> Result<()> {
    guard_visible_write_idle(file, source)?;
    let indicator_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .to_string();
    if let Some(live) =
        crate::debounce::live_buffer_diverges_from_content(&indicator_path, expected_current)
    {
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
    let actual_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;
    if actual_current == expected_current {
        return Ok(());
    }

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
            crate::ops_log::content_hash(&actual_current)
        ),
    );
    anyhow::bail!(
        "visible document write for {} deferred: document changed after the response merge was computed; retry after typing stops",
        file.display()
    )
}

fn stale_snapshot_reset_drift(snapshot_doc: &str, current_doc: &str) -> Option<(usize, usize)> {
    let snapshot_clean = strip_boundary_for_dedup(snapshot_doc);
    let current_clean = strip_boundary_for_dedup(current_doc);
    let snapshot_len = snapshot_clean.len();
    let current_len = current_clean.len();

    if snapshot_len <= current_len + STALE_SNAPSHOT_RESET_DRIFT_MIN_BYTES {
        return None;
    }
    if current_len as f64 / snapshot_len as f64 >= STALE_SNAPSHOT_RESET_DRIFT_MAX_RATIO {
        return None;
    }
    if crate::git::classify_safe_out_of_band_agent_doc_mutation(&snapshot_clean, &current_clean)
        .is_some()
    {
        return None;
    }

    Some((snapshot_len, current_len))
}

pub fn guard_no_stale_snapshot_reset_drift(
    file: &Path,
    snapshot_doc: Option<&str>,
    current_doc: &str,
    phase: &str,
) -> Result<()> {
    let Some(snapshot_doc) = snapshot_doc else {
        return Ok(());
    };
    if let Ok(Some(cleaned)) =
        crate::template::deleted_conversation_tail_cleanup(snapshot_doc, current_doc)
        && cleaned == current_doc
    {
        return Ok(());
    }
    let Some((snapshot_len, current_len)) = stale_snapshot_reset_drift(snapshot_doc, current_doc)
    else {
        return Ok(());
    };

    crate::ops_log::log_op(
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
    let mut infos = Vec::new();

    for segment in exchange.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        let trimmed = line.trim();
        let mut eligible = true;
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
            let is_target =
                target_counts.is_some_and(|counts| normalization_target_matches_line(line, counts));
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

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

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

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

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

    let final_content = if let Some(repaired_current) = adopt_current_response_without_duplication(
        file,
        base,
        &content_ours,
        &content_current,
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
        merge::merge_contents(base, &content_ours, &content_current)?
    };
    let mut final_content = normalize_final_template_content(
        file,
        base,
        snapshot_doc.as_deref(),
        Some(&content_current),
        &final_content,
        Some(&response),
    )?;
    let cleaned_resolved_backlog_prompts = cleanup_resolved_backlog_prompts_after_response(
        file,
        base,
        &content_current,
        &final_content,
    )?;
    let cleaned_resolved_backlog_prompts_applied = cleaned_resolved_backlog_prompts.is_some();
    if let Some(cleaned) = cleaned_resolved_backlog_prompts {
        final_content = normalize_template_structure_or_fail_preserving(
            &cleaned,
            file,
            Some(&content_current),
        )?;
    }

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
    guard_visible_write_idle_and_current(file, "run_template", &content_current)?;
    snapshot::save(file, snapshot_content)?;

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

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

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
            if let Err(e) = snapshot::save_crdt(file, &snapshot_crdt_state) {
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
                    }
                    Ok(false) => {
                        if let Ok(diag) = queue_skip_diagnostic_for_file(file) {
                            eprintln!("{}", diag);
                        }
                    }
                    Err(e) => eprintln!(
                        "[queue] warning: queue consume decision on stream IPC-timeout failed: {}",
                        e
                    ),
                }
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

    let (final_content, mut crdt_state) = if let Some(repaired_current) =
        adopt_current_response_without_duplication(
            file,
            base,
            &content_ours,
            &content_current,
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
    let mut final_content = normalize_final_template_content(
        file,
        base,
        snapshot_doc.as_deref(),
        Some(&content_current),
        &final_content,
        Some(&response),
    )?;
    let cleaned_resolved_backlog_prompts = cleanup_resolved_backlog_prompts_after_response(
        file,
        base,
        &content_current,
        &final_content,
    )?;
    let cleaned_resolved_backlog_prompts_applied = cleaned_resolved_backlog_prompts.is_some();
    if let Some(cleaned) = cleaned_resolved_backlog_prompts {
        final_content = normalize_template_structure_or_fail_preserving(
            &cleaned,
            file,
            Some(&content_current),
        )?;
        crdt_state = crate::crdt::CrdtDoc::from_text(&final_content).encode_state();
    }

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
    guard_visible_write_idle_and_current(file, "run_stream", &content_current)?;
    snapshot::save(file, snapshot_content)?;
    snapshot::save_crdt(file, &snapshot_crdt_state)?;

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

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

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

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
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
            snapshot::save_crdt(file, &crdt_doc.encode_state())?;
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
        let base_state = crate::crdt::CrdtDoc::from_text(base).encode_state();
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
/// Extract `### Re:` response headings from a slice of `PatchBlock`s.
///
/// Used by the late-fallback gate to decide whether an "already committed"
/// cycle's state belongs to the incoming response (skip the apply) or to a
/// different operation that landed mid-turn (rotate the cycle and apply).
///
/// Only the leading `### Re: ...` line of each patch's content is considered.
/// Section bodies and subheadings are ignored so callers can compare against
/// HEAD content via a substring check without false positives from common
/// boilerplate. Returns the trimmed heading lines (without the trailing
/// newline) in order of appearance.
fn extract_response_headings_from_patches(patches: &[crate::template::PatchBlock]) -> Vec<String> {
    let mut out = Vec::new();
    for patch in patches {
        for line in patch.content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("### Re:") {
                out.push(trimmed.to_string());
                break;
            }
        }
    }
    out
}

/// Return `true` when every `### Re:` response heading carried in the
/// incoming patches is already present in the document's `HEAD` content.
///
/// Used inside the late-fallback gate (see `#adoc-compact-during-turn-response-loss`)
/// to distinguish:
/// - "cycle committed because this response already landed" (skip apply), and
/// - "cycle committed by an unrelated mid-turn operation, but the response
///   is still waiting to be written" (rotate the cycle, apply the patch).
///
/// Returns `true` when there are no headings to check (no patches), which
/// preserves the gate's previous conservative behavior for empty patch lists.
/// Returns `false` if `git show HEAD:<file>` fails — the caller treats that
/// the same as "not in HEAD" and rotates the cycle, which is fail-safe for
/// the mid-turn race.
fn patch_response_headings_already_in_head(
    file: &Path,
    patches: &[crate::template::PatchBlock],
) -> bool {
    let headings = extract_response_headings_from_patches(patches);
    if headings.is_empty() {
        return true;
    }
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let Some(head) = rc.head_content() else {
        return false;
    };
    headings.iter().all(|h| head.contains(h.as_str()))
}

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

fn content_ours_with_pending_from_disk(file: &Path, content_ours: &str) -> String {
    match std::fs::read_to_string(file) {
        Ok(on_disk_content) => splice_pending_component(content_ours, &on_disk_content),
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to read {} while preserving pending mutations during normalization fallback: {}",
                file.display(),
                e
            );
            content_ours.to_string()
        }
    }
}

fn content_ours_merged_with_disk_edits(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
) -> String {
    let Some(base) = baseline else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    let Ok(on_disk_content) = std::fs::read_to_string(file) else {
        return content_ours_with_pending_from_disk(file, content_ours);
    };
    if strip_boundary_for_dedup(&on_disk_content) == strip_boundary_for_dedup(content_ours) {
        return content_ours.to_string();
    }
    if response_already_in_current(base, content_ours, &on_disk_content) {
        eprintln!(
            "[write] normalization fallback: response delta already in current file; adopting current content"
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_adopted_current_delta file={} delta=response_contained",
                file.display()
            ),
        );
        return on_disk_content;
    }

    let base_state = crate::crdt::CrdtDoc::from_text(base).encode_state();
    match merge::merge_contents_crdt(Some(&base_state), content_ours, &on_disk_content) {
        Ok((merged, _)) => merged,
        Err(e) => {
            eprintln!(
                "[write] WARNING: failed to merge current disk edits into normalization fallback: {}",
                e
            );
            content_ours_with_pending_from_disk(file, content_ours)
        }
    }
}

fn normalized_content_ours_fallback(
    file: &Path,
    baseline: Option<&str>,
    content_ours: &str,
    normalize_prefix_lines: &[String],
) -> String {
    let fallback = content_ours_merged_with_disk_edits(file, baseline, content_ours);
    let normalized = normalize_exchange_prefixes_for_targets(&fallback, normalize_prefix_lines);
    repair_duplicate_prompt_artifacts(
        &normalized,
        file,
        DuplicatePromptRepairOptions::new("normalization_fallback")
            .with_before(baseline)
            .preserving(baseline)
            .without_residue_guard(),
    )
    .map(|(repaired, _)| repaired)
    .unwrap_or(normalized)
}

fn repair_disk_from_normalization_fallback(file: &Path, fallback: &str) -> Result<()> {
    guard_visible_write_idle(file, "sidecar_normalization_fallback_repair")?;
    atomic_write(file, fallback).with_context(|| {
        format!(
            "failed to repair {} from normalized content_ours fallback",
            file.display()
        )
    })?;
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_repaired_working_tree file={} bytes={}",
            file.display(),
            fallback.len()
        ),
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IpcSnapshotSource {
    AckContentSidecar,
    ContentOurs,
    FileRead,
}

impl IpcSnapshotSource {
    fn label(self) -> &'static str {
        match self {
            Self::AckContentSidecar => "ack_content_sidecar",
            Self::ContentOurs => "content_ours",
            Self::FileRead => "file_read",
        }
    }

    fn is_ack_content_proven(self) -> bool {
        matches!(self, Self::AckContentSidecar)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IpcDiskRepairReason {
    PrefixDivergence,
    IpcDedupe,
    PrefixDivergenceThenIpcDedupe,
}

impl IpcDiskRepairReason {
    fn label(self) -> &'static str {
        match self {
            Self::PrefixDivergence => "prefix_divergence",
            Self::IpcDedupe => "ipc_dedupe",
            Self::PrefixDivergenceThenIpcDedupe => "prefix_divergence_then_ipc_dedupe",
        }
    }

    fn redelivery_kind(self) -> FullContentRepairRedelivery {
        match self {
            Self::PrefixDivergence => FullContentRepairRedelivery::NormalizationFallback,
            Self::IpcDedupe | Self::PrefixDivergenceThenIpcDedupe => {
                FullContentRepairRedelivery::IpcDedupe
            }
        }
    }

    fn merge_with_ipc_dedupe(self) -> Self {
        match self {
            Self::PrefixDivergence => Self::PrefixDivergenceThenIpcDedupe,
            Self::IpcDedupe | Self::PrefixDivergenceThenIpcDedupe => self,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EditorBadStateFingerprint {
    content: String,
    len: usize,
    hash: String,
}

impl EditorBadStateFingerprint {
    fn new(content: String) -> Self {
        let len = content.len();
        let hash = crate::ops_log::content_hash(&content);
        Self { content, len, hash }
    }

    fn content(&self) -> &str {
        &self.content
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IpcRepairDecision {
    snapshot_content: String,
    snap_source: IpcSnapshotSource,
    disk_repair_reason: Option<IpcDiskRepairReason>,
    editor_bad_state: Option<EditorBadStateFingerprint>,
    normalize_prefix_lines: Vec<String>,
    redeliver_editor: bool,
}

impl IpcRepairDecision {
    fn ack_content(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::AckContentSidecar,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    fn content_ours(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::ContentOurs,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    fn content_ours_prefix_fallback(
        snapshot_content: String,
        bad_state: String,
        normalize_prefix_lines: &[String],
    ) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::ContentOurs,
            disk_repair_reason: Some(IpcDiskRepairReason::PrefixDivergence),
            editor_bad_state: Some(EditorBadStateFingerprint::new(bad_state)),
            normalize_prefix_lines: normalize_prefix_lines.to_vec(),
            redeliver_editor: true,
        }
    }

    fn file_read(snapshot_content: String) -> Self {
        Self {
            snapshot_content,
            snap_source: IpcSnapshotSource::FileRead,
            disk_repair_reason: None,
            editor_bad_state: None,
            normalize_prefix_lines: Vec::new(),
            redeliver_editor: false,
        }
    }

    fn apply_ipc_dedupe(
        mut self,
        snapshot_content: String,
        bad_state_before_dedupe: String,
    ) -> Self {
        self.snapshot_content = snapshot_content;
        self.disk_repair_reason = Some(match self.disk_repair_reason {
            Some(reason) => reason.merge_with_ipc_dedupe(),
            None => IpcDiskRepairReason::IpcDedupe,
        });
        if self.editor_bad_state.is_none() {
            self.editor_bad_state = Some(EditorBadStateFingerprint::new(bad_state_before_dedupe));
        }
        self.redeliver_editor = self.editor_bad_state.is_some();
        self
    }

    fn ack_content_proven(&self) -> bool {
        self.snap_source.is_ack_content_proven()
    }

    fn replace_snapshot_with_content_ours_for_live_prompt_drift(&mut self, content_ours: &str) {
        self.snapshot_content = content_ours.to_string();
        self.snap_source = IpcSnapshotSource::ContentOurs;
        self.disk_repair_reason = None;
        self.editor_bad_state = None;
        self.normalize_prefix_lines.clear();
        self.redeliver_editor = false;
    }

    fn replace_snapshot_with_content_ours_for_prompt_duplication(
        &mut self,
        content_ours: &str,
        bad_state: String,
    ) {
        self.snapshot_content = content_ours.to_string();
        self.snap_source = IpcSnapshotSource::ContentOurs;
        self.disk_repair_reason = Some(IpcDiskRepairReason::IpcDedupe);
        self.editor_bad_state = Some(EditorBadStateFingerprint::new(bad_state));
        self.normalize_prefix_lines.clear();
        self.redeliver_editor = true;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AlreadyAppliedSnapshotOutcome {
    Persisted,
    NeedsFileFallback,
}

fn guard_ipc_snapshot_adoption_against_live_prompt_drift(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source == IpcSnapshotSource::ContentOurs {
        return false;
    }
    let (Some(base), Some(ours)) = (baseline, content_ours) else {
        return false;
    };
    if !ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
        base,
        &decision.snapshot_content,
        ours,
    ) {
        return false;
    }

    let prior_source = decision.snap_source.label();
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::types::FlowEvent::new(
            crate::flow::types::FlowName::DocumentMutation,
            crate::flow::types::FlowStage::IpcSnapshotAdoption,
            crate::flow::types::FlowOutcome::Blocked,
        )
        .with_reason("live_prompt_drift_after_preflight"),
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_blocked file={} source={} patch_id={} snap_source={} reason=live_prompt_drift_after_preflight candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            prior_source,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "live_prompt_drift_after_preflight",
        "content_ours_snapshot_next_cycle",
        &format!(
            "snap_source={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            prior_source,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    let _ = crate::cycle_state::record_ipc_snapshot_adoption_blocked(file);
    // #exchange-prompt-dropped-on-merge: persist the dropped user prompt lines
    // now, while the divergent candidate still carries them. The post-commit
    // session-check disk diff cannot win the race against an editor that
    // overwrites disk with the converged content_ours buffer, so the dropped
    // prompt guard reads this persisted evidence to fail closed instead.
    let dropped = dropped_prompt_lines_after_content_ours(base, &decision.snapshot_content, ours);
    if !dropped.is_empty() {
        if let Err(e) = crate::cycle_state::record_dropped_exchange_prompts(file, &dropped) {
            eprintln!(
                "[write] warning: failed to record dropped exchange prompt(s) for {}: {}",
                file.display(),
                e
            );
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "dropped_exchange_prompt_recorded file={} source={} patch_id={} count={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                dropped.len()
            ),
        );
    }
    // #queue-user-edit-overwrite: same silent-loss race for user-authored queue
    // edits. Record the dropped `do [#id]` queue lines now; session-check
    // filters them against committed HEAD (preserved or consumed → cleared,
    // silently deleted → fail closed).
    let dropped_queue =
        dropped_queue_prompt_lines_after_content_ours(base, &decision.snapshot_content, ours);
    if !dropped_queue.is_empty() {
        if let Err(e) = crate::cycle_state::record_dropped_queue_prompts(file, &dropped_queue) {
            eprintln!(
                "[write] warning: failed to record dropped queue prompt(s) for {}: {}",
                file.display(),
                e
            );
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "dropped_queue_prompt_recorded file={} source={} patch_id={} count={}",
                file.display(),
                source,
                patch_id.unwrap_or("-"),
                dropped_queue.len()
            ),
        );
    }
    decision.replace_snapshot_with_content_ours_for_live_prompt_drift(ours);
    true
}

fn guard_ipc_snapshot_adoption_against_prompt_duplication(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    content_ours: Option<&str>,
    decision: &mut IpcRepairDecision,
) -> bool {
    if decision.snap_source == IpcSnapshotSource::ContentOurs {
        return false;
    }
    let Some(ours) = content_ours else {
        return false;
    };
    let duplicate_count = user_prompt_count_growth(ours, &decision.snapshot_content);
    if duplicate_count == 0 {
        return false;
    }

    let prior_source = decision.snap_source.label();
    let bad_state = decision.snapshot_content.clone();
    crate::flow::proof::log_flow_event(
        file,
        crate::flow::types::FlowEvent::new(
            crate::flow::types::FlowName::DocumentMutation,
            crate::flow::types::FlowStage::IpcSnapshotAdoption,
            crate::flow::types::FlowOutcome::Blocked,
        )
        .with_reason("prompt_duplication_in_ack_content"),
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_blocked file={} source={} patch_id={} snap_source={} reason=prompt_duplication_in_ack_content duplicate_prompt_count={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            prior_source,
            duplicate_count,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "prompt_duplication_in_ack_content",
        "content_ours_snapshot_and_visible_repair",
        &format!(
            "snap_source={} duplicate_prompt_count={} candidate_len={} candidate_hash={} content_ours_len={} content_ours_hash={}",
            prior_source,
            duplicate_count,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            ours.len(),
            crate::ops_log::content_hash(ours)
        ),
    );
    let _ = crate::cycle_state::record_ipc_snapshot_adoption_blocked(file);
    decision.replace_snapshot_with_content_ours_for_prompt_duplication(ours, bad_state);
    true
}

/// Emit a diagnostic for every IPC snapshot adoption that the two fail-closed
/// guards did NOT block. Blocked adoptions already log richly; allowed ones were
/// previously silent, so a corruption that slips through as "allowed" left no
/// trace. This symmetric `ipc_snapshot_adoption_allowed` line records the final
/// snapshot shape plus an independent drift/dup re-check (both must be benign on
/// an allowed path — a non-benign re-check here flags a guard-coverage gap).
fn log_ipc_snapshot_adoption_allowed(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    decision: &IpcRepairDecision,
    was_blocked: bool,
) {
    if was_blocked {
        return;
    }
    let drift_recheck = match (baseline, content_ours) {
        (Some(base), Some(ours)) => ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(
            base,
            &decision.snapshot_content,
            ours,
        ),
        _ => false,
    };
    let dup_recheck = content_ours
        .map(|ours| user_prompt_count_growth(ours, &decision.snapshot_content))
        .unwrap_or(0);
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_snapshot_adoption_allowed file={} source={} patch_id={} snap_source={} snapshot_len={} snapshot_hash={} content_ours_len={} content_ours_hash={} drift_recheck={} dup_growth_recheck={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            decision.snap_source.label(),
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            content_ours.map(|o| o.len()).unwrap_or(0),
            content_ours
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            drift_recheck,
            dup_recheck,
        ),
    );
}

/// #ipcfullprompt-recur2 — default-on forensic capture. The fail-closed snapshot
/// guards above protect what gets *committed*, but a full-document editor-side
/// IPC mutation (e.g. `PatchWatcher.setText`) can still corrupt the
/// editor-visible buffer — deleting or duplicating a previously-committed
/// `### Re:` response block — while the user types a live prompt. This records
/// every such occurrence to `ops.log` and preserves the candidate buffer, so the
/// bug (which is not reliably reproducible) is captured the next time it happens
/// without any manual editor debug opt-in. Detection only: it never changes the
/// adoption decision — the guards above own that.
///
/// `candidate` must be the live editor buffer as received (capture it before the
/// guards replace `decision.snapshot_content`).
fn log_ipcfullprompt_corruption_if_any(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    candidate: &str,
) {
    // Scaffold duplication is a self-check on the candidate (the full-tail
    // duplication shape — two `<!-- /agent:exchange -->` markers — captured live in
    // brandon-cinquegrana.md), so it runs even when no baseline is available.
    let mut findings = crate::ipc_corruption::detect_duplicated_scaffold(candidate);
    // Response-block delete/duplicate needs the prior committed baseline.
    if let Some(base) = baseline {
        findings.extend(crate::ipc_corruption::detect_response_block_corruption(
            base, candidate,
        ));
    }
    if findings.is_empty() {
        return;
    }
    let base = baseline.unwrap_or("");
    let summary = crate::ipc_corruption::summarize_findings(&findings);
    crate::ops_log::log_op(
        file,
        &format!(
            "ipcfullprompt_corruption_suspected file={} source={} patch_id={} candidate_len={} candidate_hash={} baseline_len={} baseline_hash={} {}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            candidate.len(),
            crate::ops_log::content_hash(candidate),
            base.len(),
            crate::ops_log::content_hash(base),
            summary,
        ),
    );
    preserve_ipcfullprompt_forensic(file, patch_id, base, candidate);
}

/// Best-effort: preserve the baseline + corrupted candidate buffers under
/// `.agent-doc/logs/ipcfullprompt/` so the exact corruption shape can be analyzed
/// later (the plan's Phase-1 "preserve the pre/post for one failing cycle").
/// Never panics or returns errors.
fn preserve_ipcfullprompt_forensic(
    file: &Path,
    patch_id: Option<&str>,
    baseline: &str,
    candidate: &str,
) {
    let Ok(canonical) = file.canonicalize() else {
        return;
    };
    let Some(root) = crate::fs_util::find_project_root(&canonical) else {
        return;
    };
    let dir = root.join(".agent-doc/logs/ipcfullprompt");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = format!("{}-{}", ts, patch_id.unwrap_or("nopatch"));
    let _ = std::fs::write(dir.join(format!("{stem}.baseline.md")), baseline);
    let _ = std::fs::write(dir.join(format!("{stem}.candidate.md")), candidate);
}

fn persist_already_applied_socket_content_ours_snapshot(
    file: &Path,
    patch_id: &str,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
    expected_response: &str,
) -> Result<AlreadyAppliedSnapshotOutcome> {
    let Some(ours) = content_ours else {
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_socket_already_applied_no_content_ours_snapshot file={} patch_id={}",
                file.display(),
                patch_id
            ),
        );
        return Ok(AlreadyAppliedSnapshotOutcome::Persisted);
    };

    let current = std::fs::read_to_string(file).ok();
    let mut repair_decision = IpcRepairDecision::content_ours(ours.to_string());
    if let Some(current) = current.as_deref()
        && strip_boundary_for_dedup(current) != strip_boundary_for_dedup(ours)
    {
        let response_present = response_materialized_in_content(expected_response, current)
            || baseline.is_some_and(|base| response_already_in_current(base, ours, current));
        let prompt_drift = baseline.is_some_and(|base| {
            ipc_snapshot_would_absorb_live_prompt_drift_after_preflight(base, current, ours)
        });
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_socket_already_applied_live_buffer_diverged file={} patch_id={} response_present={} current_len={} current_hash={} content_ours_len={} content_ours_hash={} prompt_drift={}",
                file.display(),
                patch_id,
                response_present,
                current.len(),
                crate::ops_log::content_hash(current),
                ours.len(),
                crate::ops_log::content_hash(ours),
                prompt_drift
            ),
        );
        // #6cmx/#wy0y verification marker: an explicit, greppable record that the
        // operator typed into the document while finalize was writing (the live
        // buffer diverged from our content with prompt drift). `typed_delta_bytes`
        // is the live-vs-ours byte delta (their keystrokes); `response_present`
        // confirms the assistant response is still materialized in the buffer, so
        // grepping `finalize_typing_during_write` verifies a typing-during-finalize
        // run was exercised and whether the response survived intact.
        if prompt_drift {
            crate::ops_log::log_op(
                file,
                &format!(
                    "finalize_typing_during_write file={} patch_id={} typed_delta_bytes={} response_present={} resolution=content_ours_adopted",
                    file.display(),
                    patch_id,
                    current.len() as i64 - ours.len() as i64,
                    response_present
                ),
            );
        }

        if !response_present {
            log_ipc_proof_failure(
                file,
                "socket_already_applied",
                Some(patch_id),
                "disk_missing_response_probe",
                "file_ipc_fallback",
                &format!(
                    "response_sha256={} current_len={} current_hash={}",
                    crate::ops_log::content_hash(expected_response),
                    current.len(),
                    crate::ops_log::content_hash(current)
                ),
            );
            return Ok(AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
        }

        repair_decision = IpcRepairDecision::file_read(current.to_string());
        if let Some(lines) = normalize_prefix_lines
            && !lines.is_empty()
        {
            let normalized =
                normalize_exchange_prefixes_for_targets(&repair_decision.snapshot_content, lines);
            if normalized != repair_decision.snapshot_content {
                repair_decision = IpcRepairDecision::content_ours_prefix_fallback(
                    normalized,
                    current.to_string(),
                    lines,
                );
            }
        }

        let before_response_dedupe = repair_decision.snapshot_content.clone();
        let response_deduped =
            dedupe_consecutive_response_blocks(&repair_decision.snapshot_content, file);
        if response_deduped != repair_decision.snapshot_content {
            repair_decision =
                repair_decision.apply_ipc_dedupe(response_deduped, before_response_dedupe);
        }

        let pre_dedupe_snap = repair_decision.snapshot_content.clone();
        let (effective_snap, dedupe_repair) = dedupe_ipc_snapshot_content(
            file,
            baseline,
            &repair_decision.snapshot_content,
            "socket_already_applied_disk",
        )?;
        if dedupe_repair {
            repair_decision = repair_decision.apply_ipc_dedupe(effective_snap, pre_dedupe_snap);
        } else {
            repair_decision.snapshot_content = effective_snap;
        }
    }

    repair_ipc_decision_visible_state(file, &repair_decision, Some(patch_id))?;
    snapshot::save(file, &repair_decision.snapshot_content)?;
    let crdt_doc = crate::crdt::CrdtDoc::from_text(&repair_decision.snapshot_content);
    snapshot::save_crdt(file, &crdt_doc.encode_state())?;
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_socket_already_applied_snapshot file={} patch_id={} snap_source={} snap_len={} snap_hash={}",
            file.display(),
            patch_id,
            repair_decision.snap_source.label(),
            repair_decision.snapshot_content.len(),
            crate::ops_log::content_hash(&repair_decision.snapshot_content)
        ),
    );
    Ok(AlreadyAppliedSnapshotOutcome::Persisted)
}

fn normalization_prefix_observation_counts(
    content: &str,
    normalize_prefix_lines: &[String],
) -> (usize, usize) {
    let target_counts = normalization_target_counts(normalize_prefix_lines);
    let required = target_counts.values().sum();
    if required == 0 {
        return (0, 0);
    }

    let exchange = component::parse(content)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(content).to_string())
        })
        .unwrap_or_else(|| content.to_string());

    let mut observed_counts = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(&exchange, Some(&target_counts)) {
        let Some(stripped) = line.trim_end().strip_prefix("❯ ") else {
            continue;
        };
        if target_counts.contains_key(stripped) {
            *observed_counts.entry(stripped.to_string()).or_default() += 1;
        }
    }

    let observed = target_counts
        .iter()
        .map(|(target, required)| {
            observed_counts
                .get(target)
                .copied()
                .unwrap_or(0)
                .min(*required)
        })
        .sum();
    (required, observed)
}

fn duplicate_prompt_line_count(content: &str) -> usize {
    let exchange = component::parse(content)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(content).to_string())
        })
        .unwrap_or_else(|| content.to_string());

    let mut counts = std::collections::HashMap::<String, usize>::new();
    let mut duplicates = 0;
    for line in exchange_prompt_prefix_eligible_lines(&exchange, None) {
        let normalized = line
            .trim_end()
            .strip_prefix("❯ ")
            .unwrap_or(line.trim_end())
            .trim();
        if normalized.is_empty() {
            continue;
        }
        let count = counts.entry(normalized.to_string()).or_default();
        *count += 1;
        if *count > 1 {
            duplicates += 1;
        }
    }
    duplicates
}

fn ipc_repair_decision_from_sidecar(
    file: &Path,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    snap_content: String,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
) -> IpcRepairDecision {
    if let Some(lines) = normalize_prefix_lines
        && !lines.is_empty()
        && !verify_sidecar_normalization(&snap_content, lines)
    {
        if let Some(ours) = content_ours {
            let bad_state = snap_content;
            let fallback = normalized_content_ours_fallback(file, baseline, ours, lines);
            let (required_prefix_count, observed_prefix_count) =
                normalization_prefix_observation_counts(&bad_state, lines);
            let duplicate_prompt_count = duplicate_prompt_line_count(&bad_state);
            eprintln!(
                "[write] sidecar normalization diverged — falling back to content_ours ({} bytes)",
                fallback.len()
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback file={} patch_id={} snap_source=content_ours reason=prefix_divergence bad_len={} bad_hash={} fallback_len={} fallback_hash={} required_prefix_count={} observed_prefix_count={} duplicate_prompt_count={}",
                    file.display(),
                    patch_id.unwrap_or("-"),
                    bad_state.len(),
                    crate::ops_log::content_hash(&bad_state),
                    fallback.len(),
                    crate::ops_log::content_hash(&fallback),
                    required_prefix_count,
                    observed_prefix_count,
                    duplicate_prompt_count
                ),
            );
            return IpcRepairDecision::content_ours_prefix_fallback(fallback, bad_state, lines);
        }

        eprintln!(
            "[write] sidecar normalization diverged but no content_ours available — using sidecar"
        );
    }

    IpcRepairDecision::ack_content(snap_content)
}

#[derive(Clone, Copy, Debug)]
enum FullContentRepairRedelivery {
    NormalizationFallback,
    IpcDedupe,
}

impl FullContentRepairRedelivery {
    fn label(self) -> &'static str {
        match self {
            Self::NormalizationFallback => "sidecar_normalization_fallback",
            Self::IpcDedupe => "ipc_dedupe",
        }
    }

    fn success_message(self) -> &'static str {
        match self {
            Self::NormalizationFallback => {
                "[write] sidecar normalization fallback re-delivered to editor via full-content IPC"
            }
            Self::IpcDedupe => "[write] IPC duplicate-response repair re-delivered to editor",
        }
    }

    fn not_consumed_message(self) -> &'static str {
        match self {
            Self::NormalizationFallback => {
                "[write] WARNING: sidecar normalization fallback repaired disk, but editor IPC repair was not consumed; reload the document if the editor view is stale"
            }
            Self::IpcDedupe => {
                "[write] WARNING: IPC duplicate-response repair updated disk, but editor IPC repair was not consumed; reload the document if the editor view is stale"
            }
        }
    }

    fn failed_message(self, error: &anyhow::Error) -> String {
        match self {
            Self::NormalizationFallback => format!(
                "[write] WARNING: sidecar normalization fallback repaired disk, but editor IPC repair failed: {}; reload the document if the editor view is stale",
                error
            ),
            Self::IpcDedupe => format!(
                "[write] WARNING: IPC duplicate-response repair updated disk, but editor IPC repair failed: {}; reload the document if the editor view is stale",
                error
            ),
        }
    }
}

fn redeliver_full_content_repair_to_editor(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    kind: FullContentRepairRedelivery,
    source_patch_id: Option<&str>,
) -> bool {
    let current_content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(e) => {
            eprintln!(
                "[write] WARNING: {} editor repair skipped because {} could not be read: {}",
                kind.label(),
                file.display(),
                e
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_editor_redelivery_skipped file={} patch_id={} skip=read_failed error={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
            return false;
        }
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "{}_editor_redelivery_proof file={} patch_id={} proof_source=bad_editor_state expected_len={} expected_hash={} current_len={} current_hash={} redeliver={}",
            kind.label(),
            file.display(),
            source_patch_id.unwrap_or("-"),
            expected_bad_state.len(),
            crate::ops_log::content_hash(expected_bad_state),
            current_content.len(),
            crate::ops_log::content_hash(&current_content),
            current_content == expected_bad_state
        ),
    );
    if current_content != expected_bad_state {
        eprintln!(
            "[write] {} editor repair skipped: visible buffer no longer matches the bad state",
            kind.label()
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "{}_editor_redelivery_skipped file={} patch_id={} skip=stale_bad_state expected_len={} expected_hash={} current_len={} current_hash={}",
                kind.label(),
                file.display(),
                source_patch_id.unwrap_or("-"),
                expected_bad_state.len(),
                crate::ops_log::content_hash(expected_bad_state),
                current_content.len(),
                crate::ops_log::content_hash(&current_content)
            ),
        );
        return false;
    }

    match try_ipc_full_content_response_fallback_from_source(
        file,
        repaired_content,
        expected_bad_state,
    ) {
        Ok(true) => {
            eprintln!("{}", kind.success_message());
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_redelivered_editor file={} patch_id={} bytes={} expected_bad_len={} expected_bad_hash={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    repaired_content.len(),
                    expected_bad_state.len(),
                    crate::ops_log::content_hash(expected_bad_state)
                ),
            );
            true
        }
        Ok(false) => {
            eprintln!("{}", kind.not_consumed_message());
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_editor_repair_not_consumed file={} patch_id={} bytes={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    repaired_content.len()
                ),
            );
            false
        }
        Err(e) => {
            eprintln!("{}", kind.failed_message(&e));
            crate::ops_log::log_op(
                file,
                &format!(
                    "{}_editor_repair_failed file={} patch_id={} error={}",
                    kind.label(),
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
            false
        }
    }
}

fn normalization_repair_candidate_matches(
    expected_bad_state: &str,
    repaired_content: &str,
    normalize_prefix_lines: &[String],
) -> bool {
    if normalize_prefix_lines.is_empty() {
        return false;
    }
    let normalized =
        normalize_exchange_prefixes_for_targets(expected_bad_state, normalize_prefix_lines);
    strip_boundary_for_dedup(&normalized) == strip_boundary_for_dedup(repaired_content)
}

fn normalization_repair_payload(
    canonical: &Path,
    patch_id: &str,
    normalize_prefix_lines: &[String],
    expected_bad_state: &str,
    include_type: bool,
) -> serde_json::Value {
    let proof =
        crate::flow::document_mutation::FullContentSourceProof::from_content(expected_bad_state);
    let mut payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "patch_id": patch_id,
        "reposition_boundary": true,
        "preserve_head": true,
        "normalize_prefix_lines": normalize_prefix_lines,
        "expected_content_hash": proof.expected_content_hash,
        "expected_content_len": proof.expected_content_len,
    });
    if include_type {
        payload["type"] = serde_json::Value::String("patch".to_string());
    }
    payload
}

fn verify_normalization_repair_observed(
    file: &Path,
    project_root: &Path,
    patch_id: &str,
    repaired_content: &str,
    transport: &str,
) -> bool {
    let observed = match poll_ack_content_sidecar(
        project_root,
        patch_id,
        std::time::Duration::from_millis(200),
        std::time::Duration::from_millis(25),
    ) {
        Ok(Some(content)) => content,
        Ok(None) => std::fs::read_to_string(file).unwrap_or_default(),
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback_narrow_repair_ack_read_failed file={} patch_id={} transport={} error={}",
                    file.display(),
                    patch_id,
                    transport,
                    e
                ),
            );
            std::fs::read_to_string(file).unwrap_or_default()
        }
    };

    let observed_matches =
        strip_boundary_for_dedup(&observed) == strip_boundary_for_dedup(repaired_content);
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_observed file={} patch_id={} transport={} observed_len={} observed_hash={} expected_len={} expected_hash={} matched={}",
            file.display(),
            patch_id,
            transport,
            observed.len(),
            crate::ops_log::content_hash(&observed),
            repaired_content.len(),
            crate::ops_log::content_hash(repaired_content),
            observed_matches
        ),
    );
    observed_matches
}

fn try_ipc_normalization_repair_patch(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    normalize_prefix_lines: &[String],
    source_patch_id: Option<&str>,
) -> Result<bool> {
    if !normalization_repair_candidate_matches(
        expected_bad_state,
        repaired_content,
        normalize_prefix_lines,
    ) {
        crate::ops_log::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_narrow_repair_ineligible file={} patch_id={} skip=normalization_only_patch_not_equivalent normalize_targets={}",
                file.display(),
                source_patch_id.unwrap_or("-"),
                normalize_prefix_lines.len()
            ),
        );
        return Ok(false);
    }

    let current_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to read {} before normalization repair",
            file.display()
        )
    })?;
    if current_content != expected_bad_state {
        crate::ops_log::log_op(
            file,
            &format!(
                "sidecar_normalization_fallback_narrow_repair_skipped file={} patch_id={} skip=stale_bad_state expected_len={} expected_hash={} current_len={} current_hash={}",
                file.display(),
                source_patch_id.unwrap_or("-"),
                expected_bad_state.len(),
                crate::ops_log::content_hash(expected_bad_state),
                current_content.len(),
                crate::ops_log::content_hash(&current_content)
            ),
        );
        return Ok(false);
    }

    let canonical = file.canonicalize()?;
    let project_root = resolve_ipc_project_root(&canonical);
    let patch_id = uuid::Uuid::new_v4().to_string();
    let payload = normalization_repair_payload(
        &canonical,
        &patch_id,
        normalize_prefix_lines,
        expected_bad_state,
        true,
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_attempt file={} patch_id={} source_patch_id={} normalize_targets={} expected_bad_len={} expected_bad_hash={} repaired_len={} repaired_hash={}",
            file.display(),
            patch_id,
            source_patch_id.unwrap_or("-"),
            normalize_prefix_lines.len(),
            expected_bad_state.len(),
            crate::ops_log::content_hash(expected_bad_state),
            repaired_content.len(),
            crate::ops_log::content_hash(repaired_content)
        ),
    );

    if crate::ipc_socket::is_listener_active(&project_root) {
        match crate::ipc_socket::send_message(&project_root, &payload) {
            Ok(Some(_)) => {
                if verify_normalization_repair_observed(
                    file,
                    &project_root,
                    &patch_id,
                    repaired_content,
                    "socket",
                ) {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "sidecar_normalization_fallback_narrow_repaired_editor file={} patch_id={} transport=socket",
                            file.display(),
                            patch_id
                        ),
                    );
                    return Ok(true);
                }
                return Ok(false);
            }
            Ok(None) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repair_not_consumed file={} patch_id={} transport=socket",
                        file.display(),
                        patch_id
                    ),
                );
            }
            Err(e) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repair_failed file={} patch_id={} transport=socket error={}",
                        file.display(),
                        patch_id,
                        e
                    ),
                );
            }
        }
    }

    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        return Ok(false);
    }

    let hash = snapshot::doc_hash(file)?;
    let patch_file = patches_dir.join(format!("{hash}.json"));
    let payload = normalization_repair_payload(
        &canonical,
        &patch_id,
        normalize_prefix_lines,
        expected_bad_state,
        false,
    );
    atomic_write(&patch_file, &serde_json::to_string_pretty(&payload)?)?;

    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !patch_file.exists() {
            if verify_normalization_repair_observed(
                file,
                &project_root,
                &patch_id,
                repaired_content,
                "file",
            ) {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "sidecar_normalization_fallback_narrow_repaired_editor file={} patch_id={} transport=file",
                        file.display(),
                        patch_id
                    ),
                );
                return Ok(true);
            }
            return Ok(false);
        }
        std::thread::sleep(poll_interval);
    }
    let _ = std::fs::remove_file(&patch_file);
    crate::ops_log::log_op(
        file,
        &format!(
            "sidecar_normalization_fallback_narrow_repair_not_consumed file={} patch_id={} transport=file",
            file.display(),
            patch_id
        ),
    );
    Ok(false)
}

fn redeliver_normalization_fallback_to_editor(
    file: &Path,
    repaired_content: &str,
    expected_bad_state: &str,
    normalize_prefix_lines: &[String],
    source_patch_id: Option<&str>,
) -> bool {
    match try_ipc_normalization_repair_patch(
        file,
        repaired_content,
        expected_bad_state,
        normalize_prefix_lines,
        source_patch_id,
    ) {
        Ok(true) => return true,
        Ok(false) => {}
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "sidecar_normalization_fallback_narrow_repair_failed file={} patch_id={} error={}",
                    file.display(),
                    source_patch_id.unwrap_or("-"),
                    e
                ),
            );
        }
    }

    redeliver_full_content_repair_to_editor(
        file,
        repaired_content,
        expected_bad_state,
        FullContentRepairRedelivery::NormalizationFallback,
        source_patch_id,
    )
}

fn repair_disk_from_ipc_dedupe(file: &Path, content: &str) -> Result<()> {
    guard_visible_write_idle(file, "ipc_dedupe_repair")?;
    atomic_write(file, content).with_context(|| {
        format!(
            "failed to repair {} after IPC duplicate-response dedupe",
            file.display()
        )
    })?;
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_dedupe_repaired_working_tree file={} bytes={}",
            file.display(),
            content.len()
        ),
    );
    Ok(())
}

#[cfg(test)]
fn redeliver_ipc_dedupe_to_editor(file: &Path, content: &str, expected_bad_state: &str) -> bool {
    redeliver_full_content_repair_to_editor(
        file,
        content,
        expected_bad_state,
        FullContentRepairRedelivery::IpcDedupe,
        None,
    )
}

fn repair_ipc_decision_visible_state(
    file: &Path,
    decision: &IpcRepairDecision,
    patch_id: Option<&str>,
) -> Result<()> {
    let Some(reason) = decision.disk_repair_reason else {
        return Ok(());
    };
    let bad_len = decision
        .editor_bad_state
        .as_ref()
        .map(|state| state.len)
        .unwrap_or(0);
    let bad_hash = decision
        .editor_bad_state
        .as_ref()
        .map(|state| state.hash.as_str())
        .unwrap_or("-");
    let current = std::fs::read_to_string(file).ok();
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_repair_decision file={} patch_id={} snap_source={} repair_reason={} redeliver_editor={} bad_len={} bad_hash={} repaired_len={} repaired_hash={} current_len={} current_hash={} normalize_targets={} duplicate_prompt_count={}",
            file.display(),
            patch_id.unwrap_or("-"),
            decision.snap_source.label(),
            reason.label(),
            decision.redeliver_editor,
            bad_len,
            bad_hash,
            decision.snapshot_content.len(),
            crate::ops_log::content_hash(&decision.snapshot_content),
            current.as_deref().map(str::len).unwrap_or(0),
            current
                .as_deref()
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            decision.normalize_prefix_lines.len(),
            duplicate_prompt_line_count(
                decision
                    .editor_bad_state
                    .as_ref()
                    .map(EditorBadStateFingerprint::content)
                    .unwrap_or(&decision.snapshot_content)
            )
        ),
    );

    if decision.redeliver_editor
        && let Some(expected_bad_state) = decision.editor_bad_state.as_ref()
        && match reason {
            IpcDiskRepairReason::PrefixDivergence => redeliver_normalization_fallback_to_editor(
                file,
                &decision.snapshot_content,
                expected_bad_state.content(),
                &decision.normalize_prefix_lines,
                patch_id,
            ),
            IpcDiskRepairReason::IpcDedupe | IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe => {
                redeliver_full_content_repair_to_editor(
                    file,
                    &decision.snapshot_content,
                    expected_bad_state.content(),
                    reason.redelivery_kind(),
                    patch_id,
                )
            }
        }
    {
        return Ok(());
    }

    match reason {
        IpcDiskRepairReason::PrefixDivergence => {
            repair_disk_from_normalization_fallback(file, &decision.snapshot_content)
        }
        IpcDiskRepairReason::IpcDedupe | IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe => {
            repair_disk_from_ipc_dedupe(file, &decision.snapshot_content)
        }
    }
}

pub fn dedupe_ipc_snapshot_content(
    file: &Path,
    before: Option<&str>,
    content: &str,
    source: &str,
) -> Result<(String, bool)> {
    let (deduped, report) = repair_duplicate_prompt_artifacts(
        content,
        file,
        DuplicatePromptRepairOptions::new(source)
            .with_before(before)
            .preserving(before),
    )?;
    let changed = deduped != content;
    if report.changed() {
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_snapshot_deduped file={} source={} before_commit=true",
                file.display(),
                source
            ),
        );
    }
    Ok((deduped, changed))
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
    /// True when IPC was intentionally skipped because the current cycle has
    /// already reached the terminal committed state.
    pub skipped_committed_cycle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIpcRepositionResult {
    Queued,
    DeferredExistingPatch,
    Unavailable,
}

/// Remove leftover fallback patch files for a document after closeout commits.
/// Prevents late file-watcher or plugin recovery from re-applying a stale patch
/// to an already-committed document.
pub fn cleanup_fallback_patch_files(file: &Path) {
    crate::flow::closeout::cleanup_fallback_patch_files(file);
}

/// Check if the current cycle for `file` is already in Committed phase.
/// Returns `Some(cycle_id)` if committed, `None` if no cycle or cycle is open.
fn cycle_already_committed(file: &Path) -> Option<String> {
    crate::flow::closeout::cycle_already_committed(file)
}

fn write_claimed_patch_sentinel(project_root: &Path, patch_id: &str) {
    crate::flow::closeout::write_claimed_patch_sentinel(project_root, patch_id);
}

fn existing_patch_is_reposition_only(payload: &serde_json::Value) -> bool {
    payload
        .get("reposition_boundary")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        && payload
            .get("patches")
            .and_then(|value| value.as_array())
            .is_none_or(|patches| patches.is_empty())
        && payload
            .get("unmatched")
            .and_then(|value| value.as_str())
            .is_none_or(|unmatched| unmatched.trim().is_empty())
        && payload
            .get("fullContent")
            .and_then(|value| value.as_str())
            .is_none_or(|content| content.is_empty())
}

pub fn queue_file_ipc_reposition_boundary(
    file: &Path,
    boundary_id: Option<&str>,
    normalize_prefix_lines: &[String],
) -> Result<FileIpcRepositionResult> {
    let canonical = file.canonicalize()?;
    let project_root = resolve_ipc_project_root(&canonical);
    let patches_dir = project_root.join(".agent-doc/patches");
    if !patches_dir.exists() {
        return Ok(FileIpcRepositionResult::Unavailable);
    }

    let hash = snapshot::doc_hash(file)?;
    let patch_file = patches_dir.join(format!("{hash}.json"));
    if patch_file.exists() {
        let existing = std::fs::read_to_string(&patch_file).unwrap_or_default();
        match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(payload) if existing_patch_is_reposition_only(&payload) => {}
            Ok(_) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "file_ipc_reposition_deferred_existing_patch file={} patch_file={}",
                        file.display(),
                        patch_file.display()
                    ),
                );
                return Ok(FileIpcRepositionResult::DeferredExistingPatch);
            }
            Err(e) => {
                eprintln!(
                    "[commit] replacing unreadable file IPC reposition patch {}: {}",
                    patch_file.display(),
                    e
                );
            }
        }
    }

    let patch_id = uuid::Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "patch_id": patch_id,
        "reposition_boundary": true,
        "preserve_head": true,
    });
    if let Some(boundary_id) = boundary_id {
        payload["reposition_boundary_id"] = serde_json::Value::String(boundary_id.to_string());
    }
    if !normalize_prefix_lines.is_empty() {
        payload["normalize_prefix_lines"] = serde_json::Value::Array(
            normalize_prefix_lines
                .iter()
                .map(|line| serde_json::Value::String(line.clone()))
                .collect(),
        );
    }

    atomic_write(&patch_file, &serde_json::to_string_pretty(&payload)?)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "file_ipc_reposition_queued file={} patch_file={} patch_id={}",
            file.display(),
            patch_file.display(),
            payload
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("")
        ),
    );
    eprintln!(
        "[commit] file IPC reposition patch queued: {}",
        patch_file.display()
    );
    Ok(FileIpcRepositionResult::Queued)
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
    let patch_id = reuse_patch_id
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let ipc_before_content = std::fs::read_to_string(file).ok();

    // Guard: if the cycle is already committed, reject the patch to prevent
    // a late fallback from re-dirtying the document.
    //
    // Exception (#adoc-compact-during-turn-response-loss): when a binary-owned
    // commit lands mid-turn (for example a JetBrains-initiated
    // `agent-doc compact exchange` between this turn's preflight and finalize),
    // the cycle state's `Committed` phase belongs to that other operation —
    // not to the response we are about to apply. Detect that case by checking
    // whether the response headings carried in the incoming patches are
    // already present in HEAD. If they are, the gate is correct (skip).
    // If they are not, the "committed" cycle is unrelated to this response:
    // rotate the cycle state to start fresh and let the patch flow continue.
    if let Some(ref cycle_id) = cycle_already_committed(file) {
        let response_in_head = patch_response_headings_already_in_head(file, patches);
        if !response_in_head {
            eprintln!(
                "[write] mid-turn cycle rotation detected for {}: cycle {} marked committed \
                 but the incoming response heading(s) are absent from HEAD — starting a fresh \
                 cycle instead of rejecting (see #adoc-compact-during-turn-response-loss)",
                file.display(),
                cycle_id
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "mid_turn_cycle_rotation file={} prior_cycle={} patch_id={} action=fresh_cycle",
                    file.display(),
                    cycle_id,
                    patch_id
                ),
            );
            let snapshot_content = crate::snapshot::load(file)?;
            let file_content_for_state = std::fs::read_to_string(file).ok();
            let _ = crate::cycle_state::start_preflight(
                file,
                snapshot_content.as_deref(),
                file_content_for_state.as_deref(),
            );
        } else {
            eprintln!(
                "[write] rejecting late fallback patch: cycle {} already committed for {}",
                cycle_id,
                file.display()
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
                    "late_fallback_patch_rejected file={} cycle_id={} patch_id={} reason=already_committed",
                    file.display(),
                    cycle_id,
                    patch_id
                ),
            );
            cleanup_fallback_patch_files(file);
            return Ok(IpcResult {
                success: false,
                patch_id,
                skipped_committed_cycle: true,
            });
        }
    }

    // Clean up any legacy degraded marker from older versions
    cleanup_legacy_ipc_degraded(&project_root);

    // Try socket IPC first (lower latency, no inotify)
    if crate::ipc_socket::is_listener_active(&project_root) {
        // Seed the boundary from patch_id so the socket patch and any later file /
        // run_stream fallback rebuild share an IDENTICAL boundary — otherwise a
        // late socket apply + file apply land the response twice
        // (#finalize-visible-buffer-ipc-timeout-race).
        let ipc_patches_json =
            build_ipc_patches_json(file, patches, unmatched, normalize_prefix_lines, Some(&patch_id))?;
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
        if let Ok(Some(ref cs)) = crate::cycle_state::load(file) {
            socket_payload["cycle_id"] = serde_json::Value::String(cs.cycle_id.clone());
        }
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
            if ipc_patches_json.is_empty()
                && let Some(ours) = content_ours
                && full_content_ipc_scope_allows(
                    file,
                    FullContentIpcMode::ResponseFallback,
                    &patch_id,
                    ours,
                    ipc_before_content.as_deref(),
                    ipc_before_content.as_deref(),
                )
            {
                log_full_content_ipc_disabled(
                    file,
                    FullContentIpcMode::ResponseFallback,
                    &patch_id,
                    ours,
                    ipc_before_content.as_deref(),
                    ipc_before_content.as_deref(),
                );
            }
        }
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_socket_attempt file={} hash={} patch_id={} patches={} ipc_patches={} unmatched_len={} effective_unmatched_len={} baseline_len={} normalize_targets={} unmatched_marker_count={}",
                file.display(),
                hash,
                patch_id,
                patches.len(),
                ipc_patches_json.len(),
                unmatched.trim().len(),
                effective_unmatched_socket.len(),
                baseline.map(str::len).unwrap_or(0),
                normalize_prefix_lines.map(|lines| lines.len()).unwrap_or(0),
                patchback_marker_count_outside_code(unmatched)
            ),
        );
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
                    let mut repair_decision = ipc_repair_decision_from_sidecar(
                        file,
                        Some(&patch_id),
                        baseline,
                        snap_content,
                        content_ours,
                        normalize_prefix_lines,
                    );

                    let pre_dedupe_snap = repair_decision.snapshot_content.clone();
                    let (effective_snap, dedupe_repair) = dedupe_ipc_snapshot_content(
                        file,
                        ipc_before_content.as_deref(),
                        &repair_decision.snapshot_content,
                        repair_decision.snap_source.label(),
                    )?;
                    if dedupe_repair {
                        repair_decision =
                            repair_decision.apply_ipc_dedupe(effective_snap, pre_dedupe_snap);
                    } else {
                        repair_decision.snapshot_content = effective_snap;
                    }
                    // Capture the live editor buffer before the guards replace it,
                    // so the #ipcfullprompt forensic detector sees the candidate.
                    let ipcfullprompt_candidate = repair_decision.snapshot_content.clone();
                    let drift_fired = guard_ipc_snapshot_adoption_against_live_prompt_drift(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        baseline,
                        content_ours,
                        &mut repair_decision,
                    );
                    let dup_fired = guard_ipc_snapshot_adoption_against_prompt_duplication(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        content_ours,
                        &mut repair_decision,
                    );
                    log_ipc_snapshot_adoption_allowed(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        baseline,
                        content_ours,
                        &repair_decision,
                        drift_fired || dup_fired,
                    );
                    log_ipcfullprompt_corruption_if_any(
                        file,
                        "socket_ack_content",
                        Some(&patch_id),
                        baseline,
                        &ipcfullprompt_candidate,
                    );

                    let expected_response = response_materialization_probe(patches, unmatched);
                    if !ipc_response_materialized_or_fallback(
                        file,
                        "socket_ack_content",
                        &expected_response,
                        &repair_decision.snapshot_content,
                    ) {
                        repair_partial_response_materialization_before_fallback(
                            file,
                            "socket_ack_content",
                            &expected_response,
                        )?;
                        return Ok(IpcResult {
                            success: false,
                            patch_id,
                            skipped_committed_cycle: false,
                        });
                    }

                    eprintln!(
                        "[write] snapshot from {} ({} bytes)",
                        repair_decision.snap_source.label(),
                        repair_decision.snapshot_content.len()
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "ipc_socket_ack_content file={} patch_id={} snap_source={} sidecar_len={} sidecar_hash={} disk_len={} disk_hash={}",
                            file.display(),
                            patch_id,
                            repair_decision.snap_source.label(),
                            repair_decision.snapshot_content.len(),
                            crate::ops_log::content_hash(&repair_decision.snapshot_content),
                            ipc_before_content.as_deref().map(str::len).unwrap_or(0),
                            ipc_before_content
                                .as_deref()
                                .map(crate::ops_log::content_hash)
                                .unwrap_or_else(|| "-".to_string())
                        ),
                    );
                    if let Some(ref path) = fallback_patch_file {
                        let _ = std::fs::remove_file(path);
                    }
                    repair_ipc_decision_visible_state(file, &repair_decision, Some(&patch_id))?;
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "ipc_socket_delivered file={} snap_source={} snap_len={}",
                            file.display(),
                            repair_decision.snap_source.label(),
                            repair_decision.snapshot_content.len()
                        ),
                    );
                    if let Some(before) = ipc_before_content.as_deref() {
                        log_exchange_write_diagnostic(
                            file,
                            "try_ipc_socket",
                            "socket_ipc",
                            Some(&patch_id),
                            baseline,
                            before,
                            &repair_decision.snapshot_content,
                            patches,
                            unmatched,
                        );
                    }
                    if let Err(e) = snapshot::save(file, &repair_decision.snapshot_content) {
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
                                repair_decision.snapshot_content.len()
                            ),
                        );
                        let crdt_doc =
                            crate::crdt::CrdtDoc::from_text(&repair_decision.snapshot_content);
                        if let Err(e) = snapshot::save_crdt(file, &crdt_doc.encode_state()) {
                            eprintln!("[write] WARNING: CRDT state save failed: {}", e);
                        }
                    }
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                        skipped_committed_cycle: false,
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
                log_ipc_proof_failure(
                    file,
                    "socket_ipc",
                    Some(&patch_id),
                    "no_ack_content_sidecar",
                    "direct_write_fallback",
                    "ack_content_timeout=true",
                );
                if let Some(ref cycle_id) = cycle_already_committed(file) {
                    eprintln!(
                        "[write] socket IPC fallback: cycle {} already committed — skipping file IPC",
                        cycle_id
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
                            "ipc_socket_sidecar_timeout_skip_file_fallback file={} cycle_id={} reason=already_committed",
                            file.display(),
                            cycle_id
                        ),
                    );
                    cleanup_fallback_patch_files(file);
                    return Ok(IpcResult {
                        success: false,
                        patch_id,
                        skipped_committed_cycle: true,
                    });
                }
            }
            Ok(None) => {
                eprintln!("[write] socket IPC sent but no ack — falling back to file IPC");
            }
            Err(e) if crate::ipc_socket::is_already_applied_error(&e) => {
                // The plugin detected the response body is already present
                // in the live buffer and chose not to re-apply it. Re-writing
                // through the file-IPC fallback would create a duplicate
                // response. Treat as success and skip the fallback.
                // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
                // Phase 2.
                eprintln!(
                    "[write] socket IPC reported already_applied: {} — skipping file IPC fallback (response already in live buffer)",
                    e
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_skip_file_fallback file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
                let expected_response = response_materialization_probe(patches, unmatched);
                if persist_already_applied_socket_content_ours_snapshot(
                    file,
                    &patch_id,
                    baseline,
                    content_ours,
                    normalize_prefix_lines,
                    &expected_response,
                )? == AlreadyAppliedSnapshotOutcome::Persisted
                {
                    cleanup_fallback_patch_files(file);
                    return Ok(IpcResult {
                        success: true,
                        patch_id,
                        skipped_committed_cycle: false,
                    });
                }
                eprintln!(
                    "[write] socket already_applied could not prove the response on disk — falling back to file IPC"
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "ipc_socket_already_applied_fallback_to_file_ipc file={} patch_id={}",
                        file.display(),
                        patch_id
                    ),
                );
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
            skipped_committed_cycle: false,
        });
    }

    if let Some(ref cycle_id) = cycle_already_committed(file) {
        eprintln!(
            "[write] file IPC fallback: cycle {} already committed — skipping patch write",
            cycle_id
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
                "file_ipc_fallback_skip file={} cycle_id={} reason=already_committed",
                file.display(),
                cycle_id
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(IpcResult {
            success: false,
            patch_id,
            skipped_committed_cycle: true,
        });
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Build patches using shared helper (same logic as socket path). Seed the
    // boundary from patch_id so a later file/fallback rebuild reuses the same
    // boundary (#finalize-visible-buffer-ipc-timeout-race).
    let ipc_patches =
        build_ipc_patches_json(file, patches, unmatched, normalize_prefix_lines, Some(&patch_id))?;

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
    if let Ok(Some(ref cs)) = crate::cycle_state::load(file) {
        ipc_payload["cycle_id"] = serde_json::Value::String(cs.cycle_id.clone());
    }

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
        if ipc_patches.is_empty()
            && let Some(ours) = content_ours
            && full_content_ipc_scope_allows(
                file,
                FullContentIpcMode::ResponseFallback,
                &patch_id,
                ours,
                ipc_before_content.as_deref(),
                ipc_before_content.as_deref(),
            )
        {
            log_full_content_ipc_disabled(
                file,
                FullContentIpcMode::ResponseFallback,
                &patch_id,
                ours,
                ipc_before_content.as_deref(),
                ipc_before_content.as_deref(),
            );
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

    // Defense-in-depth dedupe gate for the file-IPC fallback when delivering
    // a response patch. When the plugin already applied the response via a
    // prior socket retry whose ack-write was slow, applying the same response
    // patch through file IPC would land a duplicate `### Re:` heading on top
    // of the live buffer.
    //
    // The socket-IPC path catches this via `ipc_socket::is_already_applied_error`
    // when the plugin sends `{"type":"ack","status":"error","reason":"already_applied"}`.
    // Until every plugin emits that ack (`#ipcpluginalready`), the file-IPC
    // fallback hash-compares response-patch outcomes against the current file:
    // if applying the response patches to the current file is a structural
    // no-op (boundary markers excluded), skip the write so the duplicate
    // cannot land.
    //
    // Scope: only response-bearing patches (contain at least one `### Re:`
    // heading). Pure prompt/component patches fall through to the existing
    // path, which has its own no-ack guard for unacknowledged live-edit IPC.
    //
    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 2 (remaining) / `[#ipcfilehashskip]`.
    if !patches.is_empty()
        && patches
            .iter()
            .any(|patch| patch.content.contains("### Re:"))
        && let Ok(current) = std::fs::read_to_string(file)
        && let Ok(after_apply) = crate::template::apply_patches(&current, patches, "", file)
        && strip_boundary_for_dedup(&after_apply) == strip_boundary_for_dedup(&current)
    {
        eprintln!(
            "[write] file IPC fallback: patches already present in live buffer — skipping file IPC write (defense-in-depth dedupe)"
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "file_ipc_fallback_skip_already_applied file={} patch_id={} patches={}",
                file.display(),
                patch_id,
                patches.len()
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(IpcResult {
            success: true,
            patch_id,
            skipped_committed_cycle: false,
        });
    }

    let success = write_ipc_and_poll(
        &patch_file,
        &ipc_payload,
        file,
        ipc_patches.len(),
        IpcPollOptions {
            content_ours,
            normalize_prefix_lines,
            project_root: &project_root,
            guard_committed_cycle: true,
        },
    )?;
    Ok(IpcResult {
        success,
        patch_id,
        skipped_committed_cycle: false,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullContentIpcMode {
    /// Late fallback repair for an agent response. Must not dirty an already
    /// committed cycle.
    ResponseFallback,
    /// Operator-owned replacement such as Compact Exchange. This is a new
    /// document mutation even when the previous response cycle is committed.
    OperatorMutation,
}

/// Disabled full-document editor IPC path.
///
/// This function intentionally never emits socket or file IPC payloads. It
/// keeps the terminal committed-cycle cleanup guard and diagnostic logging so
/// callers can fall back to the guarded disk/snapshot path without handing the
/// editor a whole-document replacement.
#[allow(dead_code)]
pub fn try_ipc_full_content(file: &Path, content: &str) -> Result<bool> {
    try_ipc_full_content_with_mode(file, content, FullContentIpcMode::ResponseFallback, None)
}

fn try_ipc_full_content_response_fallback_from_source(
    file: &Path,
    content: &str,
    source_content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        file,
        content,
        FullContentIpcMode::ResponseFallback,
        Some(source_content),
    )
}

#[allow(dead_code)]
pub fn try_ipc_full_content_operator_mutation(file: &Path, content: &str) -> Result<bool> {
    try_ipc_full_content_with_mode(file, content, FullContentIpcMode::OperatorMutation, None)
}

#[allow(dead_code)]
pub fn try_ipc_full_content_operator_mutation_from_source(
    file: &Path,
    content: &str,
    source_content: &str,
) -> Result<bool> {
    try_ipc_full_content_with_mode(
        file,
        content,
        FullContentIpcMode::OperatorMutation,
        Some(source_content),
    )
}

fn full_content_source_label(mode: FullContentIpcMode) -> &'static str {
    match mode {
        FullContentIpcMode::ResponseFallback => "response_fallback",
        FullContentIpcMode::OperatorMutation => "compact_exchange",
    }
}

fn log_full_content_ipc_disabled(
    file: &Path,
    mode: FullContentIpcMode,
    patch_id: &str,
    target_content: &str,
    source_content: Option<&str>,
    current_content: Option<&str>,
) {
    let source = full_content_source_label(mode);
    eprintln!(
        "[write] full-content IPC disabled for {}: falling back to guarded disk path",
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "full_content_ipc_disabled file={} source={} patch_id={} reason=disabled_by_default target_len={} target_hash={} source_len={} source_hash={} current_len={} current_hash={}",
            file.display(),
            source,
            patch_id,
            target_content.len(),
            crate::ops_log::content_hash(target_content),
            source_content.map(str::len).unwrap_or(0),
            source_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            current_content.map(str::len).unwrap_or(0),
            current_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string())
        ),
    );
}

fn frontmatter_mode_is_explicit_template(mode: &str) -> bool {
    matches!(
        mode.trim().to_ascii_lowercase().as_str(),
        "template" | "stream"
    )
}

fn content_declares_template_frontmatter(content: &str) -> bool {
    frontmatter::parse(content).ok().is_some_and(|(fm, _)| {
        fm.format == Some(frontmatter::AgentDocFormat::Template)
            || fm
                .mode
                .as_deref()
                .is_some_and(frontmatter_mode_is_explicit_template)
    })
}

fn content_has_agent_components(content: &str) -> bool {
    component::parse(content)
        .ok()
        .is_some_and(|components| !components.is_empty())
}

fn full_content_ipc_scope_rejection_reason(contents: &[Option<&str>]) -> Option<&'static str> {
    for content in contents.iter().flatten() {
        if content_declares_template_frontmatter(content) {
            return Some("template_frontmatter");
        }
        if content_has_agent_components(content) {
            return Some("agent_component_markers");
        }
    }
    None
}

fn full_content_ipc_scope_allows(
    file: &Path,
    mode: FullContentIpcMode,
    patch_id: &str,
    target_content: &str,
    source_content: Option<&str>,
    current_content: Option<&str>,
) -> bool {
    let reason = full_content_ipc_scope_rejection_reason(&[
        Some(target_content),
        source_content,
        current_content,
    ]);
    let Some(reason) = reason else {
        return true;
    };

    let source = full_content_source_label(mode);
    eprintln!(
        "[write] full-content IPC skipped for {}: {} is not eligible for whole-document editor replacement",
        file.display(),
        reason
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "full_content_ipc_scope_rejected file={} source={} patch_id={} scope={} target_len={} target_hash={} source_len={} source_hash={} current_len={} current_hash={}",
            file.display(),
            source,
            patch_id,
            reason,
            target_content.len(),
            crate::ops_log::content_hash(target_content),
            source_content.map(str::len).unwrap_or(0),
            source_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string()),
            current_content.map(str::len).unwrap_or(0),
            current_content
                .map(crate::ops_log::content_hash)
                .unwrap_or_else(|| "-".to_string())
        ),
    );
    false
}

fn try_ipc_full_content_with_mode(
    file: &Path,
    content: &str,
    mode: FullContentIpcMode,
    source_content: Option<&str>,
) -> Result<bool> {
    let _canonical = file.canonicalize()?;
    let before_content = std::fs::read_to_string(file).ok();
    let effective_source_content = match (mode, source_content) {
        (FullContentIpcMode::ResponseFallback, None) => Some(content),
        _ => source_content,
    };
    let patch_id = uuid::Uuid::new_v4().to_string();

    if mode == FullContentIpcMode::ResponseFallback
        && let Some(ref cycle_id) = cycle_already_committed(file)
    {
        eprintln!(
            "[write] full-content IPC skipped: cycle {} already committed for {}",
            cycle_id,
            file.display()
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
                "late_fallback_patch_rejected file={} cycle_id={} patch_id=full_content reason=already_committed transport=full_content",
                file.display(),
                cycle_id
            ),
        );
        cleanup_fallback_patch_files(file);
        return Ok(false);
    }

    if !full_content_ipc_scope_allows(
        file,
        mode,
        &patch_id,
        content,
        effective_source_content,
        before_content.as_deref(),
    ) {
        return Ok(false);
    }

    log_full_content_ipc_disabled(
        file,
        mode,
        &patch_id,
        content,
        effective_source_content,
        before_content.as_deref(),
    );
    Ok(false)
}

struct IpcPollOptions<'a> {
    content_ours: Option<&'a str>,
    normalize_prefix_lines: Option<&'a [String]>,
    project_root: &'a Path,
    guard_committed_cycle: bool,
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
    let snapshot_doc = crate::snapshot::load(file).ok().flatten();
    let working_doc = std::fs::read_to_string(file).ok();
    let boundary_id = snapshot_doc
        .as_deref()
        .and_then(|doc| find_boundary_id(doc, "exchange"))
        .or_else(|| {
            working_doc
                .as_deref()
                .and_then(|doc| find_boundary_id(doc, "exchange"))
        });
    let normalize_prefix_lines = match (snapshot_doc.as_deref(), working_doc.as_deref()) {
        (Some(committed), Some(working)) => {
            extract_post_commit_normalization_targets(committed, working)
        }
        _ => vec![],
    };

    if !crate::ipc_socket::is_listener_active(&project_root) {
        return match queue_file_ipc_reposition_boundary(
            file,
            boundary_id.as_deref(),
            &normalize_prefix_lines,
        ) {
            Ok(FileIpcRepositionResult::Queued) => true,
            Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
            Ok(FileIpcRepositionResult::Unavailable) => false,
            Err(e) => {
                eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                false
            }
        };
    }

    let result = if normalize_prefix_lines.is_empty() {
        crate::ipc_socket::send_reposition(
            &project_root,
            &canonical.to_string_lossy(),
            boundary_id.as_deref(),
            true, // preserve (HEAD) in editor buffer
        )
    } else {
        let mut message = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": [],
            "unmatched": "",
            "reposition_boundary": true,
            "preserve_head": true,
            "normalize_prefix_lines": normalize_prefix_lines.clone(),
        });
        if let Some(boundary_id) = boundary_id.as_deref() {
            message["reposition_boundary_id"] = serde_json::Value::String(boundary_id.to_string());
        }
        crate::ipc_socket::send_message(&project_root, &message).map(|_| true)
    };

    match result {
        Ok(true) => {
            if normalize_prefix_lines.is_empty() {
                eprintln!("[commit] IPC reposition boundary signal sent");
            } else {
                eprintln!(
                    "[commit] IPC prefix repair + boundary signal sent ({} lines)",
                    normalize_prefix_lines.len()
                );
            }
            true
        }
        Ok(false) => {
            eprintln!("[commit] IPC reposition: no ack (non-fatal)");
            match queue_file_ipc_reposition_boundary(
                file,
                boundary_id.as_deref(),
                &normalize_prefix_lines,
            ) {
                Ok(FileIpcRepositionResult::Queued) => true,
                Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
                Ok(FileIpcRepositionResult::Unavailable) => false,
                Err(e) => {
                    eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("[commit] IPC reposition failed (non-fatal): {}", e);
            match queue_file_ipc_reposition_boundary(
                file,
                boundary_id.as_deref(),
                &normalize_prefix_lines,
            ) {
                Ok(FileIpcRepositionResult::Queued) => true,
                Ok(FileIpcRepositionResult::DeferredExistingPatch) => true,
                Ok(FileIpcRepositionResult::Unavailable) => false,
                Err(e) => {
                    eprintln!("[commit] file IPC reposition queue failed (non-fatal): {e}");
                    false
                }
            }
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
    options: IpcPollOptions<'_>,
) -> Result<bool> {
    let before_content = std::fs::read_to_string(doc_file).ok();
    let patch_id_for_diagnostics = payload.get("patch_id").and_then(|value| value.as_str());
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
        if options.guard_committed_cycle
            && let Some(ref cycle_id) = cycle_already_committed(doc_file)
        {
            eprintln!(
                "[write] IPC poll skipped: cycle {} already committed for {}",
                cycle_id,
                doc_file.display()
            );
            log_closeout_guard(
                doc_file,
                crate::flow::types::FlowStage::TerminalGuard,
                crate::flow::types::FlowOutcome::Blocked,
                crate::flow::closeout::CloseoutGuardReason::AlreadyCommitted,
            );
            crate::ops_log::log_op(
                doc_file,
                &format!(
                    "file_ipc_poll_skip file={} cycle_id={} reason=already_committed",
                    doc_file.display(),
                    cycle_id
                ),
            );
            cleanup_fallback_patch_files(doc_file);
            return Ok(false);
        }
        if !patch_file.exists() {
            // Plugin consumed the patch — poll for ack-content sidecar (authoritative
            // post-apply snapshot). Falls back to file read after timeout.
            let patch_id = payload
                .get("patch_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (current_on_disk, mut repair_decision, ack_content_proven) = if !patch_id.is_empty()
            {
                match poll_ack_content_sidecar(
                    options.project_root,
                    patch_id,
                    std::time::Duration::from_millis(500),
                    std::time::Duration::from_millis(25),
                ) {
                    Ok(Some(content)) => {
                        let baseline = payload
                            .get("baseline")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.is_empty());
                        let decision = ipc_repair_decision_from_sidecar(
                            doc_file,
                            Some(patch_id),
                            baseline,
                            content,
                            options.content_ours,
                            options.normalize_prefix_lines,
                        );
                        if decision.snap_source == IpcSnapshotSource::AckContentSidecar {
                            eprintln!(
                                "[write] snapshot from ack-content sidecar ({} bytes)",
                                decision.snapshot_content.len()
                            );
                        }
                        let ack_content_proven = decision.ack_content_proven();
                        let snapshot_content = decision.snapshot_content.clone();
                        (snapshot_content, decision, ack_content_proven)
                    }
                    _ => {
                        eprintln!(
                            "[write] snapshot from file read (ack-content sidecar not available after 500ms)"
                        );
                        let content = std::fs::read_to_string(doc_file).unwrap_or_default();
                        let decision = IpcRepairDecision::file_read(content.clone());
                        (content, decision, false)
                    }
                }
            } else {
                eprintln!("[write] snapshot from file read (no patch_id for sidecar lookup)");
                let content = std::fs::read_to_string(doc_file).unwrap_or_default();
                let decision = IpcRepairDecision::file_read(content.clone());
                (content, decision, false)
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

            if let Some(full_content) = payload
                .get("fullContent")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                && current_on_disk != full_content
            {
                eprintln!(
                    "[write] IPC full-content patch consumed but final content does not match payload — falling back to disk write."
                );
                crate::ops_log::log_op(
                    doc_file,
                    &format!(
                        "full_content_ipc_post_apply_mismatch file={} expected_len={} actual_len={}",
                        doc_file.display(),
                        full_content.len(),
                        current_on_disk.len()
                    ),
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
            let expected_response = response_materialization_probe_from_ipc_payload(payload);
            if !ipc_response_materialized_or_fallback(
                doc_file,
                "file_ipc",
                &expected_response,
                &current_on_disk,
            ) {
                repair_partial_response_materialization_before_fallback(
                    doc_file,
                    "file_ipc",
                    &expected_response,
                )?;
                return Ok(false);
            }
            if file_ipc_consumed_without_live_exchange_ack(
                doc_file,
                "file_ipc",
                Some(patch_id),
                payload
                    .get("baseline")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty()),
                before_content.as_deref(),
                &current_on_disk,
                ack_content_proven,
            ) {
                return Ok(false);
            }

            // Plugin applied the patch — update snapshot as actual post-write disk state.
            // `current_on_disk` is from ack-content sidecar when available, or 200ms file read.
            // Bug 2A fix: snapshot save failure after IPC success is non-fatal.
            let pre_dedupe_content = repair_decision.snapshot_content.clone();
            let (snap_content, dedupe_repair) = dedupe_ipc_snapshot_content(
                doc_file,
                before_content.as_deref(),
                &repair_decision.snapshot_content,
                repair_decision.snap_source.label(),
            )?;
            if dedupe_repair {
                repair_decision =
                    repair_decision.apply_ipc_dedupe(snap_content, pre_dedupe_content);
            } else {
                repair_decision.snapshot_content = snap_content;
            }
            if file_ipc_consumed_without_live_exchange_ack(
                doc_file,
                "file_ipc",
                Some(patch_id),
                payload
                    .get("baseline")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty()),
                before_content.as_deref(),
                &repair_decision.snapshot_content,
                ack_content_proven,
            ) {
                return Ok(false);
            }
            let file_baseline = payload
                .get("baseline")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty());
            // Capture the live editor buffer before the guards replace it, so the
            // #ipcfullprompt forensic detector sees the candidate.
            let ipcfullprompt_candidate = repair_decision.snapshot_content.clone();
            let drift_fired = guard_ipc_snapshot_adoption_against_live_prompt_drift(
                doc_file,
                "file_ipc",
                Some(patch_id),
                file_baseline,
                options.content_ours,
                &mut repair_decision,
            );
            let dup_fired = guard_ipc_snapshot_adoption_against_prompt_duplication(
                doc_file,
                "file_ipc",
                Some(patch_id),
                options.content_ours,
                &mut repair_decision,
            );
            log_ipc_snapshot_adoption_allowed(
                doc_file,
                "file_ipc",
                Some(patch_id),
                file_baseline,
                options.content_ours,
                &repair_decision,
                drift_fired || dup_fired,
            );
            log_ipcfullprompt_corruption_if_any(
                doc_file,
                "file_ipc",
                Some(patch_id),
                file_baseline,
                &ipcfullprompt_candidate,
            );
            repair_ipc_decision_visible_state(doc_file, &repair_decision, Some(patch_id))?;
            crate::ops_log::log_op(
                doc_file,
                &format!(
                    "ipc_file_delivered file={} snap_len={}",
                    doc_file.display(),
                    repair_decision.snapshot_content.len()
                ),
            );
            if let Some(before) = before_content.as_deref() {
                let patch_id = payload.get("patch_id").and_then(|value| value.as_str());
                let baseline = payload
                    .get("baseline")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty());
                let payload_patches: Vec<template::PatchBlock> = payload
                    .get("patches")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| {
                                let name = item
                                    .get("component")
                                    .or_else(|| item.get("name"))
                                    .and_then(|value| value.as_str())?;
                                let content =
                                    item.get("content").and_then(|value| value.as_str())?;
                                Some(template::PatchBlock::new(name, content))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let unmatched = payload
                    .get("unmatched")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                log_exchange_write_diagnostic(
                    doc_file,
                    "write_ipc_and_poll",
                    "file_ipc",
                    patch_id,
                    baseline,
                    before,
                    &repair_decision.snapshot_content,
                    &payload_patches,
                    unmatched,
                );
            }
            if let Err(e) = snapshot::save(doc_file, &repair_decision.snapshot_content) {
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
                        repair_decision.snapshot_content.len()
                    ),
                );
                let crdt_doc = crate::crdt::CrdtDoc::from_text(&repair_decision.snapshot_content);
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
    log_ipc_proof_failure(
        doc_file,
        "file_ipc",
        patch_id_for_diagnostics,
        "no_ack",
        "direct_write_fallback",
        &format!(
            "timeout_secs={} patch_file={}",
            timeout.as_secs(),
            patch_file.display()
        ),
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
    let mut remaining = normalization_target_counts(prefix_lines);
    let mut result = String::with_capacity(content.len() + 2 * prefix_lines.len());
    for line in content.lines() {
        let bare = line
            .trim_end()
            .strip_prefix("\u{276f} ")
            .unwrap_or(line.trim_end());
        if crate::diff::line_looks_like_plain_response_after_prompt(bare) {
            result.push_str(line);
            result.push('\n');
            continue;
        }
        if !line.starts_with("\u{276f} ")
            && let Some(remaining_count) = remaining.get_mut(bare)
            && *remaining_count > 0
        {
            result.push_str("\u{276f} ");
            *remaining_count -= 1;
        }
        result.push_str(line);
        result.push('\n');
    }
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.truncate(result.len() - 1);
    }
    result
}

fn normalization_target_counts(
    prefix_lines: &[String],
) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for line in prefix_lines {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        *counts.entry(trimmed.to_string()).or_default() += 1;
    }
    counts
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
    boundary_seed: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let raw_doc = std::fs::read_to_string(file).unwrap_or_default();
    let summary = file.file_stem().and_then(|s| s.to_str());
    // #finalize-visible-buffer-ipc-timeout-race: when a stable seed (the IPC
    // patch_id) is supplied, derive a deterministic boundary so this write's
    // socket / file / fallback rebuilds all carry the SAME boundary. Without it,
    // each rebuild minted a fresh random boundary and the plugin appended the
    // response a second time, doubling the editor buffer.
    let current_doc = match boundary_seed {
        Some(seed) => {
            let bid = agent_doc_core::id::boundary_id_from_seed_with_summary(seed, summary);
            template::reposition_boundary_to_end_clean_with_summary_and_id(
                &raw_doc,
                Some(&bid),
                summary,
            )
        }
        None => template::reposition_boundary_to_end_clean_with_summary(&raw_doc, summary),
    };

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
                let synthesized_content = match normalize_prefix_lines {
                    Some(prefix_lines)
                        if !prefix_lines.is_empty() && is_append_mode_component(target) =>
                    {
                        normalize_patch_content(&effective_unmatched, prefix_lines)
                    }
                    _ => effective_unmatched.clone(),
                };
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (boundary {})",
                    target,
                    &bid[..8.min(bid.len())]
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": synthesized_content,
                    "boundary_id": bid,
                }));
                break;
            } else if is_append_mode_component(target) {
                let synthesized_content = match normalize_prefix_lines {
                    Some(prefix_lines) if !prefix_lines.is_empty() => {
                        normalize_patch_content(&effective_unmatched, prefix_lines)
                    }
                    _ => effective_unmatched.clone(),
                };
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (ensure_boundary)",
                    target
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": synthesized_content,
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
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn free_text_head_struck_despite_prompt_prefix_flip_on_answered_prompt() {
        // #free-text-head-consume-genuine-not-struck: the consume decision diffs
        // the normalized snapshot baseline against the LIVE editor buffer. The
        // buffer preserves `❯` prefixes on already-answered prompts that the
        // snapshot normalized to the bare form. A pure `do x` → `❯ do x`
        // prefix flip then surfaces as an added `+❯ …` diff line. It must
        // NOT be read as a new foreign prompt — that wrongly blocked the
        // free-text head strike and stalled the auto-loop.
        let head = "Evaluate axocoatl thing";
        let baseline = concat!(
            "<!-- agent:exchange -->\n",
            "do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        // Live buffer: the prior prompt regained its `❯` prefix; this cycle
        // only added the `### Re: axocoatl` answer.
        let prefix_flip = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "### Re: axocoatl\n",
            "plan written.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), prefix_flip, head),
            "a `❯` prefix flip on an already-answered baseline prompt is not new foreign work"
        );

        // A genuinely new `❯` prompt whose text never appeared at baseline still
        // counts as foreign work, keeping the free-text head queued.
        let genuine_foreign = concat!(
            "<!-- agent:exchange -->\n",
            "do earlier task\n",
            "### Re: earlier\n",
            "answered.\n",
            "❯ a brand new unrelated prompt\n",
            "### Re: axocoatl\n",
            "plan.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n",
            "- Evaluate axocoatl thing\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            cycle_answered_foreign_exchange_prompt(Some(baseline), genuine_foreign, head),
            "a genuinely new unrelated `❯` prompt absent from baseline is foreign work"
        );
    }

    // #queue-strike-on-halt: queue head consumption requires an explicit
    // closeout flag, not a `### Re:` heading that merely names the head.
    const HALT_QUEUE_DOC: &str = concat!(
        "---\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#foo]\n",
        "- do [#bar]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn explicit_signal_halt_without_flag_does_not_consume() {
        // (a) Halt response, no --done/--pending-gate/--pending-edit → no consume.
        assert!(!queue_head_has_explicit_completion_signal(HALT_QUEUE_DOC, &[], &[], &[]).unwrap());
    }

    #[test]
    fn explicit_signal_done_flag_consumes() {
        // (b) --done naming the head → consume. (c) also covers no-heading + --done.
        assert!(
            queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &["foo".to_string()],
                &[],
                &[],
            )
            .unwrap()
        );
    }

    #[test]
    fn explicit_signal_gate_and_edit_flags_consume() {
        assert!(
            queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &[],
                &["foo".to_string()],
                &[],
            )
            .unwrap(),
            "--pending-gate naming the head is a completion signal"
        );
        assert!(
            queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &[],
                &[],
                &["foo=rewritten text".to_string()],
            )
            .unwrap(),
            "--pending-edit naming the head is a completion signal"
        );
    }

    #[test]
    fn explicit_signal_flag_for_other_id_does_not_consume() {
        assert!(
            !queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &["bar".to_string()],
                &["baz".to_string()],
                &["qux=text".to_string()],
            )
            .unwrap(),
            "flags for non-head ids must not consume the head"
        );
    }

    #[test]
    fn explicit_signal_none_when_queue_inactive() {
        let inactive = HALT_QUEUE_DOC.replace("queue_active: true", "queue_active: false");
        assert!(
            !queue_head_has_explicit_completion_signal(&inactive, &["foo".to_string()], &[], &[],)
                .unwrap()
        );
    }

    #[test]
    fn done_head_consumes_despite_bundled_pending_add() {
        // #pending-add-suppresses-queue-consume: a finalize that completes the
        // queue head with --done must still consume it even when --pending-add
        // added a new backlog item in the same diff. The bundled add makes the
        // diff-based "active prompt" check return false, but the explicit --done
        // short-circuit authorizes consumption regardless.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let baseline = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#foo] head work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n- do [#bar]\n",
            "<!-- /agent:queue -->\n",
        );
        // Current = baseline + a bundled --pending-add backlog item (the diff
        // shape that used to suppress consumption).
        let current = baseline.replace(
            "- [ ] [#foo] head work\n",
            "- [ ] [#newitem] bundled follow-up\n- [ ] [#foo] head work\n",
        );
        std::fs::write(&doc, &current).unwrap();
        assert!(
            should_consume_queue_prompt_for_write(
                &doc,
                Some(baseline),
                &current,
                &["foo".to_string()],
            )
            .unwrap(),
            "--done naming the head must consume despite a bundled --pending-add"
        );
        // Without an explicit completion flag, the bare do[#id] head is NOT
        // consumed by the diff alone (#queue-strike-on-halt).
        assert!(
            !should_consume_queue_prompt_for_write(&doc, Some(baseline), &current, &[]).unwrap(),
            "bare do[#id] head needs an explicit completion flag"
        );
    }

    #[test]
    fn free_text_queue_head_detection() {
        // #free-text-queue-head-consume: a plain question typed into the queue
        // has no #id and is not a do-directive/preset/trigger → free text.
        let doc = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Is tsift properly integrated into multi-crate architecture?\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(doc).unwrap(),
            "a no-#id queue head is free text and consumable by being answered"
        );
        // A bare do[#id] head is NOT free text (needs an explicit completion flag).
        assert!(!queue_head_is_free_text_prompt(HALT_QUEUE_DOC).unwrap());
        // A #preset head carries an #id, so it is not free text either.
        let preset = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(!queue_head_is_free_text_prompt(preset).unwrap());
        // Inactive queue → no head → not free text.
        let inactive = doc.replace("queue_active: true", "queue_active: false");
        assert!(!queue_head_is_free_text_prompt(&inactive).unwrap());

        // #free-text-queue-owner-consume: a free-text head that MENTIONS ids in
        // prose (but is not a pure id directive) is still free text — it has no
        // single id to `--done`, so it must complete on being answered. This is
        // the live repro head from src/boost-client/tasks/monsterrodholders.md.
        let id_mentioning = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Approve [#shoptiers]. What are #next-steps?\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(id_mentioning).unwrap(),
            "a free-text head that merely mentions #ids must stay free text (consumable by being answered)"
        );

        // A leading action verb + bracketed id alone (`re [#id]`) is NOT a pure
        // `#id`/`[#id]`/`do [#id]` directive, so it is treated as free text and
        // completes on answer (it still has a single mentioned id, but the verb
        // makes it prose, not a bare directive).
        let verb_prefixed = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Summarize the findings for #report and ship it\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            queue_head_is_free_text_prompt(verb_prefixed).unwrap(),
            "a prose head mentioning a single #id is still free text"
        );
    }

    // #queue-consume-on-stream-ipc-timeout: the shared decision used by both the
    // strict closeout and the stream IPC-timeout `exit(75)` closeout. Mirrors the
    // exact scenario that treadmilled the auto-loop: a free-text head answered by
    // a finalize response whose write fell back to direct disk on IPC timeout.
    #[test]
    fn queue_consume_reconciles_diverged_snapshot_instead_of_bailing() {
        // #finalize-divergence-orphans-committed-head / IPC-CRDT resilience: when
        // the post-merge document queue diverges from the snapshot queue (a
        // concurrent user/editor edit the CRDT merge already reconciled), consume
        // must RECONCILE (the merged document wins) and strike the head — not
        // hard-bail and orphan the unstruck head. Regression for the divergence
        // error hit repeatedly under live editor races.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: do the thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the thing\n",
            "- user added later\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // Snapshot diverges: same head, but missing the concurrently-added item.
        let snap = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: do the thing\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do the thing\n",
            "<!-- /agent:queue -->\n",
        );
        snapshot::save(&doc, snap).unwrap();

        let outcome = consume_queue_prompt_force_disk(&doc)
            .expect("consume must not bail on a reconcilable divergence");
        assert!(outcome.is_some(), "the answered head should be consumed");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- ~do the thing~"),
            "head must be struck after reconcile:\n{result}"
        );
        assert!(
            result.contains("- user added later"),
            "the concurrently-added item must be preserved (document wins):\n{result}"
        );
    }

    #[test]
    fn consume_decision_strikes_answered_free_text_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: JB Run Agent Doc should start the queue\n\nFixed in route.rs.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- JB `Run Agent Doc` on a `queue: stop` + `agent:queue go` doc should start the queue.\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // baseline == current (no new exchange prompt this cycle), non-empty
        // response → the free-text head is answered and must be consumed.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: JB Run Agent Doc should start the queue\n\nFixed.",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "an answered free-text head must be consumed even on the IPC-timeout closeout"
        );
    }

    #[test]
    fn consume_decision_strikes_synthetic_preset_head_on_heading_match() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: #spec-test-build-install-commit-push\n\nDone.",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "a preset head answered by a matching heading id must be consumed"
        );
    }

    #[test]
    fn consume_decision_keeps_bare_do_id_head_without_explicit_flag() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        // A bare do[#id] head is halt-safe: a response that does not record an
        // explicit --done/--gate/--edit outcome must NOT strike it.
        assert!(
            !queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: not doing this, here is why",
                &[],
                &[],
                &[],
            )
            .unwrap(),
            "a bare do[#id] head must stay queued without an explicit completion flag"
        );
        // The same head WITH --done foo is consumed.
        assert!(
            queue_consumption_allowed_for_response(
                &doc,
                Some(content),
                content,
                "### Re: do [#foo]\n\nDone.",
                &["foo".to_string()],
                &[],
                &[],
            )
            .unwrap(),
            "--done naming the head id must consume it"
        );
    }

    #[test]
    fn free_text_head_kept_only_when_cycle_answered_foreign_prompt() {
        // #queue-head-struck-on-foreign-exchange-answer: the predicate that gates
        // free-text head consumption. A drain cycle (only this turn's `### Re:`
        // response added, no new user prompt) is NOT foreign → head drains. A
        // cycle that added a NEW unrelated `❯` exchange prompt IS foreign → the
        // free-text head stays queued so its work is not silently struck.
        let head = "lazily-rs plan-update";
        let baseline = "\
---
agent_doc_format: template
queue_active: true
---

<!-- agent:exchange -->
### Re: older
Old.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- lazily-rs plan-update
<!-- /agent:queue -->
";
        let drain = baseline.replace(
            "<!-- agent:boundary:x -->",
            "### Re: updated the plan\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            !cycle_answered_foreign_exchange_prompt(Some(baseline), &drain, head),
            "a drain cycle (only a new response, no new prompt) is not foreign work"
        );

        let foreign = baseline.replace(
            "<!-- agent:boundary:x -->",
            "❯ Fix the JB cache conflict instead\n### Re: fix jb\nDone.\n<!-- agent:boundary:x -->",
        );
        assert!(
            cycle_answered_foreign_exchange_prompt(Some(baseline), &foreign, head),
            "a cycle that added a new unrelated exchange prompt answered foreign work"
        );
    }

    #[test]
    fn queue_skip_diagnostic_names_head_shape_and_repair_path() {
        let id_backed = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#foo]\n",
            "<!-- /agent:queue -->\n",
        );
        let id_message = queue_skip_diagnostic_for_content(id_backed).unwrap();
        assert!(id_message.contains("[queue] kept head `do #foo`"));
        assert!(id_message.contains("`--done foo`"));
        assert!(id_message.contains("`--pending-gate foo`"));
        assert!(id_message.contains("`--pending-edit \"foo=...\"`"));
        assert!(id_message.contains("missing proof"));

        let free_text = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Review the queue diagnostics\n",
            "<!-- /agent:queue -->\n",
        );
        let free_text_message = queue_skip_diagnostic_for_content(free_text).unwrap();
        assert!(
            free_text_message
                .contains("[queue] kept free-text head `Review the queue diagnostics`")
        );
        assert!(free_text_message.contains("answered-response path"));
    }

    #[test]
    fn heading_topic_matches_head_exactly_or_by_exact_id() {
        // Codex Stop-hook path: exact-topic match, or a topic that resolves to
        // EXACTLY the head id (#queue-head-consume-on-topic-id-regression).
        assert!(response_topic_matches_queue_head("do [#foo]", "do [#foo]"));
        assert!(response_topic_matches_queue_head("#fix1", "do #fix1"));
        assert!(response_topic_matches_queue_head("#foo", "do [#foo]"));
        // Halt/modifier headings must NOT count as completion (#queue-strike-on-halt).
        assert!(!response_topic_matches_queue_head("#foo halt", "do [#foo]"));
        assert!(!response_topic_matches_queue_head(
            "#foo deferred",
            "do [#foo]"
        ));
    }

    #[test]
    fn bare_do_directive_detection() {
        // Queue parser strips the `- ` bullet, so heads arrive as `do [#id]`.
        assert!(queue_head_is_bare_do_directive("do [#foo]"));
        assert!(queue_head_is_bare_do_directive("do #foo"));
        // A synthetic/preset prompt carrying a trailing `#preset` id is NOT a
        // bare directive.
        assert!(!queue_head_is_bare_do_directive(
            "JB Run Agent Doc on tsift.md add the prompt into agent:queue.\n#spec-test-build-install-commit-push"
        ));
        // A bare preset id on its own line is also not a `do` directive.
        assert!(!queue_head_is_bare_do_directive(
            "#spec-test-build-install-commit-push"
        ));
    }

    #[test]
    fn topic_resolves_to_exact_id_rejects_modifiers() {
        assert!(topic_resolves_to_exact_id(
            "#spec-test-build-install-commit-push",
            "spec-test-build-install-commit-push"
        ));
        assert!(topic_resolves_to_exact_id("do [#foo]", "foo"));
        assert!(topic_resolves_to_exact_id("#Foo", "foo")); // case-insensitive
        // Trailing modifiers (#queue-strike-on-halt) must never resolve to the id.
        assert!(!topic_resolves_to_exact_id("#foo halt", "foo"));
        assert!(!topic_resolves_to_exact_id("#foo deferred", "foo"));
        assert!(!topic_resolves_to_exact_id("#other", "foo"));
    }

    fn patch_with_heading(heading: &str) -> crate::template::PatchBlock {
        crate::template::PatchBlock::new("exchange", format!("{heading}\n\nbody line one\n"))
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_captures_unowned_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        // candidate (disk / ack sidecar) carries the user's freshly-typed "go".
        let candidate = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );
        // content_ours (baseline + response, no user edits) does NOT have "go".
        let content_ours = baseline;

        let dropped = dropped_prompt_lines_after_content_ours(baseline, &candidate, content_ours);
        assert_eq!(dropped, vec!["go".to_string()]);
    }

    #[test]
    fn dropped_prompt_lines_after_content_ours_empty_when_content_ours_owns_prompt() {
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:b0 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let with_go = baseline.replace(
            "<!-- agent:boundary:b0 -->",
            "go\n<!-- agent:boundary:b0 -->",
        );
        // Both candidate and content_ours contain "go" → nothing is dropped.
        let dropped = dropped_prompt_lines_after_content_ours(baseline, &with_go, &with_go);
        assert!(dropped.is_empty());
    }

    #[test]
    fn extract_response_headings_returns_re_lines_in_order() {
        let patches = vec![
            patch_with_heading("### Re: first topic — opus-4-7"),
            patch_with_heading("### Re: second topic — opus-4-7"),
            // Patch with no Re: heading should be skipped.
            crate::template::PatchBlock::new("status", "Just a status update.\n"),
        ];
        let headings = extract_response_headings_from_patches(&patches);
        assert_eq!(
            headings,
            vec![
                "### Re: first topic — opus-4-7".to_string(),
                "### Re: second topic — opus-4-7".to_string(),
            ]
        );
    }

    #[test]
    fn extract_response_headings_picks_first_re_per_patch() {
        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: outer — opus-4-7\n\nbody mentioning ### Re: inner — opus-4-7 elsewhere\n",
        );
        let headings = extract_response_headings_from_patches(&[patch]);
        assert_eq!(headings, vec!["### Re: outer — opus-4-7".to_string()]);
    }

    #[test]
    fn materialization_probe_uses_patch_body_not_patch_markers() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: materialized — gpt-5\n\n",
            "Committed through boundary insertion.\n",
            "<!-- /patch:exchange -->\n",
        );

        let probe = response_materialization_probe_from_response(response);

        assert!(probe.contains("### Re: materialized — gpt-5"));
        assert!(!probe.contains("<!-- patch:exchange -->"));
        assert!(!probe.contains("<!-- /patch:exchange -->"));
    }

    #[test]
    fn patch_wrapped_response_is_materialized_by_visible_patch_body() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- /patch:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_materialized_in_content(response, content));
    }

    #[test]
    fn marker_bearing_zero_patch_parse_is_rejected_before_capture() {
        let err = reject_marker_response_with_zero_patches(1, 0).unwrap_err();

        assert!(
            err.to_string()
                .contains("parsed zero patches despite 1 patch marker")
        );
        assert!(reject_marker_response_with_zero_patches(0, 0).is_ok());
        assert!(reject_marker_response_with_zero_patches(2, 1).is_ok());
    }

    #[test]
    fn patch_response_headings_already_in_head_true_when_no_patches() {
        // Empty patch list — conservatively preserve the existing late-fallback
        // gate behavior (reject when no response evidence is present).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "doc body\n").unwrap();
        assert!(patch_response_headings_already_in_head(&doc, &[]));
    }

    fn init_repo_with_doc(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["init", "-q", "--initial-branch=main"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["config", "user.name", "Test"])
            .status()
            .unwrap();
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["add", name])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(dir)
            .args(["commit", "-q", "-m", "seed"])
            .status()
            .unwrap();
        path
    }

    #[test]
    fn patch_response_headings_already_in_head_true_when_heading_in_head() {
        let dir = TempDir::new().unwrap();
        let doc = init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: shipped — opus-4-7\n\nbody\n",
        );
        let patch = patch_with_heading("### Re: shipped — opus-4-7");
        assert!(patch_response_headings_already_in_head(&doc, &[patch]));
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_heading_missing_from_head() {
        // Mid-turn rotation signature: HEAD has been advanced by a different
        // operation (compact, sibling commit) and does not yet contain the
        // response we're about to apply. The late-fallback gate must allow
        // the patch through.
        let dir = TempDir::new().unwrap();
        let doc = init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: prior cycle — opus-4-7\n\nold\n",
        );
        let patch = patch_with_heading("### Re: new response — opus-4-7");
        assert!(
            !patch_response_headings_already_in_head(&doc, &[patch]),
            "mid-turn rotation must allow the patch (response not in HEAD)"
        );
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_any_heading_missing() {
        let dir = TempDir::new().unwrap();
        let doc = init_repo_with_doc(
            dir.path(),
            "session.md",
            "## Exchange\n\n### Re: first — opus-4-7\n\nbody\n",
        );
        let patches = vec![
            patch_with_heading("### Re: first — opus-4-7"),
            patch_with_heading("### Re: second — opus-4-7"),
        ];
        assert!(
            !patch_response_headings_already_in_head(&doc, &patches),
            "all headings must be in HEAD for the gate to skip"
        );
    }

    #[test]
    fn patch_response_headings_already_in_head_false_when_file_not_in_git() {
        // No git repo → show_head returns Ok(None). Fail-safe: treat as not
        // in HEAD so the late-fallback gate rotates the cycle rather than
        // rejecting the patch.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, "no git\n").unwrap();
        let patch = patch_with_heading("### Re: something — opus-4-7");
        assert!(!patch_response_headings_already_in_head(&doc, &[patch]));
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
    fn visible_write_guard_blocks_when_editor_typing_active() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/typing")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "body\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::document_changed(&doc_str);

        let err = guard_visible_write_idle_with_budget(&doc, "test_visible_write", 60_000, 0)
            .unwrap_err();

        assert!(err.to_string().contains("editor typing did not settle"));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=visible_write_typing_defer_active_typing:test_visible_write"));
        assert!(log.contains("visible_write_deferred_active_typing"));
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
        crate::debounce::record_live_buffer_digest(
            &doc_str,
            live_buffer.len(),
            &crate::debounce::content_hash(&live_buffer),
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
    fn capture_locked_pre_response_reads_live_content_after_lock_wait() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "original\n").unwrap();

        let lock_path = snapshot::lock_path_for(&doc).unwrap();
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

        let old_hash = snapshot::doc_hash(&old_doc).unwrap();
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
        let migrated_baseline = snapshot::baseline_path_for(&new_doc).unwrap();
        assert!(migrated_baseline.exists());

        let baseline = read_explicit_baseline(&new_doc, Some(&old_baseline))
            .unwrap()
            .expect("baseline should be recovered from migrated hash");
        assert_eq!(baseline, "preflight baseline\n");
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
        apply_template_from_string(&doc, response).unwrap();

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

        apply_template_from_string(&doc, response).unwrap();

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
    fn explicit_baseline_preserves_concurrent_user_edits_for_next_cycle() {
        let baseline = Some("baseline");
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::ContentOurs
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            content_ours
        );
    }

    #[test]
    fn explicit_baseline_preserves_concurrent_comment_tail_for_next_cycle() {
        let baseline = Some("baseline");
        let base = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let content_current = "<!-- agent:exchange -->\n❯ prompt\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";
        let content_ours = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nold note\n-->\n";
        let final_content = "<!-- agent:exchange -->\n❯ prompt\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n###\n\n<!--\nedited note\n-->\n";

        assert_eq!(
            snapshot_persist_mode_with_current(
                baseline,
                base,
                content_current,
                content_ours,
                final_content
            ),
            SnapshotPersistMode::ContentOurs
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode_with_current(
                    baseline,
                    base,
                    content_current,
                    content_ours,
                    final_content
                ),
                content_ours,
                final_content
            ),
            content_ours
        );
    }

    #[test]
    fn implicit_baseline_still_persists_final_merged_disk_state() {
        let content_ours =
            "<!-- agent:exchange -->\n### Re: answer\nDone.\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: answer\nDone.\n❯ late follow-up\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(None, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(None, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
        );
    }

    #[test]
    fn explicit_baseline_keeps_final_content_when_delta_is_prior_streamed_agent_prefix() {
        let baseline = Some("baseline");
        let content_ours = "<!-- agent:exchange -->\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";
        let final_content = "<!-- agent:exchange -->\n### Re: orchestrate streaming — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /agent:exchange -->\n";

        assert_eq!(
            snapshot_persist_mode(baseline, content_ours, final_content),
            SnapshotPersistMode::FinalContent
        );
        assert_eq!(
            snapshot_content_to_persist(
                snapshot_persist_mode(baseline, content_ours, final_content),
                content_ours,
                final_content
            ),
            final_content
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
        let patch = crate::template::PatchBlock::new("exchange", exchange_body);

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
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

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
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=no_ack")
                && log.contains("recovery=direct_write_fallback"),
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

        let patch = crate::template::PatchBlock::new(
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
                && log.contains("recovery=direct_write_fallback"),
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
                && log.contains("recovery=direct_write_fallback"),
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

        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let ack_dir = agent_doc_dir.join("ack-content");
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
                        let _ = fs::write(ack_dir.join(format!("{pid}.md")), &ack_for_watcher);
                    }
                    let _ = fs::remove_file(entry.path());
                    return;
                }
            }
        });

        let patch =
            crate::template::PatchBlock::new("exchange", "### Re: live prompt — gpt-5\n\nHandled.");
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
            before,
            "this regression models the sidecar-only path where the editor proved the apply before disk caught up"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            !log.contains("file_ipc_live_exchange_unacknowledged"),
            "ack-content proof must bypass the unacknowledged live-edit fallback:\n{log}"
        );
    }

    #[test]
    fn file_ipc_ack_content_live_prompt_drift_uses_content_ours_snapshot() {
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
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
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

        let patch = crate::template::PatchBlock::new(
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
            "snapshot must not absorb prompt-bearing drift typed after preflight"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            ack_content,
            "visible live prompt should remain in the working tree for the next cycle"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("flow=document_mutation")
                && log.contains("stage=ipc_snapshot_adoption")
                && log.contains("reason=live_prompt_drift_after_preflight")
                && log.contains("ipc_snapshot_adoption_blocked"),
            "unsafe snapshot adoption should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=live_prompt_drift_after_preflight")
                && log.contains("recovery=content_ours_snapshot_next_cycle"),
            "live prompt drift should name its failed invariant and recovery:\n{log}"
        );
    }

    #[test]
    fn ipc_snapshot_adoption_allowed_logs_benign_recheck() {
        // Every adoption that the fail-closed guards did NOT block must still leave
        // a diagnostic so a corruption slipping through as "allowed" is traceable.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n",
            "<!-- /agent:exchange -->\n",
        );
        let content_ours = concat!(
            "---\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Q\n### Re: Q — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -->\n",
        );
        let decision = IpcRepairDecision::content_ours(content_ours.to_string());

        log_ipc_snapshot_adoption_allowed(
            &doc,
            "socket_ack_content",
            Some("pid-allowed"),
            Some(baseline),
            Some(content_ours),
            &decision,
            false,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_snapshot_adoption_allowed")
                && log.contains("source=socket_ack_content")
                && log.contains("patch_id=pid-allowed")
                && log.contains("drift_recheck=false")
                && log.contains("dup_growth_recheck=0"),
            "allowed adoption must log a benign re-check:\n{log}"
        );
    }

    #[test]
    fn ipc_snapshot_adoption_allowed_is_silent_when_blocked() {
        // Blocked adoptions log their own rich diagnostic; the allowed line must not
        // also fire (it would falsely report an unguarded adoption).
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let decision = IpcRepairDecision::content_ours("snapshot".to_string());
        log_ipc_snapshot_adoption_allowed(
            &doc,
            "file_ipc",
            Some("pid-blocked"),
            None,
            None,
            &decision,
            true,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipc_snapshot_adoption_allowed"),
            "allowed diagnostic must stay silent once a guard fired:\n{log}"
        );
    }

    #[test]
    fn ipcfullprompt_corruption_logged_on_deleted_response() {
        // #ipcfullprompt-recur2: a live editor buffer (candidate) that dropped a
        // previously-committed `### Re:` block must leave a forensic ops.log line
        // and preserve the baseline + candidate for analysis — default-on capture,
        // no manual editor debug opt-in required.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );
        // candidate dropped the second response block.
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_ack_content",
            Some("pid-corrupt"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("source=socket_ack_content")
                && log.contains("patch_id=pid-corrupt")
                && log.contains("deleted=1")
                && log.contains("response_deleted(### Re: second — opus-4-8:1->0)"),
            "deleted prior response must be captured:\n{log}"
        );
        let forensic_dir = agent_doc_dir.join("logs/ipcfullprompt");
        let preserved: Vec<_> = fs::read_dir(&forensic_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            preserved.iter().any(|n| n.ends_with(".baseline.md"))
                && preserved.iter().any(|n| n.ends_with(".candidate.md")),
            "forensic baseline + candidate must be preserved: {preserved:?}"
        );
    }

    #[test]
    fn ipcfullprompt_scaffold_duplication_logged_without_baseline() {
        // The brandon-cinquegrana.md shape: a full-tail duplication leaves two
        // `<!-- /agent:exchange -->` markers around an in-progress prompt. This is
        // a self-check on the candidate, so it must fire even with no baseline.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let candidate = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: prior — opus-4-8\nAnswer.\n",
            "<!-- agent:boundary:709a41ae -->\n",
            "Is the issue still happening?\nCan it be re\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
            "Can it be rep11ro\n",
            "<!-- /agent:exchange -->\n",
            "## Queue\n<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "socket_ack_content",
            Some("pid-x"),
            None,
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("ipcfullprompt_corruption_suspected")
                && log.contains("scaffold_duplicated=")
                && log.contains("scaffold_duplicated(<!-- /agent:exchange -->:1->2)"),
            "full-tail scaffold duplication must be captured without a baseline:\n{log}"
        );
    }

    #[test]
    fn ipcfullprompt_corruption_silent_on_clean_candidate() {
        // A candidate that only *adds* a new response (expected growth) must not
        // be flagged — no false positive on normal cycles.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "placeholder").unwrap();

        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "<!-- /agent:exchange -->\n",
        );
        let candidate = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first — opus-4-8\nA1.\n",
            "### Re: second — opus-4-8\nA2.\n",
            "<!-- /agent:exchange -->\n",
        );

        log_ipcfullprompt_corruption_if_any(
            &doc,
            "file_ipc",
            Some("pid-clean"),
            Some(baseline),
            candidate,
        );

        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("ipcfullprompt_corruption_suspected"),
            "clean growth must not be flagged as corruption:\n{log}"
        );
    }

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

        let patch = crate::template::PatchBlock::new(
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

        let patch = crate::template::PatchBlock::new("exchange", "live prompt\n");
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
    fn sanitize_unmatched_escapes_exchange_markers_in_response() {
        let mut unmatched =
            "### Re: deploy\n\nDone.\n\n<!-- agent:exchange -->\nExtra\n<!-- /agent:exchange -->\n"
                .to_string();
        sanitize_unmatched(&mut unmatched);
        assert!(
            !unmatched.contains("<!-- agent:exchange -->"),
            "agent exchange markers must be escaped in unmatched text, got: {unmatched}"
        );
        assert!(
            unmatched.contains("&lt;!-- agent:exchange --&gt;"),
            "escaped markers expected, got: {unmatched}"
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
        sanitize_unmatched(&mut sanitized_unmatched);

        let result = crate::template::apply_patches(doc, &[], &sanitized_unmatched, &file).unwrap();

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

        let patch = crate::template::PatchBlock::new("exchange", "agent response content");

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

        let patches: Vec<crate::template::PatchBlock> = vec![];
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

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let unmatched = "do #expatch. spec-test-build-install-commit-push\n### Re: #expatch — gpt-5\n\nImplemented.\n";
        let prefix_lines = vec!["do #expatch. spec-test-build-install-commit-push".to_string()];
        let result =
            build_ipc_patches_json(&doc, &patches, unmatched, Some(prefix_lines.as_slice()), None)
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
    fn build_ipc_patches_json_seeded_boundary_is_stable_across_rebuilds() {
        // #finalize-visible-buffer-ipc-timeout-race ROOT-CAUSE REGRESSION:
        // a single write builds its IPC patches more than once (socket attempt →
        // file-IPC fallback → run_stream timeout re-write). Each rebuild used to
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
        assert!(bid_a.is_some(), "patch should carry a boundary_id: {build_a:?}");
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

        let seen_payload = std::sync::Arc::new(std::sync::Mutex::new(None));
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
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

        let seen_payload = std::sync::Arc::new(std::sync::Mutex::new(None));
        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
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
    fn ipc_snapshot_dedupes_extra_prompt_copy_against_before_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = "<!-- agent:exchange patch=append -->\nOld content.\nlive prompt\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\nOld content.\n❯ live prompt\nlive prompt\n### Re: response\nDone.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), after, "test_ipc").unwrap();

        assert!(changed);
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(
            !repaired.contains("❯ live prompt\nlive prompt"),
            "duplicate unprefixed prompt should be removed: {repaired}"
        );
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get("live prompt")
                .copied(),
            Some(1)
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("ipc_prompt_duplicate_repaired"));
        assert!(log.contains("ipc_snapshot_deduped"));
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
    fn duplicate_prompt_artifact_repair_runs_canonical_pipeline() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "Please keep this duplicate prompt around for canonical cleanup coverage #spec-test-build-install-commit-push";
        let prefix_short = "agent-doc on corky running opencode, the arrow key functionality works at first but once a turn starts the key log shows re ";
        let prefix_long = "agent-doc on corky running opencode, the arrow key functionality works at first but once a turn starts the key log shows received ";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "{prompt}\n",
                "<!-- agent:boundary:old -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let after = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "<!-- agent:boundary:new -->\n",
                "{prefix_short}\n",
                "{prefix_long}\n",
                "### Re: duplicate prompt cleanup — gpt-5\n\n",
                "Done.\n",
                "### Re: duplicate prompt cleanup — gpt-5\n\n",
                "Done.\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt,
            prefix_short = prefix_short,
            prefix_long = prefix_long
        );
        fs::write(&doc, &before).unwrap();

        let (repaired, report) = repair_duplicate_prompt_artifacts(
            &after,
            &doc,
            DuplicatePromptRepairOptions::new("test-canonical")
                .with_before(Some(&before))
                .preserving(Some(&before)),
        )
        .unwrap();

        assert_eq!(
            report,
            DuplicatePromptRepairReport {
                response_blocks: true,
                answered_tail: false,
                post_exchange_comments: true,
                prompt_lines_against_before: true,
                live_prefix_variants: true,
            }
        );
        assert_eq!(
            repaired.matches("### Re: duplicate prompt cleanup").count(),
            1,
            "duplicate response block should be removed:\n{repaired}"
        );
        assert!(repaired.contains(&format!("❯ {prompt}\n<!-- agent:boundary:new -->")));
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get(prompt)
                .copied(),
            Some(1)
        );
        assert!(
            !repaired.contains(&format!("❯ {prompt}\n{prompt}")),
            "before-content prompt duplicate should be removed:\n{repaired}"
        );
        assert!(!repaired.contains(prefix_short));
        assert!(repaired.contains(prefix_long));
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "post-exchange duplicate prompt comment should keep the comment shell:\n{repaired}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("duplicate_prompt_artifact_repair"));
        assert!(log.contains("source=test-canonical"));
        assert!(log.contains("response_blocks=true"));
        assert!(log.contains("post_exchange_comments=true"));
        assert!(log.contains("prompt_lines_against_before=true"));
        assert!(log.contains("live_prefix_variants=true"));
    }

    #[test]
    fn commit_prompt_repair_dedupes_exact_prefixed_raw_prompt_copy() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "lucas-huang may not have the necessary packages to use the runbooks. Please add development dependencies so any programmer can use the runbooks.";
        let snapshot = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let current = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        fs::write(&doc, &current).unwrap();

        let repaired =
            repair_commit_prompt_artifacts_against_snapshot(&doc, &snapshot, &current).unwrap();

        assert!(repaired.contains(&format!("❯ {prompt}\n#spec-test-commit-push")));
        assert!(
            !repaired.contains(&format!("❯ {prompt}\n{prompt}")),
            "commit pre-stage repair should remove the raw duplicate:\n{repaired}"
        );
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get(prompt)
                .copied(),
            Some(1)
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("source=commit-pre-stage"));
        assert!(log.contains("prompt_lines_against_before=true"));
    }

    #[test]
    fn normalize_final_template_content_dedupes_direct_merge_prompt_copy() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- agent:boundary:old -->\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n",
        );
        let merged = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "❯ live prompt\n",
            "live prompt\n",
            "### Re: response — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, before).unwrap();

        let repaired = normalize_final_template_content(
            &doc,
            before,
            Some(snapshot),
            Some(before),
            merged,
            Some("### Re: response — gpt-5\n\nDone.\n"),
        )
        .unwrap();

        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get("live prompt")
                .copied(),
            Some(1)
        );
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(repaired.contains("Done."));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("ipc_prompt_duplicate_repaired"));
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
    fn normalize_user_prompts_replaced_response_body_under_existing_heading_not_prefixed() {
        // Regression #repair-orphan-prefix-bug: when an orphaned response is
        // applied by replacing a placeholder body UNDER AN EXISTING `### Re:`
        // heading (e.g. a direct Edit-based patchback swapping a "Hello world"
        // placeholder for the real multi-line body), the heading line is Equal
        // in the snapshot→baseline diff. The replacement body lines are Insert
        // lines and must still be recognized as assistant-response body, not
        // user prompts — they must NOT receive the `❯ ` prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nHello world\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nReal answer line one.\nReal answer line two.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nReal answer line one.\nReal answer line two.\n<!-- agent:boundary:xyz -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ Real answer line one."),
            "replaced response body line one must NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Real answer line two."),
            "replaced response body line two must NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("Real answer line one.") && result.contains("Real answer line two."),
            "response body must be preserved verbatim: {}",
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
    fn normalize_user_prompts_keeps_inserted_assistant_question_bare() {
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ do #old
<!-- /agent:exchange -->
";
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Why did this happen?
This should stay answer prose.
<!-- /agent:exchange -->
";
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Why did this happen?
This should stay answer prose.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);

        assert!(
            result.contains("\nWhy did this happen?\nThis should stay answer prose.\n"),
            "assistant question/prose must stay bare:\n{result}"
        );
        assert!(
            !result.contains("\n❯ Why did this happen?")
                && !result.contains("\n❯ This should stay answer prose."),
            "inserted assistant response lines must not be prompt-prefixed:\n{result}"
        );
    }

    #[test]
    fn normalize_user_prompts_still_prefixes_real_followup_after_inserted_response() {
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ do #old
<!-- /agent:exchange -->
";
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Done.

do #next. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Done.

do #next. spec-test-build-install-commit-push
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);

        assert!(
            result.contains("\n❯ do #next. spec-test-build-install-commit-push\n"),
            "canonical prompt-target extraction must still prefix the follow-up:\n{result}"
        );
        assert!(
            result.contains("\nDone.\n"),
            "assistant response prose must stay bare:\n{result}"
        );
    }

    #[test]
    fn extract_normalization_targets_preserves_duplicate_lines() {
        let before = "<!-- agent:exchange patch=append -->\nQuestion?\nspec-test-build-install-commit-push\nQuestion?\nspec-test-build-install-commit-push\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\n❯ Question?\n❯ spec-test-build-install-commit-push\n❯ Question?\n❯ spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n";

        let targets = extract_normalization_targets(before, after);

        assert_eq!(
            targets,
            vec![
                "Question?".to_string(),
                "spec-test-build-install-commit-push".to_string(),
                "Question?".to_string(),
                "spec-test-build-install-commit-push".to_string(),
            ]
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
    fn normalize_patch_content_counts_duplicate_targets() {
        let patch_content = "spec-test-build-install-commit-push\nspec-test-build-install-commit-push\nspec-test-build-install-commit-push\n";
        let prefix_lines = vec![
            "spec-test-build-install-commit-push".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        let result = normalize_patch_content(patch_content, &prefix_lines);

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
        // next IPC write carries normalize_prefix_lines with the correct prefix target.
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

    #[test]
    fn normalize_user_prompts_multiline_prompt_after_stale_response_gets_prefix() {
        // Regression for #pfxstrip2: when a stale snapshot makes the previous
        // assistant response appear as inserted content, the normalizer enters
        // agent-block mode. A blank-separated fresh prompt run after that
        // response is still user input, and every nonblank prompt line needs
        // the prompt prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Previous prompt\n<!-- /agent:exchange -->\n";
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Previous prompt\n",
            "### Re: previous — gpt-5\n",
            "Implemented and verified.\n",
            "\n",
            "Please increment version to v0.1.1. Release to github. Create a plan for rollout.\n",
            "Miguel will be integrating the demo into the partner workspace.\n",
            "\n",
            "Please rename the gh repo ClaudeScore/buildparty-investor-demo to the final name.\n",
            "Also, please draft slack instructions for robert-ross and miguel-mendez.\n",
            "\n",
            "spec-test-news-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = baseline.replace(
            "<!-- /agent:exchange -->",
            "<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->",
        );

        let result = normalize_user_prompts_in_exchange(&content, baseline, snapshot);

        for expected in [
            "❯ Please increment version to v0.1.1. Release to github. Create a plan for rollout.",
            "❯ Miguel will be integrating the demo into the partner workspace.",
            "❯ Please rename the gh repo ClaudeScore/buildparty-investor-demo to the final name.",
            "❯ Also, please draft slack instructions for robert-ross and miguel-mendez.",
            "❯ spec-test-news-commit-push",
        ] {
            assert!(
                result.contains(expected),
                "missing expected prefixed prompt line {expected:?}:\n{result}"
            );
        }
        assert!(
            !result.contains("❯ Implemented and verified."),
            "stale assistant response body must stay unprefixed:\n{result}"
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
    fn normalize_safe_preserves_unprefixed_agent_lines_from_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "run follow-up\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "run follow-up\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("\nDone:\n- build passed\n"),
            "committed agent response lines from HEAD must stay unprefixed:\n{result}"
        );
        assert!(
            result.contains("\n❯ run follow-up\n"),
            "new user prompt should still be prefixed:\n{result}"
        );
    }

    #[test]
    fn normalize_safe_preserves_prior_response_tail_before_new_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "do [#pfxleak3]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "do [#pfxleak3]. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains(
                "\n- All 506 assertions pass.\nCommitted + pushed buildparty-investor-demo and session-share.\n❯ do [#pfxleak3]. spec-test-build-install-commit-push\n"
            ),
            "prior response tail must stay bare and only the new prompt may be prefixed:\n{result}"
        );
        assert!(
            !result.contains("\n❯ - All 506 assertions pass.\n")
                && !result.contains(
                    "\n❯ Committed + pushed buildparty-investor-demo and session-share.\n"
                ),
            "assistant tail lines from HEAD must not gain prompt prefixes:\n{result}"
        );
    }

    #[test]
    fn normalize_safe_preserves_prefixed_user_lines_from_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please increment version to v0.1.1.\n",
            "❯ Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = head;
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Please increment version to v0.1.1.\n",
            "Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Please increment version to v0.1.1.\n",
            "Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("❯ Please increment version to v0.1.1."),
            "HEAD-prefixed first prompt line must regain its prefix:\n{result}"
        );
        assert!(
            result.contains("❯ Miguel will be integrating the demo."),
            "HEAD-prefixed continuation line must regain its prefix:\n{result}"
        );
        assert!(
            !result.contains("\nPlease increment version to v0.1.1.\n"),
            "bare first prompt line must not remain:\n{result}"
        );
    }

    #[test]
    fn normalize_safe_bails_over_threshold() {
        // Construct a baseline with >50 unique "user-added" lines relative to the snapshot.
        // The safety rail should refuse to apply ❯ prefix and return content unchanged.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();
        let file = root.join("doc.md");
        std::fs::write(&file, "initial\n").unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();
        let head_before = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;

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
        let head_after = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(
            head_after, head_before,
            "normalization overrun must not force-commit the working tree"
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
    fn stale_snapshot_reset_drift_allows_small_size_delta() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = "a".repeat(1000);
        let current = "b".repeat(940);

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), &current, "stream write");

        assert!(
            result.is_ok(),
            "minor snapshot/file size drift should not block writes"
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
mod post_commit_prefix_repair_tests {
    use super::*;

    #[test]
    fn extract_post_commit_normalization_targets_finds_missing_working_tree_prefix() {
        let committed = "\
<!-- agent:exchange -->
❯ do #spfxnorm. spec-test-build-install-commit-push
### Re: #spfxnorm — gpt-5
Implemented.
<!-- agent:boundary:clean123 -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange -->
do #spfxnorm. spec-test-build-install-commit-push
### Re: #spfxnorm — gpt-5 (HEAD)
Implemented.
<!-- agent:boundary:dirty123 -->
<!-- /agent:exchange -->
";

        assert_eq!(
            extract_post_commit_normalization_targets(committed, working),
            vec!["do #spfxnorm. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_only_updates_exchange_user_region() {
        let working = "\
<!-- agent:exchange -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- agent:boundary:dirty123 -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &["do #spfxnorm. spec-test-build-install-commit-push".to_string()],
        );

        assert!(repaired.contains("❯ do #spfxnorm. spec-test-build-install-commit-push"));
        assert!(
            repaired.contains("<!-- agent:boundary:dirty123 -->\ndo #spfxnorm. spec-test-build-install-commit-push"),
            "agent region after the boundary must remain untouched: {repaired}"
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
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            // Write sidecar WITHOUT ❯ prefix (plugin failure)
                            let bad_sidecar = "---\nsession: test\n---\n\n<!-- agent:exchange -->\ndo #jbpfx2\nagent response\n<!-- /agent:exchange -->\n";
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
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

    #[test]
    fn normalization_fallback_repairs_bare_content_ours_prompt_prefix() {
        // Regression for #bppfxstrip: if sidecar verification rejects the plugin
        // snapshot, the content_ours fallback must still apply normalize_prefix_lines
        // before saving the snapshot.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines =
            vec!["do #bppfxstrip. spec-test-build-install-commit-push".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #bppfxstrip. spec-test-build-install-commit-push
agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
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

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do #bppfxstrip. spec-test-build-install-commit-push"),
            "content_ours fallback must be normalized before snapshot save; got: {}",
            snap
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #bppfxstrip. spec-test-build-install-commit-push"),
            "content_ours fallback must repair the working tree before commit; got: {}",
            disk
        );
    }

    #[test]
    fn normfallback_records_repaired_working_tree_when_sidecar_strips_prompt_prefix() {
        // Regression for #normfallback: the observed ops-log signal should be
        // backed by deterministic coverage. A plugin sidecar that drops a
        // required prompt prefix must be rejected, and the binary fallback must
        // repair the live file before any commit can capture the stripped form.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("agent-doc-bugs2.md");
        let original = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new(
            "exchange",
            "### Re: #normfallback — gpt-5\n\nCovered.",
        );
        let content_ours = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
❯ do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec!["do [#normfallback]".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
do [#normfallback]
### Re: #normfallback — gpt-5

Covered.
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
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

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do [#normfallback]"),
            "snapshot must use the normalized fallback rather than the stripped sidecar: {snap}"
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do [#normfallback]"),
            "working tree must be repaired to match the normalized fallback: {disk}"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback")
                && ops_log.contains("reason=prefix_divergence"),
            "ops log should record why the primary sidecar snapshot was rejected:\n{ops_log}"
        );
        assert!(
            ops_log.contains("sidecar_normalization_fallback_repaired_working_tree"),
            "ops log should record the explicit working-tree repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivers_narrow_patch_before_full_content() {
        // A disk-only fallback can leave an editor buffer stale. If the rejected
        // editor state differs only by prompt-prefix normalization, the repair
        // should converge the editor with a narrow normalization patch.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-diverge. spec-test-build-install-commit-push
### Re: #sidecar-diverge — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-diverge. spec-test-build-install-commit-push".to_string()];

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let listener_root = dir.path().to_path_buf();
        let listener_doc = doc.clone();
        let listener_count = call_count.clone();
        let listener_repair_payloads = seen_repair_payloads.clone();
        std::fs::create_dir_all(listener_root.join(".agent-doc")).unwrap();
        let _listener = std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let v: serde_json::Value = serde_json::from_str(msg).ok()?;
                listener_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(full_content) = v.get("fullContent").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(&listener_doc, full_content);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(serde_json::json!({"type": "ack"}).to_string());
                }

                let patches_empty = v
                    .get("patches")
                    .and_then(|value| value.as_array())
                    .is_none_or(|patches| patches.is_empty());
                if patches_empty
                    && let Some(lines) = v.get("normalize_prefix_lines").and_then(|value| {
                        value.as_array().map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                    })
                {
                    let current = std::fs::read_to_string(&listener_doc).ok()?;
                    let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                    let _ = std::fs::write(&listener_doc, repaired);
                    listener_repair_payloads.lock().unwrap().push(v.clone());
                    return Some(serde_json::json!({"type": "ack"}).to_string());
                }

                Some(serde_json::json!({"type": "ack"}).to_string())
            });
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(dir.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(dir.path()),
            "fake socket listener did not start"
        );

        let result = redeliver_normalization_fallback_to_editor(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch"),
        );
        assert!(result, "narrow normalization repair should be delivered");

        assert!(
            call_count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "fallback should send a narrow IPC repair"
        );
        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one narrow repair payload"
        );
        assert!(
            repair_payloads[0].get("fullContent").is_none(),
            "eligible prefix repair should avoid fullContent payloads: {}",
            repair_payloads[0]
        );
        assert_eq!(
            repair_payloads[0]["normalize_prefix_lines"][0],
            "do #sidecar-diverge. spec-test-build-install-commit-push"
        );
        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-diverge. spec-test-build-install-commit-push"),
            "editor narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor"),
            "ops log should record the narrow editor repair:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_file_ipc_queues_narrow_patch_before_full_content() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
---
session: test
---

<!-- agent:exchange patch=append -->
do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        let repaired = "\
---
session: test
---

<!-- agent:exchange patch=append -->
❯ do #sidecar-file. spec-test-build-install-commit-push
### Re: #sidecar-file — gpt-5

agent response
<!-- agent:boundary:test-bnd-002 -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, bad_state).unwrap();
        let normalize_prefix_lines =
            vec!["do #sidecar-file. spec-test-build-install-commit-push".to_string()];
        let patch_hash = snapshot::doc_hash(&doc).unwrap();
        let patch_file = agent_doc_dir
            .join("patches")
            .join(format!("{patch_hash}.json"));

        let seen_repair_payloads =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let watcher_doc = doc.clone();
        let watcher_patch_file = patch_file.clone();
        let watcher_ack_dir = agent_doc_dir.join("ack-content");
        let watcher_repair_payloads = seen_repair_payloads.clone();
        let watcher = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_secs(3) {
                if !watcher_patch_file.exists() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                let payload_text = match std::fs::read_to_string(&watcher_patch_file) {
                    Ok(text) => text,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                let payload: serde_json::Value = match serde_json::from_str(&payload_text) {
                    Ok(payload) => payload,
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                };
                watcher_repair_payloads
                    .lock()
                    .unwrap()
                    .push(payload.clone());
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap()
                    .to_string();
                let lines = payload
                    .get("normalize_prefix_lines")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let current = std::fs::read_to_string(&watcher_doc).unwrap();
                let repaired = normalize_exchange_prefixes_for_targets(&current, &lines);
                std::fs::write(&watcher_doc, &repaired).unwrap();
                std::fs::write(watcher_ack_dir.join(format!("{patch_id}.md")), repaired).unwrap();
                std::fs::remove_file(&watcher_patch_file).unwrap();
                return true;
            }
            false
        });

        let result = redeliver_normalization_fallback_to_editor(
            &doc,
            repaired,
            bad_state,
            &normalize_prefix_lines,
            Some("source-patch-file"),
        );
        assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");
        assert!(result, "file IPC narrow normalization repair should apply");

        let repair_payloads = seen_repair_payloads.lock().unwrap();
        assert_eq!(
            repair_payloads.len(),
            1,
            "expected exactly one file IPC repair payload"
        );
        let payload = &repair_payloads[0];
        assert!(
            payload.get("fullContent").is_none(),
            "eligible file IPC prefix repair should avoid fullContent payloads: {payload}"
        );
        assert_eq!(payload["patches"].as_array().unwrap().len(), 0);
        assert_eq!(payload["unmatched"], "");
        assert_eq!(payload["reposition_boundary"], true);
        assert_eq!(payload["preserve_head"], true);
        assert_eq!(
            payload["normalize_prefix_lines"][0],
            "do #sidecar-file. spec-test-build-install-commit-push"
        );
        assert_eq!(payload["expected_content_len"], bad_state.len());
        assert_eq!(
            payload["expected_content_hash"],
            crate::ops_log::content_hash(bad_state)
        );

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #sidecar-file. spec-test-build-install-commit-push"),
            "file IPC narrow repair should leave disk/editor content normalized: {disk}"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repaired_editor")
                && ops_log.contains("transport=file"),
            "ops log should record the file IPC narrow editor repair:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("sidecar_normalization_fallback_redelivered_editor"),
            "file IPC normalization-only repair should not fall back to fullContent:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_redelivery_skips_when_bad_state_is_stale() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = dir.path().join("test.md");
        let bad_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        let live_state = "\
<!-- agent:exchange patch=append -->
do #stale. spec-test-build-install-commit-push
live prompt typed after sidecar fallback
<!-- /agent:exchange -->
";
        let repaired = "\
<!-- agent:exchange patch=append -->
❯ do #stale. spec-test-build-install-commit-push
### Re: #stale — gpt-5

Done.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, live_state).unwrap();

        let delivered = redeliver_normalization_fallback_to_editor(
            &doc,
            repaired,
            bad_state,
            &["do #stale. spec-test-build-install-commit-push".to_string()],
            Some("source-patch"),
        );

        assert!(
            !delivered,
            "normalization fallback redelivery must skip stale bad-state proof"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live_state);
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("sidecar_normalization_fallback_narrow_repair_skipped")
                && ops_log.contains("skip=stale_bad_state")
                && ops_log.contains("sidecar_normalization_fallback_editor_redelivery_skipped"),
            "stale proof skip should be logged for narrow and full-content fallback:\n{ops_log}"
        );
    }

    #[test]
    fn normalization_fallback_dedupes_already_applied_editor_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let editor_already_applied = "\
<!-- agent:exchange patch=append -->
do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        std::fs::write(&doc, editor_already_applied).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #duppb. spec-test-build-install-commit-push".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: #duppb — gpt-5").count(),
            1,
            "fallback full-content repair must not redeliver duplicate responses: {fallback}"
        );
        assert!(fallback.contains("❯ do #duppb. spec-test-build-install-commit-push"));
    }

    #[test]
    fn normalization_fallback_adopts_ack_content_response_delta_before_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
do #ackdelta
<!-- agent:boundary:base -->
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ackdelta
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:ours -->
<!-- /agent:exchange -->
";
        let disk_after_ack_content = "\
<!-- agent:exchange patch=append -->
do #ackdelta
while typing next prompt
### Re: ack delta — gpt-5

Done.
<!-- agent:boundary:current -->
<!-- /agent:exchange -->
";
        std::fs::write(&doc, disk_after_ack_content).unwrap();

        let fallback = normalized_content_ours_fallback(
            &doc,
            Some(baseline),
            content_ours,
            &["do #ackdelta".to_string()],
        );

        assert_eq!(
            fallback.matches("### Re: ack delta — gpt-5").count(),
            1,
            "ack-content normalization fallback must not replay an already-applied response: {fallback}"
        );
        assert!(
            fallback.contains("while typing next prompt"),
            "ack-content fallback should preserve concurrent disk edits: {fallback}"
        );
        assert!(
            fallback.contains("❯ do #ackdelta"),
            "ack-content fallback should still normalize the prompt prefix: {fallback}"
        );
    }

    #[test]
    fn normalization_fallback_splices_pending_mutations_from_disk() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let on_disk_with_pending = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] [#keepme] Preserve pending add from disk
<!-- /agent:backlog -->
";
        std::fs::write(&doc, on_disk_with_pending).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #splpend
agent response
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
        let normalize_prefix_lines = vec!["do #splpend".to_string()];

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
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange -->
do #splpend
agent response
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
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

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("❯ do #splpend"),
            "snapshot must preserve normalized prompt prefix; got: {}",
            snap
        );
        assert!(
            snap.contains("- [ ] [#keepme] Preserve pending add from disk"),
            "snapshot must preserve pending mutations from disk during normalization fallback; got: {}",
            snap
        );
    }

    #[test]
    fn normalization_fallback_preserves_concurrent_comment_deletion() {
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
<!-- agent:boundary:test-bnd-001 -->
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        std::fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response");
        let content_ours = "\
---
session: test
---

<!-- agent:exchange -->
❯ do #commentdel
agent response
<!-- /agent:exchange -->

<!--
The tmux focus should be snappy.
-->
";
        let normalize_prefix_lines = vec!["do #commentdel".to_string()];

        let patches_dir = agent_doc_dir.join("patches");
        let ack_dir = agent_doc_dir.join("ack-content");
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let Ok(entries) = std::fs::read_dir(&patches_dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json") {
                        if let Ok(text) = std::fs::read_to_string(&path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
                            && let Some(pid) = json.get("patch_id").and_then(|v| v.as_str())
                        {
                            let bad_sidecar = "\
---
session: test
---

<!-- agent:exchange -->
do #commentdel
agent response
<!-- /agent:exchange -->
";
                            let _ = std::fs::write(&doc_for_watcher, bad_sidecar);
                            let _ = std::fs::write(ack_dir.join(format!("{pid}.md")), bad_sidecar);
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

        let disk = std::fs::read_to_string(&doc).unwrap();
        assert!(
            disk.contains("❯ do #commentdel"),
            "normalization fallback must still repair the prompt prefix: {disk}"
        );
        assert!(
            !disk.contains("The tmux focus should be snappy."),
            "normalization fallback must not restore a concurrently deleted scratch comment: {disk}"
        );
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snap.contains("The tmux focus should be snappy."),
            "snapshot must also respect the concurrent comment deletion: {snap}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_requires_duplicate_occurrences() {
        let sidecar = "\
---
session: test
---

<!-- agent:exchange -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ follow-up
spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let normalize_prefix_lines = vec![
            "do [#dup]. Are repeated presets handled?".to_string(),
            "spec-test-build-install-commit-push".to_string(),
            "follow-up".to_string(),
            "spec-test-build-install-commit-push".to_string(),
        ];

        assert!(
            !verify_sidecar_normalization(sidecar, &normalize_prefix_lines),
            "one earlier prefixed preset line must not mask a later bare duplicate"
        );
    }

    #[test]
    fn extract_post_commit_normalization_targets_preserves_duplicate_missing_lines() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5

Done.

❯ Why follow up?
❯ spec-test-build-install-commit-push
<!-- agent:boundary:committed -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert_eq!(
            targets,
            vec!["spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_repairs_late_duplicate_occurrence() {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#dup]. Are repeated presets handled?
❯ spec-test-build-install-commit-push
### Re: #dup — gpt-5 (HEAD)

Done.

❯ Why follow up?
spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("spec-test-build-install-commit-push")],
        );

        assert_eq!(
            repaired
                .matches("❯ spec-test-build-install-commit-push")
                .count(),
            2,
            "repair should prefix the later bare duplicate without losing the earlier one"
        );
        assert!(
            !repaired.contains("\n❯ ❯ spec-test-build-install-commit-push"),
            "repair must not double-prefix existing matches"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_verification_lists() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Implemented.

Verification:
- Passed focused tests:
  - `cargo test normalize_prefix`
- `cargo test` is still red on a pre-existing failure.
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "- Passed focused tests:".to_string(),
                "  - `cargo test normalize_prefix`".to_string(),
                "- `cargo test` is still red on a pre-existing failure.".to_string(),
                "do #verfpfx. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired.contains("Verification:\n- Passed focused tests:\n  - `cargo test normalize_prefix`\n- `cargo test` is still red on a pre-existing failure."),
            "assistant verification list must stay unprefixed:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #verfpfx. spec-test-build-install-commit-push\n"),
            "real prompt after the response boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ - Passed focused tests:")
                && !repaired.contains("\n❯   - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ - `cargo test` is still red on a pre-existing failure."),
            "assistant list items must not receive prompt prefixes:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_requires_targeted_prompt_start_after_response() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Why did this keep happening?
spec-test-build-install-commit-push
<!-- agent:boundary:previous -->
do #spfxnorm. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "spec-test-build-install-commit-push".to_string(),
                "do #spfxnorm. spec-test-build-install-commit-push".to_string(),
            ],
        );

        assert!(
            repaired
                .contains("\nWhy did this keep happening?\nspec-test-build-install-commit-push\n"),
            "assistant question and preset-looking prose must stay bare:\n{repaired}"
        );
        assert!(
            repaired.contains("\n❯ do #spfxnorm. spec-test-build-install-commit-push\n"),
            "real prompt after the boundary must still be repaired:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ spec-test-build-install-commit-push\n"),
            "a stale target inside assistant prose must not be enough to start repair:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_assistant_commit_label() {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired =
            normalize_exchange_prefixes_for_targets(working, &[String::from("Commit / push:")]);

        assert!(
            repaired.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_skips_later_assistant_commit_label_after_stale_target()
     {
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

Commit / push:
- `old-sha`
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `new-sha`
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[String::from("Commit / push:"), String::from("- `old-sha`")],
        );

        assert!(
            repaired.contains("\nCommit / push:\n- `new-sha`\n"),
            "later assistant commit label/list must stay bare:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Commit / push:\n- `new-sha`\n"),
            "later assistant commit label must not become a prompt:\n{repaired}"
        );
    }

    #[test]
    fn normalize_exchange_prefixes_for_targets_treats_prefixed_response_heading_as_assistant_boundary()
     {
        let working = "\
<!-- agent:exchange patch=append -->
❯ do [#done]. spec-test-build-install-commit-push
❯ ### Re: #done — gpt-5

Implemented.

Verification:
- `cargo test normalize_prefix`

Commit / push:
- `abc123`
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let repaired = normalize_exchange_prefixes_for_targets(
            working,
            &[
                "Implemented.".to_string(),
                "Verification:".to_string(),
                "- `cargo test normalize_prefix`".to_string(),
                "Commit / push:".to_string(),
                "- `abc123`".to_string(),
            ],
        );

        assert!(
            repaired.contains("\n❯ ### Re: #done — gpt-5\n\nImplemented.\n"),
            "prefixed response heading must still start an assistant block:\n{repaired}"
        );
        assert!(
            !repaired.contains("\n❯ Implemented.")
                && !repaired.contains("\n❯ Verification:")
                && !repaired.contains("\n❯ - `cargo test normalize_prefix`")
                && !repaired.contains("\n❯ Commit / push:")
                && !repaired.contains("\n❯ - `abc123`"),
            "assistant response body after a prefixed heading must not be prompt-prefixed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_patch_content_skips_assistant_commit_label() {
        let patch = "\
### Re: #done — gpt-5
Verification:
- `cargo test`

Commit / push:
- `git push` returned `Everything up-to-date`.
";

        let normalized = normalize_patch_content(patch, &[String::from("Commit / push:")]);

        assert!(
            normalized.contains("\nCommit / push:\n"),
            "assistant commit evidence label must stay unprefixed:\n{normalized}"
        );
        assert!(
            !normalized.contains("\n❯ Commit / push:\n"),
            "assistant commit evidence label must not receive prompt prefix:\n{normalized}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_commit_label() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: #old — gpt-5
Verified.

❯ Commit / push:
❯ do [#next]. spec-test-build-install-commit-push
### Re: #next — gpt-5
Verified.

Commit / push:
- `git push` returned `Everything up-to-date`.
<!-- agent:boundary:abc --> (HEAD)
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            !targets.iter().any(|target| target == "Commit / push:"),
            "assistant evidence label must not become a prefix repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_assistant_prose_before_next_heading() {
        let committed = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

❯ The current tree has already started making this accountable.
### Re: closeout guard — gpt-5

Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
### Re: sync latency — gpt-5

The current tree has already started making this accountable.
### Re: closeout guard — gpt-5 (HEAD)

Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "a stale prefixed assistant sentence must not become a repair target: {targets:?}"
        );
    }

    #[test]
    fn extract_post_commit_targets_ignores_prefixed_markdown_lists() {
        let committed = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
❯ - keep this bullet bare
❯   - keep this nested bullet bare
❯ 1. keep this ordered bullet bare
### Re: options — gpt-5
Done.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";
        let working = "\
<!-- agent:exchange patch=append -->
❯ Please compare these options:
- keep this bullet bare
  - keep this nested bullet bare
1. keep this ordered bullet bare
### Re: options — gpt-5 (HEAD)
Done.
<!-- agent:boundary:def -->
<!-- /agent:exchange -->
";

        let targets = extract_post_commit_normalization_targets(committed, working);

        assert!(
            targets.is_empty(),
            "stale prefixed markdown list items must not become repair targets: {targets:?}"
        );
    }

    #[test]
    fn verify_sidecar_normalization_rejects_assistant_list_prefix_substitute() {
        let sidecar = "\
<!-- agent:exchange patch=append -->
### Re: previous — gpt-5

Verification:
❯ - Passed focused tests:
<!-- agent:boundary:previous -->
do #verfpfx. spec-test-build-install-commit-push
<!-- agent:boundary:working -->
<!-- /agent:exchange -->
";

        assert!(
            !verify_sidecar_normalization(sidecar, &["- Passed focused tests:".to_string()]),
            "a prefixed assistant list item must not satisfy prompt-prefix sidecar verification"
        );
    }
}

#[cfg(test)]
mod submodule_patch_routing_tests {
    use super::*;
    use std::fs;
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

    #[test]
    fn resolve_ipc_project_root_ignores_agent_doc_outside_git_toplevel() {
        let outer_dir = TempDir::new().unwrap();
        let outer = outer_dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(outer.join(".agent-doc/patches")).unwrap();

        let nested = outer.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        git(&nested, &["init"]);
        let doc = nested.join("session.md");
        std::fs::write(
            &doc,
            "---\n---\n\n<!-- agent:exchange -->c<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let canonical = doc.canonicalize().unwrap();
        let project_root = resolve_ipc_project_root(&canonical);

        assert_eq!(
            project_root, nested,
            "a parent .agent-doc outside the current git toplevel must not capture IPC routing"
        );
    }

    #[test]
    fn required_closeout_fails_when_parent_submodule_pointer_commit_fails() {
        let parent_dir = TempDir::new().unwrap();
        let sub_src_dir = TempDir::new().unwrap();
        let parent = parent_dir.path().canonicalize().unwrap();
        let sub_src = sub_src_dir.path().canonicalize().unwrap();

        git(&sub_src, &["init"]);
        std::fs::write(sub_src.join("README.md"), "sub").unwrap();
        git(&sub_src, &["add", "README.md"]);
        git(&sub_src, &["commit", "-m", "init"]);

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
        git(&parent, &["commit", "-m", "add submodule"]);

        let submodule_root = parent.join("src/submodule");
        git(
            &submodule_root,
            &["config", "user.email", "test@example.com"],
        );
        git(&submodule_root, &["config", "user.name", "Test"]);
        std::fs::create_dir_all(submodule_root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(submodule_root.join(".agent-doc/state/cycles")).unwrap();

        let doc = submodule_root.join("session.md");
        let initial = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, initial).unwrap();
        git(&submodule_root, &["add", "session.md"]);
        git(&submodule_root, &["commit", "-m", "add doc"]);
        git(&parent, &["add", "src/submodule"]);
        git(&parent, &["commit", "-m", "record doc commit"]);

        let parent_git_dir = Command::new("git")
            .current_dir(&parent)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()
            .unwrap();
        assert!(parent_git_dir.status.success());
        let parent_git_dir = PathBuf::from(String::from_utf8_lossy(&parent_git_dir.stdout).trim());
        std::fs::write(parent_git_dir.join("index.lock"), "held by test").unwrap();

        let updated = initial.replace(
            "<!-- /agent:exchange -->\n",
            "### Re: reply — gpt-5\nbody\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, &updated).unwrap();
        crate::snapshot::save(&doc, &updated).unwrap();

        let err = super::complete_required_closeout(&doc).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("parent submodule pointer is not committed"),
            "strict closeout should name the missing parent layer, got: {message}"
        );
        assert!(
            message.contains("agent-doc commit"),
            "strict closeout should prescribe the idempotent commit recovery, got: {message}"
        );
        assert!(
            crate::git::submodule_pointer_drift(&doc).unwrap().is_some(),
            "parent gitlink should remain stale when parent commit fails"
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
                    let file = Path::new(file_path);
                    let before = std::fs::read_to_string(file).unwrap_or_default();
                    let patches = v
                        .get("patches")
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| {
                                    let name = item
                                        .get("component")
                                        .or_else(|| item.get("name"))
                                        .and_then(|value| value.as_str())?;
                                    let content =
                                        item.get("content").and_then(|value| value.as_str())?;
                                    Some(crate::template::PatchBlock::new(name, content))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let unmatched = v
                        .get("unmatched")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    let after = crate::template::apply_patches(&before, &patches, unmatched, file)
                        .unwrap_or(before);
                    let _ = std::fs::write(file, &after);
                    after
                } else {
                    String::new()
                };
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &content);
                Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
            });
        })
    }

    fn start_already_applied_listener(project_root: &Path) -> std::thread::JoinHandle<()> {
        let root = project_root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let _ = crate::ipc_socket::start_listener(&root, |_msg| {
                Some(
                    serde_json::json!({
                        "type": "ack",
                        "status": "error",
                        "reason": "already_applied"
                    })
                    .to_string(),
                )
            });
        })
    }

    fn start_fixed_ack_content_listener(
        project_root: &Path,
        ack_content: String,
    ) -> std::thread::JoinHandle<()> {
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
                let ack_dir = root_clone.join(".agent-doc/ack-content");
                let _ = std::fs::create_dir_all(&ack_dir);
                if let Some(file_path) = v.get("file").and_then(|f| f.as_str()) {
                    let _ = std::fs::write(file_path, &ack_content);
                }
                let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &ack_content);
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
    fn try_ipc_already_applied_socket_adopts_disk_when_response_is_present() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "patches",
            "snapshots",
            "crdt",
            "logs",
            "state/cycles",
            "claimed-patches",
        ] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }

        let doc = root.join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let live_already_applied_with_user_edit = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "User typed the next prompt while finalize was running.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, live_already_applied_with_user_edit).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);

        let patch = crate::template::PatchBlock::new(
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
            Some("already-applied-patch"),
        )
        .unwrap();

        assert!(
            result.success,
            "already_applied socket ack is a consumed editor write"
        );
        assert_eq!(
            crate::snapshot::load(&doc).unwrap().as_deref(),
            Some(live_already_applied_with_user_edit),
            "already_applied must adopt disk content when it contains the response plus live user edits"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_already_applied_with_user_edit,
            "live editor content should remain the committed snapshot candidate"
        );
        assert!(
            !crate::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "safe disk adoption must not leave a later snapshot-absorb block"
        );

        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_socket_already_applied_skip_file_fallback")
                && log.contains("ipc_socket_already_applied_live_buffer_diverged")
                && log.contains("ipc_socket_already_applied_snapshot")
                && log.contains("snap_source=file_read"),
            "already_applied disk adoption should be auditable:\n{log}"
        );
        // #6cmx/#wy0y: this scenario IS typing-during-finalize (live buffer has a
        // user edit beyond our content), so it must emit the explicit verification
        // marker with the response intact — one greppable line proving completion.
        assert!(
            log.contains("prompt_drift=true"),
            "user-edit divergence is a prompt-drift case:\n{log}"
        );
        assert!(
            log.contains("finalize_typing_during_write") && log.contains("response_present=true"),
            "typing-during-finalize must log finalize_typing_during_write with response_present:\n{log}"
        );
    }

    #[test]
    fn try_ipc_already_applied_socket_dedupes_duplicate_response_before_snapshot() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in [
            "patches",
            "snapshots",
            "crdt",
            "logs",
            "state/cycles",
            "claimed-patches",
        ] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }

        let doc = root.join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        let duplicated_live_buffer = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        fs::write(&doc, duplicated_live_buffer).unwrap();

        let _listener = start_already_applied_listener(&root);
        wait_for_listener(&root);

        let patch = crate::template::PatchBlock::new(
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
            Some("already-applied-duplicate"),
        )
        .unwrap();

        assert!(result.success);
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap.matches("### Re: Please reply — gpt-5").count(),
            1,
            "already_applied snapshot must dedupe duplicate response headings: {snap}"
        );
        let disk = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            disk.matches("### Re: Please reply — gpt-5").count(),
            1,
            "already_applied disk repair must converge with deduped snapshot: {disk}"
        );
        let log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("ipc_dedupe_repaired_working_tree")
                && log.contains("ipc_socket_already_applied_snapshot"),
            "dedupe repair should be logged:\n{log}"
        );
    }

    #[test]
    fn already_applied_socket_missing_disk_response_requests_file_fallback() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for subdir in ["snapshots", "crdt", "logs", "state/cycles"] {
            fs::create_dir_all(root.join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = root.join("session.md");
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: Please reply — gpt-5\n\n",
            "Answered.\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();

        let outcome = persist_already_applied_socket_content_ours_snapshot(
            &doc,
            "already-applied-missing",
            Some(baseline),
            Some(content_ours),
            None,
            "### Re: Please reply — gpt-5\n\nAnswered.\n",
        )
        .unwrap();

        assert_eq!(outcome, AlreadyAppliedSnapshotOutcome::NeedsFileFallback);
        assert_eq!(
            crate::snapshot::load(&doc).unwrap().as_deref(),
            Some(baseline),
            "missing disk response must not save stale content_ours as the snapshot"
        );
    }

    #[test]
    fn socket_ack_content_prompt_duplication_uses_content_ours_and_repairs_visible_buffer() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let agent_doc_dir = root.join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("state").join("cycles")).unwrap();

        let doc = root.join("doc.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:before -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let content_ours = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "### Re: Production key — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:ours -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let duplicated_ack_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "❯ Please set the production RESEND_API_KEY\n",
            "### Re: Production key — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:bad -->\n",
            "<!-- /agent:exchange -->\n"
        );
        fs::write(&doc, baseline).unwrap();
        crate::snapshot::save(&doc, baseline).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let _listener = start_fixed_ack_content_listener(&root, duplicated_ack_content.to_string());
        wait_for_listener(&root);

        let patch =
            crate::template::PatchBlock::new("exchange", "### Re: Production key — gpt-5\n\nDone.");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(baseline),
            Some(content_ours),
            None,
            Some("duplicated-ack-content"),
        )
        .unwrap();

        assert!(
            result.success,
            "IPC delivery should remain successful while snapshot adoption falls back"
        );
        assert_eq!(
            snapshot::load(&doc).unwrap().as_deref(),
            Some(content_ours),
            "duplicated ack-content must not become the committed snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content_ours,
            "visible duplicated ack-content should be repaired from the guarded response image"
        );
        assert!(
            crate::cycle_state::load(&doc)
                .unwrap()
                .unwrap()
                .ipc_snapshot_adoption_blocked,
            "later commit stages must not absorb the rejected duplicate sidecar"
        );
        let log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            log.contains("reason=prompt_duplication_in_ack_content")
                && log.contains("duplicate_prompt_count=1")
                && log.contains("ipc_dedupe_repaired_working_tree"),
            "duplicate sidecar rejection and visible repair should be logged:\n{log}"
        );
        assert!(
            log.contains("ipc_proof_insufficient")
                && log.contains("invariant=prompt_duplication_in_ack_content")
                && log.contains("recovery=content_ours_snapshot_and_visible_repair"),
            "duplicate prompt ACK should name its failed invariant and recovery:\n{log}"
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
    fn cleanup_resolved_backlog_prompts_removes_new_prompt_target_only() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );
        let final_content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "### Re: backlog prompt — gpt-5\n\n",
            "Committed and pushed.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#keep1] Keep this tracked item\n",
            "commit + push uncommitted files\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned =
            cleanup_resolved_backlog_prompts_after_response(&doc, base, current, final_content)
                .unwrap()
                .expect("prompt target should be cleaned");

        assert!(cleaned.contains("### Re: backlog prompt — gpt-5"));
        assert!(cleaned.contains("- [x] [#keep1] Keep this tracked item"));
        assert!(!cleaned.contains("commit + push uncommitted files"));
    }

    #[test]
    fn cleanup_resolved_backlog_prompts_preserves_non_prompt_backlog_edits() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let base = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Existing item\n",
            "- [ ] [#new1] Added tracked item\n",
            "<!-- /agent:backlog -->\n",
        );

        let cleaned =
            cleanup_resolved_backlog_prompts_after_response(&doc, base, current, current).unwrap();
        assert!(
            cleaned.is_none(),
            "ordinary tracked backlog additions are not prompt cleanup targets"
        );
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
    fn response_already_in_current_rejects_partial_line_overlap() {
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: answer — gpt-5
Done.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
Done.
<!-- /agent:exchange -->
";
        assert!(
            !response_already_in_current(base, content_ours, content_current),
            "a shared response body line is not proof that the response delta was applied"
        );
    }

    #[test]
    fn response_already_in_current_accepts_normalized_delta_with_bare_prompt() {
        let base = "\
<!-- agent:exchange patch=append -->
do #ipcd
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ do #ipcd
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
do #ipcd
while typing next prompt
### Re: #ipcd — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        assert!(
            response_already_in_current(base, content_ours, content_current),
            "the response hunk should be detected even when prompt-prefix normalization differs"
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

    #[test]
    fn adopt_current_response_without_duplication_rejects_partial_line_overlap() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let base = "\
<!-- agent:exchange patch=append -->
User prompt here.
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
User prompt here.
### Re: timeout fallback — gpt-5
Done.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
User prompt here.
Done.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, content_current).unwrap();
        let adopted = adopt_current_response_without_duplication(
            &doc,
            base,
            content_ours,
            content_current,
            None,
            "### Re: timeout fallback — gpt-5\nDone.\n",
        )
        .unwrap();

        assert!(
            adopted.is_none(),
            "socket-timeout fallback must not adopt current content from a partial line overlap"
        );
    }

    #[test]
    fn adopt_current_response_without_duplication_repairs_bare_prompt_prefix() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #scpd. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content_ours = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
❯ do #scpd. spec-test-build-install-commit-push
### Re: #scpd retry — gpt-5

Implemented.
<!-- /agent:exchange -->
";
        let content_current = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #scpd. spec-test-build-install-commit-push
### Re: #scpd retry — gpt-5

Implemented.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, content_current).unwrap();
        let repaired = adopt_current_response_without_duplication(
            &doc,
            base,
            content_ours,
            content_current,
            Some(snapshot),
            "### Re: #scpd retry — gpt-5\n\nImplemented.\n",
        )
        .unwrap()
        .expect("response should be adopted from current");

        assert!(repaired.contains("❯ do #scpd. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("\ndo #scpd. spec-test-build-install-commit-push\n"));
        assert_eq!(repaired.matches("### Re: #scpd retry — gpt-5").count(), 1);
    }

    #[test]
    fn normalize_final_template_content_repairs_bare_prompt_prefix_after_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #dupfx. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
do #dupfx. spec-test-build-install-commit-push
### Re: #dupfx — gpt-5

Implemented.
<!-- /agent:exchange -->
";

        std::fs::write(&doc, merged).unwrap();
        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, merged, None)
                .unwrap();

        assert!(repaired.contains("❯ do #dupfx. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("\ndo #dupfx. spec-test-build-install-commit-push\n"));
        assert_eq!(repaired.matches("### Re: #dupfx — gpt-5").count(), 1);
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leaked_marker() {
        // CRDT merge corruption: first non-empty line of the response body
        // got a leading `❯ `. The repair must strip it without touching real
        // user prompts elsewhere in the exchange.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #respfx. spec-test-build-install-commit-push
### Re: #respfx — opus-4-7

❯ Landed Phase 1 only this cycle. Item stays open.

#### Details

`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked ❯ on response body first line must be stripped");
        assert!(
            repaired.contains("\nLanded Phase 1 only this cycle. Item stays open.\n"),
            "stripped response body should start with the original prose, got:\n{repaired}"
        );
        assert!(
            !repaired.contains("❯ Landed"),
            "leaked ❯ must be removed, got:\n{repaired}"
        );
        // User prompt before the response heading is preserved.
        assert!(repaired.contains("❯ do #respfx. spec-test-build-install-commit-push"));
        // Heading and subsequent body lines are untouched.
        assert!(repaired.contains("### Re: #respfx — opus-4-7"));
        assert!(repaired.contains("#### Details"));
        assert!(repaired.contains("`agent-doc <FILE>` now accepts `--wait-for-ready <SECONDS>`."));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_strips_leading_run() {
        // Repair adoption can see every response paragraph prefixed when the
        // stale snapshot already had the response heading but not the body.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #leading-run. spec-test-build-install-commit-push
### Re: #leading-run — gpt-5

❯ First response paragraph.

❯ Second response paragraph.
❯ - Proof line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leading response-body prompt markers must be stripped");

        assert!(repaired.contains("\nFirst response paragraph.\n"));
        assert!(repaired.contains("\nSecond response paragraph.\n- Proof line.\n"));
        assert!(!repaired.contains("❯ First response paragraph."));
        assert!(!repaired.contains("❯ Second response paragraph."));
        assert!(!repaired.contains("❯ - Proof line."));
        assert!(repaired.contains("❯ do #leading-run. spec-test-build-install-commit-push"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_skips_when_clean() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #clean. spec-test-build-install-commit-push
### Re: #clean — opus-4-7

Landed cleanly.
<!-- /agent:exchange -->
";
        let result = strip_prompt_prefix_from_response_body_first_lines(content);
        assert!(
            result.is_none(),
            "clean document must not trigger the strip path"
        );
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_preserves_inner_prompt_like_lines() {
        // A `❯ ` appearing AFTER the first body line — e.g. quoted user input
        // inside the response prose — must be preserved. Only the leaked
        // first-line marker is stripped.
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #inner. spec-test-build-install-commit-push
### Re: #inner — opus-4-7

❯ first line gets stripped

The user said:
❯ this quoted line stays
because it is not the first body line.
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("leaked first-line ❯ must be stripped");
        assert!(repaired.contains("\nfirst line gets stripped\n"));
        assert!(!repaired.contains("❯ first line gets stripped"));
        // Inner `❯ ` is preserved — it is part of the response body text.
        assert!(repaired.contains("❯ this quoted line stays"));
    }

    #[test]
    fn strip_prompt_prefix_from_response_body_first_lines_handles_multiple_re_blocks() {
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #a
### Re: #a — opus-4-7

❯ first response

❯ do #b
### Re: #b — opus-4-7

❯ second response
<!-- /agent:exchange -->
";
        let repaired = strip_prompt_prefix_from_response_body_first_lines(content)
            .expect("multiple leaks must be stripped");
        assert!(repaired.contains("\nfirst response\n"));
        assert!(repaired.contains("\nsecond response\n"));
        assert!(!repaired.contains("❯ first response"));
        assert!(!repaired.contains("❯ second response"));
        // User prompts between blocks preserved.
        assert!(repaired.contains("❯ do #a"));
        assert!(repaired.contains("❯ do #b"));
    }

    #[test]
    fn normalize_final_template_content_removes_adjacent_duplicate_response_blocks() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let duplicated = "\
<!-- agent:exchange patch=append -->
❯ do #duppb. spec-test-build-install-commit-push
### Re: #duppb — gpt-5

Implemented.

Verification:
- `cargo test`
### Re: #duppb — gpt-5

Implemented.

Verification:
- `cargo test`
<!-- /agent:exchange -->
";

        let repaired = normalize_final_template_content(
            &doc,
            baseline,
            Some(baseline),
            None,
            duplicated,
            None,
        )
        .expect("duplicate response repair should succeed");

        assert_eq!(
            repaired.matches("### Re: #duppb — gpt-5").count(),
            1,
            "closeout normalization must remove adjacent duplicate response blocks: {repaired}"
        );
        assert!(repaired.contains("Verification:\n- `cargo test`"));
    }

    #[test]
    fn normalize_final_template_content_scrubs_duplicate_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let base = concat!(
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
        let merged = base
            .replace(
                "<!-- /agent:exchange -->",
                "### Re: duplicate prompt cleanup — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
            )
            .replace(
                "<!-- agent:backlog -->",
                "<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n-->\n\n<!-- agent:backlog -->",
            );

        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, &merged, None)
                .unwrap();

        assert!(
            repaired.contains("❯ The duplicate content corrupting document"),
            "live prompt should remain in exchange and be normalized:\n{repaired}"
        );
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "duplicate post-exchange prompt text should be scrubbed from comments:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "duplicate prompt cleanup must preserve the ordinary HTML comment shell:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"),
            "backlog scaffold should remain intact:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_preserves_baseline_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let base = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let merged = base.replace(
            "<!-- /agent:exchange -->",
            "### Re: sqlitedb graph performance next steps — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
        );

        let repaired =
            normalize_final_template_content(&doc, &base, Some(&base), None, &merged, None)
                .unwrap();

        assert!(
            repaired.contains(&format!("<!--\n{prompt}\n-->")),
            "baseline-owned post-exchange scratch text must not be scrubbed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_preserves_current_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "The html comment below this document's agent:exchange close tag had content that I put into it. This should not happen. #spec-test-build-install-commit-push";
        let base = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        let before_current = base.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));
        let merged = before_current.replace(
            "<!-- /agent:exchange -->",
            "### Re: scratch comment preservation — gpt-5\n\nImplemented.\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->",
        );

        let repaired = normalize_final_template_content(
            &doc,
            &base,
            Some(&base),
            Some(&before_current),
            &merged,
            None,
        )
        .unwrap();

        assert!(
            repaired.contains(&format!("<!--\n{prompt}\n-->")),
            "current visible post-exchange scratch text must not be scrubbed:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_preserves_unique_post_exchange_html_comment_tail() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "do #visible. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "Keep this unrelated scratch note hidden.\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(
            repaired.contains("Keep this unrelated scratch note hidden."),
            "unique scratch comments must stay outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_scrubs_answered_prompt_html_comment_body() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "❯ The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. Was this an issue because I didn't restart agent-doc on this document? #spec-test-build-install-commit-push\n",
            "### Re: backlog update and duplicate prompt corruption — gpt-5\n",
            "Implemented.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc).unwrap();

        assert!(
            repaired.contains("### Re: backlog update and duplicate prompt corruption"),
            "answered exchange turn should remain:\n{repaired}"
        );
        assert!(
            !repaired.contains(
                "\n<!--\nThe duplicate content corrupting document and duplicate prompt issues happened yet again."
            ),
            "answered duplicate prompt text should be scrubbed from the HTML comment:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "answered duplicate prompt cleanup must preserve the ordinary HTML comment shell:\n{repaired}"
        );
    }

    #[test]
    fn normalize_final_template_content_repairs_duplicate_exchange_close_after_merge() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Earlier prompt
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->

### Re: #xguard — gpt-5

Implemented.
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] keep me
<!-- /agent:backlog -->
";

        std::fs::write(&doc, merged).unwrap();
        let repaired =
            normalize_final_template_content(&doc, base, Some(snapshot), None, merged, None)
                .unwrap();

        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: #xguard — gpt-5").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "response should be restored inside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
    }

    #[test]
    fn normalize_final_template_content_repairs_response_before_prompt_tail() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let base = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
Can you preserve the second paragraph too?
<!-- agent:boundary:old -->
<!-- /agent:exchange -->
";
        let merged = "\
<!-- agent:exchange patch=append -->
❯ Please handle the timeout fallback.
### Re: timeout fallback — gpt-5

Done.
<!-- agent:boundary:new -->
Can you preserve the second paragraph too?
<!-- /agent:exchange -->
";
        let response = "### Re: timeout fallback — gpt-5\n\nDone.\n";

        let repaired = normalize_final_template_content(
            &doc,
            base,
            Some(snapshot),
            None,
            merged,
            Some(response),
        )
        .unwrap();

        let prompt_tail = repaired
            .find("Can you preserve the second paragraph too?")
            .unwrap();
        let response_heading = repaired.find("### Re: timeout fallback").unwrap();
        let boundary = repaired.find("<!-- agent:boundary:").unwrap();
        let close = repaired.find("<!-- /agent:exchange -->").unwrap();
        assert!(
            prompt_tail < response_heading,
            "prompt tail must move before response:\n{repaired}"
        );
        assert!(
            response_heading < boundary && boundary < close,
            "boundary must close the repaired response turn:\n{repaired}"
        );
    }

    #[test]
    fn normalize_template_structure_repairs_duplicate_scaffold_close() {
        let dir = TempDir::new().unwrap();
        let doc_path = dir.path().join("doc.md");
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "❯ keep this prompt\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );

        let repaired = normalize_template_structure_or_fail(content, &doc_path)
            .expect("pure duplicated scaffold should be repaired");

        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:backlog -->").count(), 1);
        assert!(repaired.contains("❯ keep this prompt"));
    }

    #[test]
    fn normalize_template_structure_rejects_duplicate_scaffold_with_user_text() {
        let dir = TempDir::new().unwrap();
        let doc_path = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "c The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "corky.md The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        fs::write(&doc_path, content).unwrap();

        let err = normalize_template_structure_or_fail(content, &doc_path).unwrap_err();

        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("flow=document_mutation"));
        assert!(log.contains("reason=mixed_duplicate_scaffold_tail"));
    }
}

#[cfg(test)]
mod queue_prompt_echo_summary_tests {
    use super::*;

    #[test]
    fn echo_copies_verbatim_when_threshold_is_none() {
        // #queue-prompt-echo-summary: default (None) preserves the verbatim copy
        // the user asked to keep "for now".
        let long = "do [#x] ".to_string() + &"word ".repeat(100);
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&long), None);
        assert!(echo.starts_with("> **Queue prompt:**\n>\n"));
        assert!(echo.contains(long.trim_end()));
        assert!(!echo.contains("more chars"));
    }

    #[test]
    fn echo_copies_verbatim_when_under_threshold() {
        let short = "do [#x] short prompt".to_string();
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&short), Some(200));
        assert!(echo.contains("> do [#x] short prompt"));
        assert!(!echo.contains("more chars"));
    }

    #[test]
    fn echo_summarizes_when_over_threshold() {
        let long = "First line is the gist.\n".to_string() + &"tail ".repeat(100);
        let echo = format_consumed_prompt_echo(std::slice::from_ref(&long), Some(40));
        // The verbatim tail must NOT appear; a bounded summary must.
        assert!(!echo.contains(&"tail ".repeat(100)));
        assert!(echo.contains("First line is the gist."));
        assert!(echo.contains("more chars; full prompt retained in agent:queue"));
        // Summary is a single quoted line plus the label.
        assert_eq!(echo.matches("more chars").count(), 1);
    }

    #[test]
    fn summarize_truncates_first_line_on_char_boundary() {
        // Multibyte content must not panic and must truncate on a char boundary.
        let text = "héllo wörld ".repeat(20);
        let summary = summarize_consumed_prompt(&text, 5);
        assert!(summary.starts_with("héllo"));
        assert!(summary.contains("more chars"));
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
    fn orchestrate_contract_rejects_non_exchange_patch() {
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

    #[test]
    fn orchestrate_contract_allows_clean_plain_response() {
        enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "### Re: orchplainresp — gpt-5\n\nImplemented and verified.",
        )
        .expect("clean plain orchestrate response should synthesize exchange append");
    }

    #[test]
    fn orchestrate_contract_allows_explicit_multi_component_patch() {
        let patches = vec![
            crate::template::PatchBlock::new("exchange", "response"),
            crate::template::PatchBlock::new("status", "updated"),
        ];
        enforce_orchestrate_template_patch_contract(Some("orchestrate"), &patches, "")
            .expect("explicit multi-component patch should be accepted");
    }

    #[test]
    fn orchestrate_contract_rejects_plain_transcript_prompt_lines() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "### Re: topic — gpt-5\n\nDone.\n❯ do #next",
        )
        .unwrap_err();
        assert!(err.to_string().contains("transcript prompt lines"));
    }

    #[test]
    fn orchestrate_contract_rejects_plain_transcript_headings() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "## User\nrequest\n\n## Assistant\nresponse",
        )
        .unwrap_err();
        assert!(err.to_string().contains("transcript headings"));
    }

    #[test]
    fn orchestrate_contract_rejects_plain_full_document_dump() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "<!-- agent:exchange -->\n### Re: topic — gpt-5\n<!-- /agent:exchange -->",
        )
        .unwrap_err();
        assert!(err.to_string().contains("component markers"));
    }

    #[test]
    fn orchestrate_contract_rejects_sanitized_full_document_dump() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "&lt;!-- agent:exchange --&gt;\n### Re: topic — gpt-5\n&lt;!-- /agent:exchange --&gt;",
        )
        .unwrap_err();
        assert!(err.to_string().contains("component markers"));
    }

    #[test]
    fn orchestrate_contract_rejects_multiple_plain_responses() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "### Re: first — gpt-5\n\nOne.\n\n### Re: second — gpt-5\n\nTwo.",
        )
        .unwrap_err();
        assert!(err.to_string().contains("only one assistant response"));
    }

    #[test]
    fn template_response_write_proof_accepts_nonempty_unmatched_body() {
        let proof = super::template_response_write_proof(&[], "### Re: topic — gpt-5\nbody\n");
        assert!(proof.has_real_body());
        assert_eq!(proof.unmatched_len, "### Re: topic — gpt-5\nbody".len());
    }

    #[test]
    fn template_response_write_proof_rejects_empty_response_shells() {
        let patches = vec![
            crate::template::PatchBlock::new("exchange", ""),
            crate::template::PatchBlock::new("frontmatter", "agent: codex"),
        ];
        let err = super::ensure_template_response_write_proof(&patches, "").unwrap_err();
        assert!(err.to_string().contains("no real response-body write"));
    }
}

#[cfg(test)]
mod precommit_pending_capture_tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command as ProcessCommand;

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
        setup_precommit_with_tracked_work(
            root,
            frontmatter,
            response,
            pending_body,
            None,
            pending_done_ids,
        )
    }

    fn setup_precommit_with_tracked_work(
        root: &std::path::Path,
        frontmatter: &str,
        response: &str,
        pending_body: &str,
        icebox_body: Option<&str>,
        pending_done_ids: &[&str],
    ) -> std::path::PathBuf {
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("doc.md");
        let mut content = format!(
            "{frontmatter}<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:pending -->\n{pending_body}<!-- /agent:pending -->\n"
        );
        if let Some(icebox_body) = icebox_body {
            content.push_str("\n<!-- agent:icebox -->\n");
            content.push_str(icebox_body);
            if !icebox_body.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("<!-- /agent:icebox -->\n");
        }
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

    fn write_backlog_doc(path: &Path, backlog_body: &str) {
        let content = format!(
            "---\nagent_doc_session: target\n---\n\n<!-- agent:backlog -->\n{backlog_body}<!-- /agent:backlog -->\n"
        );
        fs::write(path, content).unwrap();
    }

    fn backlog_component_hash(path: &Path) -> String {
        let content = fs::read_to_string(path).unwrap();
        let components = crate::component::parse(&content).unwrap();
        let component = components
            .iter()
            .find(|component| crate::component::is_backlog_component(&component.name))
            .unwrap();
        crate::ops_log::content_hash(component.content(&content))
    }

    fn init_git_repo(root: &Path, tracked: &Path) {
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", tracked.file_name().unwrap().to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
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
    fn prewrite_pending_capture_accepts_pending_done_resolution() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: #done1 — gpt-5\n\nImplemented and verified.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();

        super::prewrite_pending_capture_check(
            &doc,
            "### Re: #done1 — gpt-5\n\nImplemented and verified.\n",
            &super::WriteFlags {
                has_pending_done: true,
                pending_done_ids: vec!["done1".to_string()],
                strict_closeout: true,
                ..Default::default()
            },
        )
        .expect("pending-done should satisfy do-id backlog capture");
    }

    #[test]
    fn precommit_pending_capture_accepts_recorded_pending_done_mutation() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: strict\n---\n\n",
            "### Re: #done1 — gpt-5\n\nImplemented and verified.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_pending_done_ids(&doc, &["done1".to_string()]).unwrap();
        crate::cycle_state::mark_pending_mutations(&doc).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("recorded pending-done mutation should satisfy capture guard");
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
    fn precommit_blocks_backlog_required_review_without_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: code review — opus-4-6\n\n1. High: Queue closeout can drift.\n2. Medium: Snapshot repair is too permissive.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("requested backlog capture"));
        assert!(err.to_string().contains("--pending-add"));
    }

    #[test]
    fn precommit_allows_backlog_required_review_with_explicit_no_followups() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: code review — opus-4-6\n\nNo new backlog item came out of this change.\n",
            false,
        );
        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("explicit no-follow-up proof should satisfy backlog-required closeout");
    }

    #[test]
    fn precommit_blocks_when_explicit_backlog_target_is_unchanged() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nTransferred issue notes.\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(
            &doc,
            &[crate::cycle_state::BacklogTargetRequirement {
                path: std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string(),
                component: Some("backlog".to_string()),
                baseline_hash: Some(backlog_component_hash(&target)),
                baseline_item_ids: vec!["old1".to_string()],
            }],
        )
        .unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("required backlog capture in"));
        assert!(err.to_string().contains(target.to_string_lossy().as_ref()));
    }

    #[test]
    fn precommit_still_checks_explicit_backlog_target_after_current_pending_add() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned item:\n- [ ] [#new1] New transferred item\n",
            true,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(
            &doc,
            &[crate::cycle_state::BacklogTargetRequirement {
                path: std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string(),
                component: Some("backlog".to_string()),
                baseline_hash: Some(backlog_component_hash(&target)),
                baseline_item_ids: vec!["old1".to_string()],
            }],
        )
        .unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("required backlog capture in"));
        assert!(err.to_string().contains(target.to_string_lossy().as_ref()));
    }

    #[test]
    fn prewrite_still_checks_explicit_backlog_target_after_pending_add_flag() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — gpt-5\n\nPlanned item:\n- [ ] [#new1] New transferred item\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(
            &doc,
            &[crate::cycle_state::BacklogTargetRequirement {
                path: std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string(),
                component: Some("backlog".to_string()),
                baseline_hash: Some(backlog_component_hash(&target)),
                baseline_item_ids: vec!["old1".to_string()],
            }],
        )
        .unwrap();

        let err = super::prewrite_pending_capture_check(
            &doc,
            "### Re: #agent-doc-bug — gpt-5\n\nPlanned item:\n- [ ] [#new1] New transferred item\n",
            &super::WriteFlags {
                has_pending_add: true,
                strict_closeout: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("required backlog capture in"));
        assert!(err.to_string().contains(target.to_string_lossy().as_ref()));
    }

    #[test]
    fn precommit_allows_when_explicit_backlog_target_changed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nTransferred issue notes.\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");
        let requirement = crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some(backlog_component_hash(&target)),
            baseline_item_ids: vec!["old1".to_string()],
        };
        write_backlog_doc(
            &target,
            "- [ ] [#new1] New transferred item\n- [ ] [#old1] Existing item\n",
        );

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(&doc, &[requirement]).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("changed explicit backlog target should satisfy closeout");
    }

    #[test]
    fn precommit_blocks_when_bug_transfer_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#lvak] Routed-cycle ack follow-up\n- [ ] [#old1] Existing item\n",
        );
        let requirement = crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some("baseline".to_string()),
            baseline_item_ids: vec!["old1".to_string()],
        };

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(&doc, &[requirement]).unwrap();
        crate::cycle_state::record_required_explicit_backlog_item_count(&doc, 4).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("described at least 4 distinct issue(s)")
        );
        assert!(
            err.to_string()
                .contains("only enumerated 2 explicit backlog item(s)")
        );
    }

    #[test]
    fn precommit_blocks_when_response_promises_multiple_new_target_items_but_only_some_exist() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nPlanned agent-doc backlog items:\n- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#mcrc] Uncommitted repair follow-up\n- [ ] [#lvls] Preserve list-shape constraint\n",
            false,
        );
        let target = tmp.path().join("bugs.md");
        write_backlog_doc(&target, "- [ ] [#old1] Existing item\n");
        let requirement = crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some(backlog_component_hash(&target)),
            baseline_item_ids: vec!["old1".to_string()],
        };
        write_backlog_doc(
            &target,
            "- [ ] [#zpc0] Existing transfer that landed\n- [ ] [#old1] Existing item\n",
        );

        crate::cycle_state::record_backlog_capture_requirement(&doc, true).unwrap();
        crate::cycle_state::record_backlog_target_requirements(&doc, &[requirement]).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(err.to_string().contains("promised new tracked item(s)"));
        assert!(err.to_string().contains("#mcrc"));
        assert!(err.to_string().contains("#lvls"));
    }

    #[test]
    fn precommit_blocks_when_bug_plan_reference_inventory_is_smaller_than_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nFiled two bugs.\nPlan: `tasks/agent-doc/plan-session-check-prefix-duplication.md`\n",
            false,
        );
        let plan = tmp
            .path()
            .join("tasks/agent-doc/plan-session-check-prefix-duplication.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# Plan\n").unwrap();

        crate::cycle_state::record_required_plan_reference_count(&doc, 2).unwrap();

        let err = super::precommit_pending_capture_check(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("required at least 2 explicit plan reference(s)")
        );
        assert!(
            err.to_string()
                .contains("only cited 1 existing plan path(s)")
        );
    }

    #[test]
    fn precommit_allows_when_bug_plan_reference_inventory_matches_prompt_contract() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit(
            tmp.path(),
            "---\nagent_doc_session: test\npending_capture_guard: warn\n---\n\n",
            "### Re: #agent-doc-bug — opus-4-6\n\nFiled two bugs.\n1. **#scpd** Plan: `tasks/agent-doc/plan-session-check-prefix-duplication.md`\n2. **#nbla** Plan: `tasks/agent-doc/plan-chat-level-agent-doc-bug-contract.md`\n",
            false,
        );
        let first_plan = tmp
            .path()
            .join("tasks/agent-doc/plan-session-check-prefix-duplication.md");
        let second_plan = tmp
            .path()
            .join("tasks/agent-doc/plan-chat-level-agent-doc-bug-contract.md");
        std::fs::create_dir_all(first_plan.parent().unwrap()).unwrap();
        std::fs::write(&first_plan, "# Plan\n").unwrap();
        std::fs::write(&second_plan, "# Plan\n").unwrap();

        crate::cycle_state::record_required_plan_reference_count(&doc, 2).unwrap();

        super::precommit_pending_capture_check(&doc)
            .expect("matching plan references should satisfy closeout");
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
        assert!(err.to_string().contains("--done 4qja"));
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
    fn precommit_pending_done_auto_done_marks_item_done() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\nauto_done: true\n---\n\n",
            "### Re: #4qja streaming orchestrate patchback — gpt-5\n\nImplemented the sequential orchestration streaming path for CRDT docs.\nVerification:\n- cargo test\n",
            "- [ ] [#4qja] Stream orchestrate patchback\n",
            &[],
        );

        super::precommit_pending_done_check(&doc)
            .expect("auto_done should record and apply missing --done mutations");
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("- [x] [#4qja] Stream orchestrate patchback"));
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert!(state.pending_done_ids.contains(&"4qja".to_string()));
        assert!(state.had_pending_mutations);
    }

    #[test]
    fn precommit_pending_done_passes_when_id_was_kept_open() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #fvtg rescope — gpt-5\n\nUpdated #fvtg to keep the rollout validation item open.\nVerification:\n- cargo test\n",
            "- [ ] [#fvtg] Rollout validation item\n",
            &[],
        );
        crate::cycle_state::record_pending_kept_open_ids(&doc, &["fvtg".to_string()])
            .unwrap()
            .unwrap();

        super::precommit_pending_done_check(&doc)
            .expect("kept-open pending ids should not require --done");
    }

    #[test]
    fn prewrite_pending_done_uses_kept_open_flag_ids() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_pending(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "placeholder response",
            "- [ ] [#fvtg] Rollout validation item\n",
            &[],
        );

        super::prewrite_pending_done_check(
            &doc,
            "### Re: #fvtg rescope — gpt-5\n\nUpdated #fvtg to keep the rollout validation item open.\nVerification:\n- cargo test\n",
            &super::WriteFlags {
                pending_kept_open_ids: vec!["#FVTG".to_string()],
                strict_closeout: true,
                ..Default::default()
            },
        )
        .expect("pre-write kept-open ids should not require --done");
    }

    #[test]
    fn precommit_pending_done_blocks_for_icebox_only_item_without_recorded_done() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_precommit_with_tracked_work(
            tmp.path(),
            "---\nagent_doc_session: test\n---\n\n",
            "### Re: #ice01 parked follow-up — gpt-5\n\nImplemented the parked follow-up.\n",
            "- [ ] [#keep1] Keep backlog item\n",
            Some("- [ ] [#ice01] Parked follow-up\n"),
            &[],
        );

        let err = super::precommit_pending_done_check(&doc).unwrap_err();
        assert!(err.to_string().contains("#ice01"));
        assert!(err.to_string().contains("--done ice01"));
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

    #[test]
    fn required_closeout_fails_when_only_later_prompt_drift_remains() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/state/cycles")).unwrap();
        fs::create_dir_all(tmp.path().join(".agent-doc/captures")).unwrap();

        let doc = tmp.path().join("doc.md");
        let initial = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "body\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, initial).unwrap();
        init_git_repo(tmp.path(), &doc);
        crate::snapshot::save(&doc, initial).unwrap();

        let drifted = initial.replace(
            "<!-- /agent:exchange -->\n",
            "do #followup. spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, &drifted).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_already_current",
            Some(initial),
            Some(&drifted),
        )
        .unwrap();

        let err = super::complete_required_closeout(&doc).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("unresolved prompt-bearing user changes"));
        assert!(message.contains("do #followup. spec-test-build-install-commit-push"));
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

    fn doc_with_todo(root: &TempDir, todo_body: &str) -> (PathBuf, String) {
        let doc = root.path().join("todo.md");
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:todo patch=replace -->\n{todo_body}<!-- /agent:todo -->\n"
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
    fn normalize_pending_patch_allows_existing_alias_tag_items() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!("### Active\n", "- [ ] [#yckq] [#ss01] ShipStation fix\n");
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#new1] add phone confirmation item\n",
                "- [ ] [#yckq] [#ss01] ShipStation fix\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("existing alias-tag items should not block normalization");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("[#new1] add phone confirmation item"));
        assert!(rewritten.contains("[#yckq] [#ss01] ShipStation fix"));
    }

    #[test]
    fn normalize_pending_patch_preserves_interleaved_headers() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!(
            "### Active\n",
            "- [ ] [#keep1] existing item\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep2] later item\n"
        );
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#keep1] existing item\n",
                "- [ ] [#new1] new top item\n",
                "\n",
                "### Later\n",
                "- [ ] [#keep2] later item\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("header-preserving patch should normalize");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(
            rewritten
                .contains("### Active\n- [ ] [#keep1] existing item\n- [ ] [#new1] new top item\n")
        );
        assert!(rewritten.contains("\n\n### Later\n- [ ] [#keep2] later item\n"));
    }

    #[test]
    fn normalize_pending_patch_merges_partial_structured_prefix() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!(
            "### Active\n",
            "- [ ] [#keep1] existing item\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep2] later item\n"
        );
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#keep1] existing item\n",
                "- [ ] [#new1] new top item\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("prefix-only structured patch should merge with later sections");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("[#new1] new top item"));
        assert!(rewritten.contains("### Later\n- [ ] [#keep2] later item\n"));
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

    #[test]
    fn destructive_todo_patch_is_rejected_when_it_drops_checklist_items() {
        let tmp = TempDir::new().unwrap();
        let (_doc, content) = doc_with_todo(
            &tmp,
            concat!(
                "### Phase 1\n\n",
                "- [x] Select benchmark\n",
                "- [x] Write methodology\n\n",
                "### Phase 2\n\n",
                "- [ ] Expand git signal extraction\n",
                "- [ ] Re-score sessions\n",
            ),
        );
        let patches = vec![crate::template::PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Select benchmark\n",
                "- [x] Write methodology\n",
            ),
        )];

        let err = super::enforce_no_destructive_todo_patch(&content, &patches)
            .expect_err("subset todo patch should fail closed");
        let msg = err.to_string();
        assert!(
            msg.contains("patch:todo would reduce total checklist item count from 4 to 2"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn todo_patch_with_same_checklist_count_is_allowed() {
        let tmp = TempDir::new().unwrap();
        let (_doc, content) = doc_with_todo(
            &tmp,
            concat!(
                "### Phase 1\n\n",
                "- [ ] Original item 1\n",
                "- [ ] Original item 2\n",
            ),
        );
        let patches = vec![crate::template::PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Updated item 1\n",
                "- [ ] Updated item 2\n",
            ),
        )];

        super::enforce_no_destructive_todo_patch(&content, &patches)
            .expect("same-size todo rewrite should remain allowed");
    }
}

#[cfg(test)]
mod late_fallback_patch_guard_tests {
    use super::{
        IpcDiskRepairReason, IpcRepairDecision, IpcSnapshotSource, WriteFlags,
        cleanup_fallback_patch_files, cycle_already_committed, recover_dedupe_only_drift,
        recover_empty_response_for_strict_closeout, redeliver_ipc_dedupe_to_editor,
        repair_ipc_decision_visible_state, try_ipc, try_ipc_full_content,
        try_ipc_full_content_operator_mutation_from_source,
    };
    use crate::snapshot;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn doc_in_agent_doc_project(tmp: &TempDir, content: &str) -> std::path::PathBuf {
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("state").join("cycles")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = tmp.path().join("doc.md");
        fs::write(&doc, content).unwrap();
        doc
    }

    struct TsiftDuplicateContentFixture {
        bad_state_before_live_typing: &'static str,
        repaired_snapshot: &'static str,
        live_buffer_after_typing: &'static str,
    }

    fn tsift_md_duplicate_content_corruption_fixture() -> TsiftDuplicateContentFixture {
        TsiftDuplicateContentFixture {
            bad_state_before_live_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-bad -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            repaired_snapshot: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "<!-- agent:boundary:tsift-repaired -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
            live_buffer_after_typing: concat!(
                "---\n",
                "agent_doc_session: tsift-v0.1\n",
                "agent: codex\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "*Compacted tsift context.*\n",
                "❯ The duplicate content corrupt document bug occurred. Do we need more logic to prevent the full-document ipc?\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "### Re: benchmark and IPC payload guard — gpt-5\n\n",
                "Ran the graph backend benchmark and added IPC payload guard coverage.\n",
                "The duplicate content corrupt document bug happened on tsift.md as I was tying in a prompt. ",
                "What are #next-steps to ensure full-document IPC is not over-eager? #next-steps\n",
                "<!-- agent:boundary:tsift-live -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!-- agent:queue -->\n",
                "<!-- /agent:queue -->\n"
            ),
        }
    }

    #[test]
    fn ipc_repair_decision_records_prefix_fallback_bad_state() {
        let decision = IpcRepairDecision::content_ours_prefix_fallback(
            "fixed snapshot".to_string(),
            "bad editor state".to_string(),
            &["bad editor state".to_string()],
        );

        assert_eq!(decision.snapshot_content, "fixed snapshot");
        assert_eq!(decision.snap_source, IpcSnapshotSource::ContentOurs);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergence)
        );
        assert!(decision.redeliver_editor);
        let bad_state = decision
            .editor_bad_state
            .as_ref()
            .expect("prefix fallback should capture bad editor state");
        assert_eq!(bad_state.content(), "bad editor state");
        assert_eq!(bad_state.len, "bad editor state".len());
        assert_eq!(
            bad_state.hash,
            crate::ops_log::content_hash("bad editor state")
        );
        assert_eq!(decision.normalize_prefix_lines, vec!["bad editor state"]);
    }

    #[test]
    fn ipc_repair_decision_preserves_original_bad_state_when_dedupe_follows_prefix_fallback() {
        let decision = IpcRepairDecision::content_ours_prefix_fallback(
            "prefix fallback with duplicate response".to_string(),
            "visible sidecar before fallback".to_string(),
            &["visible sidecar before fallback".to_string()],
        )
        .apply_ipc_dedupe(
            "deduped snapshot".to_string(),
            "prefix fallback with duplicate response".to_string(),
        );

        assert_eq!(decision.snapshot_content, "deduped snapshot");
        assert_eq!(decision.snap_source, IpcSnapshotSource::ContentOurs);
        assert_eq!(
            decision.disk_repair_reason,
            Some(IpcDiskRepairReason::PrefixDivergenceThenIpcDedupe)
        );
        assert!(decision.redeliver_editor);
        assert_eq!(
            decision
                .editor_bad_state
                .as_ref()
                .expect("combined repair should keep original bad editor proof")
                .content(),
            "visible sidecar before fallback"
        );
    }

    #[test]
    fn cycle_already_committed_returns_none_when_no_state() {
        let tmp = TempDir::new().unwrap();
        let doc = tmp.path().join("nonexistent.md");
        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cycle_already_committed_returns_some_for_committed_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let result = cycle_already_committed(&doc);
        assert!(result.is_some(), "should return Some for committed cycle");
    }

    #[test]
    fn cycle_already_committed_returns_none_for_open_cycle() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        assert!(cycle_already_committed(&doc).is_none());
    }

    #[test]
    fn cleanup_fallback_patch_files_removes_patch_and_writes_sentinel() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        let patch_content = serde_json::json!({
            "patch_id": "test-patch-123",
            "type": "patch",
        });
        fs::write(
            &patch_path,
            serde_json::to_string_pretty(&patch_content).unwrap(),
        )
        .unwrap();
        assert!(patch_path.exists());

        cleanup_fallback_patch_files(&doc);

        assert!(
            !patch_path.exists(),
            "fallback patch file should be removed"
        );
        let sentinel = tmp
            .path()
            .join(".agent-doc/claimed-patches")
            .join("test-patch-123");
        assert!(sentinel.exists(), "claimed sentinel should be written");
    }

    #[test]
    fn cleanup_fallback_patch_files_noop_when_no_patch() {
        let tmp = TempDir::new().unwrap();
        let doc =
            doc_in_agent_doc_project(&tmp, "---\nagent_doc_session: test\n---\n\n## Exchange\n");
        cleanup_fallback_patch_files(&doc);
    }

    #[test]
    fn try_ipc_marks_committed_cycle_skip_as_not_consumed() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "late-patch-123"}).to_string(),
        )
        .unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "late response");
        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            None,
            None,
            None,
            Some("current-patch-456"),
        )
        .unwrap();

        assert!(
            !result.success,
            "committed-cycle IPC skip must not look like a consumed write"
        );
        assert_eq!(result.patch_id, "current-patch-456");
        assert!(
            result.skipped_committed_cycle,
            "caller must be able to stop terminal fallback handling"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/late-patch-123")
                .exists(),
            "removed stale patch should be claimed so watchers cannot replay it"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=current-patch-456"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("ipc_write_consumed"),
            "terminal skip must not be logged as an IPC consume"
        );
    }

    #[test]
    fn full_content_ipc_skips_committed_cycle_before_socket_or_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n";
        let doc = doc_in_agent_doc_project(&tmp, content);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "test",
            Some(content),
            Some(content),
            "fake-sha",
            None,
        )
        .unwrap();
        crate::cycle_state::mark_write_applied(&doc, "test", Some(content), Some(content)).unwrap();
        crate::cycle_state::mark_committed(&doc, "test", Some(content), Some(content)).unwrap();

        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let stale_patch_path = tmp
            .path()
            .join(".agent-doc/patches")
            .join(format!("{hash}.json"));
        fs::write(
            &stale_patch_path,
            serde_json::json!({"patch_id": "full-content-stale"}).to_string(),
        )
        .unwrap();

        let result = try_ipc_full_content(&doc, "stale full-content repair").unwrap();

        assert!(!result, "committed-cycle full-content IPC must be skipped");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            content,
            "full-content IPC must not dirty an already committed cycle"
        );
        assert!(
            !stale_patch_path.exists(),
            "stale full-content fallback patch should be removed"
        );
        assert!(
            tmp.path()
                .join(".agent-doc/claimed-patches/full-content-stale")
                .exists(),
            "removed full-content fallback patch should be claimed"
        );

        let ops_log = fs::read_to_string(tmp.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(ops_log.contains("late_fallback_patch_rejected"));
        assert!(ops_log.contains("patch_id=full_content"));
        assert!(ops_log.contains(
            "flow=closeout stage=terminal_guard outcome=blocked reason=already_committed"
        ));
        assert!(
            !ops_log.contains("socket_full_content"),
            "full-content socket diagnostic must not be emitted after committed-cycle skip"
        );
    }

    #[test]
    fn full_content_operator_ipc_is_disabled_before_source_buffer_delivery() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted when the disk buffer already contains live drift"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must not overwrite live prompt drift"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "disabled full-content path must not hand a patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=compact_exchange"),
            "disabled full-content path should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn full_content_operator_ipc_rejects_late_post_exchange_scratch_comment() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let prompt = "The full-document IPC scratch comment was typed below exchange after target computation. #spec-test-build-install-commit-push";
        let source = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n"
        );
        let live = source.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));
        let target = source.replace(
            "### Re: previous — gpt-5\n\nDone.\n",
            "### Session Summary\n\nCompacted.\n",
        );
        fs::write(&doc, &live).unwrap();

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, &target, source).unwrap();

        assert!(
            !result,
            "operator full-content IPC must not be emitted after a late post-exchange scratch edit"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale full-content replacement must preserve the live scratch comment"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "failed full-content IPC must not save a snapshot"
        );
        let patch_count = fs::read_dir(agent_doc_dir.join("patches"))
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(
            patch_count, 0,
            "scope/source guards must not hand a full-content patch to file IPC"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("source=compact_exchange"),
            "component-scope rejection should be logged before source-buffer proof:\n{ops_log}"
        );
    }

    #[test]
    fn response_fallback_full_content_is_disabled_before_socket_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let fallback = "before\n";
        let live = "before\nlive prompt typed after fallback was computed\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result = try_ipc_full_content(&doc, fallback).unwrap();

        assert!(
            !result,
            "stale response fallback full-content IPC must be skipped before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale response fallback full-content payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live,
            "stale response fallback must not overwrite live prompt drift"
        );
        assert!(snapshot::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=response_fallback"),
            "disabled full-content path should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_full_content_redelivery_is_disabled() {
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, bad_state).unwrap();

        let seen_payload = Arc::new(Mutex::new(None::<serde_json::Value>));
        let listener_seen = seen_payload.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                *listener_seen.lock().unwrap() = Some(payload.clone());
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_ipc_dedupe_to_editor(&doc, repaired, bad_state);

        assert!(!delivered, "full-content redelivery is disabled");
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            bad_state,
            "disabled full-content redelivery must not mutate the editor-visible file"
        );
        assert!(
            seen_payload.lock().unwrap().is_none(),
            "listener should not receive a disabled full-content payload"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled")
                && ops_log.contains("source=response_fallback"),
            "disabled redelivery should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn ipc_dedupe_redelivery_skips_when_bad_state_is_stale() {
        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let bad_state = "before\n### Re: issue — gpt-5\nDone.\n### Re: issue — gpt-5\nDone.\n";
        let live_state = "before\nlive prompt typed after repair planning\n";
        let repaired = "before\n### Re: issue — gpt-5\nDone.\n";
        fs::write(&doc, live_state).unwrap();

        let delivered = redeliver_ipc_dedupe_to_editor(&doc, repaired, bad_state);

        assert!(
            !delivered,
            "redelivery must be skipped when the visible bad-state proof is stale"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            live_state,
            "stale redelivery must not overwrite live content"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale redelivery skip should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn template_ipc_dedupe_repair_uses_disk_not_full_content_redelivery() {
        let tmp = TempDir::new().unwrap();
        let bad_state = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: issue — gpt-5\nDone.\n",
            "<!-- /agent:exchange -->\n"
        );
        let doc = doc_in_agent_doc_project(&tmp, bad_state);
        let agent_doc_dir = tmp.path().join(".agent-doc");

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let decision = IpcRepairDecision::file_read(bad_state.to_string())
            .apply_ipc_dedupe(repaired.to_string(), bad_state.to_string());
        repair_ipc_decision_visible_state(&doc, &decision, Some("source-patch")).unwrap();

        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "component-scoped template repairs must not send socket fullContent payloads"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            repaired,
            "template duplicate repair should fall back to guarded disk repair"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_scope_rejected")
                && ops_log.contains("scope=template_frontmatter")
                && ops_log.contains("ipc_dedupe_repaired_working_tree"),
            "template fullContent rejection and disk repair should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn tsift_md_duplicate_content_fixture_skips_stale_full_document_redelivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let fixture = tsift_md_duplicate_content_corruption_fixture();
        let doc = tmp.path().join("tasks/software/tsift.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, fixture.live_buffer_after_typing).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let delivered = redeliver_ipc_dedupe_to_editor(
            &doc,
            fixture.repaired_snapshot,
            fixture.bad_state_before_live_typing,
        );

        assert!(
            !delivered,
            "tsift.md fixture must skip full-document redelivery when the visible buffer changed after repair planning"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "stale tsift.md repair proof must be rejected before any socket fullContent payload"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            fixture.live_buffer_after_typing,
            "live tsift.md prompt text typed after repair planning must remain untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("ipc_dedupe_editor_redelivery_proof")
                && ops_log.contains("redeliver=false")
                && ops_log.contains("ipc_dedupe_editor_redelivery_skipped")
                && ops_log.contains("skip=stale_bad_state"),
            "stale tsift.md fixture should log proof and skip diagnostics:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_is_disabled_before_payload_delivery() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let live = "before\nlive prompt typed during compact\n";
        let target = "after\n";
        fs::write(&doc, live).unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let listener_calls = calls.clone();
        let listener_doc = doc.clone();
        let listener_root = tmp.path().to_path_buf();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                listener_calls.fetch_add(1, Ordering::SeqCst);
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                if let Some(full_content) = payload.get("fullContent").and_then(|v| v.as_str()) {
                    fs::write(&listener_doc, full_content).ok()?;
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        for _ in 0..100 {
            if crate::ipc_socket::is_listener_active(tmp.path()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            crate::ipc_socket::is_listener_active(tmp.path()),
            "fake socket listener did not start"
        );

        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "disabled full-content path should reject before socket delivery"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "socket listener must not receive stale full-content payloads"
        );
        assert_eq!(fs::read_to_string(&doc).unwrap(), live);
        assert!(snapshot::load(&doc).unwrap().is_none());
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(ops_log.contains("full_content_ipc_disabled"));

        let _ = fs::remove_file(crate::ipc_socket::socket_path(tmp.path()));
        drop(server);
    }

    #[test]
    fn socket_full_content_disabled_path_does_not_save_snapshot() {
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let agent_doc_dir = tmp.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("ack-content")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();

        let doc = tmp.path().join("test.md");
        let source = "before\n";
        let target = "after\n";
        fs::write(&doc, source).unwrap();

        let root = tmp.path().to_path_buf();
        let listener_root = root.clone();
        let ack_root = root.clone();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload.get("patch_id")?.as_str()?;
                let ack_dir = ack_root.join(".agent-doc/ack-content");
                fs::create_dir_all(&ack_dir).ok()?;
                fs::write(ack_dir.join(format!("{patch_id}.md")), "wrong\n").ok()?;
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });

        std::thread::sleep(Duration::from_millis(100));
        let result =
            try_ipc_full_content_operator_mutation_from_source(&doc, target, source).unwrap();

        assert!(
            !result,
            "socket full-content IPC must be disabled before payload delivery"
        );
        assert!(
            snapshot::load(&doc).unwrap().is_none(),
            "mismatched socket ack-content must not become the saved snapshot"
        );
        assert_eq!(
            fs::read_to_string(&doc).unwrap(),
            source,
            "socket mismatch rejection must leave disk content untouched"
        );
        let ops_log = fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("full_content_ipc_disabled"),
            "disabled full-content path should be logged:\n{ops_log}"
        );

        let _ = fs::remove_file(crate::ipc_socket::socket_path(&root));
        drop(server);
    }

    fn init_git_repo(root: &Path) {
        use std::process::Command;
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
        Command::new("git")
            .current_dir(root)
            .args(["config", "commit.gpgsign", "false"])
            .output()
            .unwrap();
    }

    fn git_commit_file(root: &Path, rel: &str, content: &str, msg: &str) {
        use std::process::Command;
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "--", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }

    fn head_count(root: &Path) -> usize {
        use std::process::Command;
        let out = Command::new("git")
            .current_dir(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    }

    #[test]
    fn recover_dedupe_only_drift_commits_when_file_matches_dedupe_of_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        // Simulate what `agent-doc dedupe` produced: file + snapshot both equal
        // the deduped form, HEAD still holds the duplicate.
        let deduped = crate::dedupe::dedupe_responses(duplicated);
        assert_ne!(
            deduped, duplicated,
            "test setup: duplicated content must actually dedupe"
        );
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let head_before = head_count(root);
        let recovered =
            recover_dedupe_only_drift(&doc).expect("dedupe-only drift recovery should succeed");
        assert!(
            recovered,
            "file matching dedupe(HEAD) must be recognized as a dedupe-only drift"
        );

        // Commit landed through the binary path.
        let head_after = head_count(root);
        assert_eq!(
            head_after,
            head_before + 1,
            "dedupe-only recovery must produce exactly one new commit"
        );
        let head_content = crate::git::show_head(&doc).unwrap().unwrap();
        assert_eq!(
            head_content.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snapshot_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "snapshot must hold the deduped response (boundary markers may differ from disk)"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_file_matches_head() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let clean = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", clean, "add clean");
        let doc = root.join("session.md");
        crate::snapshot::save(&doc, clean).unwrap();

        let recovered = recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "no drift between file and HEAD should not trigger dedupe-only recovery"
        );
    }

    #[test]
    fn recover_dedupe_only_drift_skips_when_drift_is_not_a_dedupe_outcome() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let original = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", original, "add original");
        let doc = root.join("session.md");

        // Working tree differs from HEAD by an arbitrary user edit, not by
        // dedupe. Recovery must refuse so we don't auto-commit unrelated drift.
        let user_edit = original.replace("Implemented.", "Implemented and tested.");
        fs::write(&doc, &user_edit).unwrap();
        crate::snapshot::save(&doc, &user_edit).unwrap();

        let recovered = recover_dedupe_only_drift(&doc).unwrap();
        assert!(
            !recovered,
            "arbitrary working-tree drift must not be auto-committed as a dedupe recovery"
        );
    }

    // Plan: tasks/agent-doc/plan-ipc-corruption-and-duplicate-during-typing.md
    // Phase 4 + Phase 5 regression coverage. Exercises the full
    // `agent-doc dedupe` → `agent-doc write --commit` (empty stdin) recovery
    // path through the strict-closeout entry point that the four `run` /
    // `stream` / `write` call sites use.
    #[test]
    fn recover_empty_response_for_strict_closeout_lands_dedupe_only_drift_through_binary_commit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");

        let deduped = crate::dedupe::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let strict = WriteFlags {
            strict_closeout: true,
            ..Default::default()
        };
        let head_before = head_count(root);
        let recovered = recover_empty_response_for_strict_closeout(&doc, &strict)
            .expect("strict-closeout empty-stdin path should recognize dedupe-only drift");
        assert!(
            recovered,
            "empty stdin + strict closeout + dedupe-only drift must commit through the binary path"
        );
        assert_eq!(
            head_count(root),
            head_before + 1,
            "exactly one new commit should land via the dedupe recovery wrapper"
        );

        let head_after = crate::git::show_head(&doc).unwrap().unwrap();
        assert_eq!(
            head_after.matches("### Re: topic — opus-4-7").count(),
            1,
            "committed HEAD must hold the deduped response"
        );
    }

    #[test]
    fn recover_empty_response_for_strict_closeout_refuses_when_not_strict_closeout() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        init_git_repo(root);
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();

        let duplicated = "\
---
agent_doc_session: test
agent_doc_format: template
---

<!-- agent:exchange patch=append -->
### Re: topic — opus-4-7

Implemented.
### Re: topic — opus-4-7

Implemented.
<!-- /agent:exchange -->
";
        git_commit_file(root, "session.md", duplicated, "add duplicate");
        let doc = root.join("session.md");
        let deduped = crate::dedupe::dedupe_responses(duplicated);
        fs::write(&doc, &deduped).unwrap();
        crate::snapshot::save(&doc, &deduped).unwrap();

        let lenient = WriteFlags::default();
        let head_before = head_count(root);
        let recovered = recover_empty_response_for_strict_closeout(&doc, &lenient).unwrap();
        assert!(
            !recovered,
            "non-strict empty-stdin path must not silently auto-commit dedupe drift"
        );
        assert_eq!(
            head_count(root),
            head_before,
            "non-strict path should not produce a commit"
        );
    }
}
