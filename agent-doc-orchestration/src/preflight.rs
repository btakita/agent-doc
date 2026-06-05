//! # Module: preflight
//!
//! ## Spec
//! - `run(file)`: executes the full pre-agent preparation sequence for a
//!   session document and emits a single JSON object to stdout.
//! - Bails immediately if the file does not exist.
//! - Step 0 — layout check: calls `check_layout()` to detect tmux structural
//!   problems (window index, session drift); issues are
//!   included in output but do not abort the run.
//! - Step 0-pre — interrupted-cycle guard: inspects persisted cycle state.
//!   For any open prior cycle, preflight auto-attempts `repair::run(file)` +
//!   `git::commit(file)` before diffing again. For an open `preflight_started`
//!   cycle with no recoverable response and unresolved prompt-bearing drift,
//!   preflight fails closed before the no-op commit path can mark an empty
//!   cycle committed. Non-prompt drift may still use the narrow no-op closeout
//!   that stages the snapshot only and leaves later live working-tree edits
//!   uncommitted; if that closeout still cannot prove the prior cycle is durable,
//!   preflight fails closed instead of diffing again.
//! - Step 1 — repair: calls `repair::run(file)` to detect and apply any
//!   orphaned pending agent responses from a previous interrupted cycle.
//! - Step 2 — commit: calls `git::commit(file)` to record the previous
//!   exchange cycle; failure is downgraded to a warning, not a hard error.
//! - Step 3 — claims: reads `.agent-doc/claims.log` line-by-line via
//!   `read_and_truncate_claims`, then truncates the log to empty; claims are
//!   returned to the caller in the JSON output.
//! - Step 3b — debounce: waits up to 3 seconds (polling every 100 ms) for
//!   both the file mtime to be at least 500 ms old and the cross-process
//!   typing indicator to be inactive before proceeding to the diff step.
//! - Step 3c — linked docs: calls `check_linked_docs(file)` to inspect
//!   `links` from frontmatter. For local file links, compares git commit
//!   times against the snapshot mtime. For URL links (`http://`/`https://`),
//!   fetches content via `ureq`, converts HTML to markdown via `htmd`
//!   (stripping script/style/nav/footer/noscript/svg), caches in
//!   `.agent-doc/links_cache/<sha256(url)>.txt`, and reports changes by
//!   comparing against the cached content.
//! - Step 4 — diff: calls `diff::compute(file)` to compare the current
//!   document against the last snapshot; `no_changes=true` when they match.
//! - Also emits a bounded `session_accretion` advisory when local exchange/log
//!   heuristics detect churn-heavy growth or restart-heavy reopen patterns.
//! - Serializes `PreflightOutput` as pretty JSON to stdout; all diagnostic
//!   messages go to stderr.
//! - `check_layout()`: inspects the current tmux session for structural issues:
//!   missing window index 0 (base-index compliance) and session drift. Stash
//!   windows may have non-idle panes (backgrounded sessions). Read-only; no mutations.
//!   Returns an empty vec when not inside tmux (silent).
//! - `read_and_truncate_claims(file)`: locates `.agent-doc/claims.log` relative
//!   to the project root, collects non-empty lines, truncates the file to empty,
//!   and returns the lines. Returns empty vec if the log is absent or unreadable.
//!
//! ## Agentic Contracts
//! - All output intended for the SKILL workflow is on stdout as valid JSON;
//!   callers must not parse stderr.
//! - `no_changes=true` in the output means the SKILL workflow should skip
//!   sending to the agent; `diff` will be `null` in this case.
//! - `layout_issues` reports structural tmux issues that remain after any
//!   immediate pre-diff layout repair has run.
//! - The claims log is consumed (truncated) exactly once per `preflight` call;
//!   a second call in the same cycle will return empty claims.
//! - Recovery (`recovered=true`) means the document was modified before the
//!   diff step; the `diff` and `document` fields reflect post-recovery state.
//! - Debounce waits for user typing to settle before computing the diff;
//!   if the 3-second timeout expires, `run` proceeds and logs a warning to
//!   stderr — it never blocks indefinitely.
//! - `check_layout` is always safe to call outside tmux; it returns `[]`.
//!
//! ## Evals
//! - `preflight_produces_valid_json`: document with matching snapshot →
//!   `run` returns `Ok(())` and emits parseable JSON with `no_changes=true`.
//! - `preflight_file_not_found`: missing path → `Err` containing "file not found".
//! - `preflight_detects_diff`: snapshot saved at original content, document
//!   updated with new content → `diff::compute` returns `Some(_)` (non-null diff).
//! - `preflight_claims_read_and_truncated`: claims.log with two entries →
//!   `read_and_truncate_claims` returns both lines and the log is empty afterwards.
//! - `preflight_no_claims_log_returns_empty`: no claims.log present →
//!   `read_and_truncate_claims` returns an empty vec without error.
//! - `preflight_output_serializes_correctly`: `PreflightOutput` with known
//!   values serializes to JSON with correct field names and types.
//! - `preflight_output_null_diff_when_no_changes`: `diff=None` + `no_changes=true`
//!   → JSON has `"diff": null` and `"no_changes": true`.
//! - `check_layout_returns_empty_outside_tmux`: `TMUX` env var unset →
//!   `check_layout()` returns empty vec without invoking tmux.
//! - `check_layout_detects_session_drift`: two alive registered panes in
//!   different sessions → `layout_issues` contains a "session drift" entry.
//! - `preflight_output_includes_layout_issues`: `PreflightOutput` with one
//!   layout issue → JSON `layout_issues` array has length 1 with correct text.
//! - `preflight_output_slash_commands_from_diff`: diff containing `+/clear` →
//!   `builtin_commands` array has one entry `"/clear"` (built-in, not in `slash_commands`).
//! - `is_url_detects_http`: `http://` and `https://` prefixes → true;
//!   relative paths and empty strings → false.
//! - `is_html_content_detects_html`: `text/html` and `application/xhtml` → true;
//!   `application/json` and `text/plain` → false.
//! - `html_to_markdown_converts_basic_html`: `<h1>` and `<strong>` → markdown
//!   heading and bold syntax.
//! - `html_to_markdown_strips_script_and_style`: script/style content removed
//!   from output, visible content preserved.
//! - `html_to_markdown_strips_nav_and_footer`: nav/footer content removed,
//!   main content preserved.
//! - `url_cache_path_is_deterministic`: same URL → same path; different URL →
//!   different path; extension is `.txt`.
//! - `links_cache_dir_creates_directory`: creates `.agent-doc/links_cache/` and
//!   returns `Some(path)` when `.agent-doc/` exists.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::component::{is_backlog_component, is_review_component, is_tracked_work_component};
use crate::{config, diff, frontmatter, git, repair, resync, sessions, snapshot, sync};

/// A change detected in a related document since the last cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedDocChange {
    /// Path to the related document (as declared in frontmatter).
    pub path: String,
    /// Human-readable summary of what changed.
    pub summary: String,
    /// Whether the related document exists on disk.
    pub exists: bool,
}

/// A non-blocking preflight warning intended for skill/user visibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightWarning {
    /// Stable machine-readable warning code.
    pub code: String,
    /// Human-readable warning message.
    pub message: String,
    /// Optional document-declared agent/harness value from frontmatter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_agent: Option<String>,
    /// Optional active harness detected from the current process environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_harness: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreflightOutput {
    /// Non-blocking warnings the skill should surface before responding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PreflightWarning>,
    /// Tmux layout issues found (empty = healthy).
    pub layout_issues: Vec<String>,
    /// Whether an orphaned pending response was recovered and applied.
    pub recovered: bool,
    /// Whether a git commit was made for the previous cycle.
    pub committed: bool,
    /// Lines from `.agent-doc/claims.log` (truncated after read).
    pub claims: Vec<String>,
    /// Unified diff text, or `null` if there are no changes.
    pub diff: Option<String>,
    /// True when the snapshot matches the document (no new user input).
    pub no_changes: bool,
    /// Changes detected in linked documents since last cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_changes: Vec<RelatedDocChange>,
    /// Path to the baseline file saved after commit (for `--baseline-file` in write).
    /// Saved after step 2 (commit + boundary reposition) so it matches the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_file: Option<String>,
    /// Classification of the diff for skill routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_type: Option<String>,
    /// Reason for the diff classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_type_reason: Option<String>,
    /// Annotated diff with content-source markers (`[agent]`, `[user+]`, `[user-]`, `[user~]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotated_diff: Option<String>,
    /// Skill slash commands found in user-added diff lines (non-built-ins, e.g. `["/agent-doc foo.md", "/caveman"]`).
    /// Guards applied: code fences, blockquotes, non-added lines.
    /// Built-in Claude Code commands are excluded here — see `builtin_commands`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slash_commands: Vec<String>,
    /// Claude Code built-in commands found in user-added diff lines (e.g. `["/compact", "/clear"]`).
    /// These affect Claude Code session state and cannot be invoked via the Skill tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builtin_commands: Vec<String>,
    /// Natural-language orchestration request detected from the user diff.
    ///
    /// When present, the skill should dispatch `agent-doc orchestrate <FILE>
    /// --mode <mode> --from-exchange` before attempting any manual response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_request: Option<crate::diff::OrchestrationRequest>,
    /// Prompt preset references requested from the changed exchange content.
    ///
    /// Values are preset names such as `#1` or `release-check`, in request order
    /// with duplicates removed. Preflight validates these against frontmatter
    /// `prompt_presets` and fails closed if any requested preset is missing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_presets_requested: Vec<String>,
    /// Explicit cross-document backlog targets resolved from prompt/preset text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub explicit_backlog_targets: Vec<String>,
    /// Resolved model tier the skill should use to gate this cycle.
    /// Computed from (in precedence order): inline `/model` command,
    /// `<!-- agent:model -->` component, `agent_doc_model_tier` frontmatter,
    /// diff heuristic. Single field for skill consumption — gating is a simple
    /// `>` comparison against the running model's tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_tier: Option<String>,
    /// Hard-gate tier from `<!-- agent:model -->` component or `agent_doc_model_tier`
    /// frontmatter. The skill should refuse to proceed if the running model's tier is
    /// below this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_tier: Option<String>,
    /// Advisory tier computed from diff structural signals (diff type, lines added,
    /// document path). The skill may surface this as a suggestion but should not gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_tier: Option<String>,
    /// Concrete model name from an inline `/model <x>` command in the diff
    /// (e.g., `"opus"`). Set when the user wrote `/model opus` (or `/model high`,
    /// resolved via the harness's tier map). The corresponding diff line is
    /// stripped from `diff` and `annotated_diff` so it does not propagate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_switch: Option<String>,
    /// Resolved tier for `model_switch` (e.g., `"high"` for `opus`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_switch_tier: Option<String>,
    /// Pending callback requests from `agent-doc cleanup` or other IPC callers.
    /// Non-empty when another process wrote a request and is waiting for this
    /// session to respond.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_callbacks: Vec<crate::callback::PendingCallback>,
    /// Structured owner-pane self-invocation contract
    /// (`#codex-owned-pane-prompt-miss-followups`). Non-null only when a Codex
    /// owner-pane re-invocation has unresolved exchange work (an unanswered
    /// prompt or a ready active auto-queue head) that must be answered in THIS
    /// owner turn rather than dispatched to a nested child. Codex guidance reads
    /// this to drive an in-pane response cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_pane_self_invocation: Option<crate::run::OwnedPaneSelfInvocation>,
    /// Environment variables from frontmatter `env` field (unexpanded).
    /// Values may contain shell expressions like `$(passage ...)` or `$VAR`.
    /// A `null` value means "unset this key" — the skill should emit
    /// `unset KEY` instead of `export KEY=...`.
    /// Order is preserved from the document for sequential evaluation.
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub env: indexmap::IndexMap<String, Option<String>>,
    /// True when the pending component's id order changed between snapshot and current.
    /// When set, the skill MUST NOT reorder pending this cycle — user intent wins.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending_reordered: bool,
    /// Count of pending items currently in `[/]` gated state.
    /// Surfaced so the skill can highlight blocked items in its response and
    /// decide whether to address gated work this cycle. Zero is omitted from
    /// JSON to keep the common case quiet.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub pending_gated_count: usize,
    /// Count of non-done items currently in `agent:review`.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub review_count: usize,
    /// Count of review items currently in `[/]` gated state.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub review_gated_count: usize,
    /// Canonical ordered list of user-authored changes that need prompt-aware handling.
    /// `prompt_target` items require a response, `content_edit` items are corrections
    /// the agent must incorporate, and `recovery_artifact` / `boundary_artifact`
    /// items indicate document-state cleanup rather than ordinary conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_bearing_changes: Vec<crate::diff::PromptBearingChange>,
    /// `prompt_bearing_changes` with managed-component state edits filtered
    /// out (queue activity toggle, queue items, backlog/review/done items,
    /// `queue_active:` frontmatter toggle). The Claude Code auto-loop guard
    /// uses this field instead of `prompt_bearing_changes` so routine
    /// session bookkeeping does not block the auto-loop. Plan: `#ccloopguard`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_intent_prompt_changes: Vec<crate::diff::PromptBearingChange>,
    /// Legacy compatibility field: inline user edits inside prior agent responses.
    /// Derived from `prompt_bearing_changes` by keeping only `prompt_target` and
    /// `content_edit` items.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_annotations: Vec<String>,
    /// Short model name for attribution in `### Re:` response headers.
    ///
    /// Resolved from the frontmatter `model` field only. Full model IDs are
    /// shortened to their human-readable suffix (for example
    /// `claude-sonnet-4-6` → `sonnet-4-6`), while already-short names such as
    /// `gpt-5` are preserved as-is. `None` when no model is known.
    /// The skill appends this to `### Re: topic` as `### Re: topic — <model>`
    /// and must never substitute the harness label (`codex`, `claude`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    /// Ordered prompt texts from the `agent:queue` component.
    /// Non-empty only when the queue is active and contains prompts.
    /// The first entry is the effective user prompt for this cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_prompts: Vec<String>,
    /// Whether the queue is currently active (consuming prompts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_active: Option<bool>,
    /// True when a time-gated start fence defers activation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queue_deferred: bool,
    /// Raw datetime string from `--- start at <time>` when deferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_start_at: Option<String>,
    /// How the queue was activated (auto, start_fence, exchange_request, persisted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_trigger: Option<crate::queue::QueueTrigger>,
    /// If non-null, the queue was halted this cycle. Value is the reason:
    /// `"stop_fence"` (hit a `--- stop` breakpoint) or `"item_modified"`
    /// (user edited the next-to-consume prompt between cycles).
    /// When halted, `queue_prompts` is empty and `queue_active` is `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_halted: Option<String>,
    /// Bounded session-growth / churn advisory derived from local exchange and
    /// per-document cycle/session logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_accretion: Option<crate::session_accretion::SessionAccretionReport>,
    /// Live finalize-pipeline state (`#fmrunid-wire`): `run_id` / `step` /
    /// `turn_id` / `queue_task_id` for the current cycle. Resume-detection
    /// observability so any invocation or editor plugin can see where a crashed
    /// or in-flight cycle left off. Derived from the authoritative cycle-state
    /// when one exists; otherwise read from the document `agent_doc_pipeline:`
    /// frontmatter block as a fallback hint (cycle-state wins on conflict). Null
    /// when neither is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<crate::frontmatter::AgentDocPipeline>,
}

fn is_zero_usize(n: &usize) -> bool {
    *n == 0
}

