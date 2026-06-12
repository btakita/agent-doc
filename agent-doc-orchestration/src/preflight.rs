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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

/// AST-backed semantic summary for the current preflight diff.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SemanticDiffSummary {
    /// Schema version for additive changes to this JSON object.
    pub schema_version: u8,
    /// Components touched by this diff, in stable sorted order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_components: Vec<String>,
    /// Component-level additions/removals/changes with bounded navigation spans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub component_changes: Vec<SemanticComponentChange>,
    /// Node-keyed item events from the markdown AST overlay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_events: Vec<SemanticNodeEvent>,
    /// Prompt-bearing change previews, preserving encounter order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_changes: Vec<SemanticPromptChange>,
}

/// Component-level semantic operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemanticComponentOp {
    Added,
    Removed,
    Changed,
}

/// A changed agent component plus before/after navigation handles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticComponentChange {
    pub component: String,
    pub occurrence: usize,
    pub op: SemanticComponentOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<SemanticNavTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<SemanticNavTarget>,
}

/// Bounded source navigation target for an agent component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticNavTarget {
    pub handle: String,
    pub component: String,
    pub occurrence: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// A node-keyed item event suitable for preflight JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticNodeEvent {
    pub component: String,
    pub node_key: String,
    pub op: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_node_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_node_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_preview: Option<String>,
}

/// Bounded preview of a prompt-bearing semantic change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SemanticPromptChange {
    pub kind: crate::diff::PromptBearingChangeKind,
    pub text_preview: String,
}

/// Per-item opportunistic gated-review verification result (`#optverify`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateVerifyResult {
    /// Review item id (no `#` prefix).
    pub id: String,
    /// Scan status: `provable`, `failed`, or `pending`.
    pub status: String,
    /// The matched proof/disproof substring (absent when pending).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// Epoch seconds of the matched marker line (absent when pending).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u64>,
    /// True when the opt-in auto-flipped this gate to `[x]` this cycle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreflightOutput {
    /// Non-blocking warnings the skill should surface before responding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PreflightWarning>,
    /// Tmux layout issues found (empty = healthy).
    /// #per-cycle-protocol-output-overhead: omit when empty so a healthy cycle
    /// does not spend per-cycle context bytes on `"layout_issues": []`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_issues: Vec<String>,
    /// Whether an orphaned pending response was recovered and applied.
    pub recovered: bool,
    /// Whether a git commit was made for the previous cycle.
    pub committed: bool,
    /// Lines from `.agent-doc/claims.log` (truncated after read).
    /// #per-cycle-protocol-output-overhead: omit when empty (the common case).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    /// Structured semantic navigation for the same diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_diff: Option<SemanticDiffSummary>,
    /// Operation manifest for the current turn (`#op-scoped-drift-2`): the
    /// driver node plus the read/write addresses the turn touches. Derived from
    /// `prompt_targets` at turn start; the substrate the phase-3 affectedness
    /// classifier reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_scope: Option<agent_doc_core::turn_scope::TurnScope>,
    /// Affectedness classification of this cycle's node ops against `turn_scope`
    /// (`#op-scoped-drift-3`): each op routed into the 5-class taxonomy, plus an
    /// aggregate `turn_affected`. Independent/provenance-spoofed ops integrate
    /// and persist without affecting the turn instead of tripping a coarse gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_affectedness: Option<agent_doc_core::turn_scope::CycleAffectedness>,
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
    /// Opportunistic gated-review verification results (`#optverify`). One entry
    /// per gated review item carrying a verify predicate, with its `ops.log`
    /// scan status (`provable` / `failed` / `pending`). `auto_resolved` is true
    /// when the `agent_doc_gate_autoverify` opt-in flipped a provable gate to
    /// `[x]` this cycle. Empty (and omitted) in the common no-predicate case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gate_verify: Vec<GateVerifyResult>,
    /// Canonical ordered list of user-authored changes that need prompt-aware handling.
    /// `prompt_target` items require a response, `content_edit` items are corrections
    /// the agent must incorporate, and `recovery_artifact` / `boundary_artifact`
    /// items indicate document-state cleanup rather than ordinary conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_bearing_changes: Vec<crate::diff::PromptBearingChange>,
    /// `prompt_bearing_changes` with managed-component state edits filtered
    /// out (queue activity toggle, queue items, backlog/review/done items,
    /// `queue_active:` frontmatter toggle), AND with edits the affectedness
    /// classifier scoped as independent of the current turn dropped when
    /// `op_affectedness.turn_affected` is `false` (`#queue-no-stop-unrelated-edit`).
    /// The Claude Code auto-loop guard uses this field instead of
    /// `prompt_bearing_changes` so neither routine session bookkeeping nor an
    /// edit unrelated to the current turn blocks the auto-loop — only a real
    /// user prompt (which edits the in-scope `exchange` tail and classifies as
    /// turn-affecting) preempts. Plan: `#ccloopguard`, `#queue-no-stop-unrelated-edit`.
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

mod semantic_diff;
pub(crate) use semantic_diff::*;

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

/// Grace window (seconds) so an artifact built shortly before the source commit
/// is not flagged. In the normal build → install → verify → commit ordering the
/// install necessarily predates the commit it covers (the install cannot know
/// about a commit that follows it), and a careful cycle's install→commit gap can
/// run a couple of minutes. 5 minutes comfortably covers that while still
/// catching genuine staleness — the real `#install-stale-guard` failure left the
/// install ~11 minutes behind a same-version commit, and forgotten reinstalls
/// leave sessions on hours-old code.
const STALE_INSTALL_GRACE_SECS: u64 = 300;

/// Pure classifier for `#install-stale-guard`: given the unix timestamp of the
/// latest source commit and a set of `(label, mtime)` installed artifacts
/// (`None` mtime = artifact absent), return the labels whose mtime predates the
/// source commit by more than `grace_secs`. Extracted so the staleness rule is
/// deterministically unit-testable without touching git or the filesystem.
fn classify_stale_install_artifacts(
    source_commit_ts: u64,
    artifacts: &[(&'static str, Option<u64>)],
    grace_secs: u64,
) -> Vec<&'static str> {
    artifacts
        .iter()
        .filter_map(|(label, mtime)| match mtime {
            Some(m) if m.saturating_add(grace_secs) < source_commit_ts => Some(*label),
            _ => None,
        })
        .collect()
}

/// Unix mtime (seconds) of `path`, following symlinks (installed cdylibs are
/// symlinks into `target/release`). `None` when missing/unreadable.
fn artifact_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// `~/.cargo/bin` (honoring `CARGO_HOME`), or `None` when unresolvable.
fn cargo_bin_dir() -> Option<PathBuf> {
    if let Ok(cargo_home) = std::env::var("CARGO_HOME")
        && !cargo_home.is_empty()
    {
        return Some(PathBuf::from(cargo_home).join("bin"));
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".cargo/bin"))
}

/// Newest mtime among `<bin_dir>/libagent_doc-*.so` (the lib-installed cdylib
/// the JetBrains plugin hot-reloads). Version-globbed because the cdylib is
/// named after the `agent-doc` binary crate version, not this crate's.
fn installed_cdylib_mtime(bin_dir: &Path) -> Option<u64> {
    std::fs::read_dir(bin_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            (name.starts_with("libagent_doc-") && name.ends_with(".so"))
                .then(|| artifact_mtime_secs(&entry.path()))
                .flatten()
        })
        .max()
}

/// Locate the `agent-doc` source repo relative to the document's git root: the
/// root itself (standalone checkout) or `<root>/src/agent-doc` (the dogfood
/// submodule layout), identified by a `Cargo.toml` declaring the binary crate.
fn locate_agent_doc_source_repo(doc_git_root: &Path) -> Option<PathBuf> {
    [
        doc_git_root.to_path_buf(),
        doc_git_root.join("src/agent-doc"),
    ]
    .into_iter()
    .find(|candidate| {
        std::fs::read_to_string(candidate.join("Cargo.toml"))
            .map(|toml| toml.lines().any(|l| l.trim() == "name = \"agent-doc\""))
            .unwrap_or(false)
    })
}