fn push_unique_strings(target: &mut Vec<String>, extras: Vec<String>) {
    for value in extras {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

fn push_unique_prompt_bearing_changes(
    target: &mut Vec<crate::diff::PromptBearingChange>,
    extras: Vec<crate::diff::PromptBearingChange>,
) {
    for value in extras {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

fn relocate_out_of_exchange_prompt_before_diff(
    file: &Path,
    doc_content: &str,
) -> Result<Option<String>> {
    let (frontmatter, _) = frontmatter::parse(doc_content)
        .with_context(|| format!("failed to parse document frontmatter {}", file.display()))?;
    if !frontmatter.resolve_mode().is_template() {
        return Ok(None);
    }

    let Some(mut repaired) = crate::template::repair_prompt_tail_outside_exchange(doc_content)?
    else {
        return Ok(None);
    };

    if let Some(snapshot_content) = snapshot::load(file)? {
        repaired = crate::write::normalize_user_prompts_in_exchange_safe(
            &repaired,
            &repaired,
            &snapshot_content,
            file,
        );
        repaired = crate::write::normalize_template_structure_or_fail(&repaired, file)?;
    }

    Ok((repaired != doc_content).then_some(repaired))
}

fn remove_duplicate_answered_exchange_prompt_tail_for_preflight(file: &Path) -> Result<bool> {
    let Some(cleaned_doc) = crate::template::remove_duplicate_answered_exchange_prompt_tail(
        &std::fs::read_to_string(file)?,
    ) else {
        return Ok(false);
    };

    crate::write::atomic_write_pub(file, &cleaned_doc)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "duplicate_answered_exchange_prompt_tail_removed file={} source=preflight",
            file.display()
        ),
    );
    eprintln!(
        "[preflight] removed duplicate answered prompt tail after exchange boundary in {}",
        file.display()
    );
    Ok(true)
}

fn remove_post_exchange_duplicate_prompt_comments_for_preflight(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<bool> {
    let current = std::fs::read_to_string(file)?;
    let snapshot_doc = crate::snapshot::load(file).ok().flatten();
    let head_doc = rc.head_content();
    let mut preserve_docs = Vec::new();
    preserve_docs.push(current.as_str());
    if let Some(head_doc) = head_doc.as_deref() {
        preserve_docs.push(head_doc.as_str());
    }
    if let Some(snapshot_doc) = snapshot_doc.as_deref() {
        preserve_docs.push(snapshot_doc);
    }
    let Some(cleaned_doc) =
        crate::template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
            &current,
            &preserve_docs,
        )
    else {
        return Ok(false);
    };

    crate::write::atomic_write_pub(file, &cleaned_doc)?;
    crate::ops_log::log_op(
        file,
        &format!(
            "post_exchange_duplicate_prompt_comment_removed file={} source=preflight",
            file.display()
        ),
    );
    eprintln!(
        "[preflight] scrubbed duplicate prompt text from comment after exchange in {}",
        file.display()
    );
    Ok(true)
}

fn tracked_work_component_fingerprint(
    content: &str,
) -> Result<(Option<String>, Option<String>, Vec<String>)> {
    let components =
        crate::component::parse(content).context("failed to parse document components")?;
    let component = components
        .iter()
        .find(|component| is_backlog_component(&component.name))
        .or_else(|| {
            components
                .iter()
                .find(|component| is_tracked_work_component(&component.name))
        });
    let Some(component) = component else {
        return Ok((None, None, Vec::new()));
    };

    let name = if is_backlog_component(&component.name) {
        "backlog".to_string()
    } else {
        component.name.clone()
    };
    let hash = crate::ops_log::content_hash(component.content(content));
    let (_, items, _) = crate::pending::parse_items(component.content(content));
    let item_ids = items
        .into_iter()
        .filter(|item| !item.is_done())
        .map(|item| item.id.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect();
    Ok((Some(name), Some(hash), item_ids))
}

fn explicit_backlog_target_requirements(
    source_file: &Path,
    source_frontmatter: &frontmatter::Frontmatter,
    targets: &[PathBuf],
) -> Result<Vec<crate::cycle_state::BacklogTargetRequirement>> {
    let mut requirements = Vec::new();
    for target in targets {
        let target_existing = if target.exists() {
            Some(
                std::fs::read_to_string(target)
                    .with_context(|| format!("failed to read {}", target.display()))?,
            )
        } else {
            None
        };
        let target_frontmatter = if let Some(content) = target_existing.as_ref() {
            Some(frontmatter::parse_for_file(content, target)?.0)
        } else {
            None
        };
        crate::security::enforce_cross_document_review(
            "preflight prompt contract",
            source_file,
            source_frontmatter,
            target,
            target_frontmatter.as_ref(),
        )?;
        let (component, baseline_hash, baseline_item_ids) = match target_existing.as_deref() {
            Some(content) => tracked_work_component_fingerprint(content)?,
            None => (None, None, Vec::new()),
        };
        requirements.push(crate::cycle_state::BacklogTargetRequirement {
            path: std::fs::canonicalize(target)
                .unwrap_or_else(|_| target.to_path_buf())
                .display()
                .to_string(),
            component,
            baseline_hash,
            baseline_item_ids,
        });
    }
    Ok(requirements)
}

/// Extract a human-readable short model name from a full model ID.
///
/// Strips well-known provider prefixes so the response header stays compact:
/// - `claude-sonnet-4-6` → `sonnet-4-6`
/// - `claude-opus-4` → `opus-4`
/// - `claude-haiku-4-5` → `haiku-4-5`
/// - Short names such as `gpt-5` / `gpt-5.4` are returned as-is.
fn short_model_name(model_id: &str) -> &str {
    // Strip leading "claude-" prefix if present
    if let Some(suffix) = model_id.strip_prefix("claude-") {
        return suffix;
    }
    model_id
}

/// Resolve the agent model short name for attribution in `### Re:` headers.
///
/// Source: frontmatter `model` field only. `ANTHROPIC_MODEL` env var is
/// deliberately ignored — it reflects the user's shell, not the model
/// Claude Code is actually running with (Claude Code does not export
/// `ANTHROPIC_MODEL` to child shells). The SKILL running inside Claude
/// Code always knows its own model identity and stamps attribution
/// directly when `agent_model` is null.
///
/// Full model IDs are shortened via `short_model_name`; already-short names
/// like `gpt-5` pass through unchanged.
fn resolve_agent_model(
    frontmatter_model: Option<&str>,
    harness: &str,
    model_config: &agent_doc_core::model_tier::ModelConfig,
) -> Option<String> {
    let m = frontmatter_model?;
    let canonical = agent_doc_core::model_tier::canonical_model_name(m, harness, model_config);
    // The Claude Code `opus` alias is deferred — agent-doc never pins a concrete
    // opus version, so it cannot attribute a specific id. Return None so the
    // running skill self-stamps its real model identity (always the current
    // opus, e.g. `opus-4-8`), keeping attribution from lagging a release.
    // Explicitly pinned ids (e.g. `claude-opus-4-8`) still stamp their short name.
    if harness == "claude-code" && canonical.trim() == "opus" {
        return None;
    }
    Some(short_model_name(&canonical).to_string())
}

fn canonical_harness_name(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    match normalized.as_str() {
        "" | "default" | "generic" => None,
        "claude" | "claude-code" | "claudecode" | "claude-code-cli" => {
            Some("claude-code".to_string())
        }
        "codex" | "codex-cli" | "openai-codex" => Some("codex".to_string()),
        "opencode" | "open-code" | "opencode-ai" => Some("opencode".to_string()),
        other => Some(other.to_string()),
    }
}

fn harness_mismatch_warning(
    document_agent: Option<&str>,
    active_harness: &str,
) -> Option<PreflightWarning> {
    let declared_raw = document_agent?.trim();
    if declared_raw.is_empty() {
        return None;
    }
    let declared = canonical_harness_name(declared_raw)?;
    let active = canonical_harness_name(active_harness)?;
    if declared == active {
        return None;
    }
    Some(PreflightWarning {
        code: "harness_mismatch".to_string(),
        message: format!(
            "Document declares agent: {} but active harness is {}; responses will use the active harness attribution and closeout path.",
            declared_raw, active
        ),
        document_agent: Some(declared_raw.to_string()),
        active_harness: Some(active),
    })
}

fn post_exchange_comment_prompt_preset_warning(
    file: &Path,
    content: &str,
    prompt_presets: &indexmap::IndexMap<String, String>,
) -> Option<PreflightWarning> {
    let mut referenced = Vec::new();
    for comment in post_exchange_ordinary_html_comments(content) {
        if !prompt_presets.is_empty() {
            push_unique_strings(
                &mut referenced,
                crate::prompt_contract::requested_prompt_presets(
                    std::slice::from_ref(&comment),
                    &[],
                    prompt_presets,
                ),
            );
        }
        push_unique_strings(
            &mut referenced,
            post_exchange_comment_directive_signals(&comment),
        );
    }
    if referenced.is_empty() {
        return None;
    }

    Some(PreflightWarning {
        code: "post_exchange_comment_prompt_preset".to_string(),
        message: format!(
            "Post-exchange HTML comment in {} references prompt preset/directive text ({}) that is preserved as a non-executable user note. Move it into `agent:exchange` or `agent:queue` if it should run.",
            file.display(),
            referenced.join(", ")
        ),
        document_agent: None,
        active_harness: None,
    })
}

/// `#preset-item-id-collision`: collect identities that resolve under more than
/// one active source — a frontmatter `prompt_presets` key, or an active (not
/// done) `agent:backlog` / `agent:review` / `agent:icebox` item id. When the
/// same `#id` exists in two sources, `do #id`, queue generation, and
/// "top backlog item: #id" are ambiguous between preset expansion and item
/// execution. Ids are normalized by stripping a leading `#`; `agent:done` /
/// archived ids are intentionally excluded (they are not active lookup targets).
/// Returns one `#id (sourceA + sourceB)` diagnostic per colliding identity.
/// Build the full active-identity registry for a document: every normalized
/// `#id` (leading `#` stripped) mapped to the active sources that define it — a
/// frontmatter `prompt_presets` key, or an active (not done) `agent:backlog` /
/// `agent:review` / `agent:icebox` item id. `agent:done` / archived ids are
/// intentionally excluded (not active lookup targets). Shared by collision
/// detection and mutation-time collision enforcement.
pub fn document_active_identities(
    content: &str,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut sources: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    if let Ok((fm, _)) = crate::frontmatter::parse(content) {
        for key in fm.prompt_presets.keys() {
            let norm = key.trim().trim_start_matches('#').to_string();
            if !norm.is_empty() {
                sources
                    .entry(norm)
                    .or_default()
                    .push("prompt_presets".to_string());
            }
        }
    }
    if let Ok(components) = crate::component::parse(content) {
        for component in &components {
            let label = if crate::component::is_backlog_component(&component.name) {
                "agent:backlog"
            } else if crate::component::is_review_component(&component.name) {
                "agent:review"
            } else if crate::component::is_icebox_component(&component.name) {
                "agent:icebox"
            } else {
                continue;
            };
            let (_, items, _) = crate::pending::parse_items(component.content(content));
            for item in items.iter().filter(|item| !item.is_done()) {
                if !item.id.is_empty() {
                    sources
                        .entry(item.id.clone())
                        .or_default()
                        .push(label.to_string());
                }
            }
        }
    }
    sources
}

/// `#preset-item-id-collision`: collect identities that resolve under more than
/// one active source. When the same `#id` exists in two sources, `do #id`, queue
/// generation, and "top backlog item: #id" are ambiguous between preset
/// expansion and item execution. Returns one `#id (sourceA + sourceB)`
/// diagnostic per colliding identity.
pub fn detect_identity_collisions(content: &str) -> Vec<String> {
    document_active_identities(content)
        .into_iter()
        .filter(|(_, srcs)| srcs.len() > 1)
        .map(|(id, srcs)| format!("#{id} ({})", srcs.join(" + ")))
        .collect()
}

/// `#preset-item-id-collision-enforce`: return the existing active sources that
/// an explicit new `candidate_id` would collide with, or `None` when the id is
/// free. Normalizes the candidate the same way as the registry (lowercase,
/// leading `#` stripped). Used to reject a colliding `--pending-add id=<id>` /
/// `[#id]` at mutation time before a new ambiguous identity is written.
pub fn identity_collision_for_new_id(content: &str, candidate_id: &str) -> Option<Vec<String>> {
    let norm = candidate_id
        .trim()
        .trim_start_matches('#')
        .to_ascii_lowercase();
    if norm.is_empty() {
        return None;
    }
    document_active_identities(content)
        .get(&norm)
        .filter(|srcs| !srcs.is_empty())
        .cloned()
}

fn preset_item_id_collision_warning(content: &str) -> Option<PreflightWarning> {
    let collisions = detect_identity_collisions(content);
    if collisions.is_empty() {
        return None;
    }
    Some(PreflightWarning {
        code: "preset_item_id_collision".to_string(),
        message: format!(
            "Ambiguous identities — the same #id resolves under multiple active sources: {}. Each #id must have one active meaning per document, so `do #id`, queue generation, and \"top backlog item\" are unambiguous. Rename the colliding prompt preset or tracked item before dispatch. (#preset-item-id-collision)",
            collisions.join("; ")
        ),
        document_agent: None,
        active_harness: None,
    })
}

/// Attributes that are only meaningful on the `agent:queue` component. Seeing
/// one of these on any other component is a misplaced-attribute mistake.
const QUEUE_ONLY_COMPONENT_ATTRS: &[&str] = &["auto", "preset", "start", "go", "stop"];

/// Component attribute keys recognized anywhere in the document (excluding the
/// queue-only set above). A key outside both sets is almost certainly a typo
/// (for example `auot` for `auto`) and must never be silently accepted.
const KNOWN_COMPONENT_ATTRS: &[&str] = &[
    "patch",
    "mode",
    "max_lines",
    "archive",
    "transfer-source",
    "timestamp",
    "broken",
];

/// Warn when a component carries a queue-only attribute on the wrong component
/// (e.g. `auto` on `agent:backlog`) or an unrecognized attribute key (e.g. the
/// `auot` typo). Root cause for `#backlog-auto-marker-misfire`: such attributes
/// were previously parsed and silently ignored, so a misplaced `auto` gave the
/// user no feedback. The attribute is still never mutated — the auto-loop only
/// triggers from `<!-- agent:queue auto -->` — this warning just makes the
/// silent misfire visible.
fn misplaced_component_attr_warning(file: &Path, content: &str) -> Option<PreflightWarning> {
    let components = crate::component::parse(content).ok()?;
    let mut issues: Vec<String> = Vec::new();
    for component in &components {
        for (key, value) in &component.attrs {
            if QUEUE_ONLY_COMPONENT_ATTRS.contains(&key.as_str()) {
                if component.name != "queue" {
                    issues.push(format!(
                        "`{key}` is a queue-only attribute but appears on `agent:{}` (did you mean `<!-- agent:queue {key} -->`?)",
                        component.name
                    ));
                }
            } else if key == "queue"
                && matches!(component.name.as_str(), "backlog" | "icebox" | "pending")
            {
                // `queue` is a recognized backlog/icebox sync attribute
                // (#backlog-queue-sync-attr). Surface only an unrecognized mode
                // value as a typo; the bare token and sync/append/prepend are valid.
                if crate::queue::BacklogQueueSyncMode::parse(value).is_none() {
                    issues.push(format!(
                        "`queue={value}` on `agent:{}` is not a recognized sync mode (use `sync`, `append`, or `prepend`)",
                        component.name
                    ));
                }
            } else if key == "priority"
                && matches!(
                    component.name.as_str(),
                    "backlog" | "icebox" | "pending" | "queue"
                )
            {
                // `priority` is a recognized backlog/icebox/queue ordering
                // attribute (#backlog-priority-attribute). It is a bare token;
                // per-item priority lives in `priority=<1..9>` item tokens.
            } else if !KNOWN_COMPONENT_ATTRS.contains(&key.as_str()) {
                issues.push(format!(
                    "`{key}` on `agent:{}` is not a recognized component attribute (possible typo)",
                    component.name
                ));
            }
        }
    }
    if issues.is_empty() {
        return None;
    }
    // attrs is a HashMap, so sort for deterministic message ordering.
    issues.sort();
    Some(PreflightWarning {
        code: "misplaced_component_attr".to_string(),
        message: format!(
            "{}: {}. The attribute is ignored (no mutation); the auto-loop triggers from `queue: start` (alias `go`) in frontmatter, the `start`/`go` marker control, or the legacy `<!-- agent:queue auto -->`.",
            file.display(),
            issues.join("; ")
        ),
        document_agent: None,
        active_harness: None,
    })
}

fn post_exchange_ordinary_html_comments(content: &str) -> Vec<String> {
    let Ok(components) = crate::component::parse(content) else {
        return Vec::new();
    };
    let Some(exchange_close_end) = components
        .iter()
        .filter(|component| component.name == "exchange")
        .map(|component| component.close_end)
        .max()
    else {
        return Vec::new();
    };

    let mut comments = Vec::new();
    let mut tail_start = exchange_close_end;
    let mut tail = &content[tail_start..];
    while let Some(open) = tail.find("<!--") {
        let after_open = &tail[open + "<!--".len()..];
        let Some(close) = after_open.find("-->") else {
            break;
        };
        let absolute_open = tail_start + open;
        let inner = after_open[..close].trim();
        if !crate::component::is_agent_marker(inner)
            && !components.iter().any(|component| {
                absolute_open >= component.open_start && absolute_open < component.close_end
            })
            && !comment_is_user_note(inner)
        {
            comments.push(inner.to_string());
        }
        let consumed = open + "<!--".len() + close + "-->".len();
        tail_start += consumed;
        tail = &content[tail_start..];
    }
    comments
}

fn comment_is_user_note(inner: &str) -> bool {
    let lines: Vec<&str> = inner.lines().collect();
    if lines.len() < 2 {
        return false;
    }
    let has_horizontal_rule = lines.iter().any(|l| l.trim() == "---");
    let has_prose = lines.iter().any(|l| {
        let t = l.trim();
        !t.is_empty()
            && t != "---"
            && !t.starts_with('#')
            && !t.starts_with('/')
            && !t.starts_with("dispatch ")
            && !t.starts_with("preset ")
    });
    has_horizontal_rule && has_prose
}

fn post_exchange_comment_directive_signals(comment: &str) -> Vec<String> {
    let mut signals = Vec::new();
    for line in comment.lines() {
        let trimmed = line.trim().trim_start_matches('❯').trim();
        if let Some(rest) = trimmed.strip_prefix("dispatch ") {
            push_unique_strings(&mut signals, vec![format!("dispatch {}", first_word(rest))]);
        } else if let Some(rest) = trimmed.strip_prefix("preset ") {
            push_unique_strings(&mut signals, vec![format!("preset {}", first_word(rest))]);
        } else if looks_like_slash_command(trimmed) {
            push_unique_strings(&mut signals, vec![first_word(trimmed).to_string()]);
        }
    }
    signals
}

fn first_word(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or(text)
}

fn looks_like_slash_command(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('/') else {
        return false;
    };
    rest.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase())
}

/// Trigger an automatic `resync --fix` when session-drift has been detected
/// on two consecutive preflights.
///
/// The drift counter lives at `.agent-doc/state/drift.count`. Each call either
/// increments it (drift present) or deletes it (drift absent). When the counter
/// reaches >= 2 we invoke `resync::run(true, None, None)` and reset it to 0 so we do
/// not loop on every cycle.
fn maybe_auto_resync_on_drift(file: &std::path::Path, layout_issues: &[String]) {
    let has_drift = layout_issues
        .iter()
        .any(|i| i.starts_with("session drift:"));

    let Ok(canonical) = file.canonicalize() else {
        return;
    };
    let Some(project_root) = snapshot::find_project_root(&canonical) else {
        return;
    };
    let state_dir = project_root.join(".agent-doc/state");
    let counter_path = state_dir.join("drift.count");

    if !has_drift {
        // Drift cleared — reset the counter.
        if counter_path.exists() {
            let _ = std::fs::remove_file(&counter_path);
        }
        return;
    }

    let current: u32 = std::fs::read_to_string(&counter_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let next = current + 1;

    if let Err(e) = std::fs::create_dir_all(&state_dir) {
        eprintln!("[preflight] drift state dir create failed: {}", e);
        return;
    }
    if let Err(e) = std::fs::write(&counter_path, next.to_string()) {
        eprintln!("[preflight] drift counter write failed: {}", e);
    }

    if next >= 2 {
        eprintln!(
            "[preflight] session drift detected {}x consecutively — running `resync --fix`",
            next
        );
        crate::ops_log::log_op(file, &format!("auto_resync_on_drift consecutive={}", next));
        if let Err(e) = resync::run(true, None, None) {
            eprintln!("[preflight] auto-resync failed: {}", e);
        } else {
            // Reset after successful fix — next cycle re-evaluates.
            let _ = std::fs::remove_file(&counter_path);
        }
    } else {
        eprintln!(
            "[preflight] session drift detected (count={}) — will auto-resync on next detection",
            next
        );
    }
}

fn clear_base_index_repair_counter(file: &std::path::Path) {
    let Ok(canonical) = file.canonicalize() else {
        return;
    };
    let Some(project_root) = snapshot::find_project_root(&canonical) else {
        return;
    };
    let counter_path = project_root.join(".agent-doc/state/base-index-repair.count");
    if counter_path.exists() {
        let _ = std::fs::remove_file(counter_path);
    }
}

fn current_tmux_session_name() -> Option<String> {
    sessions::Tmux::default_server().current_session()
}

fn maybe_auto_repair_base_index(file: &std::path::Path, layout_issues: &[String]) -> bool {
    let has_base_index_issue = layout_issues
        .iter()
        .any(|i| i.contains("window index 0 missing"));

    if !has_base_index_issue {
        clear_base_index_repair_counter(file);
        return false;
    }

    // Older builds used a consecutive-detection counter before repairing.
    // Once the issue is visible in preflight, leaving it for the next turn makes
    // the active response cycle nondeterministic, so clean the stale marker and
    // repair immediately.
    clear_base_index_repair_counter(file);

    if !sessions::in_tmux() {
        eprintln!(
            "[preflight] window index 0 missing but no tmux context is available; run `agent-doc session doctor {} --repair` from the target tmux session",
            file.display()
        );
        return false;
    }

    let Some(name) = current_tmux_session_name() else {
        eprintln!(
            "[preflight] window index 0 missing but tmux session lookup failed; run `agent-doc session doctor {} --repair`",
            file.display()
        );
        return false;
    };

    eprintln!("[preflight] window index 0 missing — running repair_layout immediately");
    crate::ops_log::log_op(
        file,
        &format!("auto_repair_base_index immediate session={}", name),
    );
    let tmux = sessions::Tmux::default_server();
    if let Err(e) = sync::repair_layout(&tmux, &name, "agent-doc") {
        eprintln!(
            "[preflight] auto repair_layout failed: {}; run `agent-doc session doctor {} --repair`",
            e,
            file.display()
        );
        return false;
    }

    true
}

/// Check tmux layout health for the current session.
///
/// Returns a list of human-readable issue strings. An empty vec means the
/// layout is healthy. This is read-only — no mutations are performed.
///
/// If not running inside tmux, returns an empty vec silently.
pub fn check_layout() -> Vec<String> {
    if !sessions::in_tmux() {
        return vec![];
    }

    let mut issues = Vec::new();

    // Get the owning pane's current session name. Bare display-message can
    // follow another attached client and report the wrong session.
    let Some(session_name) = sessions::Tmux::default_server().current_session() else {
        return issues;
    };

    if session_name.is_empty() {
        return issues;
    }

    // List windows: index, name, pane count.
    let window_output = match Command::new("tmux")
        .args([
            "list-windows",
            "-t",
            &format!("{}:", session_name),
            "-F",
            "#{window_index}\t#{window_name}\t#{window_panes}",
        ])
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => return issues,
    };

    let windows: Vec<u32> = window_output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let index: u32 = parts.next()?.parse().ok()?;
            Some(index)
        })
        .collect();

    // Check 1: Window 0 should exist (base-index compliance).
    if !windows.contains(&0) {
        issues.push(format!(
            "window index 0 missing in session '{}' (base-index compliance)",
            session_name,
        ));
    }

    // Check 3: Session-drift — registered panes spanning multiple tmux sessions.
    // Check 4: Duplicate claims — multiple sessions claiming the same document file.
    let registry_path = sessions::registry_path();
    let registry: Option<tmux_router::Registry> = std::fs::read_to_string(&registry_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    if let Some(registry) = registry {
        let mut pane_sessions: HashSet<String> = HashSet::new();
        for entry in registry.values() {
            let pane = &entry.pane;
            // Only check alive panes.
            let pane_sess = Command::new("tmux")
                .args(["display-message", "-t", pane, "-p", "#{session_name}"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            if !pane_sess.is_empty() {
                pane_sessions.insert(pane_sess);
            }
        }
        if pane_sessions.len() > 1 {
            let mut sessions_vec: Vec<&str> = pane_sessions.iter().map(|s| s.as_str()).collect();
            sessions_vec.sort();
            issues.push(format!(
                "session drift: registered panes span {} tmux sessions: {}",
                pane_sessions.len(),
                sessions_vec.join(", "),
            ));
        }

        // Check 4: duplicate file claims — two sessions pointing to the same document.
        issues.extend(detect_duplicate_claims(&registry));
    }

    issues
}

/// Detect duplicate file claims in a registry snapshot.
///
/// Returns one issue string per file that has two or more sessions claiming it.
/// Entries with an empty `file` field are skipped (legacy entries).
fn detect_duplicate_claims(registry: &tmux_router::Registry) -> Vec<String> {
    let mut file_sessions: HashMap<String, Vec<String>> = HashMap::new();
    for (registry_key, entry) in registry {
        let file_identity = if std::path::Path::new(registry_key).is_absolute() {
            registry_key.clone()
        } else {
            entry.file.clone()
        };
        if file_identity.is_empty() {
            continue;
        }
        file_sessions
            .entry(file_identity)
            .or_default()
            .push(if entry.session_id.is_empty() {
                registry_key.clone()
            } else {
                entry.session_id.clone()
            });
    }
    let mut issues = Vec::new();
    for (file, session_ids) in &file_sessions {
        if session_ids.len() > 1 {
            let mut sorted = session_ids.clone();
            sorted.sort();
            issues.push(format!(
                "duplicate claims: {} sessions claim '{}': {}",
                session_ids.len(),
                file,
                sorted.join(", "),
            ));
        }
    }
    issues
}

/// Run the preflight sequence for a session document.
///
/// Steps (in order):
/// 0. Check tmux layout health (`check_layout`)
/// 1. Repair orphaned pending response (`repair::run`)
/// 2. Commit previous cycle (`git::commit`)
/// 3. Check claims log (read + truncate `.agent-doc/claims.log`)
/// 4. Compute diff (`diff::compute`)
/// 5. Read document HEAD from disk
///
/// Outputs JSON to stdout. Progress/diagnostic messages go to stderr.
fn enforce_cycle_completion(file: &Path) -> Result<(bool, bool)> {
    let state = crate::cycle_state::load(file)?;
    let missing_commit_event = if state.as_ref().map(|state| state.is_open()).unwrap_or(false) {
        None
    } else {
        crate::session_check::detect_write_completed_commit_missing(file)?
    };
    if let Some(event) = missing_commit_event.as_deref() {
        eprintln!(
            "[preflight] WARNING: previous cycle wrote the response but no commit followed ({}) — attempting commit-boundary recovery before diff",
            event
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "write_completed_commit_missing file={} last_event={}",
                file.display(),
                event
            ),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "resume_commit_attempt file={} last_event={}",
                file.display(),
                event
            ),
        );

        let recovered = match repair::run(file) {
            Ok(outcome) => outcome.repaired(),
            Err(e) => {
                let message = e.to_string();
                if message.contains(repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
                    || message.contains(repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR)
                    || message.contains(repair::EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR)
                {
                    anyhow::bail!("{}", e);
                }
                eprintln!("[preflight] interrupted-cycle repair warning: {}", e);
                false
            }
        };

        let committed = match git::commit(file) {
            Ok(did_commit) => did_commit,
            Err(e) => {
                eprintln!("[preflight] interrupted-cycle commit warning: {}", e);
                false
            }
        };

        match crate::session_check::inspect(file)? {
            crate::session_check::SessionCheckStatus::Ok(_) => {
                crate::ops_log::log_op(
                    file,
                    &format!("resume_commit_success file={}", file.display()),
                );
                return Ok((recovered, committed));
            }
            crate::session_check::SessionCheckStatus::Interrupted(reason) => {
                let reason = reason.replace('\n', " ");
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "resume_commit_blocked_drift file={} reason={}",
                        file.display(),
                        reason
                    ),
                );
                anyhow::bail!("{}", reason);
            }
        }
    }

    let Some(state) = state else {
        return Ok((false, false));
    };
    if !state.is_open() {
        return Ok((false, false));
    }

    let ipc_hint = crate::session_check::latest_ipc_proof_diagnostic_hint(file)?
        .map(|hint| format!("; {hint}"))
        .unwrap_or_default();
    eprintln!(
        "[preflight] WARNING: previous cycle `{}` is still `{}` ({}){} — attempting recovery before diff",
        state.cycle_id,
        match state.phase {
            crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
            crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
            crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
            crate::cycle_state::CyclePhase::Committed => "committed",
            crate::cycle_state::CyclePhase::Abandoned => "abandoned",
        },
        state.last_event,
        ipc_hint
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "interrupted_cycle_detected file={} cycle_id={} phase={:?} event={}",
            file.display(),
            state.cycle_id,
            state.phase,
            state.last_event
        ),
    );
    if matches!(state.phase, crate::cycle_state::CyclePhase::WriteApplied) {
        crate::ops_log::log_op(
            file,
            &format!(
                "resume_commit_attempt file={} cycle_id={}",
                file.display(),
                state.cycle_id
            ),
        );
    }

    let recovered = match repair::run(file) {
        Ok(outcome) => outcome.repaired(),
        Err(e) => {
            let message = e.to_string();
            if message.contains(repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
                || message.contains(repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR)
                || message.contains(repair::EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR)
            {
                anyhow::bail!("{}", e);
            }
            eprintln!("[preflight] interrupted-cycle repair warning: {}", e);
            false
        }
    };

    let committed = match git::commit(file) {
        Ok(did_commit) => did_commit,
        Err(e) => {
            eprintln!("[preflight] interrupted-cycle commit warning: {}", e);
            false
        }
    };

    if let Some(after) = crate::cycle_state::load(file)?
        && after.is_open()
    {
        let marker_note = if matches!(
            after.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        ) {
            crate::session_check::detect_bypassed_response_write(file)?
                .map(|marker| format!("; found likely direct response patchback: {}", marker))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let ipc_hint = crate::session_check::latest_ipc_proof_diagnostic_hint(file)?
            .map(|hint| format!("; {hint}"))
            .unwrap_or_default();
        anyhow::bail!(
            "previous cycle `{}` is still `{}` after recovery/commit ({}){}{}",
            after.cycle_id,
            match after.phase {
                crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
                crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
                crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
                crate::cycle_state::CyclePhase::Committed => "committed",
                crate::cycle_state::CyclePhase::Abandoned => "abandoned",
            },
            after.last_event,
            marker_note,
            ipc_hint
        );
    }

    if matches!(state.phase, crate::cycle_state::CyclePhase::WriteApplied) {
        crate::ops_log::log_op(
            file,
            &format!("resume_commit_success file={}", file.display()),
        );
    }

    Ok((recovered, committed))
}

fn enforce_no_uncommitted_closeout_drift(file: &Path, rc: &crate::graph::RunContext) -> Result<()> {
    // Route can enqueue a dispatch behind a busy authoritative actor by writing
    // `agent:queue auto` plus the saved snapshot, then return before a normal
    // response closeout exists. If the user keeps editing that prompt before
    // the next preflight, the working tree no longer matches the queued
    // snapshot and the generic snapshot-vs-HEAD guard used to require a manual
    // `write --commit`. Commit the route-owned snapshot first; the later live
    // edit stays unstaged and becomes the next prompt diff.
    if recover_route_queue_snapshot_commit_boundary(file, rc)? {
        return Ok(());
    }

    // Accepted JetBrains File Cache Conflict dialogs can replay a stale editor
    // patch after the response already reached HEAD. If the only working-tree
    // drift is an adjacent duplicate response and dedupe(current) is HEAD, drop
    // the replay before the generic direct-patchback guard fires.
    if let Some(replay) =
        crate::session_check::detect_jb_cache_conflict_accept_duplicate_replay_with_context(
            file, rc,
        )?
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "jb_cache_conflict_accept_duplicate_replay_repaired file={} heading={}",
                file.display(),
                replay.heading.replace('\n', " ")
            ),
        );
        eprintln!(
            "[preflight] jb_cache_conflict_accept: removing duplicate response replay at `{}` for {}",
            replay.heading,
            file.display()
        );
        crate::write::atomic_write_pub(file, &replay.deduped_content)?;
        crate::snapshot::save(file, &replay.deduped_content)?;
        return Ok(());
    }

    // Late-IPC reposition / stale-patch replay re-inserted the committed
    // response into the working tree after it already reached HEAD (possibly
    // wrapped in redundant boundary markers and non-adjacent, which the
    // consecutive-only dedupe replay path above misses). Restore the committed
    // HEAD over the working tree + snapshot before the generic direct-patchback
    // guard fires. See tasks/agent-doc/plan-duplicate-response-after-commit.md.
    if let Some(overapplication) =
        crate::session_check::detect_late_ipc_response_overapplication_with_context(file, rc)?
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "late_ipc_response_overapplication_repaired file={}",
                file.display()
            ),
        );
        eprintln!(
            "[preflight] late_ipc_overapplication: restoring committed HEAD over re-added response for {}",
            file.display()
        );
        crate::write::atomic_write_pub(file, &overapplication.remediated_content)?;
        crate::snapshot::save(file, &overapplication.remediated_content)?;
        return Ok(());
    }

    // Phase 3 (#jbccc3): JB File Cache Conflict cancel auto-recovery.
    //
    // When the binary-owned write path applied the response (snapshot has it,
    // working tree mirrors snapshot) but the commit boundary never landed
    // (HEAD lacks it), run `git::commit` instead of bailing. This converts a
    // wedged cycle that previously required manual `agent-doc write --commit`
    // into a transparent recovery on the next preflight, while still failing
    // closed if the commit attempt itself errors out.
    //
    // `session_check::detect_uncommitted_closeout_drift` already returns
    // `Ok(None)` for the same pattern, but the drift will recur on the next
    // call until something actually commits — that "something" lives here.
    if crate::session_check::detect_jb_cache_conflict_cancel_recoverable_with_context(file, rc)? {
        crate::ops_log::log_op(
            file,
            &format!(
                "jb_cache_conflict_cancel_auto_recovery_attempt file={}",
                file.display()
            ),
        );
        eprintln!(
            "[preflight] jb_cache_conflict_cancel: response written but not committed for {} — running auto-commit",
            file.display()
        );
        match crate::git::commit(file) {
            Ok(_) => {
                rc.invalidate_head_content();
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "jb_cache_conflict_cancel_auto_recovery_succeeded file={}",
                        file.display()
                    ),
                );
                return Ok(());
            }
            Err(e) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "jb_cache_conflict_cancel_auto_recovery_failed file={} error={}",
                        file.display(),
                        e.to_string().replace('\n', " ")
                    ),
                );
                eprintln!(
                    "[preflight] jb_cache_conflict_cancel auto-commit failed for {}: {}",
                    file.display(),
                    e
                );
                // Fall through to the standard drift-bail path below so the
                // operator still sees the actionable recovery hint.
            }
        }
    }
    if let Some(message) =
        crate::session_check::detect_uncommitted_closeout_drift_with_context(file, rc)?
    {
        crate::ops_log::log_op(
            file,
            &format!(
                "preflight_blocked_uncommitted_closeout_drift file={} reason={}",
                file.display(),
                message.replace('\n', " ")
            ),
        );
        anyhow::bail!("{}", message);
    }
    Ok(())
}

fn recover_route_queue_snapshot_commit_boundary(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<bool> {
    if !detect_route_queue_snapshot_commit_boundary_recoverable(file, rc)? {
        return Ok(false);
    }
    crate::ops_log::log_op(
        file,
        &format!(
            "route_queue_snapshot_auto_recovery_attempt file={}",
            file.display()
        ),
    );
    eprintln!(
        "[preflight] route_queue_snapshot: queued dispatch snapshot is not committed for {}; running auto-commit",
        file.display()
    );
    match crate::git::commit(file) {
        Ok(_) => {
            rc.invalidate_head_content();
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_queue_snapshot_auto_recovery_succeeded file={}",
                    file.display()
                ),
            );
            Ok(true)
        }
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_queue_snapshot_auto_recovery_failed file={} error={}",
                    file.display(),
                    e.to_string().replace('\n', " ")
                ),
            );
            eprintln!(
                "[preflight] route_queue_snapshot auto-commit failed for {}: {}",
                file.display(),
                e
            );
            Ok(false)
        }
    }
}

fn detect_route_queue_snapshot_commit_boundary_recoverable(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<bool> {
    let Some(state) = crate::cycle_state::load(file)? else {
        return Ok(false);
    };
    if state.is_open() {
        return Ok(false);
    }
    if !matches!(
        rc.snapshot_commit_status(),
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ) {
        return Ok(false);
    }

    let Some(snapshot) = rc.snapshot_content() else {
        return Ok(false);
    };
    let Some(head) = rc.head_content() else {
        return Ok(false);
    };
    if crate::session_check::detect_bypassed_response_write_between(&head, &snapshot).is_some() {
        return Ok(false);
    }

    let snapshot_prompts = route_queue_prompt_texts(&snapshot)?;
    let head_prompts = route_queue_prompt_texts(&head)?;
    // Recover only genuine active-auto-queue commit-boundary churn: either the
    // snapshot still carries the queued dispatch (enqueue case) or HEAD carried
    // an active auto-queue that the snapshot has since drained to inactive
    // residue via queue maintenance (#drained-done-queue-clear). The drained
    // case reduces to an empty stripped diff below (queue body + `queue_active`
    // are both stripped before comparison), so it auto-commits only when no
    // non-queue user change exists. Bail on any other snapshot/HEAD drift.
    if snapshot_prompts.is_empty() && head_prompts.is_empty() {
        return Ok(false);
    }

    let head_norm = strip_route_queue_state_for_boundary_compare(&head);
    let snapshot_norm = strip_route_queue_state_for_boundary_compare(&snapshot);
    let Some(diff_text) = crate::diff::unified_diff_from_contents(&head_norm, &snapshot_norm)
    else {
        return Ok(true);
    };
    let changes = crate::diff::classify_prompt_bearing_changes(&diff_text)
        .into_iter()
        .filter(|change| {
            !matches!(
                change.kind,
                crate::diff::PromptBearingChangeKind::RecoveryArtifact
                    | crate::diff::PromptBearingChangeKind::BoundaryArtifact
            )
        })
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return Ok(true);
    }

    Ok(changes.iter().all(|change| {
        change.kind == crate::diff::PromptBearingChangeKind::PromptTarget
            && snapshot_prompts
                .iter()
                .any(|prompt| prompt == &normalize_route_queue_prompt_text(&change.text))
    }))
}

fn route_queue_prompt_texts(content: &str) -> Result<Vec<String>> {
    let (fm, body) = crate::frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(Vec::new());
    }
    let components = crate::component::parse(body)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(Vec::new());
    };
    if !crate::queue::has_auto_attr(&queue_component.attrs) {
        return Ok(Vec::new());
    }
    let entries = crate::queue::parse(queue_component.content(body))?;
    Ok(crate::queue::prompts(&entries)
        .into_iter()
        .map(|prompt| normalize_route_queue_prompt_text(&prompt.text))
        .filter(|text| !text.is_empty())
        .collect())
}

fn strip_route_queue_state_for_boundary_compare(content: &str) -> String {
    let mut result = content
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            // Both the canonical `queue:` control and the deprecated
            // `queue_active:` line are transient queue-maintenance state
            // (#queue-state-unify); normalize them away together.
            !t.starts_with("queue_active:") && !t.starts_with("queue:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    if let Ok(components) = crate::component::parse(&result) {
        for component in components.iter().rev() {
            if component.name == "queue" {
                result.replace_range(component.open_start..component.close_end, "");
            }
        }
    }
    crate::git::normalize_transient_agent_doc_markers(&result)
}

fn normalize_route_queue_prompt_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("<!-- agent:boundary:"))
        .map(|line| {
            line.strip_prefix('❯')
                .or_else(|| line.strip_prefix('>'))
                .map(str::trim)
                .unwrap_or(line)
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn preflight_debounce_ms(file: &Path) -> u64 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|content| {
            frontmatter::parse(&content)
                .ok()
                .and_then(|(fm, _)| fm.debounce_ms)
        })
        .unwrap_or(2000)
}

fn preflight_debounce_max_wait(debounce_ms: u64) -> std::time::Duration {
    std::time::Duration::from_secs(if debounce_ms > 3000 {
        (debounce_ms / 1000) + 1
    } else {
        3
    })
}

fn wait_for_typing_idle_before_mutation(file: &Path, debounce_ms: u64) -> Result<()> {
    let max_wait = preflight_debounce_max_wait(debounce_ms);
    let poll = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    let file_str = file.to_string_lossy();

    loop {
        let typing_active = crate::debounce::is_typing_via_file(&file_str, debounce_ms);
        if !typing_active {
            return Ok(());
        }
        if start.elapsed() >= max_wait {
            crate::ops_log::log_op(
                file,
                &format!(
                    "preflight_visible_mutation_deferred_active_typing file={} debounce_ms={} timeout_ms={}",
                    file.display(),
                    debounce_ms,
                    max_wait.as_millis()
                ),
            );
            anyhow::bail!(
                "preflight deferred for {}: editor typing did not settle within {}ms; retry after typing stops",
                file.display(),
                max_wait.as_millis()
            );
        }
        std::thread::sleep(poll);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SweepOwner {
    pane: String,
    source: String,
}

enum ActorSweepOwner {
    Active(SweepOwner),
    Inactive,
    Unknown,
}

fn actor_sweep_owner(audit_file: &Path, root: &Path, doc_path: &Path) -> ActorSweepOwner {
    match crate::project_controller::authoritative_actor_binding(root, doc_path) {
        Ok(Some(record)) if record.state != crate::session_actor::ActorState::Closed => {
            ActorSweepOwner::Active(SweepOwner {
                pane: record.pane_id,
                source: format!("actor:{}", record.state.as_str()),
            })
        }
        Ok(Some(_)) => ActorSweepOwner::Inactive,
        Ok(None) => ActorSweepOwner::Unknown,
        Err(err) => {
            eprintln!(
                "[preflight] sweep: owner warning for {}: {}",
                doc_path.display(),
                err
            );
            crate::ops_log::log_op(
                audit_file,
                &format!(
                    "foreign_owned_sweep_owner_warning file={} error={}",
                    doc_path.display(),
                    err.to_string().replace('\n', " ")
                ),
            );
            ActorSweepOwner::Unknown
        }
    }
}

fn registry_sweep_owner(
    root: &Path,
    registry: &sessions::SessionRegistry,
    doc_path: &Path,
) -> Option<SweepOwner> {
    let key = sessions::canonical_registry_key_in(root, &doc_path.to_string_lossy());
    registry.get(&key).and_then(|entry| {
        (!entry.pane.trim().is_empty()).then(|| SweepOwner {
            pane: entry.pane.clone(),
            source: "sessions.json".to_string(),
        })
    })
}

fn sweep_owner_for_doc(
    audit_file: &Path,
    root: &Path,
    registry: &sessions::SessionRegistry,
    doc_path: &Path,
) -> Option<SweepOwner> {
    match actor_sweep_owner(audit_file, root, doc_path) {
        ActorSweepOwner::Active(owner) => Some(owner),
        ActorSweepOwner::Inactive => None,
        ActorSweepOwner::Unknown => registry_sweep_owner(root, registry, doc_path),
    }
}

fn current_sweep_owner(
    audit_file: &Path,
    root: &Path,
    registry: &sessions::SessionRegistry,
    current_doc: &Path,
) -> Option<SweepOwner> {
    sweep_owner_for_doc(audit_file, root, registry, current_doc)
}

fn should_skip_foreign_owned_sweep(
    audit_file: &Path,
    doc_path: &Path,
    current_owner: Option<&SweepOwner>,
    sibling_owner: Option<&SweepOwner>,
) -> bool {
    let (Some(current_owner), Some(sibling_owner)) = (current_owner, sibling_owner) else {
        return false;
    };
    if current_owner.pane == sibling_owner.pane {
        return false;
    }

    eprintln!(
        "[preflight] sweep: skipping {} (foreign-owned by pane {}; current owner pane {})",
        doc_path.display(),
        sibling_owner.pane,
        current_owner.pane
    );
    crate::ops_log::log_op(
        audit_file,
        &format!(
            "foreign_owned_sweep_skip file={} owner_pane={} owner_source={} current_pane={} current_source={}",
            doc_path.display(),
            sibling_owner.pane,
            sibling_owner.source,
            current_owner.pane,
            current_owner.source
        ),
    );
    true
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

/// Run preflight with default (dispatch/response-bound) options.
pub fn run(file: &Path) -> Result<()> {
    run_with_options(file, PreflightOptions::default())
}

pub fn run_with_options(file: &Path, options: PreflightOptions) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
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
        diff_result = Some(diff::synthetic_added_lines_diff(head_prompt, "queue"));
        classification = diff_result.as_ref().map(|d| diff::classify_diff(d));
        diff_from_queue_head_only = true;
    }

    let no_changes = diff_result.is_none();
    if !no_changes {
        if options.probe {
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
    let prompt_diff_result = diff_result.as_ref().map(|d| {
        if queue_active_for_prompt_extraction {
            d.clone()
        } else {
            diff::suppress_inactive_queue_additions(d, &diff_result_with_current.current)
        }
    });
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

    // Step 4d: Extract slash commands from user-added diff lines (classified into skill vs built-in).
    let mut parsed_commands = prompt_diff_result
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
        user_intent_prompt_changes: if diff_from_queue_head_only {
            // Synthetic auto-queue continuation only — no user intent this cycle.
            Vec::new()
        } else {
            prompt_bearing_changes
                .iter()
                .filter(|change| !crate::diff::change_is_managed_state_only(change))
                .cloned()
                .collect()
        },
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

/// Resolve the live finalize-pipeline view surfaced in preflight output
/// (`#fmrunid-wire`). Cycle-state is authoritative; the document
/// `agent_doc_pipeline:` frontmatter block is only a fallback hint when no live
/// cycle-state exists (e.g. a crash that wiped `.agent-doc/state` but left the
/// document mirror behind). Returns `None` when neither is present.
fn resolve_pipeline_state(file: &Path) -> Result<Option<crate::frontmatter::AgentDocPipeline>> {
    if let Some(state) = crate::cycle_state::load(file)? {
        return Ok(Some(state.to_pipeline()));
    }
    let current = std::fs::read_to_string(file).unwrap_or_default();
    Ok(match crate::frontmatter::parse(&current) {
        Ok((fm, _)) if !fm.pipeline.is_empty() => Some(fm.pipeline),
        _ => None,
    })
}

#[derive(Debug, Clone, Default)]
pub struct PendingMaintenanceReport {
    pub reordered: bool,
    pub pending_gated_count: usize,
    pub review_count: usize,
    pub review_gated_count: usize,
    pub legacy_gated_in_backlog_count: usize,
}

/// Run pending-component maintenance: lazy backfill, reap `[x]`, and reorder detection.
///
/// Any write-through (backfill / reap) is persisted and committed in the same pass.
/// Silent no-op when the document has no tracked-work component.
pub fn run_pending_maintenance(file: &Path) -> Result<PendingMaintenanceReport> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(PendingMaintenanceReport::default()),
    };
    let components = match crate::component::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(PendingMaintenanceReport::default()),
    };
    let tracked_surfaces: Vec<String> = components
        .iter()
        .filter(|c| is_tracked_work_component(&c.name))
        .map(|c| c.name.clone())
        .collect();
    if tracked_surfaces.is_empty() {
        return Ok(PendingMaintenanceReport::default());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let doc_id = snapshot::doc_hash(&canonical).unwrap_or_else(|_| file.display().to_string());

    let mut current_content = content.clone();
    let mut snapshot_content = snapshot::load(file)?;
    // Reorder detection (step 4) compares the file's backlog order against the
    // snapshot as it was at cycle start. Capture it before the loop re-syncs the
    // snapshot to the file (#pending-gate-snapshot-desync), otherwise the synced
    // snapshot masks a same-cycle reorder.
    let snapshot_at_start = snapshot_content.clone();
    let mut mutated = false;
    // #pending-gate-snapshot-desync: the snapshot may need re-syncing to the
    // file's tracked surfaces even when maintenance itself makes no change —
    // the write phase can apply --pending-gate / --pending-edit / --review-add
    // to the file without those reaching the content_ours snapshot. Tracked
    // separately from `mutated` so the snapshot is re-saved without an
    // unnecessary working-tree rewrite.
    let mut snapshot_mutated = false;
    let mut saw_completed_before = false;

    for surface in &tracked_surfaces {
        let components = crate::component::parse(&current_content)
            .with_context(|| format!("failed to parse components while maintaining {}", surface))?;
        let comp = components
            .into_iter()
            .find(|c| component_matches_tracked_surface(&c.name, surface))
            .with_context(|| format!("document is missing the {} component", surface))?;
        let body = comp.content(&current_content);

        let mut current_body = body.to_string();
        let surface_label = maintenance_surface_label(surface);
        saw_completed_before |= !completed_pending_items(&current_body).is_empty();

        let (after_backfill, changed) =
            crate::pending::backfill(&current_body, &doc_id, &std::collections::HashSet::new());
        if changed {
            eprintln!(
                "[preflight] {}: backfilled missing hash ids / checkboxes",
                surface_label
            );
            current_body = after_backfill;
            mutated = true;
        }

        let (after_reap, removed_items) = crate::pending::reap_with_items(&current_body)?;
        if !removed_items.is_empty() {
            let removed_ids: Vec<String> = removed_items.iter().map(|i| i.id.clone()).collect();
            eprintln!(
                "[preflight] {}: reaped {} item(s): {}",
                surface_label,
                removed_items.len(),
                removed_ids.join(", ")
            );
            let _ = crate::cycle_state::record_reaped_pending_ids(file, &removed_ids);
            current_body = after_reap;
            mutated = true;
        }

        // Priority sort (#backlog-priority-attribute): when the component marker
        // carries `priority`, stable-sort items by their per-item `priority=<1..9>`
        // token (1 = highest; absent = lowest) so a downstream `agent:queue` sync
        // inherits the prioritized order.
        if comp.attrs.contains_key("priority")
            && let Some(sorted) = crate::pending::sort_by_priority(&current_body)
        {
            eprintln!("[preflight] {}: sorted by priority", surface_label);
            current_body = sorted;
            mutated = true;
        }

        // Re-sync the snapshot's tracked surface to the file's body whenever the
        // two diverge — even if maintenance made no change to it this pass. The
        // write phase persists --pending-gate / --pending-edit / --review-add to
        // the file but saves the content_ours snapshot (baseline + response)
        // before those mutations, so a pure gate/edit/review-add would otherwise
        // leave the snapshot stale and the mutation stranded as post-commit drift
        // (#pending-gate-snapshot-desync). --done already reaches this via reap,
        // which sets `mutated`; this also covers the no-reap mutations.
        if let Some(ref mut snap_content) = snapshot_content {
            let snap_comps = crate::component::parse(snap_content).ok();
            let snap_comp = snap_comps
                .and_then(|cs| {
                    cs.into_iter()
                        .find(|c| component_matches_tracked_surface(&c.name, surface))
                })
                .with_context(|| {
                    format!(
                        "pending maintenance: snapshot is missing the {} component",
                        surface
                    )
                })?;
            let snap_body = snap_comp.content(snap_content).to_string();
            if snap_body != current_body {
                *snap_content = snap_comp.replace_content(snap_content, &current_body);
                snapshot_mutated = true;
            }
            if !removed_items.is_empty()
                && let Some(archived) = archive_pending_done(file, snap_content, &removed_items)?
            {
                *snap_content = archived;
                snapshot_mutated = true;
            }
        }

        if current_body == body {
            continue;
        }

        current_content = comp.replace_content(&current_content, &current_body);
        if !removed_items.is_empty()
            && let Some(archived) = archive_pending_done(file, &current_content, &removed_items)?
        {
            current_content = archived;
        }
    }

    if let Some(reconciled) =
        crate::status_cmd::reconcile_top_backlog_status_content(&current_content)?
    {
        eprintln!("[preflight] status: reconciled stale top-backlog marker");
        current_content = reconciled;
        mutated = true;
    }
    if let Some(ref mut snap_content) = snapshot_content
        && let Some(reconciled) =
            crate::status_cmd::reconcile_top_backlog_status_content(snap_content)?
    {
        *snap_content = reconciled;
        snapshot_mutated = true;
    }

    // 3. Persist any mutations to the working tree file and/or the snapshot.
    //    Writing to both (surgically, via component replace) keeps the two in
    //    sync so the upcoming step-2 `git::commit` stages the reaped+archived
    //    snapshot in a single commit. We no longer call `git::commit` here —
    //    see #64mb: calling commit inside maintenance produced a second commit
    //    per preflight whenever anything mutated. The snapshot is saved
    //    independently of the file write so a write-phase pending mutation that
    //    only diverged the snapshot (gate/edit/review-add) is still committed
    //    rather than stranded (#pending-gate-snapshot-desync).
    if mutated {
        std::fs::write(file, &current_content)
            .with_context(|| format!("failed to write pending updates to {}", file.display()))?;
    }
    if (mutated || snapshot_mutated)
        && let Some(snap_content) = &snapshot_content
        && let Err(e) = snapshot::save(file, snap_content)
    {
        eprintln!("[preflight] pending: snapshot sync warning: {}", e);
    }

    if saw_completed_before {
        let persisted_content = if mutated {
            current_content.clone()
        } else {
            std::fs::read_to_string(file)
                .with_context(|| format!("failed to verify reap in {}", file.display()))?
        };
        ensure_no_completed_tracked_items(&persisted_content, "working tree")?;

        let snapshot_content = snapshot::load(file)?.with_context(|| {
            format!(
                "pending maintenance reaped completed tracked items in {} but the snapshot is missing",
                file.display()
            )
        })?;
        ensure_no_completed_tracked_items(&snapshot_content, "snapshot")?;
    }

    // 4. Reorder detection: compare the cycle-start snapshot's pending component
    //    to the current body. Uses the pre-sync snapshot (`snapshot_at_start`)
    //    rather than re-loading from disk, since step 3 may have re-synced the
    //    on-disk snapshot to the file (#pending-gate-snapshot-desync) which would
    //    otherwise hide a same-cycle reorder.
    let current_body = tracked_body_for_reorder(&current_content);
    let reordered = match snapshot_at_start {
        Some(snap) => {
            let snap_comp = crate::component::parse(&snap)
                .ok()
                .and_then(|comps| comps.into_iter().find(|c| is_backlog_component(&c.name)));
            if let (Some(sc), Some(current_body)) = (snap_comp, current_body) {
                let snap_body = &snap[sc.open_end..sc.close_start];
                crate::pending::detect_reorder(snap_body, current_body).is_some()
            } else {
                false
            }
        }
        None => false,
    };
    if reordered {
        eprintln!("[preflight] pending: reorder detected (skill must not reorder this cycle)");
    }

    // 5. Count legacy gated items in backlog and review items in review.
    let pending_gated_count = current_body
        .map(|body| {
            let (_, items, _) = crate::pending::parse_items(body);
            items
                .iter()
                .filter(|i| matches!(i.state, crate::pending::PendingState::Gated))
                .count()
        })
        .unwrap_or(0);
    if pending_gated_count > 0 {
        eprintln!("[preflight] pending: {} gated item(s)", pending_gated_count);
    }

    let (review_count, review_gated_count) = review_counts(&current_content);
    if review_count > 0 {
        eprintln!(
            "[preflight] review: {} item(s), {} gated",
            review_count, review_gated_count
        );
    }

    Ok(PendingMaintenanceReport {
        reordered,
        pending_gated_count,
        review_count,
        review_gated_count,
        legacy_gated_in_backlog_count: pending_gated_count,
    })
}

fn component_matches_tracked_surface(name: &str, surface: &str) -> bool {
    if is_backlog_component(surface) {
        is_backlog_component(name)
    } else {
        name == surface
    }
}

fn maintenance_surface_label(surface: &str) -> &str {
    if is_backlog_component(surface) {
        "pending"
    } else {
        "icebox"
    }
}

fn tracked_body_for_reorder(content: &str) -> Option<&str> {
    crate::component::parse(content).ok().and_then(|comps| {
        comps
            .into_iter()
            .find(|component| is_backlog_component(&component.name))
            .map(|component| component.content(content))
    })
}

fn review_counts(content: &str) -> (usize, usize) {
    let Some(body) = crate::component::parse(content).ok().and_then(|comps| {
        comps
            .into_iter()
            .find(|component| is_review_component(&component.name))
            .map(|component| component.content(content).to_string())
    }) else {
        return (0, 0);
    };
    let (_, items, _) = crate::pending::parse_items(&body);
    let review_items: Vec<_> = items.into_iter().filter(|item| !item.is_done()).collect();
    let gated = review_items
        .iter()
        .filter(|item| matches!(item.state, crate::pending::PendingState::Gated))
        .count();
    (review_items.len(), gated)
}

fn ensure_no_completed_tracked_items(content: &str, surface: &str) -> Result<()> {
    let components = crate::component::parse(content).with_context(|| {
        format!("failed to parse {surface} components during pending reap check")
    })?;
    let completed: Vec<crate::pending::PendingItem> = components
        .into_iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| completed_pending_items(component.content(content)))
        .collect();
    if completed.is_empty() {
        return Ok(());
    }

    let refs = completed
        .into_iter()
        .map(|item| {
            if item.id.is_empty() {
                format!("<missing-id> {}", item.text)
            } else {
                format!("#{}", item.id)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("pending maintenance left completed tracked items in the {surface}: {refs}");
}

fn completed_pending_items(body: &str) -> Vec<crate::pending::PendingItem> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .into_iter()
        .filter(crate::pending::PendingItem::is_done)
        .collect()
}

fn enforce_no_shadow_open_backlog(file: &Path) -> Result<()> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog shadow state in {}",
            file.display()
        )
    })?;
    let report = crate::pending::detect_shadow_open_items(&content)?;
    if !report.duplicated_in_live_backlog.is_empty() {
        eprintln!(
            "[preflight] pending shadow warning: open backlog item(s) also appear outside live agent:backlog: {}",
            format_shadow_refs(&report.duplicated_in_live_backlog)
        );
    }
    if !report.shadow_only.is_empty() {
        anyhow::bail!(
            "open backlog item(s) exist only outside live agent:backlog: {}. Move them back into the live backlog or mark them complete before continuing",
            format_shadow_refs(&report.shadow_only)
        );
    }
    Ok(())
}

fn format_shadow_refs(items: &[crate::pending::ShadowPendingItem]) -> String {
    items
        .iter()
        .map(crate::pending::ShadowPendingItem::reference)
        .collect::<Vec<_>>()
        .join(", ")
}

fn enforce_no_dropped_backlog(file: &Path, rc: &crate::graph::RunContext) -> Result<()> {
    let head_content = match rc.head_content() {
        Some(content) => content,
        None => return Ok(()),
    };
    let current_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog replay state in {}",
            file.display()
        )
    })?;
    let resolved_ids = crate::cycle_state::resolved_pending_ids(file)?;

    let external_done_ids = external_done_archive_ids(file, &current_content)?;
    let report = crate::pending::detect_dropped_from_history_with_extra_current_ids(
        &current_content,
        &head_content,
        &resolved_ids,
        &external_done_ids,
    )?;
    if !report.dropped.is_empty() {
        let refs = report
            .dropped
            .iter()
            .map(crate::pending::DroppedBacklogItem::reference)
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!(
            "open backlog item(s) from recent committed history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done before continuing",
            refs
        );
    }
    Ok(())
}

/// Queue component state extracted during maintenance.
///
/// Returned by `run_queue_maintenance` for later composition into `PreflightOutput`.
/// The `queue_prompts` are only populated when the queue is active.
#[derive(Debug, Default)]
struct QueueState {
    queue_prompts: Vec<String>,
    queue_active: Option<bool>,
    queue_deferred: bool,
    queue_start_at: Option<String>,
    queue_trigger: Option<crate::queue::QueueTrigger>,
    queue_halted: Option<String>,
    synced_queue_ids: Vec<String>,
    warnings: Vec<PreflightWarning>,
}

/// Run queue component maintenance: resolve activation, consume start fences,
/// persist `queue_active` state, and emit queue prompts for the skill.
///
/// Mutations (consumed start fences, `queue_active` changes) are persisted to
/// BOTH the working tree file and the snapshot, same as pending maintenance.
///
/// The `diff` parameter is optional — only needed for detecting exchange-level
/// `do queue`/`run queue` triggers. Pass `None` on the first call (before diff
/// computation) and the exchange trigger will be resolved in a later step.
/// Collect the backlog→queue sync request from `agent:backlog` / `agent:icebox`
/// (and the legacy `pending` alias) components carrying a `queue` attribute
/// (`#backlog-queue-sync-attr`). Returns the effective mode (the first
/// queue-tagged component's mode wins) and the active item ids from every
/// queue-tagged source component, in document order. Returns `None` when no
/// source component carries a recognized `queue` attribute.
/// Narrow the raw `do [#id]` directive target ids to the set that must reach a
/// `--done`/`--pending-gate` lifecycle outcome this cycle: ids still open in the
/// live backlog, minus any id that the backlog→queue sync auto-populated this
/// cycle (`#queue-sync-auto-pending-done-guard-misfire`). Synced ids are agent
/// queue maintenance, not user directives, so demanding they be resolved in the
/// populating cycle is a false-closed misfire.
fn filter_expect_done_or_gate_ids(
    directive_ids: &[String],
    open_backlog_ids: &std::collections::HashSet<String>,
    synced_queue_ids: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    directive_ids
        .iter()
        .map(|id| crate::pending::normalize_pending_id(id))
        .filter(|id| open_backlog_ids.contains(id))
        .filter(|id| !synced_queue_ids.contains(id))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn queue_entry_do_id(entry: &crate::queue::QueueEntry) -> Option<String> {
    match entry {
        crate::queue::QueueEntry::Prompt(prompt) | crate::queue::QueueEntry::Completed(prompt) => {
            queue_prompt_done_id(&prompt.text)
        }
        _ => None,
    }
}

fn collect_backlog_queue_sync(
    components: &[crate::component::Component],
    content: &str,
) -> Option<(crate::queue::BacklogQueueSyncMode, Vec<String>)> {
    let mut mode: Option<crate::queue::BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        let Some(comp_mode) = crate::queue::BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        let body = &content[comp.open_end..comp.close_start];
        ids.extend(crate::pending::active_item_ids(body));
    }
    mode.map(|m| (m, ids))
}

/// Build an id→priority-rank map from active `agent:backlog` / `agent:icebox`
/// items (`#backlog-priority-attribute`) for ordering a synced `agent:queue`.
/// First-seen rank wins on duplicate ids across components.
fn collect_backlog_priority_ranks(
    components: &[crate::component::Component],
    content: &str,
) -> std::collections::HashMap<String, u8> {
    let mut rank = std::collections::HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, r) in crate::pending::active_item_priorities(body) {
            rank.entry(id).or_insert(r);
        }
    }
    rank
}