/// `git log -1 <fmt>` over buildable source paths in `repo`. Restricting to
/// source pathspecs keeps doc-only commits from tripping the staleness check.
fn source_head_git_field(repo: &Path, fmt: &str) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args([
            "log",
            "-1",
            fmt,
            "--",
            "*.rs",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Warn when the installed/built `agent-doc` artifacts predate the latest source
/// commit, so live sessions (tmux, JetBrains) do not silently run stale code at
/// an unchanged version string (`#install-stale-guard`). Best-effort: only fires
/// when an `agent-doc` source repo is locatable (development / dogfooding) and
/// silently no-ops otherwise (for example a crates.io install with no source).
fn stale_install_warning(doc_git_root: &Path) -> Option<PreflightWarning> {
    let repo = locate_agent_doc_source_repo(doc_git_root)?;
    let commit_ts = source_head_git_field(&repo, "--format=%ct")?
        .parse::<u64>()
        .ok()?;

    let bin_dir = cargo_bin_dir();
    let release_dir = repo.join("target/release");
    let artifacts: Vec<(&'static str, Option<u64>)> = vec![
        (
            "~/.cargo/bin/agent-doc",
            bin_dir
                .as_deref()
                .and_then(|d| artifact_mtime_secs(&d.join("agent-doc"))),
        ),
        (
            "~/.cargo/bin cdylib",
            bin_dir.as_deref().and_then(installed_cdylib_mtime),
        ),
        (
            "target/release/agent-doc",
            artifact_mtime_secs(&release_dir.join("agent-doc")),
        ),
        (
            "target/release cdylib",
            artifact_mtime_secs(&release_dir.join("libagent_doc.so")),
        ),
    ];

    let stale = classify_stale_install_artifacts(commit_ts, &artifacts, STALE_INSTALL_GRACE_SECS);
    if stale.is_empty() {
        return None;
    }

    let short_hash =
        source_head_git_field(&repo, "--format=%h").unwrap_or_else(|| "HEAD".to_string());
    Some(PreflightWarning {
        code: "stale_install".to_string(),
        message: format!(
            "stale agent-doc install: {} predate(s) source commit {} — live sessions (tmux / JetBrains) may run pre-{} code at an unchanged version. Run `make install` in {} to rebuild the binary + cdylib.",
            stale.join(", "),
            short_hash,
            short_hash,
            repo.display()
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
            } else if key == "queue" && matches!(component.name.as_str(), "backlog" | "pending") {
                // `queue` is a recognized backlog/pending sync attribute
                // (#backlog-queue-sync-attr). Surface only an unrecognized mode
                // value as a typo; the bare token and sync/append/prepend are valid.
                if crate::queue::BacklogQueueSyncMode::parse(value).is_none() {
                    issues.push(format!(
                        "`queue={value}` on `agent:{}` is not a recognized sync mode (use `sync`, `append`, or `prepend`)",
                        component.name
                    ));
                }
            } else if key == "queue" && component.name == "icebox" {
                issues.push(
                    "`queue` on `agent:icebox` does not auto-populate `agent:queue`; move the item to `agent:backlog` or use a per-item enqueue marker".to_string(),
                );
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
            // #canonical-session-close-autodetect: if registered panes still span
            // multiple tmux sessions after resync, close the superseded ones
            // around the canonical (active agent-doc window) session.
            close_superseded_drift_sessions(file);
        }
    } else {
        eprintln!(
            "[preflight] session drift detected (count={}) — will auto-resync on next detection",
            next
        );
    }
}

/// After an auto-resync, close tmux sessions superseded by the canonical
/// (active agent-doc window) session when registered panes still span more than
/// one session (`#canonical-session-close-autodetect`). Best effort: never
/// blocks a cycle, and `close_superseded_session` preserves any session with a
/// live agent or an unmanaged user window.
fn close_superseded_drift_sessions(file: &std::path::Path) {
    let tmux = sessions::Tmux::default_server();
    let registry = match sessions::load() {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!(
                "[preflight] session-drift close: registry load failed: {}",
                e
            );
            return;
        }
    };
    let drift_sessions = resync::registered_pane_sessions(&tmux, &registry);
    if drift_sessions.len() <= 1 {
        return;
    }
    let Some(canonical) = resync::canonical_session_for_document(&tmux, &registry, file) else {
        eprintln!(
            "[preflight] session-drift: no canonical agent-doc session resolved for {}; preserving all sessions",
            file.display()
        );
        return;
    };
    match resync::close_superseded_drift_sessions(&tmux, &canonical, &drift_sessions) {
        Ok(0) => {}
        Ok(n) => eprintln!(
            "[preflight] session-drift: closed {} superseded session(s) around canonical '{}'",
            n, canonical
        ),
        Err(e) => eprintln!("[preflight] session-drift superseded close failed: {}", e),
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

mod run;
pub use run::*;

mod maintenance;
pub use maintenance::*;

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
            // #mps Rung 2 (pin): when the cutover is enabled, also persist the
            // baseline as the model overlay so finalize can project it. Best
            // effort — the `.md` baseline above is the fail-safe.
            if snapshot::mps_enabled() {
                match snapshot::save_baseline_model(file, content) {
                    Ok(()) => {}
                    Err(e) => eprintln!("[preflight] #mps baseline model pin failed: {}", e),
                }
            }
            Some(baseline_path.to_string_lossy().to_string())
        }
        Err(e) => {
            eprintln!("[preflight] failed to save baseline: {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests;