fn run_queue_maintenance(file: &Path, diff: Option<&str>) -> Result<QueueState> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(QueueState::default()),
    };
    let components = match crate::component::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(QueueState::default()),
    };
    let comp = match components.iter().find(|c| c.name == "queue") {
        Some(c) => c,
        None => return Ok(QueueState::default()),
    };

    let body = &content[comp.open_end..comp.close_start];
    let entries = match crate::queue::parse(body) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[preflight] queue parse warning: {}", e);
            return Ok(QueueState::default());
        }
    };

    let mut entries = entries;
    let mut mutated = false;
    let mut current_content = content.clone();
    let mut queue_warnings = Vec::new();
    let mut synced_queue_ids = Vec::new();

    // `#ynra`: collect `agent:done` ids ONCE up front. The backlog→queue sync
    // below must never re-mint a `do [#id]` whose id is already completed
    // (archived in `agent:done`) — otherwise the strike pass removes it every
    // cycle, the sync re-injects it the next cycle, and the queue churns forever
    // on a completed ref. `agent:done` is not mutated by any queue maintenance
    // step, so this set is valid for both the sync filter and the later strike.
    let project_root = file.canonicalize().ok().and_then(|canonical| {
        snapshot::find_project_root(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

    // Backlog→queue sync (#backlog-queue-sync-attr): when an `agent:backlog` /
    // `agent:icebox` component carries a `queue` attribute, regenerate the queue
    // `do [#id]` prompts from its active items BEFORE activation so a freshly
    // synced queue can auto-activate on the same cycle.
    if let Some((mode, mut backlog_ids)) = collect_backlog_queue_sync(&components, &content) {
        // Drop ids already in `agent:done` so completed refs are never
        // re-injected into the queue (#ynra). A lingering active backlog `[ ]`
        // bullet whose id is also archived in `agent:done` would otherwise be
        // minted → struck → minted on every cycle.
        if !done_ids.is_empty() {
            let done_lower: std::collections::HashSet<String> =
                done_ids.iter().map(|id| id.to_ascii_lowercase()).collect();
            let before = backlog_ids.len();
            backlog_ids.retain(|id| !done_lower.contains(&id.trim().to_ascii_lowercase()));
            let excluded = before - backlog_ids.len();
            if excluded > 0 {
                eprintln!(
                    "[preflight] queue: excluded {excluded} completed id(s) from backlog→queue sync (already in agent:done; #ynra)"
                );
            }
        }
        if let Some(synced) = crate::queue::sync_backlog_into_queue(&entries, &backlog_ids, mode) {
            let pre_sync_ids = entries
                .iter()
                .filter_map(queue_entry_do_id)
                .collect::<std::collections::HashSet<String>>();
            let mut seen_synced_ids = std::collections::HashSet::new();
            synced_queue_ids = synced
                .iter()
                .filter_map(queue_entry_do_id)
                .filter(|id| !pre_sync_ids.contains(id))
                .filter(|id| seen_synced_ids.insert(id.clone()))
                .collect();
            let new_body = crate::queue::render(&synced);
            current_content = {
                let comps = crate::component::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            let pre_sync_prompt_count = entries
                .iter()
                .filter(|e| matches!(e, crate::queue::QueueEntry::Prompt(_)))
                .count();
            eprintln!(
                "[preflight] queue: synced backlog → queue ({:?}, {} active id(s))",
                mode,
                backlog_ids.len()
            );
            if pre_sync_prompt_count == 0 {
                queue_warnings.push(PreflightWarning {
                    code: "backlog_queue_sync_pending".to_string(),
                    message: format!(
                        "{}: agent:backlog carries `queue` attribute but the queue had 0 live prompts before this sync. \
                         The binary synced {} item(s) this cycle. \
                         For manual one-shot sync outside binary preflight: `agent-doc queue sync <FILE>`.",
                        file.display(),
                        synced_queue_ids.len()
                    ),
                    document_agent: None,
                    active_harness: None,
                });
            }
            entries = synced;
            mutated = true;
        }
    }

    // Queue priority ordering (#backlog-priority-attribute): when the queue
    // marker carries `priority`, stable-sort its do-prompts by the priority of
    // the matching backlog/icebox item so append-built or manual queues come out
    // prioritized. The backlog itself is priority-sorted earlier in the pipeline
    // by run_pending_maintenance, so the rank map read here is already current.
    if comp.attrs.contains_key("priority") {
        let rank = collect_backlog_priority_ranks(&components, &content);
        if !rank.is_empty()
            && let Some(sorted) = crate::queue::sort_prompts_by_priority(&entries, &rank)
        {
            let new_body = crate::queue::render(&sorted);
            current_content = {
                let comps = crate::component::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            eprintln!("[preflight] queue: sorted do-prompts by backlog priority");
            entries = sorted;
            mutated = true;
        }
    }

    // Read current state. A marker-side queue control (`start`/`go`/`stop`,
    // #queue-state-unify) is the marker spelling of the canonical `queue:`
    // frontmatter control: `start`/`go` are a fresh-activation gesture
    // equivalent to the legacy `auto` attribute (routed through the Auto trigger,
    // not the continuation-only Persisted path), and `stop` forces the queue
    // inactive this cycle. The control token is stripped from the tag below when
    // the queue drains, mirroring `auto`.
    let marker_control = crate::queue::marker_control(&comp.attrs);
    let marker_stop = matches!(
        marker_control,
        Some(agent_doc_core::frontmatter::QueueControl::Stop)
    );
    let has_auto = crate::queue::has_auto_attr(&comp.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_core::frontmatter::QueueControl::Start)
        );
    let exchange_triggered = diff.map(crate::diff::detect_queue_trigger).unwrap_or(false);
    let (fm, _) = frontmatter::parse(&current_content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);

    let mut activation =
        crate::queue::resolve_activation(&entries, has_auto, exchange_triggered, persisted_active);
    // A `stop` marker control forces the queue inactive this cycle regardless of
    // any other activation signal (#queue-state-unify), so the later
    // drain/clear path halts a running queue and strips the control token.
    if marker_stop && activation.active {
        activation = crate::queue::QueueActivation {
            entries_after: activation.entries_after,
            ..Default::default()
        };
    }
    let snapshot_was_active = snapshot_proves_queue_was_active(file);

    // Collapse duplicate live prompts before any other maintenance. Two
    // identical live queue prompts are never valid; they only appear when a
    // divergent IPC-buffer/snapshot CRDT/3-way merge duplicates a queue line
    // (#adoc-queue-ipc-drift). Converging here stops the duplicate from growing
    // on each preflight and re-syncs the rendered queue body.
    if let Some(deduped) = crate::queue::dedup_live_prompts(&activation.entries_after) {
        let dropped = activation.entries_after.len() - deduped.len();
        let new_body = crate::queue::render(&deduped);
        current_content = {
            let comps = crate::component::parse(&current_content)?;
            let q = comps.iter().find(|c| c.name == "queue").unwrap();
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = deduped;
        mutated = true;
        eprintln!("[preflight] queue: collapsed {dropped} duplicate live prompt(s)");
    }

    // Consume start fence if needed
    if activation.consumed_start_fence {
        let new_body = crate::queue::render(&activation.entries_after);
        current_content = {
            let comps = crate::component::parse(&current_content)?;
            let q = comps.iter().find(|c| c.name == "queue").unwrap();
            q.replace_content(&current_content, &new_body)
        };
        mutated = true;
        eprintln!("[preflight] queue: consumed start fence");
    }

    // Auto-strike queue head prompts whose `#id` is already in `agent:done`.
    //
    // Without this, the queue stays wedged on the first done item whenever
    // the cycle's diff does not literally match the queue head text — for
    // example after the user types new prompts into the exchange or after a
    // commit-mode finalize that reaped the backlog item via `--done` but
    // could not advance the queue because the prompt-text did not match
    // verbatim. The `should_consume_queue_prompt_for_diff_content` gate is
    // intentionally strict; this preflight-side maintenance pass is the
    // catch-up path that keeps the auto queue moving across already-resolved
    // items.
    //
    // Fixes the user-reported "queue gets stuck after 1 turn" symptom.
    // `project_root` / `done_ids` were computed once before the backlog→queue
    // sync (above) and reused here — `agent:done` is untouched by queue
    // maintenance, so the set is still current.
    let gated_ids = collect_agent_review_gated_ids(&current_content);
    let mut eligible_ids: std::collections::HashSet<String> = done_ids.clone();
    for id in &gated_ids {
        eligible_ids.insert(id.clone());
    }
    // `activation.entries_after` already reflects start-fence consumption and
    // the duplicate-prompt collapse above, so it is the authoritative current
    // entry set for the strike pass in every branch.
    let entries_for_strike = activation.entries_after.clone();
    if !eligible_ids.is_empty()
        && let Some((new_entries, struck)) =
            strike_done_queue_head_prompts(&entries_for_strike, &eligible_ids)
    {
        let new_body = crate::queue::render(&new_entries);
        current_content = {
            let comps = crate::component::parse(&current_content)?;
            let q = comps.iter().find(|c| c.name == "queue").unwrap();
            q.replace_content(&current_content, &new_body)
        };
        mutated = true;
        for prompt in &struck {
            let source = match queue_prompt_done_id(&prompt.text) {
                Some(id) if done_ids.contains(&id) => "done",
                Some(id) if gated_ids.contains(&id) => "review_gated",
                _ => "unknown",
            };
            eprintln!(
                "[preflight] queue: auto-struck already-resolved head prompt {:?} source={}",
                prompt.text, source
            );
        }
        // Recompute activation against the rewritten entry list so subsequent
        // halt / step / dispatch maintenance phases see the post-strike head.
        activation.entries_after = new_entries;
        // If the strike consumed the entire live head set, the queue is now
        // drained residue — every queued `do [#id]` was resolved via
        // `agent:done` / review-gate. `resolve_activation` ran on the
        // pre-strike entries (live prompts present) so `active` is stale-true;
        // flip it false here so the drain-cleanup path below clears
        // `queue_active`, strips `auto`, and empties the body. Without this the
        // stale `active: true` either trips the `item_modified` halt (the
        // post-strike head is `None` vs a still-live snapshot head) or leaves
        // the queue reported active with an empty prompt set. (#drained-done-queue-clear)
        if crate::queue::prompts(&activation.entries_after).is_empty() {
            activation.active = false;
            activation.trigger = None;
        }
    }

    // Phase 3: halt detection — stop fences and item modification
    if activation.active {
        // Stop fence at head → halt the queue
        if crate::queue::has_stop_fence_at_head(&activation.entries_after) {
            eprintln!("[preflight] queue: halt — stop fence at head");
            // Consume the stop fence
            let after_stop: Vec<crate::queue::QueueEntry> = activation.entries_after[1..].to_vec();
            let new_body = crate::queue::render(&after_stop);
            current_content = {
                let comps = crate::component::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            // Strip auto and clear queue_active
            if has_auto {
                let comps = crate::component::parse(&current_content)?;
                if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                    let raw = &current_content[q.open_start..q.open_end];
                    let new_tag = crate::queue::strip_auto_from_tag(raw);
                    if new_tag != raw {
                        let mut rebuilt = String::with_capacity(current_content.len());
                        rebuilt.push_str(&current_content[..q.open_start]);
                        rebuilt.push_str(&new_tag);
                        rebuilt.push_str(&current_content[q.open_end..]);
                        current_content = rebuilt;
                    }
                }
            }
            if persisted_active {
                current_content =
                    frontmatter::merge_queue_state(&current_content, false)?;
            }
            // Persist to file + snapshot
            std::fs::write(file, &current_content)
                .with_context(|| format!("queue halt: failed to write {}", file.display()))?;
            converge_live_buffer_queue_shape(file, &current_content, project_root.as_deref());
            if let Ok(Some(snap)) = snapshot::load(file) {
                let mut new_snap = snap.clone();
                if let Ok(sc) = crate::component::parse(&new_snap)
                    && let Some(sq) = sc.iter().find(|c| c.name == "queue")
                {
                    new_snap = sq.replace_content(&new_snap, &new_body);
                    if has_auto
                        && let Ok(sc2) = crate::component::parse(&new_snap)
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
                    if persisted_active
                        && let Ok(m) = frontmatter::merge_queue_state(&new_snap, false)
                    {
                        new_snap = m;
                    }
                    if new_snap != snap
                        && let Err(e) = snapshot::save(file, &new_snap)
                    {
                        eprintln!("[preflight] queue halt: snapshot sync warning: {}", e);
                    }
                }
            }
            return Ok(QueueState {
                queue_prompts: vec![],
                queue_active: Some(false),
                queue_deferred: false,
                queue_start_at: None,
                queue_trigger: activation.trigger,
                queue_halted: Some("stop_fence".into()),
                synced_queue_ids,
                warnings: Vec::new(),
            });
        }

        // Time gate at head → defer if not yet time
        if let Some(dt) = crate::queue::time_gate_at_head(&activation.entries_after) {
            eprintln!("[preflight] queue: deferred — time gate at head: {}", dt);
            return Ok(QueueState {
                queue_prompts: vec![],
                queue_active: None,
                queue_deferred: true,
                queue_start_at: Some(dt.to_string()),
                queue_trigger: activation.trigger,
                queue_halted: None,
                synced_queue_ids,
                warnings: Vec::new(),
            });
        }

        // Change detection: compare head prompt between snapshot and file, but
        // only for a queue that was already active. A newly auto/start/request
        // activated queue is operator-authored input for this cycle, not an
        // in-flight queue item edit.
        if snapshot_was_active
            && let Ok(Some(snap_content)) = snapshot::load(file)
            && let Ok(snap_comps) = crate::component::parse(&snap_content)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let snap_body = &snap_content[snap_q.open_end..snap_q.close_start];
            if let Ok(snap_entries) = crate::queue::parse(snap_body)
                && {
                    // Apply the same done/gated strike to the snapshot's
                    // entries before comparing heads. A cycle that resolved a
                    // leading queue head via `--done` (so the strike pass above
                    // converted it to `Completed`) otherwise reads as a
                    // head-text change vs the still-live snapshot head and
                    // false-halts as `item_modified`, wedging the remaining
                    // live head behind drained residue. Striking both sides
                    // leaves only genuine operator head edits visible.
                    // (#drained-done-queue-clear)
                    let snap_entries_struck = if eligible_ids.is_empty() {
                        snap_entries
                    } else {
                        strike_done_queue_head_prompts(&snap_entries, &eligible_ids)
                            .map(|(entries, _)| entries)
                            .unwrap_or(snap_entries)
                    };
                    crate::queue::detect_head_prompt_modified(
                        &snap_entries_struck,
                        &activation.entries_after,
                    )
                }
            {
                // #queue-no-stall-on-head-edit: a head prompt edit between
                // cycles only pauses the loop while the operator is actively
                // mid-edit. Once the buffer settles, adopt the edited head as
                // the new prompt and keep the queue armed instead of stripping
                // `auto` + forcing queue_active:false (the old behavior stalled
                // the loop on every settled head edit). The pause is retained
                // only while a live typing indicator proves the buffer is still
                // being edited, so we never grab a half-typed head.
                let head_edit_mid_typing = crate::debounce::is_typing_via_file(
                    &file.to_string_lossy(),
                    preflight_debounce_ms(file),
                );
                if !head_edit_mid_typing {
                    eprintln!(
                        "[preflight] queue: head prompt modified but buffer settled — adopting edited head, continuing loop (#queue-no-stall-on-head-edit)"
                    );
                    adopt_edited_queue_head_into_snapshot(file, &current_content);
                    // Fall through to normal active-queue handling below; the
                    // queue stays active with the edited head as the new prompt.
                } else {
                eprintln!("[preflight] queue: pause — head prompt modified mid-edit (buffer not settled); not grabbing a half-typed head");
                // Strip auto and clear queue_active
                if has_auto {
                    let comps = crate::component::parse(&current_content)?;
                    if let Some(q) = comps.iter().find(|c| c.name == "queue") {
                        let raw = &current_content[q.open_start..q.open_end];
                        let new_tag = crate::queue::strip_auto_from_tag(raw);
                        if new_tag != raw {
                            let mut rebuilt = String::with_capacity(current_content.len());
                            rebuilt.push_str(&current_content[..q.open_start]);
                            rebuilt.push_str(&new_tag);
                            rebuilt.push_str(&current_content[q.open_end..]);
                            current_content = rebuilt;
                        }
                    }
                }
                if persisted_active {
                    current_content =
                        frontmatter::merge_queue_state(&current_content, false)?;
                }
                std::fs::write(file, &current_content)
                    .with_context(|| format!("queue halt: failed to write {}", file.display()))?;
                converge_live_buffer_queue_shape(file, &current_content, project_root.as_deref());
                // Update snapshot
                if let Ok(Some(snap2)) = snapshot::load(file) {
                    let mut ns = snap2.clone();
                    if has_auto
                        && let Ok(sc) = crate::component::parse(&ns)
                        && let Some(sq) = sc.iter().find(|c| c.name == "queue")
                    {
                        let raw = &ns[sq.open_start..sq.open_end];
                        let new_tag = crate::queue::strip_auto_from_tag(raw);
                        if new_tag != raw {
                            let mut rebuilt = String::with_capacity(ns.len());
                            rebuilt.push_str(&ns[..sq.open_start]);
                            rebuilt.push_str(&new_tag);
                            rebuilt.push_str(&ns[sq.open_end..]);
                            ns = rebuilt;
                        }
                    }
                    if persisted_active
                        && let Ok(m) = frontmatter::merge_queue_state(&ns, false)
                    {
                        ns = m;
                    }
                    if ns != snap2
                        && let Err(e) = snapshot::save(file, &ns)
                    {
                        eprintln!("[preflight] queue halt: snapshot sync warning: {}", e);
                    }
                }
                return Ok(QueueState {
                    queue_prompts: vec![],
                    queue_active: Some(false),
                    queue_deferred: false,
                    queue_start_at: None,
                    queue_trigger: activation.trigger,
                    queue_halted: Some("item_modified".into()),
                    synced_queue_ids,
                    warnings: Vec::new(),
                });
                }
            }
        }
    }

    // Handle queue drain: if the queue has no remaining prompts, clear
    // queue_active, strip auto, and remove completed/directive residue.
    let queue_has_prompts = !crate::queue::prompts(&activation.entries_after).is_empty();
    let drained_residue = queue_entries_are_drained_residue(&activation.entries_after);
    let need_sync_newly_activated_queue_snapshot = activation.active && !snapshot_was_active;
    let need_set_active = activation.active && !persisted_active;
    let need_clear_active = !activation.active && persisted_active && !activation.deferred;
    let need_strip_auto = has_auto && !queue_has_prompts;
    let need_clear_non_auto_residue = !has_auto
        && !activation.active
        && !activation.deferred
        && drained_residue;
    let need_clear_drained_body =
        (need_strip_auto || need_clear_non_auto_residue) && !activation.deferred;

    if need_clear_drained_body {
        let comps = crate::component::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        if !q.content(&current_content).trim().is_empty() {
            current_content = q.replace_content(&current_content, "");
            mutated = true;
            eprintln!("[preflight] queue: cleared drained queue body");
        }
    }

    if !activation.active
        && !activation.deferred
        && !activation.entries_after.is_empty()
        && !need_clear_drained_body
    {
        // `inactive_queue_residue` is a per-*edit* signal, not a per-preflight
        // nag. It is useful when the operator just added/changed content in an
        // inactive queue (so a `do [#id]` they expected to run silently will
        // not). It is pure noise when the inactive queue is unchanged from the
        // committed snapshot — exactly the steady state an `item_modified` halt
        // leaves behind, where re-warning on every preflight with no user edit
        // drives the #adoc-queue-ipc-drift loop. Only warn when the inactive
        // queue body actually changed since the snapshot this cycle.
        if inactive_queue_changed_vs_snapshot(file, &activation.entries_after) {
            queue_warnings.push(PreflightWarning {
                code: "inactive_queue_residue".to_string(),
                message: "agent:queue is inactive but still contains directive/item residue; only active queue state is executable priority context".to_string(),
                document_agent: None,
                active_harness: None,
            });
        } else {
            eprintln!(
                "[preflight] queue: inactive with retained entries unchanged from snapshot — stable, not re-flagged as residue"
            );
        }
    }

    // Strip auto attribute from opening tag when queue drains
    // Strip the activation token from the opening tag when the queue drains
    // (`auto`/`go`/`start`) or when a `stop` marker halts it (#queue-state-unify).
    // The token is the ephemeral activation gesture; once consumed it must not
    // re-trigger on the next cycle.
    if need_strip_auto || marker_stop {
        let comps = crate::component::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        let raw_tag = &current_content[q.open_start..q.open_end];
        let new_tag = crate::queue::strip_control_from_tag(&crate::queue::strip_auto_from_tag(
            raw_tag,
        ));
        if new_tag != raw_tag {
            let mut rebuilt = String::with_capacity(current_content.len());
            rebuilt.push_str(&current_content[..q.open_start]);
            rebuilt.push_str(&new_tag);
            rebuilt.push_str(&current_content[q.open_end..]);
            current_content = rebuilt;
            mutated = true;
            eprintln!(
                "[preflight] queue: stripped activation token ({})",
                if marker_stop { "stop" } else { "drained" }
            );
        }
    }

    // Persist canonical queue activation state to frontmatter (#queue-state-unify
    // phase 4: emit `queue: start`/`queue: stop`, migrating off `queue_active:`).
    if need_set_active {
        current_content = frontmatter::merge_queue_state(&current_content, true)?;
        mutated = true;
        eprintln!("[preflight] queue: set queue: start");
    } else if need_clear_active {
        current_content = frontmatter::merge_queue_state(&current_content, false)?;
        mutated = true;
        eprintln!("[preflight] queue: set queue: stop");
    }

    // Persist file mutations.
    if mutated {
        std::fs::write(file, &current_content)
            .with_context(|| format!("failed to write queue updates to {}", file.display()))?;
        converge_live_buffer_queue_shape(file, &current_content, project_root.as_deref());
    }

    // Persist snapshot mutations. For newly activated queues, sync the queue
    // component from the visible document into the snapshot so later closeout
    // consumption can prove the same head prompt in both places.
    if (mutated || need_sync_newly_activated_queue_snapshot)
        && let Ok(Some(snap_content)) = snapshot::load(file)
    {
        let mut new_snap = snap_content.clone();

        if need_sync_newly_activated_queue_snapshot
            && let Ok(current_comps) = crate::component::parse(&current_content)
            && let Some(current_q) = current_comps
                .iter()
                .find(|component| component.name == "queue")
            && let Ok(snap_comps) = crate::component::parse(&new_snap)
            && let Some(snap_q) = snap_comps
                .iter()
                .find(|component| component.name == "queue")
        {
            let queue_region = &current_content[current_q.open_start..current_q.close_end];
            let mut rebuilt = String::with_capacity(new_snap.len() + queue_region.len());
            rebuilt.push_str(&new_snap[..snap_q.open_start]);
            rebuilt.push_str(queue_region);
            rebuilt.push_str(&new_snap[snap_q.close_end..]);
            new_snap = rebuilt;
        }

        // Apply queue body change to snapshot
        if !need_sync_newly_activated_queue_snapshot
            && (activation.consumed_start_fence || need_strip_auto || need_clear_drained_body)
            && let Ok(snap_comps) = crate::component::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let new_body = if need_clear_drained_body {
                String::new()
            } else {
                crate::queue::render(&activation.entries_after)
            };
            new_snap = snap_q.replace_content(&new_snap, &new_body);

            if need_strip_auto
                && let Ok(snap_comps2) = crate::component::parse(&new_snap)
                && let Some(snap_q2) = snap_comps2.iter().find(|c| c.name == "queue")
            {
                let raw_tag = &new_snap[snap_q2.open_start..snap_q2.open_end];
                let new_tag = crate::queue::strip_auto_from_tag(raw_tag);
                if new_tag != raw_tag {
                    let mut rebuilt = String::with_capacity(new_snap.len());
                    rebuilt.push_str(&new_snap[..snap_q2.open_start]);
                    rebuilt.push_str(&new_tag);
                    rebuilt.push_str(&new_snap[snap_q2.open_end..]);
                    new_snap = rebuilt;
                }
            }
        }

        // Apply frontmatter change to snapshot
        if need_set_active
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, true)
        {
            new_snap = merged;
        } else if need_sync_newly_activated_queue_snapshot
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, true)
        {
            new_snap = merged;
        } else if need_clear_active
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, false)
        {
            new_snap = merged;
        }
        if need_clear_drained_body
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, false)
        {
            new_snap = merged;
        }

        if new_snap != snap_content
            && let Err(e) = snapshot::save(file, &new_snap)
        {
            eprintln!("[preflight] queue: snapshot sync warning: {}", e);
        }
    }

    // Build output
    let queue_prompts: Vec<String> = if activation.active {
        crate::queue::prompts(&activation.entries_after)
            .iter()
            .map(|p| p.text.clone())
            .collect()
    } else {
        vec![]
    };

    Ok(QueueState {
        queue_prompts,
        queue_active: if activation.active {
            Some(true)
        } else if activation.deferred {
            None
        } else if persisted_active {
            Some(false)
        } else {
            None
        },
        queue_deferred: activation.deferred,
        queue_start_at: activation.start_at,
        queue_trigger: activation.trigger,
        queue_halted: None,
        synced_queue_ids,
        warnings: queue_warnings,
    })
}

/// Converge a live route-owned editor buffer to the queue shape just written to
/// `file` by queue maintenance.
///
/// Queue maintenance writes the corrected queue body, opening-tag `auto`
/// attribute, and `queue_active:` frontmatter to disk + snapshot. When a live
/// IPC listener owns the document it keeps its own working buffer; without this
/// push it overwrites the disk write on its next flush — re-adding `auto` and
/// `queue_active: true` — and the snapshot/HEAD drift regenerates on every
/// preflight (`#adoc-queue-ipc-buffer-divergence`). A content-only IPC patch
/// cannot converge an opening-tag attribute or frontmatter, so we send the
/// dedicated convergence message carrying the desired `auto` state plus the
/// `queue_active` frontmatter. Best-effort: a missing listener or send failure
/// is logged, never fatal — the disk/snapshot write remains the source of truth.
fn converge_live_buffer_queue_shape(file: &Path, content: &str, project_root: Option<&Path>) {
    let Some(root) = project_root else {
        return;
    };
    if !crate::ipc_socket::is_listener_active(root) {
        return;
    }
    let want_auto = match crate::component::parse(content) {
        Ok(comps) => comps
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| crate::queue::has_auto_attr(&q.attrs))
            .unwrap_or(false),
        Err(e) => {
            eprintln!("[preflight] queue: live convergence skipped — component parse failed: {e}");
            return;
        }
    };
    let queue_active = frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.queue_active)
        .unwrap_or(false);
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let fm_yaml = format!("queue_active: {queue_active}");
    match crate::ipc_socket::send_queue_convergence(
        root,
        &canonical.to_string_lossy(),
        want_auto,
        Some(&fm_yaml),
    ) {
        Ok(_) => eprintln!(
            "[preflight] queue: converged live editor buffer (auto={want_auto}, queue_active={queue_active})"
        ),
        Err(e) => {
            eprintln!("[preflight] queue: live buffer convergence send failed (non-fatal): {e}")
        }
    }
}

/// Absorb an operator's edited queue head into the snapshot when the loop adopts
/// it instead of halting (#queue-no-stall-on-head-edit). Copying the live file's
/// queue region into the snapshot makes the adopted head prove the same prompt at
/// closeout queue-consume and keeps the next cycle from re-detecting a spurious
/// `item_modified` edit. Best-effort: a parse/load/save failure is logged, never
/// fatal — the loop still continues with the edited head from the live file.
fn adopt_edited_queue_head_into_snapshot(file: &Path, current_content: &str) {
    let snap_now = match snapshot::load(file) {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[preflight] queue: adopt-head snapshot load warning (non-fatal): {e}");
            return;
        }
    };
    let Ok(cur_comps) = crate::component::parse(current_content) else {
        return;
    };
    let Some(cur_q) = cur_comps.iter().find(|c| c.name == "queue") else {
        return;
    };
    let Ok(snap_comps) = crate::component::parse(&snap_now) else {
        return;
    };
    let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue") else {
        return;
    };
    let queue_region = &current_content[cur_q.open_start..cur_q.close_end];
    let mut rebuilt = String::with_capacity(snap_now.len() + queue_region.len());
    rebuilt.push_str(&snap_now[..snap_q.open_start]);
    rebuilt.push_str(queue_region);
    rebuilt.push_str(&snap_now[snap_q.close_end..]);
    if rebuilt != snap_now
        && let Err(e) = snapshot::save(file, &rebuilt)
    {
        eprintln!("[preflight] queue: adopt-head snapshot sync warning (non-fatal): {e}");
    }
}

/// True when the current inactive-queue entry set differs from the queue body
/// recorded in the snapshot (the committed baseline for this cycle). Used to
/// scope the `inactive_queue_residue` warning to genuine operator edits instead
/// of re-warning every preflight on a stable, already-committed inactive queue
/// (the steady state an `item_modified` halt leaves behind — #adoc-queue-ipc-drift).
///
/// Comparison is normalized through `queue::parse` + `queue::render` so trivial
/// whitespace / boundary churn does not register as a change. A missing or
/// unreadable snapshot, or a snapshot with no queue component, is treated as
/// "changed" so a freshly-populated inactive queue still warns.
fn inactive_queue_changed_vs_snapshot(
    file: &Path,
    current_entries: &[crate::queue::QueueEntry],
) -> bool {
    let Ok(Some(snapshot_content)) = snapshot::load(file) else {
        return true;
    };
    let Ok(components) = crate::component::parse(&snapshot_content) else {
        return true;
    };
    let Some(snap_queue) = components.iter().find(|c| c.name == "queue") else {
        return true;
    };
    let snap_body = &snapshot_content[snap_queue.open_end..snap_queue.close_start];
    let Ok(snap_entries) = crate::queue::parse(snap_body) else {
        return true;
    };
    crate::queue::render(&snap_entries) != crate::queue::render(current_entries)
}

fn queue_entries_are_drained_residue(entries: &[crate::queue::QueueEntry]) -> bool {
    !entries.is_empty()
        && entries.iter().all(|entry| {
            matches!(
                entry,
                crate::queue::QueueEntry::Completed(_)
                    | crate::queue::QueueEntry::Preset(_)
                    | crate::queue::QueueEntry::Dispatch(_)
            )
        })
}

/// Collect every `[#id]` hash present in the document's `agent:done` (and the
/// legacy `agent:backlog-done` / `agent:pending-done`) components. When the
/// component carries an `archive=<path>` attribute, also walk the referenced
/// archive file so externally-archived done items still feed the queue-strike
/// maintenance pass.
///
/// Lower-cased so the comparison against queue prompt ids stays canonical.
#[cfg(test)]
fn collect_agent_done_ids(content: &str) -> std::collections::HashSet<String> {
    collect_agent_done_ids_with_root(content, None)
}

fn collect_agent_done_ids_with_root(
    content: &str,
    project_root: Option<&Path>,
) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let Ok(components) = crate::component::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !crate::component::is_backlog_done_component(&comp.name) {
            continue;
        }
        for id in crate::pending::extract_pending_ids_from_text(comp.content(content)) {
            ids.insert(id);
        }
        if let Some(archive) = comp.attrs.get("archive")
            && let Some(root) = project_root
        {
            let archive_path = root.join(archive);
            if let Ok(archive_content) = std::fs::read_to_string(&archive_path) {
                for id in crate::pending::extract_pending_ids_from_text(&archive_content) {
                    ids.insert(id);
                }
            }
        }
    }
    ids
}

/// Collect `#id` values from `agent:review` items marked `- [/]` (pending-gate
/// — code-complete, awaiting an external gate). These items represent work
/// whose committed phase satisfied the user's queued `do [#id]` directive even
/// though a parent multi-phase plan still has open phases. Returning them
/// lets queue auto-advance skip past phased items without requiring the agent
/// to mis-mark them `--done`.
///
/// Plan: tasks/agent-doc/plan-queue-auto-advance-past-pending-gate.md
fn collect_agent_review_gated_ids(content: &str) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let Ok(components) = crate::component::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !crate::component::is_review_component(&comp.name) {
            continue;
        }
        for line in comp.content(content).lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("- [/]") {
                continue;
            }
            let after_status = &trimmed[5..];
            let Some(start) = after_status.find("[#") else {
                continue;
            };
            let after = &after_status[start + 2..];
            let Some(end) = after.find(']') else {
                continue;
            };
            let id = &after[..end];
            if !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                ids.insert(id.to_ascii_lowercase());
            }
        }
    }
    ids
}

/// Convert **every** queue `Prompt` whose `#id` is already resolved (in
/// `done_ids`) to a `Completed` (`~text~`) entry, regardless of position.
///
/// Originally head-only, this now strikes resolved prompts anywhere in the
/// queue: a `do [#id]` for a completed (`agent:done`) or pending-gated id is a
/// stale ref that must never dispatch and, left behind a still-live head, would
/// otherwise sit forever as an orphaned ref and trip the shadow-backlog guard
/// (`#ynra`). Live (non-resolved) prompts are preserved in place, so no live
/// work is skipped — the first surviving prompt is still the consumption head.
///
/// Returns `None` when nothing changed. On match, returns the rewritten
/// entries plus the prompts that were struck (for telemetry).
fn strike_done_queue_head_prompts(
    entries: &[crate::queue::QueueEntry],
    done_ids: &std::collections::HashSet<String>,
) -> Option<(
    Vec<crate::queue::QueueEntry>,
    Vec<crate::queue::QueuePrompt>,
)> {
    let mut rewritten: Vec<crate::queue::QueueEntry> = Vec::with_capacity(entries.len());
    let mut struck: Vec<crate::queue::QueuePrompt> = Vec::new();
    for entry in entries {
        if let crate::queue::QueueEntry::Prompt(prompt) = entry
            && let Some(id) = queue_prompt_done_id(&prompt.text)
            && done_ids.contains(&id)
        {
            struck.push(prompt.clone());
            rewritten.push(crate::queue::QueueEntry::Completed(prompt.clone()));
            continue;
        }
        rewritten.push(entry.clone());
    }
    if struck.is_empty() {
        None
    } else {
        Some((rewritten, struck))
    }
}

/// Extract the `#id` from a queue prompt text like `do [#abcd]` or
/// `do #abcd ...`. Returns the lower-cased id without `#` / brackets.
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

fn snapshot_proves_queue_was_active(file: &Path) -> bool {
    let Ok(Some(snapshot_content)) = snapshot::load(file) else {
        return false;
    };
    let Ok((fm, _)) = frontmatter::parse(&snapshot_content) else {
        return false;
    };
    if fm.queue_active.unwrap_or(false) {
        return true;
    }
    let Ok(components) = crate::component::parse(&snapshot_content) else {
        return false;
    };
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let body = &snapshot_content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = crate::queue::parse(body) else {
        return false;
    };
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);
    crate::queue::resolve_activation(&entries, has_auto, false, false).active
}

/// Archive reaped pending items to `agent:done`.
///
/// When the archive component is absent, create a visible
/// `## Completed / Reaped` section after the tracked work components before
/// appending the entries. Returns `Some(new_content)` when archival happened,
/// `None` only when there is no tracked-work anchor to place the archive.
///
/// Entry format: `- YYYY-MM-DD [#id] text` — ISO date prefix for chronology,
/// hash preserved so the archive is grep-compatible with the live list, text
/// verbatim from the reaped item so context survives.
///
/// New entries are appended AFTER any existing archive body. The component
/// is always rendered with a trailing blank line so successive turns don't
/// pack entries onto one line.
pub fn archive_pending_done(
    file: &Path,
    content: &str,
    removed: &[crate::pending::PendingItem],
) -> Result<Option<String>> {
    if removed.is_empty() {
        return Ok(None);
    }
    let mut content_with_archive = content.to_string();
    let components = crate::component::parse(&content_with_archive)?;
    if !components
        .iter()
        .any(|c| crate::component::is_backlog_done_component(&c.name))
    {
        content_with_archive = insert_pending_done_component(&content_with_archive)
            .context("failed to insert agent:done component")?;
    }
    let components = crate::component::parse(&content_with_archive)?;
    let archive = components
        .into_iter()
        .find(|c| crate::component::is_backlog_done_component(&c.name))
        .context("document is missing agent:done component")?;
    let existing_body = &content_with_archive[archive.open_end..archive.close_start];

    // Use the `date` command so we stay on agent-doc's no-chrono policy
    // (see git.rs::chrono_timestamp). Fallback to "unknown-date" if the
    // command fails — archival still succeeds with a legible placeholder.
    let today = std::process::Command::new("date")
        .args(["+%Y-%m-%d"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown-date".to_string());

    if let Some(archive_path) = archive.attrs.get("archive") {
        let target = resolve_done_archive_target(file, archive_path)?;
        append_external_done_archive(&target, &today, removed)?;
        let pointer = format!("\n<!-- completed work archived in {} -->\n", archive_path);
        let new_body = if existing_body.trim().is_empty() || existing_body.trim() == pointer.trim()
        {
            pointer
        } else {
            existing_body.to_string()
        };
        return Ok(Some(
            archive.replace_content(&content_with_archive, &new_body),
        ));
    }

    let mut new_body = existing_body.to_string();
    if !new_body.is_empty() && !new_body.ends_with('\n') {
        new_body.push('\n');
    }
    for item in removed {
        new_body.push_str(&render_done_archive_entry(&today, item));
    }

    Ok(Some(
        archive.replace_content(&content_with_archive, &new_body),
    ))
}

fn insert_pending_done_component(content: &str) -> Option<String> {
    let components = crate::component::parse(content).ok()?;
    let anchor = components
        .iter()
        .filter(|c| crate::component::is_tracked_work_component(&c.name))
        .max_by_key(|c| c.close_end)?;
    let insert_at = anchor.close_end;
    let mut result = String::with_capacity(content.len() + 96);
    result.push_str(&content[..insert_at]);
    if !result.ends_with("\n\n") {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
    }
    result.push_str("## Completed / Reaped\n\n<!-- agent:done -->\n<!-- /agent:done -->\n");
    result.push_str(&content[insert_at..]);
    Some(result)
}

pub fn external_done_archive_ids(file: &Path, content: &str) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    let components = crate::component::parse(content)?;
    for archive in components
        .iter()
        .filter(|c| crate::component::is_backlog_done_component(&c.name))
    {
        let Some(archive_path) = archive.attrs.get("archive") else {
            continue;
        };
        let target = resolve_done_archive_target(file, archive_path)?;
        let archive_content = match std::fs::read_to_string(&target) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to read done archive {}", target.display()));
            }
        };
        ids.extend(crate::pending::extract_pending_ids_from_text(
            &archive_content,
        ));
    }
    Ok(ids)
}

fn resolve_done_archive_target(file: &Path, archive_path: &str) -> Result<PathBuf> {
    if archive_path.trim().is_empty() {
        bail!("agent:done archive= must not be empty");
    }
    if !archive_path.ends_with(".done.md") {
        bail!(
            "agent:done archive={} must point to a .done.md file",
            archive_path
        );
    }
    let relative = Path::new(archive_path);
    if relative.is_absolute() {
        bail!(
            "agent:done archive={} must be repo-relative, not absolute",
            archive_path
        );
    }
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        bail!(
            "agent:done archive={} must not escape the repository",
            archive_path
        );
    }

    let canonical_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = crate::snapshot::find_project_root(&canonical_file).with_context(|| {
        format!(
            "failed to find repository root for done archive resolution from {}",
            file.display()
        )
    })?;
    let target = root.join(relative);
    if let Ok(canonical_target) = target.canonicalize() {
        if !canonical_target.starts_with(&root) {
            bail!(
                "agent:done archive={} resolves outside the repository",
                archive_path
            );
        }
    } else if let Some(parent) = target.parent()
        && let Ok(canonical_parent) = parent.canonicalize()
        && !canonical_parent.starts_with(&root)
    {
        bail!(
            "agent:done archive={} resolves outside the repository",
            archive_path
        );
    }
    Ok(target)
}

fn append_external_done_archive(
    target: &Path,
    today: &str,
    removed: &[crate::pending::PendingItem],
) -> Result<()> {
    let mut existing = match std::fs::read_to_string(target) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            "# Agent Doc Completed Work\n\n".to_string()
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read done archive {}", target.display()));
        }
    };
    let mut changed = false;
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
        changed = true;
    }
    for item in removed {
        let first_line = format!("- {} [#{}] {}", today, item.id, item.text);
        if existing.lines().any(|line| line == first_line) {
            continue;
        }
        existing.push_str(&render_done_archive_entry(today, item));
        changed = true;
    }
    if changed || !target.exists() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create done archive directory {}",
                    parent.display()
                )
            })?;
        }
        crate::write::atomic_write_pub(target, &existing)
            .with_context(|| format!("failed to write done archive {}", target.display()))?;
    }
    Ok(())
}

fn render_done_archive_entry(today: &str, item: &crate::pending::PendingItem) -> String {
    let mut entry = format!("- {} [#{}] {}", today, item.id, item.text);
    if item.continuation.is_empty() {
        entry.push('\n');
    } else {
        entry.push('\n');
        entry.push_str(&item.continuation);
        if !item.continuation.ends_with('\n') {
            entry.push('\n');
        }
    }
    entry
}

/// Read the claims log and truncate it. Returns lines as a `Vec<String>`.
/// Returns an empty vec if the log doesn't exist or can't be read.
fn read_and_truncate_claims(file: &Path) -> Vec<String> {
    // Canonicalize to find project root reliably.
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let root = match snapshot::find_project_root(&canonical) {
        Some(r) => r,
        None => return vec![],
    };

    let log_path = root.join(".agent-doc/claims.log");

    let contents = match std::fs::read_to_string(&log_path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    if contents.is_empty() {
        return vec![];
    }

    // Collect non-empty lines.
    let claims: Vec<String> = contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();

    // Truncate the log.
    if let Err(e) = std::fs::write(&log_path, "") {
        eprintln!("[preflight] failed to truncate claims log: {}", e);
    }

    claims
}

/// Check related documents for changes since our last snapshot.
///
/// Parses `links` from the document's frontmatter, then for each path:
/// - Resolves relative to the document's parent directory
/// - Checks if the file exists
/// - Compares the related doc's last git commit time against our snapshot mtime
/// - If newer, summarizes the recent commits
fn is_url(link: &str) -> bool {
    link.starts_with("http://") || link.starts_with("https://")
}

/// Resolve the links cache directory, creating it if needed.
fn links_cache_dir(file: &Path) -> Option<std::path::PathBuf> {
    let mut search = file.parent();
    while let Some(d) = search {
        let candidate = d.join(".agent-doc");
        if candidate.is_dir() {
            let cache = candidate.join("links_cache");
            std::fs::create_dir_all(&cache).ok()?;
            return Some(cache);
        }
        search = d.parent();
    }
    None
}

/// Compute a cache filename for a URL.
fn url_cache_path(cache_dir: &Path, url: &str) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(url.as_bytes()));
    cache_dir.join(format!("{}.txt", hash))
}

/// Fetch a URL and compare against cached content. Returns a change entry if content differs.
/// Convert HTML content to markdown, stripping boilerplate elements.
fn html_to_markdown(html: &str) -> String {
    use htmd::HtmlToMarkdown;
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "nav", "footer", "noscript", "svg"])
        .build();
    converter.convert(html).unwrap_or_else(|_| html.to_string())
}

/// Returns true if the response content-type indicates HTML.
fn is_html_content(content_type: &str) -> bool {
    content_type.contains("text/html") || content_type.contains("application/xhtml")
}

fn check_url_link(url: &str, cache_dir: &Path) -> RelatedDocChange {
    let cache_path = url_cache_path(cache_dir, url);
    let cached = std::fs::read_to_string(&cache_path).ok();

    // Fetch with a reasonable timeout
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let response = agent.get(url).call();

    match response {
        Ok(resp) => {
            let content_type = resp.header("content-type").unwrap_or("").to_string();
            let body = match resp.into_string() {
                Ok(b) => b,
                Err(e) => {
                    return RelatedDocChange {
                        path: url.to_string(),
                        summary: format!("fetch error: {}", e),
                        exists: false,
                    };
                }
            };

            // Convert HTML to markdown for cleaner agent context
            let content = if is_html_content(&content_type) {
                html_to_markdown(&body)
            } else {
                body
            };

            match cached {
                Some(ref old) if old == &content => {
                    // No change — don't include in output
                    RelatedDocChange {
                        path: url.to_string(),
                        summary: String::new(), // empty = no change
                        exists: true,
                    }
                }
                Some(_) => {
                    // Content changed — update cache and report
                    let _ = std::fs::write(&cache_path, &content);
                    RelatedDocChange {
                        path: url.to_string(),
                        summary: format!("content changed ({} bytes)", content.len()),
                        exists: true,
                    }
                }
                None => {
                    // First fetch — cache it and report as new
                    let _ = std::fs::write(&cache_path, &content);
                    RelatedDocChange {
                        path: url.to_string(),
                        summary: format!("initial fetch ({} bytes)", content.len()),
                        exists: true,
                    }
                }
            }
        }
        Err(e) => RelatedDocChange {
            path: url.to_string(),
            summary: format!("fetch failed: {}", e),
            exists: false,
        },
    }
}

fn check_linked_docs(file: &Path) -> Vec<RelatedDocChange> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let fm = match frontmatter::parse(&content) {
        Ok((fm, _)) => fm,
        Err(_) => return vec![],
    };
    if fm.links.is_empty() {
        return vec![];
    }

    // Get our snapshot mtime as the baseline for comparison.
    let our_snapshot_mtime = snapshot::path_for(file)
        .ok()
        .and_then(|p| std::fs::metadata(&p).ok())
        .and_then(|m| m.modified().ok());

    let doc_dir = match file.parent() {
        Some(d) => d,
        None => return vec![],
    };

    let cache_dir = links_cache_dir(file);

    let mut changes = Vec::new();
    for link in &fm.links {
        if is_url(link) {
            // URL link — fetch and compare against cache
            if let Some(ref cache) = cache_dir {
                let change = check_url_link(link, cache);
                // Only include if there's an actual change or error
                if !change.summary.is_empty() {
                    changes.push(change);
                }
            } else {
                eprintln!(
                    "[preflight] warning: cannot resolve links cache for URL: {}",
                    link
                );
            }
            continue;
        }

        // File link — existing behavior
        let resolved = doc_dir.join(link);
        if !resolved.exists() {
            changes.push(RelatedDocChange {
                path: link.clone(),
                summary: "file not found".to_string(),
                exists: false,
            });
            continue;
        }

        // Compare last commit time of related doc against our snapshot mtime.
        let related_mtime = match git::last_commit_mtime(&resolved) {
            Ok(Some(t)) => t,
            _ => continue, // Not tracked or no commits — skip silently.
        };

        let is_newer = match our_snapshot_mtime {
            Some(snap_time) => related_mtime > snap_time,
            None => true, // No snapshot yet — treat everything as new.
        };

        if !is_newer {
            continue;
        }

        // Get recent commit summaries.
        let summary = recent_commit_summary(&resolved, our_snapshot_mtime);
        changes.push(RelatedDocChange {
            path: link.clone(),
            summary,
            exists: true,
        });
    }

    changes
}

/// Get a human-readable summary of recent commits for a file.
fn recent_commit_summary(file: &Path, since: Option<std::time::SystemTime>) -> String {
    let since_arg = since.and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| format!("--since={}", d.as_secs()))
    });

    let (git_root, resolved) = match git::resolve_to_git_root(file) {
        Ok(pair) => pair,
        Err(_) => return "changed (git unavailable)".to_string(),
    };
    let rel_path = resolved.strip_prefix(&git_root).unwrap_or(&resolved);

    let mut args = vec!["log", "--oneline", "-5"];
    let since_str;
    if let Some(ref s) = since_arg {
        since_str = s.clone();
        args.push(&since_str);
    }
    args.push("--");
    let rel_str = rel_path.to_string_lossy().to_string();
    args.push(&rel_str);

    let output = std::process::Command::new("git")
        .current_dir(&git_root)
        .args(&args)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            let lines: Vec<&str> = text.lines().take(5).collect();
            if lines.is_empty() {
                "changed".to_string()
            } else {
                lines.join("; ")
            }
        }
        _ => "changed (git log failed)".to_string(),
    }
}

fn save_baseline_content(file: &Path, content: &str) -> Option<String> {
    let baseline_path = match snapshot::baseline_path_for(file) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("[preflight] failed to resolve baseline path: {}", e);
            return None;
        }
    };
    if let Some(parent) = baseline_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&baseline_path, content) {
        Ok(()) => {
            eprintln!("[preflight] baseline saved: {}", baseline_path.display());
            Some(baseline_path.to_string_lossy().to_string())
        }
        Err(e) => {
            eprintln!("[preflight] failed to save baseline: {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn detect_identity_collisions_flags_preset_vs_backlog_id() {
        // #preset-item-id-collision: monsterrodholders.md repro — a #next-steps
        // prompt preset AND an active #next-steps backlog item collide.
        let content = concat!(
            "---\n",
            "prompt_presets:\n",
            "  '#next-steps': Any follow-up items?\n",
            "  '#commit-push': commit + push\n",
            "---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#next-steps] do the next steps\n",
            "- [ ] [#other1] unrelated work\n",
            "<!-- /agent:backlog -->\n",
        );
        let collisions = detect_identity_collisions(content);
        assert_eq!(collisions.len(), 1, "{collisions:?}");
        assert!(collisions[0].contains("#next-steps"), "{collisions:?}");
        assert!(collisions[0].contains("prompt_presets"), "{collisions:?}");
        assert!(collisions[0].contains("agent:backlog"), "{collisions:?}");
    }

    #[test]
    fn detect_identity_collisions_flags_duplicate_active_ids_across_components() {
        // The same active id in backlog and review is also ambiguous.
        let content = concat!(
            "---\nagent_doc_session: t\n---\n\n",
            "<!-- agent:backlog -->\n- [ ] [#dup7] in backlog\n<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n- [/] [#dup7] also gated in review\n<!-- /agent:review -->\n",
        );
        let collisions = detect_identity_collisions(content);
        assert_eq!(collisions.len(), 1, "{collisions:?}");
        assert!(collisions[0].contains("#dup7"), "{collisions:?}");
    }

    #[test]
    fn detect_identity_collisions_ignores_done_ids_and_clean_docs() {
        // A clean doc (unique ids) and done items must not flag.
        let content = concat!(
            "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
            "<!-- agent:backlog -->\n- [ ] [#alpha] active\n<!-- /agent:backlog -->\n\n",
            // A done item reusing the preset name is archived, not an active target.
            "<!-- agent:review -->\n- [x] [#next-steps] completed long ago\n<!-- /agent:review -->\n",
        );
        assert!(
            detect_identity_collisions(content).is_empty(),
            "done ids and unique active ids must not collide"
        );
    }

    #[test]
    fn identity_collision_for_new_id_reports_existing_sources() {
        // #preset-item-id-collision-enforce: a candidate id matching a preset
        // key or active item id reports the existing source(s); a free id is None.
        let content = concat!(
            "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
            "<!-- agent:backlog -->\n- [ ] [#alpha] active\n<!-- /agent:backlog -->\n",
        );
        assert_eq!(
            identity_collision_for_new_id(content, "next-steps"),
            Some(vec!["prompt_presets".to_string()])
        );
        // Normalization: leading `#` and case are ignored.
        assert_eq!(
            identity_collision_for_new_id(content, "#ALPHA"),
            Some(vec!["agent:backlog".to_string()])
        );
        assert_eq!(identity_collision_for_new_id(content, "fresh01"), None);
        assert_eq!(identity_collision_for_new_id(content, ""), None);
    }

    #[test]
    fn strike_done_queue_head_prompts_marks_done_items_completed() {
        let entries = vec![
            crate::queue::QueueEntry::Preset("#spec-test-build-install-commit-push".to_string()),
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#jbrsrbusyint]".to_string(),
                multiline: false,
            }),
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#jbrsrbusysim]".to_string(),
                multiline: false,
            }),
        ];
        let done_ids: std::collections::HashSet<String> =
            ["jbrsrbusyint".to_string()].into_iter().collect();

        let (rewritten, struck) =
            super::strike_done_queue_head_prompts(&entries, &done_ids).expect("expected strike");

        assert_eq!(struck.len(), 1);
        assert_eq!(struck[0].text, "do [#jbrsrbusyint]");
        match &rewritten[1] {
            crate::queue::QueueEntry::Completed(prompt) => {
                assert_eq!(prompt.text, "do [#jbrsrbusyint]");
            }
            other => panic!("expected Completed for head prompt, got {:?}", other),
        }
        // The live head (`#jbrsrbusysim`) must stay intact for the normal
        // consumption path.
        match &rewritten[2] {
            crate::queue::QueueEntry::Prompt(prompt) => {
                assert_eq!(prompt.text, "do [#jbrsrbusysim]");
            }
            other => panic!("expected Prompt for live head, got {:?}", other),
        }
    }

    #[test]
    fn strike_done_queue_head_prompts_returns_none_when_head_is_live() {
        let entries = vec![crate::queue::QueueEntry::Prompt(
            crate::queue::QueuePrompt {
                text: "do [#stillopen]".to_string(),
                multiline: false,
            },
        )];
        let done_ids: std::collections::HashSet<String> =
            ["somethingelse".to_string()].into_iter().collect();

        assert!(super::strike_done_queue_head_prompts(&entries, &done_ids).is_none());
    }

    #[test]
    fn strike_done_queue_prompts_strikes_non_head_resolved_ref() {
        // #ynra: a resolved (done) ref behind a live head must be struck, not
        // left as an orphaned ref (which trips the shadow-backlog guard). The
        // live head is preserved in place.
        let entries = vec![
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#liveone]".to_string(),
                multiline: false,
            }),
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#donetail]".to_string(),
                multiline: false,
            }),
        ];
        let done_ids: std::collections::HashSet<String> =
            ["donetail".to_string()].into_iter().collect();

        let (rewritten, struck) =
            super::strike_done_queue_head_prompts(&entries, &done_ids).expect("expected a strike");
        assert_eq!(struck.len(), 1);
        assert_eq!(struck[0].text, "do [#donetail]");
        // Live head preserved as a Prompt; the trailing resolved ref struck.
        assert!(matches!(
            &rewritten[0],
            crate::queue::QueueEntry::Prompt(p) if p.text == "do [#liveone]"
        ));
        assert!(matches!(
            &rewritten[1],
            crate::queue::QueueEntry::Completed(p) if p.text == "do [#donetail]"
        ));
    }

    #[test]
    fn collect_agent_review_gated_ids_extracts_only_gated_marker() {
        let content = "\
<!-- agent:review -->
- [/] [#alpha] First gated item with a plan reference.
- [x] [#beta] Already-done item in review (legacy).
- [ ] [#charlie] Open item in review — not gated.
- [/] [#delta] [partial] Another gated item.
- [/] no id here.
<!-- /agent:review -->
";
        let ids = super::collect_agent_review_gated_ids(content);
        assert!(
            ids.contains("alpha"),
            "expected gated [/] item to be collected, got {:?}",
            ids
        );
        assert!(
            ids.contains("delta"),
            "expected second gated [/] item to be collected, got {:?}",
            ids
        );
        assert!(
            !ids.contains("beta"),
            "[x] marker is not gated, must not be collected"
        );
        assert!(
            !ids.contains("charlie"),
            "[ ] marker is not gated, must not be collected"
        );
        assert_eq!(
            ids.len(),
            2,
            "only [/] items should be collected: {:?}",
            ids
        );
    }

    #[test]
    fn collect_agent_review_gated_ids_returns_empty_when_no_review_component() {
        let content =
            "<!-- agent:backlog -->\n- [ ] [#alpha] backlog only\n<!-- /agent:backlog -->\n";
        let ids = super::collect_agent_review_gated_ids(content);
        assert!(ids.is_empty(), "no review component → empty: {:?}", ids);
    }

    #[test]
    fn collect_agent_review_gated_ids_ignores_backlog_open_items() {
        let content = "\
<!-- agent:backlog -->
- [ ] [#openbk] open in backlog
<!-- /agent:backlog -->
<!-- agent:review -->
- [/] [#gatedrv] gated in review
<!-- /agent:review -->
";
        let ids = super::collect_agent_review_gated_ids(content);
        assert!(ids.contains("gatedrv"));
        assert!(
            !ids.contains("openbk"),
            "backlog open items must NOT be collected as gated"
        );
    }

    #[test]
    fn strike_done_queue_head_prompts_strikes_review_gated_items() {
        // Queue head matches a gated `[/]` item in agent:review — auto-strike
        // must advance the queue past it just like an agent:done item.
        let entries = vec![
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#gatedphase]".to_string(),
                multiline: false,
            }),
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#stillopen]".to_string(),
                multiline: false,
            }),
        ];
        let eligible_ids: std::collections::HashSet<String> =
            ["gatedphase".to_string()].into_iter().collect();

        let (rewritten, struck) = super::strike_done_queue_head_prompts(&entries, &eligible_ids)
            .expect("expected gated head to be struck");
        assert_eq!(struck.len(), 1);
        assert_eq!(struck[0].text, "do [#gatedphase]");
        match &rewritten[1] {
            crate::queue::QueueEntry::Prompt(prompt) => {
                assert_eq!(prompt.text, "do [#stillopen]");
            }
            other => panic!("expected live head to remain Prompt, got {:?}", other),
        }
    }

    #[test]
    fn collect_agent_done_ids_extracts_from_done_component() {
        let content = "<!-- agent:done -->\n- [x] [#alpha] One thing\n- [x] [#bravo] Another\n<!-- /agent:done -->\n";
        let ids = super::collect_agent_done_ids(content);
        assert!(ids.contains("alpha"));
        assert!(ids.contains("bravo"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn collect_agent_done_ids_reads_archive_attr_when_present() {
        let dir = TempDir::new().unwrap();
        let archive_rel = "tasks/done-archive.md";
        let archive_path = dir.path().join(archive_rel);
        std::fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
        std::fs::write(
            &archive_path,
            "- [x] [#archived1] First archived item\n- [x] [#archived2] Second\n",
        )
        .unwrap();
        let content = format!(
            "<!-- agent:done archive={} -->\n<!-- /agent:done -->\n",
            archive_rel
        );
        let ids = super::collect_agent_done_ids_with_root(&content, Some(dir.path()));
        assert!(
            ids.contains("archived1"),
            "expected ids to include archived1 from archive file: {:?}",
            ids
        );
        assert!(ids.contains("archived2"));
        // Without the root, the archive path cannot be resolved → empty.
        let ids_no_root = super::collect_agent_done_ids(&content);
        assert!(ids_no_root.is_empty());
    }

    #[test]
    fn queue_prompt_done_id_parses_canonical_bracket_form() {
        assert_eq!(
            super::queue_prompt_done_id("do [#jbrsrbusyint]"),
            Some("jbrsrbusyint".to_string())
        );
        assert_eq!(
            super::queue_prompt_done_id("do #jbrsrbusyint more text"),
            Some("jbrsrbusyint".to_string())
        );
        assert_eq!(super::queue_prompt_done_id("plain prompt"), None);
    }

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    /// Set up a minimal project directory with .agent-doc/ structure and a git repo.
    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();

        // Initialize a bare git repo so `git commit` doesn't fail fatally.
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@test.com"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .ok();

        dir
    }

    fn commit_all(root: &Path, message: &str, commit_date: Option<&str>) {
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        let mut commit = Command::new("git");
        commit
            .current_dir(root)
            .args(["commit", "-m", message, "--no-verify"]);
        if let Some(date) = commit_date {
            commit
                .env("GIT_COMMITTER_DATE", date)
                .env("GIT_AUTHOR_DATE", date);
        }
        let output = commit.output().unwrap();
        assert!(
            output.status.success(),
            "git commit {message:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn initialize_git_head(root: &Path) {
        let readme = root.join("README.md");
        std::fs::write(&readme, "# project\n").unwrap();
        commit_all(root, "initial", None);
    }

    fn write_committed_doc(
        root: &Path,
        rel: &str,
        content: &str,
        message: &str,
        commit_date: Option<&str>,
    ) -> PathBuf {
        let doc = root.join(rel);
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        commit_all(root, message, commit_date);
        doc
    }

    fn write_sessions_json(root: &Path, entries: &[(&str, &str, &Path, &str, &str)]) {
        let mut sessions = serde_json::Map::new();
        for (session_id, pane, file, window, started) in entries {
            sessions.insert(
                (*session_id).to_string(),
                serde_json::json!({
                    "pane": pane,
                    "pid": 9999,
                    "cwd": root.to_string_lossy(),
                    "started": started,
                    "file": file.strip_prefix(root).unwrap().to_string_lossy(),
                    "window": window
                }),
            );
        }
        std::fs::write(
            root.join(".agent-doc/sessions.json"),
            serde_json::to_string_pretty(&serde_json::Value::Object(sessions)).unwrap(),
        )
        .unwrap();
    }

    fn age_cycle_state(file: &Path, age_secs: u64) {
        let canonical = file.canonicalize().unwrap();
        let root = crate::snapshot::find_project_root(&canonical).unwrap();
        let hash = crate::snapshot::doc_hash(&canonical).unwrap();
        let path = root
            .join(".agent-doc/state/cycles")
            .join(format!("{hash}.json"));
        let mut state: crate::cycle_state::CycleState =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        state.started_at = state.started_at.saturating_sub(age_secs);
        state.updated_at = state.updated_at.saturating_sub(age_secs);
        std::fs::write(path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }

    fn write_cycles_log(doc: &Path, entries: &[crate::ops_log::CycleEntry]) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }

    #[test]
    fn preflight_produces_valid_json() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello\n").unwrap();

        // Snapshot matches document → no_changes = true.
        snapshot::save(&doc, &std::fs::read_to_string(&doc).unwrap()).unwrap();

        run(&doc).unwrap();
        // If run() returns Ok(()), the JSON was printed to stdout without error.
        // The test verifies no panic and no error return.
    }

    #[test]
    fn preflight_fails_closed_when_required_ssh_doc_mapping_resolves_no_targets() {
        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[ssh.docs.\"tasks/monsterrodholders.md\"]\nprofile = \"missing\"\n",
        )
        .unwrap();
        let doc = dir.path().join("tasks/monsterrodholders.md");
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
        snapshot::save(&doc, old_doc).unwrap();
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
        snapshot::save(&doc, new_doc).unwrap();
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

        let doc = root.join("monsterrodholders.md");
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
        snapshot::save(&doc, committed).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "monsterrodholders.md"])
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
    fn preflight_detects_diff() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();

        // Save snapshot of original, then add new content.
        snapshot::save(&doc, original).unwrap();
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n## User\n\nHello\n\nNew question here.\n",
        )
        .unwrap();

        // diff::compute should detect changes → no_changes = false.
        let diff_result = diff::compute(&doc).unwrap();
        assert!(diff_result.is_some(), "diff should detect new content");
    }

    #[test]
    fn preflight_closes_stale_starting_actors_even_when_daily_gc_stamp_is_fresh() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        std::fs::write(dir.path().join(".agent-doc/gc.stamp"), "").unwrap();

        let stale_doc = dir.path().join("tasks/stale-starting.md");
        std::fs::create_dir_all(stale_doc.parent().unwrap()).unwrap();
        std::fs::write(&stale_doc, "body").unwrap();
        let stale_record = crate::session_actor::ActorRecord {
            document_id: stale_doc.to_string_lossy().to_string(),
            session_id: "session-stale-starting".to_string(),
            generation: 1,
            pane_id: "%71".to_string(),
            window_id: "@7".to_string(),
            harness: "codex".to_string(),
            state: crate::session_actor::ActorState::Starting,
            last_transition: crate::session_actor::ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 1,
                prior_generation: 0,
                new_generation: 1,
            },
        };
        crate::project_controller::store_actor_record(dir.path(), Some(0), &stale_record).unwrap();

        run(&doc).unwrap();

        let updated =
            crate::project_controller::load_actor_record(dir.path(), &stale_record.document_id)
                .unwrap()
                .unwrap();
        assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
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
        snapshot::save(&doc, content).unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #code-review", doc.display()),
        );

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
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
            "<!-- agent:queue auto -->\n",
            "- do [#oobpmt]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#oobpmt] Fix OOB prompt absorption.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
            "active queue prompt should open a cycle even when the file matches the snapshot"
        );
    }

    #[test]
    fn preflight_probe_does_not_open_cycle_even_with_dispatchable_diff() {
        // #preflight-probe-side-effect-free: the SAME active-queue input that
        // opens a `preflight_started` cycle in the dispatch path (see
        // `preflight_opens_cycle_from_active_queue_when_document_has_no_diff`)
        // must leave NO open cycle when run as a pure inspection probe, so a
        // diagnostic preflight never wedges a later `session-check`.
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
            "<!-- agent:queue auto -->\n",
            "- do [#oobpmt]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#oobpmt] Fix OOB prompt absorption.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_with_options(&doc, PreflightOptions { probe: true }).unwrap();

        // The probe must not leave an OPEN cycle (`preflight_started` /
        // `response_captured` / `write_applied`) — that is the state that wedges
        // a later `session-check`. A terminal `committed`/`abandoned` cycle from
        // the (idempotent) commit step is acceptable.
        if let Some(state) = crate::cycle_state::load(&doc).unwrap() {
            assert!(
                matches!(
                    state.phase,
                    crate::cycle_state::CyclePhase::Committed
                        | crate::cycle_state::CyclePhase::Abandoned
                ),
                "a probe preflight must not leave an open cycle, got {:?}",
                state.phase
            );
        }
    }

    #[test]
    fn run_queue_maintenance_syncs_backlog_into_empty_queue() {
        // #backlog-queue-sync-attr: a backlog carrying `queue=sync` regenerates
        // the (empty) queue with `do [#id]` for active items; gated/done excluded.
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
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "- [/] [#gated] blocked\n",
            "- [ ] [#beta] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            state.synced_queue_ids,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert!(
            updated.contains("- do [#alpha]"),
            "synced queue:\n{updated}"
        );
        assert!(updated.contains("- do [#beta]"));
        assert!(
            !updated.contains("- do [#gated]"),
            "gated item must not be queued:\n{updated}"
        );
        assert!(
            state
                .warnings
                .iter()
                .any(|w| w.code == "backlog_queue_sync_pending"),
            "empty-queue-before-sync must emit backlog_queue_sync_pending warning, got {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_no_warning_when_queue_already_synced() {
        // When the queue already matches the backlog, no backlog_queue_sync_pending
        // warning should fire (sync_backlog_into_queue returns None → no warning path).
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
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert!(
            !state
                .warnings
                .iter()
                .any(|w| w.code == "backlog_queue_sync_pending"),
            "already-synced queue must NOT emit backlog_queue_sync_pending warning, got {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_marker_go_activates_like_auto() {
        // #queue-state-unify: a `go`/`start` marker control freshly activates the
        // queue through the Auto trigger, identical to the legacy `auto` attribute.
        for token in ["go", "start"] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            let content = format!(
                concat!(
                    "---\nagent_doc_session: test\nagent_doc_format: template\n",
                    "agent_doc_write: crdt\n---\n\n",
                    "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue {} -->\n- please do the thing\n<!-- /agent:queue -->\n",
                ),
                token
            );
            std::fs::write(&doc, &content).unwrap();
            snapshot::save(&doc, &content).unwrap();

            let state = run_queue_maintenance(&doc, None).unwrap();
            assert_eq!(
                state.queue_active,
                Some(true),
                "marker `{token}` must activate the queue"
            );
            assert_eq!(state.queue_trigger, Some(crate::queue::QueueTrigger::Auto));
            let updated = std::fs::read_to_string(&doc).unwrap();
            assert!(
                updated.contains("queue: start"),
                "marker `{token}` must persist queue_active:\n{updated}"
            );
        }
    }

    #[test]
    fn run_queue_maintenance_marker_stop_halts_active_queue() {
        // #queue-state-unify: a `stop` marker control forces an otherwise-active
        // queue inactive and clears persisted queue_active.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue stop -->\n- please do the thing\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.queue_active,
            Some(false),
            "marker `stop` must halt the active queue"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("queue: stop"),
            "marker `stop` must clear queue_active:\n{updated}"
        );
        assert!(
            !updated.contains("agent:queue stop"),
            "marker `stop` token must be stripped after halt:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_excludes_done_ids_from_backlog_sync() {
        // #ynra: a lingering active backlog `[ ]` bullet whose id is also archived
        // in `agent:done` must NOT be re-minted into the queue (it would be struck
        // every cycle and re-injected the next → forever churn). The fresh active
        // id is still minted.
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
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#na3x] completed-but-lingering\n",
            "- [ ] [#fresh] genuinely open\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-06-01 [#na3x] completed-but-lingering\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("[#na3x]") || !updated.contains("do [#na3x]"),
            "completed id must not be minted into the queue:\n{updated}"
        );
        assert!(
            !updated.contains("do [#na3x]"),
            "completed id must not appear as a queue do-prompt:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]"),
            "fresh active id must still be queued:\n{updated}"
        );
        assert_eq!(state.synced_queue_ids, vec!["fresh".to_string()]);
    }

    #[test]
    fn run_queue_maintenance_excludes_external_archive_done_ids() {
        // #ynra (external-archive variant): a completed id reaped to the EXTERNAL
        // `agent:done archive=<file>` (not inline) must also be excluded from the
        // backlog→queue sync and struck from the queue. Done-id collection reads
        // the archive file, so the queue must not churn on an externally-archived
        // completed ref.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let archive_rel = "session.done.md";
        std::fs::write(
            dir.path().join(archive_rel),
            "# Done\n\n- 2026-06-01 [#extdone] archived externally\n",
        )
        .unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#extdone]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#extdone] lingering active dup of an externally-archived id\n",
            "- [ ] [#fresh] genuinely open\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("- do [#extdone]"),
            "externally-archived completed ref must be struck/excluded, not left live:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]"),
            "fresh active id must still be queued:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_backlog_sync_is_idempotent() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            state.synced_queue_ids.is_empty(),
            "idempotent sync should not report freshly-added ids"
        );
        assert_eq!(
            updated.matches("- do [#alpha]").count(),
            1,
            "append must not duplicate an already-queued id:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_records_only_newly_synced_ids() {
        // The existing queue head must stay outside the synced-id exclusion set
        // so pending_done_guard still requires the consumed `do [#worked]` item
        // to be done/gated.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#worked]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=prepend -->\n",
            "- [ ] [#worked] the real queue head\n",
            "- [ ] [#alpha] freshly synced\n",
            "- [ ] [#beta] freshly synced\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.synced_queue_ids,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        let open_backlog: std::collections::HashSet<String> = ["worked", "alpha", "beta"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let synced_queue_ids = state
            .synced_queue_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<String>>();
        let result = filter_expect_done_or_gate_ids(
            &[
                "worked".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
            ],
            &open_backlog,
            &synced_queue_ids,
        );
        assert_eq!(result, vec!["worked".to_string()]);
    }

    #[test]
    fn preflight_new_auto_queue_from_inactive_snapshot_does_not_halt_on_changed_head() {
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
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#oldhead]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#newhead] Run the newly queued head.\n",
            "- [ ] [#nexthead] Run the next queued item.\n",
            "<!-- /agent:backlog -->\n"
        );
        let current_content = snapshot_content
            .replace("<!-- agent:queue -->", "<!-- agent:queue auto -->")
            .replace("- do [#oldhead]", "- do [#newhead]\n- do [#nexthead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_halted, None);
        assert_eq!(
            state.queue_prompts,
            vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()]
        );

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: start"));
        assert!(updated.contains("<!-- agent:queue auto -->"));
        assert!(updated.contains("- do [#newhead]"));
        assert!(!updated.contains("- do [#oldhead]"));

        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("queue: start")
                && snap.contains("<!-- agent:queue auto -->")
                && snap.contains("- do [#newhead]")
                && !snap.contains("- do [#oldhead]"),
            "newly activated queue must be snapshotted as the closeout baseline:\n{snap}"
        );

        let done_ids = vec!["newhead".to_string()];
        let outcome =
            crate::write::consume_queue_prompts_for_done_ids_with_outcome(&doc, &done_ids)
                .unwrap()
                .expect("newly activated queue head should be consumable");
        assert_eq!(outcome.consumed_count, 1);
        assert_eq!(outcome.remaining, 1);

        let consumed = std::fs::read_to_string(&doc).unwrap();
        assert!(consumed.contains("- ~do [#newhead]~"));
        assert!(consumed.contains("- do [#nexthead]"));
    }

    #[test]
    fn queue_maintenance_drains_all_done_queue_without_item_modified_halt() {
        // #drained-done-queue-clear: a fully resolved auto-queue (every `do
        // [#id]` already in agent:done) plus a batch dispatch directive must
        // drain — not false-halt as `item_modified`. Before the fix the
        // strike pass converted every live head to Completed, leaving the
        // post-strike head `None` vs a still-live snapshot head, which
        // detect_head_prompt_modified read as an edit and halted before the
        // drain-cleanup path ran. The Corky live-repro shape: template doc,
        // dispatch preset, multiple bracketed `do [#id]` prompts, no diff.
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
            "<!-- agent:queue auto -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#alpha] First done.\n",
            "- [x] [#beta] Second done.\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "fully-resolved queue must drain, not halt as item_modified"
        );
        assert_eq!(state.queue_active, Some(false));
        assert!(state.queue_prompts.is_empty());

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: stop"), "file: {updated}");
        assert!(
            !updated.contains("agent:queue auto"),
            "auto must be stripped on drain: {updated}"
        );
        assert!(
            !updated.contains("- do [#alpha]") && !updated.contains("- do [#beta]"),
            "drained queue body must be cleared: {updated}"
        );

        // Snapshot matches the drained file so the closeout commit boundary
        // does not strand the maintenance mutation.
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(snap.contains("queue: stop"));
        assert!(!snap.contains("agent:queue auto"));
        assert!(!snap.contains("- do [#alpha]"));
    }

    #[test]
    fn queue_maintenance_partial_done_strike_advances_to_live_head_without_halt() {
        // #drained-done-queue-clear (partial case): a leading queue head that
        // is already done must be struck and the queue advanced to the next
        // live head — without false-halting as item_modified. The snapshot is
        // struck the same way before the head-modified comparison so only a
        // genuine operator head edit can halt.
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
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#alpha] First done.\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "striking a done head must not halt while a live head remains"
        );
        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_prompts, vec!["do [#beta]".to_string()]);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- ~do [#alpha]~"),
            "done head struck to completed: {updated}"
        );
        assert!(updated.contains("- do [#beta]"));
        assert!(updated.contains("agent:queue auto"));
        assert!(updated.contains("queue_active: true"));
    }

    #[test]
    fn queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt() {
        // SimWorld repro for #adoc-queue-ipc-buffer-divergence (root cause #2):
        // a live route-owned IPC listener owns the document. When an
        // already-active auto-queue's head prompt changes between cycles, queue
        // maintenance halts (item_modified), strips `auto`, and clears
        // `queue_active` on disk + snapshot. Without convergence the live editor
        // buffer would re-add `auto`/`queue_active: true` on its next flush and
        // the snapshot/HEAD drift loop regenerates every preflight. This test
        // proves maintenance pushes a queue-tag + frontmatter convergence message
        // to the listener, and that a follow-up maintenance pass is idempotent
        // (no second divergence, no second convergence send).
        use std::sync::{Arc, Mutex};

        let dir = setup_project();
        let root = dir.path().canonicalize().unwrap();
        let doc = root.join("session.md");

        let received: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();
        let listener_root = root.clone();
        let server = std::thread::spawn(move || {
            crate::ipc_socket::start_listener(&listener_root, move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg) {
                    received_clone.lock().unwrap().push(v);
                }
                Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
            })
            .ok();
        });
        std::thread::sleep(std::time::Duration::from_millis(150));

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
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        // A live editor is actively mid-edit on the head prompt, so the loop
        // must still pause/halt rather than adopt a half-typed head
        // (#queue-no-stall-on-head-edit gates adopt on a settled buffer).
        crate::debounce::document_changed(&doc.to_string_lossy());

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.queue_halted, Some("item_modified".into()));
        assert_eq!(state.queue_active, Some(false));

        // Disk converged to the inactive shape.
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("queue: stop"));

        // Listener received exactly one queue convergence message carrying the
        // queue-tag + frontmatter shape that a content-only patch cannot deliver.
        std::thread::sleep(std::time::Duration::from_millis(100));
        {
            let msgs = received.lock().unwrap();
            let convergences: Vec<&serde_json::Value> = msgs
                .iter()
                .filter(|m| m.get("queue_auto").is_some())
                .collect();
            assert_eq!(
                convergences.len(),
                1,
                "expected exactly one queue convergence message, got: {msgs:?}"
            );
            let conv = convergences[0];
            assert_eq!(conv["queue_auto"], serde_json::json!(false));
            assert_eq!(
                conv["frontmatter"],
                serde_json::json!("queue_active: false")
            );
        }

        // Idempotency: a follow-up maintenance pass on the converged document
        // mutates nothing and sends no further convergence.
        let state2 = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state2.queue_halted, None);
        std::thread::sleep(std::time::Duration::from_millis(100));
        {
            let msgs = received.lock().unwrap();
            let convergences = msgs
                .iter()
                .filter(|m| m.get("queue_auto").is_some())
                .count();
            assert_eq!(
                convergences, 1,
                "follow-up maintenance must not re-diverge / re-send convergence"
            );
        }

        let _ = std::fs::remove_file(crate::ipc_socket::socket_path(&root));
        drop(server);
    }

    #[test]
    fn preflight_pauses_when_active_queue_head_changes_mid_edit() {
        // #queue-no-stall-on-head-edit (pause case): while a live editor is
        // actively mid-edit on the head prompt, the loop must still pause/halt
        // rather than grab a half-typed head. The settled-buffer adopt path is
        // covered separately by
        // `preflight_adopts_edited_queue_head_when_buffer_settled`.
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
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        // Mark the document as actively being typed so the head edit reads as
        // a half-typed buffer.
        crate::debounce::document_changed(&doc.to_string_lossy());

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(false));
        assert_eq!(state.queue_halted.as_deref(), Some("item_modified"));
        assert!(state.queue_prompts.is_empty());

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: stop"));
        assert!(updated.contains("<!-- agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("- do [#newhead]"));
    }

    #[test]
    fn preflight_adopts_edited_queue_head_when_buffer_settled() {
        // #queue-no-stall-on-head-edit (adopt case): when an already-active
        // auto-queue's head prompt changes between cycles and the buffer is
        // settled (no live typing indicator), the loop must adopt the edited
        // head as the new prompt and stay armed — NOT strip `auto` / force
        // queue_active:false. The snapshot must absorb the edited head so
        // closeout queue-consume proves the same prompt and the next cycle sees
        // no spurious item_modified edit.
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
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();
        // No typing indicator written → buffer is settled.

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "settled head edit must adopt + continue, not halt"
        );
        assert_eq!(state.queue_active, Some(true));
        assert_eq!(
            state.queue_prompts,
            vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()],
            "loop continues with the edited head as the new prompt"
        );

        // File keeps the armed auto-queue with the edited head.
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("agent:queue auto"), "auto preserved: {updated}");
        assert!(updated.contains("queue_active: true"), "active preserved: {updated}");
        assert!(updated.contains("- do [#newhead]"));

        // Snapshot absorbed the edited head so a follow-up pass is idempotent
        // (no spurious item_modified on the now-converged head).
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("- do [#newhead]"),
            "snapshot must absorb the adopted head: {snap}"
        );
        assert!(
            !snap.contains("- do [#oldhead]"),
            "snapshot must drop the stale head: {snap}"
        );
        let state2 = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state2.queue_halted, None,
            "converged head must not re-halt on the next pass"
        );
        assert_eq!(state2.queue_active, Some(true));
    }

    #[test]
    fn preflight_collapses_duplicate_live_queue_prompt() {
        // #adoc-queue-ipc-drift: a merge-duplicated live head must converge to a
        // single prompt and persist deduped, so the drift does not grow on each
        // preflight.
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
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#adoc-sqlite-seam]~\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("- do [#adoc-orch-shim-cleanup]").count(),
            1,
            "duplicate live queue prompt must collapse to one:\n{updated}"
        );
        // The remaining single live prompt is still an executable queue head.
        assert_eq!(
            state.queue_prompts,
            vec!["do [#adoc-orch-shim-cleanup]".to_string()],
            "deduped queue exposes exactly one live prompt: {state:?}"
        );
        // Re-running maintenance on the converged doc is a no-op (stable).
        let before = std::fs::read_to_string(&doc).unwrap();
        let _ = run_queue_maintenance(&doc, None).unwrap();
        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            before, after,
            "queue maintenance must be idempotent after dedup"
        );
    }

    #[test]
    fn preflight_does_not_reflag_stable_inactive_queue_as_residue() {
        // #adoc-queue-ipc-drift root cause #1: after an `item_modified` halt the
        // queue goes inactive (queue_active: false, no `auto`) with a retained
        // live tail, and the halt synced that shape into the snapshot. On the
        // NEXT preflight the inactive queue is unchanged from the snapshot, so
        // re-emitting `inactive_queue_residue` every cycle (with no user edit)
        // is pure loop noise and must be suppressed.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        // Snapshot == file: a stable, already-committed inactive queue with a
        // retained tail (the post-halt steady state).
        let content = concat!(
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
            "- ~do [#first-done]~\n",
            "- do [#second-live]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert!(
            !state
                .warnings
                .iter()
                .any(|w| w.code == "inactive_queue_residue"),
            "stable inactive queue (unchanged vs snapshot) must not re-warn residue: {:?}",
            state.warnings
        );
        // The retained tail is preserved, and maintenance is idempotent.
        let before = std::fs::read_to_string(&doc).unwrap();
        assert!(before.contains("- do [#second-live]"));
        let _ = run_queue_maintenance(&doc, None).unwrap();
        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(before, after, "stable inactive queue must not be mutated");
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
        snapshot::save(&doc, snapshot_content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(!updated.contains("preset #spec-test-build-install-commit-push"));
        assert!(!updated.contains("[#crossdocpend]"));
        assert!(!updated.contains("[#spfxnorm]"));
        assert!(updated.contains("queue_active: false"));

        let snap = snapshot::load(&doc).unwrap().unwrap();
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
        snapshot::save(&doc, content).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
            "completed non-auto queue should be cleared without snapshot proof:\n{updated}"
        );
        assert!(!updated.contains("[#item-a]"));
        assert!(!updated.contains("[#item-b]"));
        assert!(updated.contains("queue_active: false"));
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
        snapshot::save(&doc, content).unwrap();

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
        snapshot::save(&doc, snapshot_content).unwrap();

        run(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
            "proven drained non-auto queue should be cleared:\n{updated}"
        );
        assert!(!updated.contains("dispatch #spec-test-build-install-commit-push"));
        assert!(!updated.contains("[#cspe]"));
        assert!(updated.contains("queue_active: false"));

        let snap = snapshot::load(&doc).unwrap().unwrap();
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
        snapshot::save(&doc, baseline).unwrap();

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
    }

    #[test]
    fn preflight_auto_commits_open_write_applied_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nAnswer\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(content),
            Some(content),
        )
        .unwrap();

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
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
        snapshot::save(&doc, original).unwrap();
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
        snapshot::save(&doc, patched).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(patched),
            Some(patched),
        )
        .unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(patched), Some(patched))
            .unwrap();
        let pre_state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(pre_state.phase, crate::cycle_state::CyclePhase::Committed);
        assert!(matches!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ));
        assert!(
            crate::session_check::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
            "preconditions: cancel pattern should be detected before recovery"
        );

        run(&doc).unwrap();

        assert!(matches!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::Committed
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

    /// #drained-done-queue-clear: a standalone no-diff preflight that drains a
    /// fully-resolved auto-queue writes the drained shape to disk + snapshot
    /// but leaves HEAD on the active-queue commit. The next preflight commit
    /// step must self-heal that pure queue-maintenance drift via the route
    /// queue commit-boundary recovery instead of stranding it for manual
    /// `agent-doc commit`. The drained snapshot has no active prompts, so this
    /// shape recovers only because HEAD proves the prior active auto-queue and
    /// nothing but queue state differs.
    #[test]
    fn route_queue_commit_boundary_recovers_drained_queue_snapshot() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let active = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, active).unwrap();
        snapshot::save(&doc, active).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "active queue", "--no-verify"])
            .output()
            .unwrap();

        // Standalone maintenance drained the queue: queue_active cleared, auto
        // stripped, body emptied — on disk and in the snapshot — but HEAD still
        // carries the active auto-queue.
        let drained = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, drained).unwrap();
        snapshot::save(&doc, drained).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(active), Some(active))
            .unwrap();

        assert!(matches!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ));
        let rc = crate::graph::RunContext::new(doc.clone());
        assert!(
            detect_route_queue_snapshot_commit_boundary_recoverable(&doc, &rc).unwrap(),
            "drained-queue maintenance drift must be recoverable"
        );

        assert!(recover_route_queue_snapshot_commit_boundary(&doc, &rc).unwrap());
        assert!(
            matches!(
                crate::git::verify_snapshot_committed(&doc).unwrap(),
                crate::git::SnapshotCommitStatus::Committed
            ),
            "drained queue must be committed after recovery"
        );
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        let head = String::from_utf8_lossy(&show.stdout);
        assert!(head.contains("queue_active: false"), "HEAD: {head}");
        assert!(!head.contains("agent:queue auto"), "HEAD: {head}");
    }

    /// #drained-done-queue-clear guard: the route queue commit-boundary
    /// recovery must NOT fire when a real user edit rides alongside the queue
    /// drain. Only pure queue-state churn is auto-committable.
    #[test]
    fn route_queue_commit_boundary_skips_drained_queue_with_user_edit() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let active = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, active).unwrap();
        snapshot::save(&doc, active).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "active queue", "--no-verify"])
            .output()
            .unwrap();

        // Drained queue PLUS an unrelated exchange edit — must not auto-commit.
        let drained_plus_edit = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n\nAn extra user line.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, drained_plus_edit).unwrap();
        snapshot::save(&doc, drained_plus_edit).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(active), Some(active))
            .unwrap();

        let rc = crate::graph::RunContext::new(doc.clone());
        assert!(
            !detect_route_queue_snapshot_commit_boundary_recoverable(&doc, &rc).unwrap(),
            "a user edit alongside the drain must block auto-commit"
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
        snapshot::save(&doc, original).unwrap();
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
        snapshot::save(&doc, patched).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(patched),
            Some(patched),
        )
        .unwrap();

        let pre_state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            pre_state.phase,
            crate::cycle_state::CyclePhase::WriteApplied
        );
        assert!(matches!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ));
        assert!(
            crate::session_check::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
            "preconditions: write_applied cancel pattern should be detected before recovery"
        );

        run(&doc).unwrap();

        assert!(matches!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::Committed
        ));
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
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
        snapshot::save(&doc, original).unwrap();
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
        crate::repair::save_pending(&doc, response).unwrap();
        let capture = crate::capture::load_active(&doc).unwrap().unwrap();
        let pending_path = snapshot::pending_path_for(&doc).unwrap();
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
        snapshot::save(&doc, &materialized).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some(&materialized),
            Some(&materialized),
        )
        .unwrap();

        assert!(
            crate::session_check::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
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
            crate::session_check::inspect(&doc).unwrap(),
            crate::session_check::SessionCheckStatus::Ok(_)
        ));

        let refreshed = crate::capture::load_by_id(&doc, &capture.capture_id)
            .unwrap()
            .unwrap();
        let snapshot_content = snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(refreshed.state, crate::capture::CaptureState::Committed);
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(crate::ops_log::content_hash(&content).as_str()),
            "capture file hash should refresh to the recovered visible file"
        );
        assert_eq!(
            refreshed.snapshot_hash.as_deref(),
            Some(crate::ops_log::content_hash(&snapshot_content).as_str()),
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
        snapshot::save(&doc, committed).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
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
            crate::session_check::detect_jb_cache_conflict_accept_duplicate_replay(&doc)
                .unwrap()
                .is_some(),
            "preconditions: accepted-conflict duplicate replay should be detected"
        );

        run(&doc).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), committed);
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
        snapshot::save(&doc, committed).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
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
            crate::session_check::detect_jb_cache_conflict_accept_duplicate_replay(&doc)
                .unwrap()
                .is_none(),
            "preconditions: non-adjacent duplicate is missed by the consecutive replay detector"
        );
        assert!(
            crate::session_check::detect_late_ipc_response_overapplication(&doc)
                .unwrap()
                .is_some(),
            "preconditions: late-IPC over-application should be detected"
        );

        run(&doc).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), committed);
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
        snapshot::save(&doc, committed).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
        crate::cycle_state::mark_committed(
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
            !crate::dedupe::is_committed_response_overapplication(&replayed, committed),
            "preconditions: strict over-application must NOT match a drifted-body replay"
        );
        assert!(
            crate::session_check::detect_late_ipc_response_overapplication(&doc)
                .unwrap()
                .is_some(),
            "the stale-replay fallback should detect the over-application"
        );

        run(&doc).unwrap();

        assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
        assert_eq!(snapshot::load(&doc).unwrap().unwrap(), committed);
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
        snapshot::save(&doc, original).unwrap();
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
        let capture = crate::capture::capture_response(&doc, response).unwrap();

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
        snapshot::save(&doc, &current).unwrap();
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

        let refreshed = crate::capture::load_by_id(&doc, &capture.capture_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            refreshed.file_hash.as_deref(),
            Some(crate::ops_log::content_hash(&current).as_str()),
            "preflight should refresh the capture file hash before replay"
        );
        assert_eq!(
            refreshed.snapshot_hash.as_deref(),
            Some(crate::ops_log::content_hash(&current).as_str()),
            "preflight should refresh the capture snapshot hash before replay"
        );
        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_baseline_refreshed_for_benign_drift"),
            "preflight must drive validate_replay's baseline refresh path:\n{log}"
        );
    }

    #[test]
    fn preflight_resumes_commit_when_write_landed_without_open_cycle_state() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();
        snapshot::save(&doc, original).unwrap();
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

        let patched =
            "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nRecovered answer\n";
        std::fs::write(&doc, patched).unwrap();
        snapshot::save(&doc, patched).unwrap();
        let ops = root.join(".agent-doc/logs/ops.log");
        std::fs::write(
            &ops,
            format!(
                "[100] snapshot_saved_file_ipc file={} snap_len={}\n",
                doc.display(),
                patched.len()
            ),
        )
        .unwrap();

        let (recovered, committed) = enforce_cycle_completion(&doc).unwrap();
        assert!(
            !recovered,
            "no replay should be needed when file already has the response"
        );
        assert!(
            committed,
            "commit boundary should resume and create a commit"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed_doc = String::from_utf8_lossy(&show.stdout);
        assert!(
            committed_doc.contains("Recovered answer"),
            "HEAD should include the resumed response closeout:\n{committed_doc}"
        );

        let log = std::fs::read_to_string(ops).unwrap();
        assert!(
            log.contains("resume_commit_success file="),
            "resume commit success should be logged:\n{log}"
        );
    }

    #[test]
    fn pending_maintenance_reaps_completed_items_from_file_and_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#reap1] Reap me\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let report = run_pending_maintenance(&doc).unwrap();
        assert!(!report.reordered);
        assert_eq!(report.pending_gated_count, 0);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_backlog_after = crate::component::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_backlog_after.contains("[#reap1]"));
        assert!(file_after.contains("[#keep1]"));
        assert!(file_after.contains("## Completed / Reaped"));
        assert!(file_after.contains("<!-- agent:done -->"));
        assert!(file_after.contains("[#reap1] Reap me"));

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        let snapshot_backlog_after = crate::component::parse(&snapshot_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_backlog_after.contains("[#reap1]"));
        assert!(snapshot_after.contains("[#keep1]"));
        assert!(snapshot_after.contains("## Completed / Reaped"));
        assert!(snapshot_after.contains("<!-- agent:done -->"));
        assert!(snapshot_after.contains("[#reap1] Reap me"));
    }

    #[test]
    fn archive_pending_done_inserts_canonical_done_component() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&file, content).unwrap();
        let archived = archive_pending_done(
            &file,
            content,
            &[crate::pending::PendingItem {
                marker: crate::pending::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: crate::pending::PendingState::Done,
                gate_type: None,
                text: "completed item".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        assert!(archived.contains("<!-- agent:done -->"));
        assert!(archived.contains("<!-- /agent:done -->"));
        assert!(!archived.contains("<!-- agent:backlog-done -->"));
        assert!(!archived.contains("<!-- agent:pending-done -->"));
        assert!(archived.contains("[#done1] completed item"));
    }

    #[test]
    fn archive_pending_done_ignores_removed_pending_done_alias() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        std::fs::write(&file, content).unwrap();
        let archived = archive_pending_done(
            &file,
            content,
            &[crate::pending::PendingItem {
                marker: crate::pending::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: crate::pending::PendingState::Done,
                gate_type: None,
                text: "completed item".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        assert!(archived.contains("<!-- agent:pending-done -->"));
        assert!(archived.contains("<!-- agent:done -->"));
        assert!(!archived.contains("<!-- agent:backlog-done -->"));
        assert!(archived.contains("[#done1] completed item"));
    }

    #[test]
    fn archive_pending_done_appends_to_external_done_archive() {
        let dir = setup_project();
        let file = dir.path().join("tasks/session.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks/session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&file, content).unwrap();

        let archived = archive_pending_done(
            &file,
            content,
            &[crate::pending::PendingItem {
                marker: crate::pending::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: crate::pending::PendingState::Done,
                gate_type: None,
                text: "completed externally".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        let external = std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
        assert!(external.contains("[#done1] completed externally"));
        assert!(!archived.contains("[#done1]"));
        assert!(archived.contains("completed work archived in tasks/session.done.md"));

        archive_pending_done(
            &file,
            &archived,
            &[crate::pending::PendingItem {
                marker: crate::pending::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: crate::pending::PendingState::Done,
                gate_type: None,
                text: "completed externally".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();
        let external_after =
            std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
        assert_eq!(external_after.matches("[#done1]").count(), 1);
    }

    #[test]
    fn archive_pending_done_rejects_invalid_external_archive_paths() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let item = crate::pending::PendingItem {
            marker: crate::pending::PendingListMarker::Bullet,
            id: "done1".to_string(),
            state: crate::pending::PendingState::Done,
            gate_type: None,
            text: "completed item".to_string(),
            continuation: String::new(),
        };
        for archive_path in [
            "/tmp/session.done.md",
            "../session.done.md",
            "tasks/session.md",
        ] {
            let content = format!(
                "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n<!-- agent:done archive={} -->\n<!-- /agent:done -->\n",
                archive_path
            );
            std::fs::write(&file, &content).unwrap();
            let err = archive_pending_done(&file, &content, std::slice::from_ref(&item))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("agent:done archive="),
                "unexpected error for {archive_path}: {err}"
            );
        }
    }

    #[test]
    fn external_done_archive_ids_satisfy_dropped_history_guard() {
        let dir = setup_project();
        let file = dir.path().join("tasks/session.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            dir.path().join("tasks/session.done.md"),
            "# Agent Doc Completed Work\n\n- 2026-05-13 [#item1] Was open\n",
        )
        .unwrap();
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks/session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&file, current).unwrap();

        let external_ids = external_done_archive_ids(&file, current).unwrap();
        let report = crate::pending::detect_dropped_from_history_with_extra_current_ids(
            current,
            baseline,
            &HashSet::new(),
            &external_ids,
        )
        .unwrap();

        assert!(report.dropped.is_empty());
    }

    #[test]
    fn preflight_allows_user_marked_done_item_reaped_in_same_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [/] [#done1] Waiting on manual validation\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, baseline).unwrap();
        snapshot::save(&doc, baseline).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "baseline", "--no-verify"])
            .output()
            .unwrap();

        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Waiting on manual validation\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(baseline), Some(current)).unwrap();

        let report = run_pending_maintenance(&doc).unwrap();
        assert!(!report.reordered);
        assert_eq!(report.pending_gated_count, 0);
        let rc = crate::graph::RunContext::new(doc.clone());
        enforce_no_dropped_backlog(&doc, &rc)
            .expect("same-cycle reap should count as intentional completion");
    }

    #[test]
    fn pending_maintenance_reaps_completed_icebox_items_from_file_and_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Icebox\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] [#ice01] Reap me from icebox\n",
            "- [ ] [#keep2] Keep me parked\n",
            "<!-- /agent:icebox -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let report = run_pending_maintenance(&doc).unwrap();
        assert!(!report.reordered);
        assert_eq!(report.pending_gated_count, 0);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_icebox_after = crate::component::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "icebox")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_icebox_after.contains("[#ice01]"));
        assert!(file_after.contains("[#keep2]"));
        assert!(file_after.contains("## Completed / Reaped"));
        assert!(file_after.contains("[#ice01] Reap me from icebox"));

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        let snapshot_icebox_after = crate::component::parse(&snapshot_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "icebox")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_icebox_after.contains("[#ice01]"));
        assert!(snapshot_after.contains("[#keep2]"));
        assert!(snapshot_after.contains("## Completed / Reaped"));
        assert!(snapshot_after.contains("[#ice01] Reap me from icebox"));
    }

    #[test]
    fn pending_maintenance_syncs_snapshot_for_write_phase_gate_without_reap() {
        // #pending-gate-snapshot-desync: the write phase moved #g1 from backlog
        // to review (a --pending-gate) on the FILE, but the content_ours snapshot
        // still shows #g1 in backlog and an empty review. Maintenance makes no
        // reap/backfill change, yet it must re-sync the snapshot's tracked
        // surfaces to the file so the upcoming commit stages the gate instead of
        // stranding it as post-commit drift.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#g1] Gated, awaiting review\n",
            "<!-- /agent:review -->\n"
        );
        // Snapshot lags the file: #g1 still in backlog, review empty (the
        // baseline+response content_ours saved before the gate mutation).
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "- [ ] [#g1] Gated, awaiting review\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, file_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        run_pending_maintenance(&doc).unwrap();

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        let comps = crate::component::parse(&snapshot_after).unwrap();
        let snap_backlog = comps
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        let snap_review = comps
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        // Snapshot now matches the file: #g1 gated into review, gone from backlog.
        assert!(
            !snap_backlog.contains("[#g1]"),
            "snapshot backlog must drop the gated item: {snap_backlog}"
        );
        assert!(
            snap_review.contains("[/] [#g1]"),
            "snapshot review must carry the gated item: {snap_review}"
        );
        assert!(snap_backlog.contains("[#keep1]"));
    }

    #[test]
    fn pending_maintenance_fails_closed_when_snapshot_backlog_cannot_be_synced() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#reap1] Reap me\n",
            "<!-- /agent:backlog -->\n"
        );
        let snapshot_content =
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\nNo backlog here.\n";
        std::fs::write(&doc, file_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let err = run_pending_maintenance(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("snapshot is missing the backlog component")
        );
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
        snapshot::save(&doc, content).unwrap();

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
        snapshot::save(&doc, content).unwrap();

        run(&doc).unwrap();
    }

    #[test]
    fn preflight_closes_response_captured_cycle_when_snapshot_already_matches_head() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let committed = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, committed).unwrap();
        snapshot::save(&doc, committed).unwrap();
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

        let visible_snapshot = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        snapshot::save(&doc, visible_snapshot).unwrap();

        let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
        std::fs::write(&doc, &with_user_edit).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(visible_snapshot), Some(&with_user_edit))
            .unwrap();
        crate::cycle_state::mark_response_captured(
            &doc,
            "response_captured",
            Some(visible_snapshot),
            Some(&with_user_edit),
            "sha256",
            None,
        )
        .unwrap();

        let (recovered, committed) = enforce_cycle_completion(&doc).unwrap();
        assert!(
            recovered,
            "the missing commit boundary should be recovered from already-committed HEAD"
        );
        assert!(
            !committed,
            "HEAD-current closeout should not create a duplicate git commit"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_already_current file="),
            "preflight should record the no-op closeout instead of failing:\n{log}"
        );
        assert!(
            !log.contains("commit_failed"),
            "preflight should not log a false commit_failed for HEAD-current closeout:\n{log}"
        );
    }

    #[test]
    fn preflight_reruns_cleanly_after_open_preflight_started_cycle() {
        let dir = setup_project();
        let root = dir.path();
        let doc = dir.path().join("session.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::git::commit(&doc).unwrap();
        let prior =
            crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello again\n").unwrap();

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
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
        crate::snapshot::save(&doc, snapshot).unwrap();
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
        let prior =
            crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

        let live = snapshot.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#root-empty-preflight]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, crate::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
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
        crate::snapshot::save(&doc, snapshot).unwrap();
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
        let prior =
            crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

        let prompt = "Left/Right buttons still do not work with agent-doc opencode. #next-steps";
        let live = snapshot.replace(
            "<!-- agent:boundary:abc123 -->\n",
            &format!("{prompt}\n<!-- agent:boundary:abc123 -->\n"),
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, crate::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
        assert_ne!(
            state.cycle_id, prior.cycle_id,
            "preflight should abandon the stale empty cycle and open a fresh cycle for the prompt"
        );
        assert!(
            state.requires_backlog_capture,
            "the inline #next-steps prompt should still require backlog capture"
        );
        let diff = crate::diff::compute(&doc).unwrap().unwrap();
        let prompt_targets = crate::diff::classify_prompt_bearing_changes(&diff)
            .into_iter()
            .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
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
        crate::snapshot::save(&doc, snapshot).unwrap();
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

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
            "compact follow-up should open a response cycle instead of becoming no_changes"
        );
        assert!(
            state.requires_backlog_capture,
            "compact follow-up #next-steps should carry the backlog-capture contract"
        );
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
        crate::snapshot::save(&doc, head).unwrap();
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

        crate::snapshot::save(&doc, &queued).unwrap();
        std::fs::write(&doc, &live).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(&queued), Some(&queued))
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
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snapshot_after.contains(original_prompt),
            "snapshot should stay on the route queued prompt:\n{snapshot_after}"
        );
        assert!(
            !snapshot_after.contains("Same with this file"),
            "preflight must not absorb the live edit into the committed snapshot:\n{snapshot_after}"
        );
        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
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
        snapshot::save(&doc, snapshot).unwrap();
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

        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(live)).unwrap();

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

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
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
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

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
            message.contains(crate::repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR),
            "expected fail-closed ambiguous patchback error, got: {message}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
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
        crate::snapshot::save(&doc, snapshot).unwrap();
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
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(live)).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(crate::repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR),
            "expected uncommitted response patchback error, got: {message}"
        );

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
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
        snapshot::save(&doc, snapshot).unwrap();
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

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
            "preflight should still open a response cycle for the live prompt"
        );

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(file_after.contains("do #statusws. spec-test-build-install-commit-push"));
        assert!(!file_after.contains("- [x] [#scopeid] completed item"));

        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        snapshot::save(&doc, snapshot).unwrap();
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
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
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

        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        snapshot::save(&doc, snapshot).unwrap();
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
            "- [ ] keep me\n",
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
    fn preflight_warns_on_prompt_preset_text_inside_post_exchange_html_comment() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "Scratch note while testing.\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let (fm, _) = crate::frontmatter::parse(content).unwrap();
        let warning = post_exchange_comment_prompt_preset_warning(
            Path::new("session.md"),
            content,
            &fm.prompt_presets,
        )
        .expect("known prompt preset in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(
            warning
                .message
                .contains("#spec-test-build-install-commit-push")
        );
        assert!(warning.message.contains("non-executable user note"));
    }

    #[test]
    fn preflight_comment_prompt_preset_warning_ignores_agent_components() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:done -->\n",
            "<!-- archived #spec-test-build-install-commit-push -->\n",
            "<!-- /agent:done -->\n",
        );
        let (fm, _) = crate::frontmatter::parse(content).unwrap();

        assert!(
            post_exchange_comment_prompt_preset_warning(
                Path::new("session.md"),
                content,
                &fm.prompt_presets,
            )
            .is_none(),
            "agent-owned queue directives remain executable state, not ordinary scratch comments"
        );
    }

    #[test]
    fn misplaced_component_attr_warning_flags_auto_on_backlog() {
        // #backlog-auto-marker-misfire: `auto` is a queue-only attribute; on the
        // backlog it must be surfaced (no longer silently tolerated).
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog auto -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
            .expect("`auto` on agent:backlog should warn");
        assert_eq!(warning.code, "misplaced_component_attr");
        assert!(warning.message.contains("queue-only attribute"));
        assert!(warning.message.contains("agent:backlog"));
        assert!(warning.message.contains("agent:queue auto"));
        assert!(warning.message.contains("no mutation"));
    }

    #[test]
    fn misplaced_component_attr_warning_flags_unknown_attr_typo() {
        // The reported trigger was the typo `auot`; an unrecognized key must warn.
        let content = concat!(
            "<!-- agent:backlog auot -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
            .expect("typo'd attribute on a component should warn");
        assert_eq!(warning.code, "misplaced_component_attr");
        assert!(
            warning
                .message
                .contains("not a recognized component attribute")
        );
        assert!(warning.message.contains("auot"));
    }

    #[test]
    fn misplaced_component_attr_warning_allows_queue_sync_attr_on_backlog() {
        // #backlog-queue-sync-attr: `queue`, `queue=sync|append|prepend` on the
        // backlog/icebox are recognized sync attributes and must not warn.
        for marker in [
            "<!-- agent:backlog queue -->",
            "<!-- agent:backlog queue=sync -->",
            "<!-- agent:backlog queue=append -->",
            "<!-- agent:icebox queue=prepend -->",
        ] {
            let content = format!(
                "{marker}\n- [ ] [#x1] keep this\n<!-- /agent:{}-->\n",
                if marker.contains("icebox") {
                    "icebox "
                } else {
                    "backlog "
                }
            );
            assert!(
                misplaced_component_attr_warning(Path::new("session.md"), &content).is_none(),
                "recognized queue sync attr must not warn: {marker}"
            );
        }
    }

    #[test]
    fn misplaced_component_attr_warning_allows_priority_attr() {
        // #backlog-priority-attribute: bare `priority` on backlog/icebox/queue
        // is a recognized ordering attribute and must not warn.
        for content in [
            "<!-- agent:backlog priority -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n",
            "<!-- agent:backlog priority queue -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n",
            "<!-- agent:queue priority -->\n- do [#a]\n<!-- /agent:queue -->\n",
        ] {
            assert!(
                misplaced_component_attr_warning(Path::new("session.md"), content).is_none(),
                "priority attr must not warn: {content}"
            );
        }
    }

    #[test]
    fn run_pending_maintenance_sorts_backlog_by_priority() {
        // #backlog-priority-attribute: a backlog carrying `priority` stable-sorts
        // items by their per-item priority token each cycle.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#low] priority=5 later\n",
            "- [ ] [#high] priority=1 first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_pending_maintenance(&doc).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let high = updated.find("[#high]").unwrap();
        let low = updated.find("[#low]").unwrap();
        assert!(
            high < low,
            "priority=1 item must sort before priority=5:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_orders_synced_queue_by_priority() {
        // #backlog-priority-attribute + #backlog-queue-sync-attr: a priority queue
        // synced from a priority backlog comes out prioritized.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue priority -->\n",
            "- do [#low]\n",
            "- do [#high]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#low] priority=5 later\n",
            "- [ ] [#high] priority=1 first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let q = updated.find("<!-- agent:queue").unwrap();
        let qend = updated[q..].find("<!-- /agent:queue").unwrap() + q;
        let queue_region = &updated[q..qend];
        let high = queue_region.find("do [#high]").unwrap();
        let low = queue_region.find("do [#low]").unwrap();
        assert!(
            high < low,
            "priority=1 must sort before priority=5 in queue:\n{queue_region}"
        );
    }

    #[test]
    fn misplaced_component_attr_warning_flags_invalid_queue_mode() {
        let content = concat!(
            "<!-- agent:backlog queue=nope -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
            .expect("unrecognized queue mode should warn");
        assert_eq!(warning.code, "misplaced_component_attr");
        assert!(warning.message.contains("not a recognized sync mode"));
        assert!(warning.message.contains("queue=nope"));
    }

    #[test]
    fn collect_backlog_queue_sync_reads_mode_and_active_ids() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#a] one\n",
            "- [/] [#g] gated\n",
            "- [ ] [#b] two\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = crate::component::parse(content).unwrap();
        let (mode, ids) = collect_backlog_queue_sync(&components, content)
            .expect("backlog with queue attr should produce a sync request");
        assert_eq!(mode, crate::queue::BacklogQueueSyncMode::Sync);
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn filter_expect_done_or_gate_excludes_synced_queue_ids() {
        // #queue-sync-auto-pending-done-guard-misfire: a cycle that works one
        // directive (#worked) while the backlog→queue sync auto-populated
        // do[#a]/do[#b]/#worked into the active queue must demand only the
        // genuine worked directive, never the freshly-synced siblings.
        let directive_ids = vec!["worked".to_string(), "a".to_string(), "b".to_string()];
        let open_backlog: std::collections::HashSet<String> =
            ["worked", "a", "b"].iter().map(|s| s.to_string()).collect();
        let synced_queue_ids: std::collections::HashSet<String> =
            ["a", "b"].iter().map(|s| s.to_string()).collect();
        let result =
            filter_expect_done_or_gate_ids(&directive_ids, &open_backlog, &synced_queue_ids);
        assert_eq!(result, vec!["worked".to_string()]);
    }

    #[test]
    fn filter_expect_done_or_gate_keeps_open_directives_without_sync() {
        // No sync attribute → no exclusion; an open user directive stays demanded,
        // a directive whose backlog item is already resolved drops out, and
        // duplicates collapse.
        let directive_ids = vec![
            "open".to_string(),
            "open".to_string(),
            "resolved".to_string(),
        ];
        let open_backlog: std::collections::HashSet<String> =
            ["open"].iter().map(|s| s.to_string()).collect();
        let synced_queue_ids = std::collections::HashSet::new();
        let result =
            filter_expect_done_or_gate_ids(&directive_ids, &open_backlog, &synced_queue_ids);
        assert_eq!(result, vec!["open".to_string()]);
    }

    #[test]
    fn collect_backlog_queue_sync_none_without_attr() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] one\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = crate::component::parse(content).unwrap();
        assert!(collect_backlog_queue_sync(&components, content).is_none());
    }

    #[test]
    fn misplaced_component_attr_warning_allows_queue_auto_and_known_attrs() {
        // `auto` on the queue and known attrs elsewhere must not warn.
        let content = concat!(
            "<!-- agent:queue auto -->\n",
            "- do #fix1\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange patch=append max_lines=50 -->\n",
            "### Re: prior — gpt-5\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks/x.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        assert!(
            misplaced_component_attr_warning(Path::new("session.md"), content).is_none(),
            "`auto` on queue plus recognized attrs elsewhere must not warn"
        );
    }

    #[test]
    fn misplaced_component_attr_warning_allows_queue_control_markers() {
        // `start` / `go` / `stop` are recognized queue-only control markers
        // (#queue-state-unify) — preflight migrates them into `queue:` frontmatter.
        for token in ["start", "go", "stop"] {
            let content = format!(
                "<!-- agent:queue preset=\"#p\" {token} -->\n- do #fix1\n<!-- /agent:queue -->\n",
            );
            assert!(
                misplaced_component_attr_warning(Path::new("session.md"), &content).is_none(),
                "`{token}` on queue must be a recognized control marker, not a typo warning"
            );
        }
    }

    #[test]
    fn misplaced_component_attr_warning_allows_preset_on_queue() {
        let content = concat!(
            "<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" -->\n",
            "- do #fix1\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(
            misplaced_component_attr_warning(Path::new("session.md"), content).is_none(),
            "`preset` on queue is a recognized queue-only attribute"
        );
    }

    #[test]
    fn misplaced_component_attr_warning_flags_preset_on_non_queue() {
        let content = concat!(
            "<!-- agent:backlog preset=\"#spec-test-build-install-commit-push\" -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning =
            misplaced_component_attr_warning(Path::new("session.md"), content).expect(
                "`preset` on backlog should warn as a queue-only attribute on wrong component",
            );
        assert_eq!(warning.code, "misplaced_component_attr");
        assert!(
            warning.message.contains("queue-only"),
            "warning should mention queue-only: {}",
            warning.message
        );
    }

    #[test]
    fn auto_on_backlog_does_not_activate_queue() {
        // #backlog-auto-marker-misfire regression: the auto-loop reads `auto`
        // only from the queue component, never from the backlog.
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- do #fix1\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog auto -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = crate::component::parse(content).unwrap();
        let queue = components.iter().find(|c| c.name == "queue").unwrap();
        assert!(
            !crate::queue::has_auto_attr(&queue.attrs),
            "queue has no auto attribute"
        );
        let backlog = components.iter().find(|c| c.name == "backlog").unwrap();
        assert!(
            crate::queue::has_auto_attr(&backlog.attrs),
            "backlog carries the misplaced auto attribute"
        );
        let body = &content[queue.open_end..queue.close_start];
        let entries = crate::queue::parse(body).unwrap();
        // Activation is driven solely by the queue component's auto flag.
        let activation = crate::queue::resolve_activation(&entries, false, false, false);
        assert!(
            !activation.active,
            "backlog `auto` must never activate the auto-loop"
        );
    }

    #[test]
    fn preflight_warns_on_dispatch_text_inside_post_exchange_html_comment_without_presets() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "dispatch #manual-review\n",
            "/clear\n",
            "-->\n",
        );
        let (fm, _) = crate::frontmatter::parse(content).unwrap();
        let warning = post_exchange_comment_prompt_preset_warning(
            Path::new("session.md"),
            content,
            &fm.prompt_presets,
        )
        .expect("dispatch-looking text in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(warning.message.contains("dispatch #manual-review"));
        assert!(warning.message.contains("/clear"));
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
        let (fm, _) = crate::frontmatter::parse(content).unwrap();
        let warning = post_exchange_comment_prompt_preset_warning(
            Path::new("session.md"),
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
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        snapshot::save(&doc, snapshot).unwrap();
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
                "- [ ] keep me\n",
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
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
        assert!(
            !snapshot_after.contains(prompt),
            "snapshot must not absorb the live prompt during preflight:\n{snapshot_after}"
        );
    }

    #[test]
    fn preflight_preserves_duplicate_prompt_comment_from_snapshot() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let snapshot = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
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
        std::fs::write(&doc, &snapshot).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
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

        let rc = crate::graph::RunContext::new(doc.clone());
        let changed =
            remove_post_exchange_duplicate_prompt_comments_for_preflight(&doc, &rc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !changed,
            "preflight cleanup should not rewrite baseline-owned scratch comments"
        );
        assert!(
            file_after.contains(&format!("<!--\n{prompt}\n-->")),
            "preflight must not scrub post-exchange scratch text that already existed in HEAD:\n{file_after}"
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
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        snapshot::save(&doc, snapshot).unwrap();
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
                "- [ ] keep me\n",
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
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        std::fs::write(&doc, &snapshot).unwrap();
        snapshot::save(&doc, &snapshot).unwrap();
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
        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        snapshot::save(&doc, snapshot).unwrap();
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
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        std::fs::write(&doc, &live).unwrap();

        let doc_for_thread = doc.clone();
        let doc_str = doc.to_string_lossy().to_string();
        crate::debounce::document_changed(&doc_str);
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
        snapshot::save(&doc, snapshot).unwrap();
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
                crate::ops_log::CycleEntry {
                    timestamp: now.saturating_sub(10).to_string(),
                    file: relative.clone(),
                    op: "commit_noop".to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                crate::ops_log::CycleEntry {
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

        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
            "  cargo test -p agent-doc pending::\n",
            "Diff:\n",
            "@@ -1 +1 @@\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, snapshot).unwrap();
        snapshot::save(&doc, snapshot).unwrap();
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
            "  cargo test -p agent-doc pending::\n",
            "Diff:\n",
            "@@ -1 +1 @@\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, live).unwrap();

        run(&doc).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = crate::component::parse(&file_after).unwrap();
        let backlog_after = backlog_after
            .iter()
            .find(|component| crate::component::is_backlog_component(&component.name))
            .map(|component| component.content(&file_after))
            .unwrap();
        assert!(file_after.contains("do #statusws. spec-test-build-install-commit-push"));
        assert!(!backlog_after.contains("- [x] [#scopeid] completed item"));
        assert!(!backlog_after.contains("Commands:"));
        assert!(!backlog_after.contains("@@ -1 +1 @@"));

        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
        let snapshot_backlog = crate::component::parse(&snapshot_after).unwrap();
        let snapshot_backlog = snapshot_backlog
            .iter()
            .find(|component| crate::component::is_backlog_component(&component.name))
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
        snapshot::save(&doc, snapshot).unwrap();
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

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(
            state.phase,
            crate::cycle_state::CyclePhase::PreflightStarted,
            "preflight should still open a response cycle for the prompt-preset status edit"
        );

        let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
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
        snapshot::save(&doc, tracked).unwrap();
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

        let state = crate::cycle_state::load(&doc).unwrap();
        assert!(
            state.as_ref().is_none_or(|state| !state.is_open()),
            "boundary-artifact-only preflight must not leave an open cycle"
        );

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("preflight_diff_start file="),
            "boundary-artifact-only diff must not log preflight_diff_start:\n{log}"
        );
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(_) => {}
            status => panic!(
                "expected clean closeout after boundary-artifact-only preflight, got {status:?}"
            ),
        }
    }

    #[test]
    fn preflight_recovers_response_captured_cycle_without_pending_file() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::repair::save_pending(&doc, "Recovered answer.").unwrap();
        let pending = snapshot::pending_path_for(&doc).unwrap();
        std::fs::remove_file(&pending).unwrap();

        run(&doc).unwrap();

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Recovered answer."));
    }

    #[test]
    fn preflight_claims_read_and_truncated() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();
        snapshot::save(&doc, "# Doc\n").unwrap();

        // Write a claims log.
        let log_path = dir.path().join(".agent-doc/claims.log");
        std::fs::write(&log_path, "claim A\nclaim B\n").unwrap();

        let claims = read_and_truncate_claims(&doc);
        assert_eq!(claims, vec!["claim A", "claim B"]);

        // Log should be truncated.
        let after = std::fs::read_to_string(&log_path).unwrap();
        assert!(after.is_empty(), "claims log should be empty after read");
    }

    #[test]
    fn preflight_no_claims_log_returns_empty() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();

        // No claims.log exists.
        let claims = read_and_truncate_claims(&doc);
        assert!(claims.is_empty());
    }

    #[test]
    fn preflight_output_serializes_correctly() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec!["foo".to_string()],
            diff: Some("+new line\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["recovered"], false);
        assert_eq!(parsed["committed"], true);
        assert_eq!(parsed["claims"][0], "foo");
        assert_eq!(parsed["no_changes"], false);
        assert!(parsed["diff"].as_str().is_some());
        assert!(
            parsed.get("document").is_none(),
            "document field must be absent"
        );
    }

    #[test]
    fn preflight_output_includes_orchestration_request() {
        let output = PreflightOutput {
            no_changes: false,
            orchestration_request: Some(crate::diff::OrchestrationRequest {
                mode: crate::diff::OrchestrationRequestMode::Sequential,
                trigger_text: "Synchronous orcestra.".to_string(),
                task_count: 5,
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["orchestration_request"]["mode"], "sequential");
        assert_eq!(parsed["orchestration_request"]["task_count"], 5);
        assert_eq!(
            parsed["orchestration_request"]["trigger_text"],
            "Synchronous orcestra."
        );
    }

    #[test]
    fn preflight_output_omits_orchestration_request_when_absent() {
        let output = PreflightOutput {
            no_changes: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("orchestration_request").is_none(),
            "orchestration_request should be omitted when absent"
        );
    }

    #[test]
    fn preflight_output_includes_prompt_presets_requested() {
        let output = PreflightOutput {
            no_changes: false,
            prompt_presets_requested: vec!["#1".to_string(), "release-check".to_string()],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["prompt_presets_requested"][0], "#1");
        assert_eq!(parsed["prompt_presets_requested"][1], "release-check");
    }

    #[test]
    fn preflight_output_omits_prompt_presets_requested_when_empty() {
        let output = PreflightOutput {
            no_changes: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("prompt_presets_requested").is_none(),
            "prompt_presets_requested should be omitted when empty"
        );
    }

    #[test]
    fn harness_mismatch_warning_normalizes_aliases() {
        assert!(
            harness_mismatch_warning(Some("claude"), "claude-code").is_none(),
            "claude and claude-code are the same canonical harness"
        );
        let warning = harness_mismatch_warning(Some("codex"), "claude-code").unwrap();
        assert_eq!(warning.code, "harness_mismatch");
        assert_eq!(warning.document_agent.as_deref(), Some("codex"));
        assert_eq!(warning.active_harness.as_deref(), Some("claude-code"));
        assert!(warning.message.contains("Document declares agent: codex"));
    }

    #[test]
    fn harness_mismatch_warning_skips_unknown_active_harness() {
        assert!(harness_mismatch_warning(Some("codex"), "default").is_none());
        assert!(harness_mismatch_warning(None, "claude-code").is_none());
    }

    #[test]
    fn codex_network_access_warning_for_non_codex_harness() {
        let content = "---\nagent_doc_session: test\nagent: opencode\ncodex_network_access: enabled\n---\n\ntest\n";
        let (fm, _) = crate::frontmatter::parse(content).unwrap();
        assert!(
            fm.codex_network_access.is_some(),
            "frontmatter should have codex_network_access"
        );
        let active = "opencode";
        assert_ne!(
            canonical_harness_name(active).as_deref(),
            Some("codex"),
            "opencode should not be canonical codex"
        );
        assert!(
            canonical_harness_name(&active).is_some(),
            "opencode is a known harness"
        );
        let has_guard = canonical_harness_name("codex").as_deref() == Some("codex")
            && canonical_harness_name(active).as_deref() != Some("codex")
            && fm.codex_network_access.is_some();
        assert!(
            has_guard,
            "guard condition should fire for opencode + codex_network_access: enabled"
        );
    }

    #[test]
    fn preflight_output_includes_warnings() {
        let output = PreflightOutput {
            warnings: vec![PreflightWarning {
                code: "harness_mismatch".to_string(),
                message: "Document declares agent: codex but active harness is claude-code."
                    .to_string(),
                document_agent: Some("codex".to_string()),
                active_harness: Some("claude-code".to_string()),
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["warnings"][0]["code"], "harness_mismatch");
        assert_eq!(parsed["warnings"][0]["document_agent"], "codex");
        assert_eq!(parsed["warnings"][0]["active_harness"], "claude-code");
    }

    #[test]
    fn preflight_output_omits_warnings_when_empty() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("warnings").is_none(),
            "warnings should be omitted when empty"
        );
    }

    #[test]
    fn preflight_output_null_diff_when_no_changes() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["diff"].is_null());
        assert_eq!(parsed["no_changes"], true);
    }

    #[test]
    fn check_layout_returns_empty_outside_tmux() {
        // When TMUX env var is not set (typical in CI / test), check_layout
        // should return an empty vec silently.
        let _env_guard = crate::test_support::env_lock();
        let saved = std::env::var("TMUX").ok();
        // SAFETY: test is single-threaded; we restore the value immediately after.
        unsafe { std::env::remove_var("TMUX") };
        let issues = check_layout();
        // Restore if it was set.
        if let Some(val) = saved {
            unsafe { std::env::set_var("TMUX", val) };
        }
        assert!(
            issues.is_empty(),
            "expected no issues outside tmux, got: {:?}",
            issues
        );
    }

    #[test]
    fn preflight_output_includes_layout_issues() {
        let output = PreflightOutput {
            layout_issues: vec!["window index 0 missing".to_string()],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["layout_issues"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["layout_issues"][0], "window index 0 missing");
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

        let _env_guard = crate::test_support::env_lock();
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
    fn maybe_auto_repair_base_index_noop_without_issue() {
        let dir = tempfile::tempdir().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("state")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\n---\n").unwrap();
        let issues: Vec<String> = vec![];
        maybe_auto_repair_base_index(&file, &issues);
        let counter_path = agent_doc_dir.join("state/base-index-repair.count");
        assert!(
            !counter_path.exists(),
            "no counter file should be created when no base-index issue"
        );
    }

    #[test]
    fn detect_duplicate_claims_empty_registry() {
        let registry = tmux_router::Registry::new();
        assert!(detect_duplicate_claims(&registry).is_empty());
    }

    #[test]
    fn detect_duplicate_claims_no_duplicates() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: "tasks/foo.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: "tasks/bar.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        assert!(detect_duplicate_claims(&registry).is_empty());
    }

    #[test]
    fn detect_duplicate_claims_two_sessions_same_file() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        let issues = detect_duplicate_claims(&registry);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("duplicate claims"));
        assert!(issues[0].contains("tasks/shared.md"));
        assert!(issues[0].contains("session-a"));
        assert!(issues[0].contains("session-b"));
    }

    #[test]
    fn detect_duplicate_claims_skips_empty_file_entries() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: String::new(), // legacy entry — no file
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: String::new(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        assert!(detect_duplicate_claims(&registry).is_empty());
    }

    #[test]
    fn is_url_detects_http() {
        assert!(is_url("http://example.com"));
        assert!(is_url("https://example.com/path"));
        assert!(!is_url("../relative/path.md"));
        assert!(!is_url("tasks/software/agent-doc.md"));
        assert!(!is_url(""));
    }

    #[test]
    fn is_html_content_detects_html() {
        assert!(is_html_content("text/html; charset=utf-8"));
        assert!(is_html_content("text/html"));
        assert!(is_html_content("application/xhtml+xml"));
        assert!(!is_html_content("application/json"));
        assert!(!is_html_content("text/plain"));
    }

    #[test]
    fn html_to_markdown_converts_basic_html() {
        let html = "<h1>Title</h1><p>Hello <strong>world</strong>.</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Title"), "should contain heading text");
        assert!(md.contains("**world**"), "should convert bold");
    }

    #[test]
    fn html_to_markdown_strips_script_and_style() {
        let html =
            "<p>Visible</p><script>alert('xss')</script><style>.foo{}</style><p>Also visible</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Visible"));
        assert!(md.contains("Also visible"));
        assert!(!md.contains("alert"), "script content should be stripped");
        assert!(!md.contains(".foo"), "style content should be stripped");
    }

    #[test]
    fn html_to_markdown_strips_nav_and_footer() {
        let html =
            "<nav><a href='/'>Home</a></nav><main><p>Content</p></main><footer>Copyright</footer>";
        let md = html_to_markdown(html);
        assert!(md.contains("Content"));
        assert!(!md.contains("Home"), "nav content should be stripped");
        assert!(
            !md.contains("Copyright"),
            "footer content should be stripped"
        );
    }

    #[test]
    fn url_cache_path_is_deterministic() {
        let dir = TempDir::new().unwrap();
        let p1 = url_cache_path(dir.path(), "https://example.com");
        let p2 = url_cache_path(dir.path(), "https://example.com");
        assert_eq!(p1, p2, "same URL should produce same cache path");

        let p3 = url_cache_path(dir.path(), "https://other.com");
        assert_ne!(
            p1, p3,
            "different URLs should produce different cache paths"
        );
        assert!(p1.extension().unwrap() == "txt");
    }

    #[test]
    fn links_cache_dir_creates_directory() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();

        let cache = links_cache_dir(&doc);
        assert!(cache.is_some());
        let cache_path = cache.unwrap();
        assert!(cache_path.exists());
        assert!(cache_path.ends_with("links_cache"));
    }

    #[test]
    fn preflight_output_includes_baseline_file() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: Some("/tmp/baseline.md".to_string()),
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["baseline_file"], "/tmp/baseline.md");
    }

    #[test]
    fn preflight_output_omits_baseline_file_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("baseline_file").is_none(),
            "baseline_file should be omitted when None"
        );
    }

    #[test]
    fn preflight_output_includes_diff_type_when_set() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: Some("+go\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: Some("approval".to_string()),
            diff_type_reason: Some("single approval word: \"go\"".to_string()),
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["diff_type"], "approval");
        assert!(parsed["diff_type_reason"].as_str().unwrap().contains("go"));
    }

    #[test]
    fn preflight_output_omits_diff_type_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("diff_type").is_none(),
            "diff_type should be omitted when None"
        );
        assert!(
            parsed.get("diff_type_reason").is_none(),
            "diff_type_reason should be omitted when None"
        );
    }

    #[test]
    fn preflight_output_includes_annotated_diff_when_set() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: Some("+line\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: Some("[user+] line".to_string()),
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["annotated_diff"], "[user+] line");
    }

    #[test]
    fn preflight_output_omits_annotated_diff_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("annotated_diff").is_none(),
            "annotated_diff should be omitted when None"
        );
    }

    #[test]
    fn preflight_output_includes_inline_annotations() {
        let output = PreflightOutput {
            inline_annotations: vec![
                "This is wrong, fix it".to_string(),
                "Broaden the gate".to_string(),
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let anns = parsed["inline_annotations"].as_array().unwrap();
        assert_eq!(anns.len(), 2);
        assert_eq!(anns[0], "This is wrong, fix it");
        assert_eq!(anns[1], "Broaden the gate");
    }

    #[test]
    fn preflight_output_omits_inline_annotations_when_empty() {
        let output = PreflightOutput {
            inline_annotations: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("inline_annotations").is_none(),
            "inline_annotations should be omitted when empty"
        );
    }

    #[test]
    fn preflight_output_includes_prompt_bearing_changes() {
        let output = PreflightOutput {
            prompt_bearing_changes: vec![
                crate::diff::PromptBearingChange {
                    kind: crate::diff::PromptBearingChangeKind::PromptTarget,
                    text: "❯ Why was this missed?".to_string(),
                },
                crate::diff::PromptBearingChange {
                    kind: crate::diff::PromptBearingChangeKind::ContentEdit,
                    text: "This line should say 503, not 401.".to_string(),
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let changes = parsed["prompt_bearing_changes"].as_array().unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0]["kind"], "prompt_target");
        assert_eq!(changes[0]["text"], "❯ Why was this missed?");
        assert_eq!(changes[1]["kind"], "content_edit");
    }

    #[test]
    fn preflight_output_omits_prompt_bearing_changes_when_empty() {
        let output = PreflightOutput {
            prompt_bearing_changes: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("prompt_bearing_changes").is_none(),
            "prompt_bearing_changes should be omitted when empty"
        );
    }

    #[test]
    fn preflight_output_includes_session_accretion_when_present() {
        let output = PreflightOutput {
            session_accretion: Some(crate::session_accretion::SessionAccretionReport {
                level: crate::session_accretion::SessionAccretionLevel::Warn,
                exchange_lines: 220,
                response_sections: 9,
                recent_committed_cycles: 7,
                recent_noop_closeouts: 2,
                recent_restart_count: 0,
                recent_session_loss_count: 0,
                startup_miss_active: false,
                reasons: vec!["exchange has grown".to_string()],
                guidance: vec!["Run `agent-doc compact session.md --commit`.".to_string()],
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["session_accretion"]["level"], "warn");
        assert_eq!(parsed["session_accretion"]["exchange_lines"], 220);
    }

    #[test]
    fn preflight_output_omits_session_accretion_when_absent() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("session_accretion").is_none(),
            "session_accretion should be omitted when absent"
        );
    }

    #[test]
    fn preflight_output_slash_commands_from_diff() {
        // /clear is a built-in command — goes to builtin_commands, not slash_commands
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+/clear\n";
        let parsed_cmds = crate::diff::parse_slash_commands_classified(diff);
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: Some(diff.to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: parsed_cmds.skill_commands,
            builtin_commands: parsed_cmds.builtin_commands,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // /clear is a built-in — appears in builtin_commands, not slash_commands
        assert_eq!(parsed["builtin_commands"][0], "/clear");
        assert!(
            parsed["slash_commands"].is_null()
                || parsed["slash_commands"]
                    .as_array()
                    .is_none_or(|a| a.is_empty())
        );
    }

    #[test]
    fn preflight_output_no_document_field() {
        // The `document` field was removed — it must not appear in serialized JSON.
        // Having it would send full document content to the agent every cycle,
        // wasting tokens on every invocation.
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("document").is_none(),
            "document key must be absent from preflight JSON — it would waste tokens on every cycle"
        );
    }

    #[test]
    fn preflight_output_no_large_content() {
        // Regression: preflight JSON must not embed document content.
        // Any field containing the full file body would be sent to the agent
        // on every cycle, burning tokens proportional to document size.
        let large_content = "x".repeat(10_000);
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: Some(format!("+{large_content}")), // diff can include content
            no_changes: false,
            linked_changes: vec![],
            baseline_file: Some("/tmp/baseline.md".to_string()),
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Only `diff` may contain the large content (it's the actual user change).
        // No OTHER field should contain it.
        let diff_str = parsed["diff"].as_str().unwrap_or("");
        for (key, val) in parsed.as_object().unwrap() {
            if key == "diff" {
                continue;
            }
            let val_str = val.to_string();
            assert!(
                !val_str.contains(&large_content),
                "field `{key}` contains large content — this would waste tokens on every preflight cycle"
            );
            assert!(
                val_str.len() < 1_000 || key == "annotated_diff",
                "field `{key}` is suspiciously large ({} bytes) — preflight should not embed document content",
                val_str.len()
            );
        }
        // Diff itself is allowed to contain the content
        assert!(diff_str.contains(&large_content));
    }

    // --- Fix 5: cross-document sweep ---

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
        snapshot::save(&primary, primary_content).unwrap();
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

        // Secondary doc (tracked in sessions.json, snapshot newer than file — needs sweep)
        let secondary = root.join("secondary.md");
        let secondary_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&secondary, secondary_content).unwrap();
        snapshot::save(&secondary, secondary_content).unwrap();
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
        let snap_rel = snapshot::path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        let new_snap = format!("{}\n<!-- agent updated -->", secondary_content);
        fs::write(&snap_abs, &new_snap).unwrap();

        // Write sessions.json with secondary tracked
        let sessions_path = root.join(".agent-doc/sessions.json");
        let sessions = serde_json::json!({
            "secondary-session": {
                "pane": "%1",
                "pid": 9999,
                "cwd": root.to_string_lossy(),
                "started": "2026-01-01",
                "file": "secondary.md",
                "window": "@1"
            }
        });
        fs::write(
            &sessions_path,
            serde_json::to_string_pretty(&sessions).unwrap(),
        )
        .unwrap();

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
        snapshot::save(&primary, primary_content).unwrap();
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
        snapshot::save(&secondary, snap_content).unwrap();
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
        let snap_rel = snapshot::path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&snap_abs, snap_content).unwrap();

        // Write sessions.json with secondary tracked
        let sessions_path = root.join(".agent-doc/sessions.json");
        let sessions = serde_json::json!({
            "secondary-session": {
                "pane": "%1",
                "pid": 9999,
                "cwd": root.to_string_lossy(),
                "started": "2026-01-01",
                "file": "secondary.md",
                "window": "@1"
            }
        });
        fs::write(
            &sessions_path,
            serde_json::to_string_pretty(&sessions).unwrap(),
        )
        .unwrap();

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

        let snap_rel = snapshot::path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(
            &snap_abs,
            format!("{}\n<!-- agent updated -->", secondary_content),
        )
        .unwrap();

        write_sessions_json(
            root,
            &[
                ("primary-session", "%70", &primary, "@1", "2026-01-01"),
                ("secondary-session", "%73", &secondary, "@2", "2026-01-01"),
            ],
        );
        crate::session_actor::project_binding_in(
            root,
            &primary.to_string_lossy(),
            "primary-session",
            "%70",
            "@1",
            "test",
            "primary_owner",
        )
        .unwrap();
        crate::session_actor::project_binding_in(
            root,
            &secondary.to_string_lossy(),
            "secondary-session",
            "%73",
            "@2",
            "test",
            "secondary_owner",
        )
        .unwrap();

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

        let snap_rel = snapshot::path_for(&secondary).unwrap();
        let snap_abs = root.join(&snap_rel);
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(
            &snap_abs,
            format!("{}\n<!-- agent updated -->", secondary_content),
        )
        .unwrap();

        write_sessions_json(
            root,
            &[
                ("primary-session", "%70", &primary, "@1", "2026-01-01"),
                ("secondary-session", "%70", &secondary, "@1", "2026-01-01"),
            ],
        );
        crate::session_actor::project_binding_in(
            root,
            &primary.to_string_lossy(),
            "primary-session",
            "%70",
            "@1",
            "test",
            "primary_owner",
        )
        .unwrap();
        crate::session_actor::project_binding_in(
            root,
            &secondary.to_string_lossy(),
            "secondary-session",
            "%70",
            "@1",
            "test",
            "secondary_owner",
        )
        .unwrap();

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

    // --- #cce5: resolve_agent_model / short_model_name tests ---

    #[test]
    fn short_model_name_strips_claude_prefix() {
        assert_eq!(short_model_name("claude-sonnet-4-6"), "sonnet-4-6");
        assert_eq!(short_model_name("claude-opus-4"), "opus-4");
        assert_eq!(short_model_name("claude-haiku-4-5"), "haiku-4-5");
    }

    #[test]
    fn short_model_name_returns_as_is_without_prefix() {
        assert_eq!(short_model_name("sonnet-4-6"), "sonnet-4-6");
        assert_eq!(short_model_name("gpt-4o"), "gpt-4o");
        assert_eq!(short_model_name("gpt-5"), "gpt-5");
        assert_eq!(short_model_name("gpt-5.4"), "gpt-5.4");
        assert_eq!(short_model_name("opus-4-6"), "opus-4-6");
        assert_eq!(short_model_name(""), "");
    }

    #[test]
    fn resolve_agent_model_uses_frontmatter_only() {
        // ANTHROPIC_MODEL env var is deliberately ignored — only frontmatter matters.
        let cfg = agent_doc_core::model_tier::ModelConfig::default();
        let result = resolve_agent_model(Some("claude-opus-4"), "claude-code", &cfg);
        assert_eq!(result, Some("opus-4".to_string()));
    }

    #[test]
    fn resolve_agent_model_strips_claude_prefix_from_frontmatter() {
        let cfg = agent_doc_core::model_tier::ModelConfig::default();
        let result = resolve_agent_model(Some("claude-haiku-4-5"), "claude-code", &cfg);
        assert_eq!(result, Some("haiku-4-5".to_string()));
    }

    #[test]
    fn resolve_agent_model_defers_claude_code_opus_alias() {
        // The bare `opus` alias is deferred: agent-doc pins no version, so
        // attribution returns None and the running skill self-stamps its real
        // model identity (always the current opus).
        let cfg = agent_doc_core::model_tier::ModelConfig::default();
        let result = resolve_agent_model(Some("opus"), "claude-code", &cfg);
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_agent_model_stamps_pinned_concrete_opus() {
        // An explicitly pinned concrete opus id still stamps its short name.
        let cfg = agent_doc_core::model_tier::ModelConfig::default();
        let result = resolve_agent_model(Some("claude-opus-4-8"), "claude-code", &cfg);
        assert_eq!(result, Some("opus-4-8".to_string()));
    }

    #[test]
    fn resolve_agent_model_preserves_short_openai_style_name() {
        let cfg = agent_doc_core::model_tier::ModelConfig::default();
        let result = resolve_agent_model(Some("gpt-5"), "codex", &cfg);
        assert_eq!(result, Some("gpt-5".to_string()));
    }

    #[test]
    fn resolve_agent_model_none_when_no_frontmatter() {
        // No frontmatter → None, regardless of env var state.
        let cfg = agent_doc_core::model_tier::ModelConfig::default();
        let result = resolve_agent_model(None, "claude-code", &cfg);
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_pipeline_state_none_without_cycle_or_frontmatter() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        assert!(resolve_pipeline_state(&doc).unwrap().is_none());
    }

    #[test]
    fn resolve_pipeline_state_falls_back_to_frontmatter_block() {
        // No cycle-state on disk → read the document `agent_doc_pipeline:` mirror.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_pipeline:\n  run_id: cycle-77\n  step: write_applied\n---\n\nbody\n",
        )
        .unwrap();
        let p = resolve_pipeline_state(&doc).unwrap().expect("frontmatter fallback");
        assert_eq!(p.run_id.as_deref(), Some("cycle-77"));
        assert_eq!(p.step.as_deref(), Some("write_applied"));
    }

    #[test]
    fn resolve_pipeline_state_cycle_state_wins_over_frontmatter() {
        // Cycle-state is authoritative; a stale frontmatter block must not override it.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_pipeline:\n  run_id: stale-mirror\n  step: committed\n---\n\nbody\n",
        )
        .unwrap();
        let state = crate::cycle_state::start_preflight_with_task(
            &doc,
            Some("snap"),
            Some("body"),
            Some("#fmrunid-wire"),
            Some("#fmrunid-wire"),
        )
        .unwrap();

        let p = resolve_pipeline_state(&doc).unwrap().expect("cycle-state present");
        assert_eq!(p.run_id.as_deref(), Some(state.cycle_id.as_str()));
        assert_eq!(p.step.as_deref(), Some("preflight_started"));
        assert_eq!(p.turn_id.as_deref(), Some("#fmrunid-wire"));
        assert_ne!(p.run_id.as_deref(), Some("stale-mirror"));
    }
}
