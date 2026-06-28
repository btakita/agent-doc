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
    /// `#qpausego`: true when an accepted controller `admin queue pause` is the
    /// effective queue-control state. The pause suppresses the *unattended*
    /// supervisor idle-watch auto-injection (the flood it fixes) and is surfaced
    /// here for visibility, but it deliberately does NOT drop
    /// `queue_continuation_required` / `queue_drainable_head_count`: the attended
    /// in-session `/loop` is the legitimate single-owner drain and must keep
    /// going (stalling it on a pause strands real backlog). Use `queue: stop`
    /// frontmatter / `--- stop` fences to stop the in-session loop. Cleared by
    /// `admin queue resume`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queue_paused: bool,
    /// `#qpausemix`: controller-recorded pause reason when `queue_paused` is true.
    /// Included so callers do not have to infer whether a pause is operator-set
    /// or a transient coordination artifact from mixed continuation signals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pause_reason: Option<String>,
    /// `#cleardrainsignal`: count of agent-drainable heads (not deferred/noise) at
    /// the active queue head. Always emitted. `0` while `queue_active == true` means
    /// every remaining head is `[clean-session]` (under live IPC) / `[operator-verify]`
    /// / inert noise — a no-op `#qchurn` churn cycle.
    #[serde(default)]
    pub queue_drainable_head_count: usize,
    /// `#cleardrainsignal`: whether the queue has agent-drainable continuation work
    /// this session. Always emitted. The Claude Code auto-loop and the agent must
    /// NOT loop/dispatch when this is `false`, even if `queue_active == true` and a
    /// `queue_trigger` is present — the authoritative no-stall signal that does not
    /// depend on the route-owned supervisor being on the latest binary.
    #[serde(default)]
    pub queue_continuation_required: bool,
    /// `#degraded-ipc-no-stall`: explicit non-stall guidance, populated only
    /// when `queue_continuation_required == true`. Centralized in
    /// [`crate::queue_continuation::CONTINUATION_NO_STALL_GUIDANCE`] so the
    /// agent has a binary-authoritative "keep draining" signal and does not
    /// re-derive a stop reason by hand from a degraded transport (file-IPC
    /// fallback / stale supervisor), session-accretion, or a
    /// `semantic_completion_match` warning. Null when continuation is not
    /// required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_continuation_guidance: Option<String>,
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
    /// `#semmerge-ack-turn` (semantic_merge Phase 4): node-keyed acks carried from
    /// the prior cycle's convergence semantic merge. Non-empty when the operator
    /// deleted an agent-edited node, overrode the same node, or revived an
    /// agent-deleted node — operator content already won in the committed document,
    /// and the agent must acknowledge the non-applied change in an exchange turn
    /// THIS cycle. Cleared automatically next cycle (surfaced exactly once). A
    /// companion `semantic_merge_ack_pending` warning carries the same summary so
    /// the existing "surface warnings" skill path drives the acknowledgement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantic_merge_acks: Vec<crate::cycle_state::PendingSemanticMergeAck>,
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

/// Grace window (seconds) so an artifact built effectively at the same time as
/// the latest source edit is not flagged. `#supstaledetect`: the staleness basis
/// is now the newest source-FILE mtime, which a successful build always touches
/// the binary mtime to AFTER, so a fresh install is exact (no build→commit skew
/// to absorb). The grace just tolerates coarse mtime resolution / a source file
/// touched moments after the build started; 5 minutes still comfortably catches
/// genuine staleness (forgotten reinstalls leave sessions on hours-old code).
const STALE_INSTALL_GRACE_SECS: u64 = 300;

/// Pure classifier for `#install-stale-guard`: given the unix timestamp of the
/// latest source edit (`#supstaledetect`: the newest source-FILE mtime, not the
/// HEAD commit timestamp) and a set of `(label, mtime)` installed artifacts
/// (`None` mtime = artifact absent), return the labels whose mtime predates the
/// source by more than `grace_secs`. Extracted so the staleness rule is
/// deterministically unit-testable without touching git or the filesystem.
fn classify_stale_install_artifacts(
    source_ts: u64,
    artifacts: &[(&'static str, Option<u64>)],
    grace_secs: u64,
) -> Vec<&'static str> {
    artifacts
        .iter()
        .filter_map(|(label, mtime)| match mtime {
            Some(m) if m.saturating_add(grace_secs) < source_ts => Some(*label),
            _ => None,
        })
        .collect()
}

/// Unix mtime (seconds) of `path`, following symlinks. `None` when
/// missing/unreadable.
fn artifact_mtime_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn newest_artifact_mtime(paths: &[PathBuf]) -> Option<u64> {
    paths
        .iter()
        .filter_map(|path| artifact_mtime_secs(path))
        .max()
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

/// Warn when the installed/built `agent-doc` artifacts predate the latest local
/// source edit, so live sessions (tmux, JetBrains) do not silently run stale code
/// at an unchanged version string (`#install-stale-guard`). Best-effort: only
/// fires when an `agent-doc` source repo is locatable (development / dogfooding)
/// and silently no-ops otherwise (for example a crates.io install with no source).
///
/// `#supstaledetect`: the staleness basis is the newest source-FILE mtime
/// (`newest_crate_source_mtime_secs`, the same signal the supervisor auto-install
/// path uses), NOT the HEAD source-commit timestamp. The dogfood flow is
/// edit → build → install → verify → THEN commit, so a freshly built binary
/// always predates the commit object that covers it; comparing against the commit
/// timestamp false-positived a fresh binary as stale whenever the build→commit gap
/// exceeded the grace (observed live: an ~11-minute gap with no intervening source
/// edits). Unifying onto the source-file mtime keeps this warning in agreement
/// with the auto-install staleness signal.
fn stale_install_warning(doc_git_root: &Path) -> Option<PreflightWarning> {
    let repo = locate_agent_doc_source_repo(doc_git_root)?;
    let source_ts = crate::project_controller::newest_crate_source_mtime_secs(&repo)?;

    let bin_dir = cargo_bin_dir();
    let release_dir = repo.join("target/release");
    let local_install_dir = repo.join("target/local-install/release-local");
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
            "built agent-doc",
            newest_artifact_mtime(&[
                release_dir.join("agent-doc"),
                local_install_dir.join("agent-doc"),
            ]),
        ),
        (
            "built cdylib",
            newest_artifact_mtime(&[
                release_dir.join("libagent_doc.so"),
                local_install_dir.join("libagent_doc.so"),
            ]),
        ),
    ];

    let stale = classify_stale_install_artifacts(source_ts, &artifacts, STALE_INSTALL_GRACE_SECS);
    if stale.is_empty() {
        return None;
    }

    Some(PreflightWarning {
        code: "stale_install".to_string(),
        message: format!(
            "stale agent-doc install: {} predate the latest local source edit — live sessions (tmux / JetBrains) may run pre-edit code at an unchanged version. Run `make install` in {} to rebuild the binary + cdylib.",
            stale.join(", "),
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
    let ipc_dogfood_note_appended = if recovered {
        match append_latest_ipc_dogfood_note(file) {
            Ok(appended) => appended,
            Err(e) => {
                eprintln!("[preflight] IPC dogfood note warning: {}", e);
                false
            }
        }
    } else {
        false
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

    Ok((recovered || ipc_dogfood_note_appended, committed))
}

fn append_latest_ipc_dogfood_note(file: &Path) -> Result<bool> {
    let Some(diagnostic) = crate::session_check::latest_ipc_proof_diagnostic(file)? else {
        return Ok(false);
    };
    append_ipc_dogfood_note_for_diagnostic(file, &diagnostic)
}

pub(crate) fn append_ipc_dogfood_note_for_diagnostic(
    file: &Path,
    diagnostic: &str,
) -> Result<bool> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for IPC dogfood note", file.display()))?;
    let Some(updated) = append_ipc_dogfood_note_to_content(&content, diagnostic)? else {
        return Ok(false);
    };
    std::fs::write(file, updated)
        .with_context(|| format!("failed to write IPC dogfood note to {}", file.display()))?;
    crate::ops_log::log_op(
        file,
        &format!("ipc_dogfood_note_appended file={}", file.display()),
    );
    eprintln!(
        "[preflight] IPC dogfood note appended to {}",
        file.display()
    );
    Ok(true)
}

fn append_ipc_dogfood_note_to_content(content: &str, diagnostic: &str) -> Result<Option<String>> {
    if content.contains(diagnostic) {
        return Ok(None);
    }
    let components =
        crate::component::parse(content).context("failed to parse document components")?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let note = format_ipc_dogfood_note(diagnostic);
    let updated = exchange.append_with_caret(content, &note, None);
    if updated == content {
        Ok(None)
    } else {
        Ok(Some(updated))
    }
}

fn format_ipc_dogfood_note(diagnostic: &str) -> String {
    let diagnostic = diagnostic.replace("```", "'''");
    // The note opens with a `### Re:` response heading and folds the body into
    // a fenced block so the prompt-bearing diff classifier sees ONE
    // `[user+]` block whose first line is a recovery artifact heading → it is
    // classified as a binary-authored `RecoveryArtifact`, never a user
    // `PromptTarget`. Without this, the bare-text note closes the exchange
    // tail, is classified as a fresh prompt, gets `❯`-normalized, and becomes
    // an unresolved prompt-only tail that forces an extra acknowledgment
    // cycle (`#ipcqproof`). A fence-opening line after a blank joins the
    // heading's block, and blank lines inside a fence never split it.
    format!(
        "### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc\n\n\
```text\n\
**IPC proof issue dogfood log**\n\
Appended automatically during interrupted-cycle recovery to record the editor IPC issue.\n\
This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.\n\
Issue class: `ipc_proof_insufficient`\n\
Affected component: editor IPC / writeback\n\n\
{}\n\
```",
        diagnostic
    )
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
    // `#ipctruncrecover`: a degraded editor-IPC write can leave the on-disk working
    // tree TRUNCATED below HEAD while the live editor buffer still holds the
    // authoritative content (operator edits + the last committed response). The
    // generic `SnapshotDiffersFromHead` guard below would bail and force the operator
    // / agent into `git stash`/`reset`/`checkout` recovery. Per operator directive,
    // recover from the EDITOR BUFFER instead: flush it to disk (editor = source of
    // truth) and reset the snapshot to HEAD so the editor's edits become the normal
    // next-cycle prompt diff. Fail-OPEN: any missing listener / un-acked flush / a
    // buffer that itself lost the committed response falls through to the bail+hint —
    // never block typing, never auto-commit a response-less document.
    if recover_ipc_truncated_worktree_from_editor_buffer(file, rc)? {
        return Ok(());
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

/// `#ipctruncrecover`: reconcile an IPC-truncated working tree from the live editor
/// buffer instead of bailing the layout guard. See the call site for the rationale.
///
/// Returns `Ok(true)` only when it actually recovered (flushed the editor buffer to
/// disk, verified it preserved HEAD's committed `exchange`, and reset the snapshot to
/// HEAD). Returns `Ok(false)` — letting the caller fall through to the standard
/// bail+hint — whenever recovery is not safely possible: not the truncation shape, an
/// unstarted prompt-bearing diff (its own path owns it), no live IPC listener, an
/// un-acked flush, or a flushed buffer that lost committed response content. Fail-open
/// by construction: it never blocks and never commits a response-less document.
fn recover_ipc_truncated_worktree_from_editor_buffer(
    file: &Path,
    rc: &crate::graph::RunContext,
) -> Result<bool> {
    // Only the snapshot-vs-HEAD divergence shape that bails today.
    if !matches!(
        rc.snapshot_commit_status(),
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ) {
        return Ok(false);
    }
    // An unstarted prompt-bearing diff has its own normal handling — don't interfere.
    if crate::session_check::detect_unstarted_prompt_bearing_diff(file)?.is_some() {
        return Ok(false);
    }
    // HEAD is the baseline we reset the snapshot to; without it we cannot recover.
    let Some(head) = rc.head_content() else {
        return Ok(false);
    };
    let Ok(canonical) = file.canonicalize() else {
        return Ok(false);
    };
    let project_root = crate::write::resolve_ipc_project_root_pub(&canonical);
    // The editor buffer is only authoritative when a live editor is attached.
    if !crate::ipc_socket::is_listener_active(&project_root) {
        return Ok(false);
    }

    // Flush the live editor buffer to disk (editor = source of truth). Fail-open: an
    // error or absent ack means we cannot trust disk == buffer, so fall through.
    let patch_id = uuid::Uuid::new_v4().to_string();
    let path_str = canonical.to_string_lossy().to_string();
    let barrier = crate::debounce::await_editor_sync_barrier(&path_str, 75, 150);
    let in_flight = barrier
        .statuses
        .iter()
        .filter(|status| status.in_flight)
        .count();
    crate::ops_log::log_op(
        file,
        &format!(
            "editor_sync_barrier file={} barrier=ipc_truncation_recover outcome={:?} statuses={} in_flight={} typing_recent={}",
            file.display(),
            barrier.kind,
            barrier.statuses.len(),
            in_flight,
            barrier.typing_recent
        ),
    );
    match crate::ipc_socket::send_save_document(&project_root, &path_str, &patch_id) {
        Ok(true) => {}
        Ok(false) | Err(_) => return Ok(false),
    }

    // Re-read disk (now the flushed editor buffer) and refuse to trust a buffer that
    // itself dropped the committed response — that case falls through to the safe bail
    // rather than auto-committing a response-less document.
    let flushed = std::fs::read_to_string(&canonical).with_context(|| {
        format!(
            "ipc-truncation recover: re-read failed {}",
            canonical.display()
        )
    })?;
    if !crate::write::editor_buffer_preserved_head_exchange(&flushed, &head) {
        crate::ops_log::log_op(
            file,
            &format!(
                "ipc_truncation_recover_rejected file={} reason=editor_buffer_lost_committed_exchange flushed_len={} head_len={}",
                file.display(),
                flushed.len(),
                head.len()
            ),
        );
        return Ok(false);
    }

    // Reset the snapshot to HEAD: the editor's uncommitted edits now read as the
    // normal next-cycle prompt diff (editor-on-disk vs snapshot=HEAD).
    crate::snapshot::save(file, &head)?;
    rc.invalidate_snapshot_content();
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_truncation_recovered_from_editor_buffer file={} flushed_len={} head_len={} patch_id={}",
            file.display(),
            flushed.len(),
            head.len(),
            patch_id
        ),
    );
    eprintln!(
        "[preflight] #ipctruncrecover: reconciled IPC-truncated working tree from the live editor buffer for {} (no git recovery needed)",
        file.display()
    );
    Ok(true)
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

pub(crate) fn preflight_debounce_ms(file: &Path) -> u64 {
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
        ActorSweepOwner::Inactive | ActorSweepOwner::Unknown => {
            registry_sweep_owner(root, registry, doc_path)
        }
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

mod queue_tombstone;

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
        // #donemirrorreap: collect only each done item's OWN leading id, not every
        // bracketed id cited in its prose — otherwise a `[#other]` mentioned inside
        // one entry's body falsely marks `#other` done and reaps its open mirror.
        for id in crate::pending::extract_done_item_own_ids(comp.content(content)) {
            ids.insert(id);
        }
        if let Some(archive) = comp.attrs.get("archive")
            && let Some(root) = project_root
        {
            let archive_path = root.join(archive);
            if let Ok(archive_content) = std::fs::read_to_string(&archive_path) {
                for id in crate::pending::extract_done_item_own_ids(&archive_content) {
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
            let mut completed = prompt.clone();
            completed.text = crate::queue::strip_in_progress_marker(&completed.text);
            struck.push(completed.clone());
            rewritten.push(crate::queue::QueueEntry::Completed(completed));
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

fn claims_log_path(file: &Path) -> Option<std::path::PathBuf> {
    // Canonicalize to find project root reliably.
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return None,
    };

    let root = match snapshot::find_project_root(&canonical) {
        Some(root) => root,
        None => return None,
    };

    Some(root.join(".agent-doc/claims.log"))
}

/// Read the claims log without mutating it. Returns lines as a `Vec<String>`.
/// Returns an empty vec if the log doesn't exist or can't be read.
fn read_claims(file: &Path) -> Vec<String> {
    let Some(log_path) = claims_log_path(file) else {
        return vec![];
    };

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

    claims
}

/// Read the claims log and truncate it. Returns lines as a `Vec<String>`.
/// Returns an empty vec if the log doesn't exist or can't be read.
fn read_and_truncate_claims(file: &Path) -> Vec<String> {
    let Some(log_path) = claims_log_path(file) else {
        return vec![];
    };

    let claims = read_claims(file);
    if claims.is_empty() {
        return claims;
    }

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
    let hash = hex::encode(Sha256::digest(url.as_bytes()));
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
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .into();
    let response = agent.get(url).call();

    match response {
        Ok(mut resp) => {
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = match resp.body_mut().read_to_string() {
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
mod th {
    use super::*;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;
    // The source-repo locator accepts the document's git root when it is the
    // `agent-doc` crate, the `src/agent-doc` dogfood submodule layout, and
    // returns `None` (silent no-op) when no `agent-doc` Cargo.toml is present.
    // #per-cycle-protocol-output-overhead: empty Vec fields must not spend
    // per-cycle context bytes. A healthy/default PreflightOutput omits the empty
    // `claims` and `layout_issues` arrays from its JSON, and still round-trips
    // back to empty Vecs (serde default) so consumers reading the struct are safe.
    pub(crate) struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl EnvGuard {
        pub(crate) fn set(key: &'static str, value: &str) -> Self {
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
    pub(crate) fn setup_project() -> TempDir {
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
    pub(crate) fn commit_all(root: &Path, message: &str, commit_date: Option<&str>) {
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
    pub(crate) fn initialize_git_head(root: &Path) {
        let readme = root.join("README.md");
        std::fs::write(&readme, "# project\n").unwrap();
        commit_all(root, "initial", None);
    }
    pub(crate) fn write_committed_doc(
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
    pub(crate) fn write_sessions_json(root: &Path, entries: &[(&str, &str, &Path, &str, &str)]) {
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
    pub(crate) fn age_cycle_state(file: &Path, age_secs: u64) {
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
    pub(crate) fn write_cycles_log(doc: &Path, entries: &[crate::ops_log::CycleEntry]) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }
    // #opsproof-samecycle-add: a gated review/backlog item added THIS cycle (its
    // text legitimately cites a shipped dependency commit) must NOT be ops-proof
    // auto-completed on the same cycle it first appears — even though the
    // write/finalize path already re-synced the on-disk snapshot to include it,
    // which defeats the snapshot-only same-cycle guard. cycle_state records the
    // added id; the reap must honor it.
    // #opsproof-falsepos: an open actionable backlog item whose completion
    // marker only describes already-landed *dependency* work (a cited commit
    // hash in mid-sentence prose) must NOT be auto-reaped. Only a marker that is
    // the item's own leading status verb proves the item itself is done.
    // #opsproofgate: a live-verify / operator-drive gate that cites a shipped
    // commit hash (e.g. "Code SHIPPED 1edb20d2") in its text must NOT be
    // auto-completed on evidence=commit — even when it has existed for several
    // cycles (not a same-cycle add). Only an anchored structured ops.log marker
    // driven live by the operator may close it.
    // #opsproof-falsepos: never auto-archive an item on the same cycle it is
    // added. A brand-new add is absent from the cycle-start snapshot, so even a
    // leading-status completion marker must not reap it this cycle.
    pub(crate) fn write_optverify_doc(
        dir: &TempDir,
        predicate_annotation: &str,
    ) -> std::path::PathBuf {
        let doc = dir.path().join("session.md");
        let file_content = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Review\n\n",
                "<!-- agent:review -->\n",
                "- [/] [#saev] early-ack live verify {}\n",
                "<!-- /agent:review -->\n"
            ),
            predicate_annotation
        );
        std::fs::write(&doc, &file_content).unwrap();
        snapshot::save(&doc, &file_content).unwrap();
        doc
    }
    pub(crate) fn write_ops_log(dir: &TempDir, body: &str) {
        let logs = dir.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("ops.log"), body).unwrap();
    }
    pub(crate) fn user_prompt_change(text: &str) -> crate::diff::PromptBearingChange {
        crate::diff::PromptBearingChange {
            kind: crate::diff::PromptBearingChangeKind::PromptTarget,
            text: text.to_string(),
        }
    }
    pub(crate) fn affectedness(
        turn_affected: bool,
    ) -> agent_doc_core::turn_scope::CycleAffectedness {
        use agent_doc_core::turn_scope::{AffectednessClass, ClassifiedOp};
        agent_doc_core::turn_scope::CycleAffectedness {
            turn_affected,
            classified: vec![ClassifiedOp {
                component: "queue".to_string(),
                node_key: "queue:0:other:0".to_string(),
                op_kind: "move".to_string(),
                actor: agent_doc_core::op_log::OpActor::User,
                class: if turn_affected {
                    AffectednessClass::InputAffecting
                } else {
                    AffectednessClass::Independent
                },
            }],
        }
    }
    // --- Fix 5: cross-document sweep ---
    // --- #cce5: resolve_agent_model / short_model_name tests ---
}
#[cfg(test)]
pub(crate) use th::*;

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;
    #[test]
    fn stale_install_classifier_flags_only_artifacts_older_than_source_edit() {
        // `#supstaledetect`: the timestamp passed in is the newest source-FILE
        // mtime (not a commit timestamp), but the classifier rule is unchanged.
        let source = 10_000u64;
        let grace = 60u64;

        // All artifacts newer than the source edit → nothing stale.
        assert!(
            classify_stale_install_artifacts(
                source,
                &[("bin", Some(source + 5)), ("cdylib", Some(source + 1))],
                grace,
            )
            .is_empty()
        );

        // One artifact older than the source edit by more than the grace → flagged.
        let stale = classify_stale_install_artifacts(
            source,
            &[("bin", Some(source - 600)), ("cdylib", Some(source + 1))],
            grace,
        );
        assert_eq!(stale, vec!["bin"]);

        // Built just inside the grace window (mtime resolution / a source file
        // touched moments after the build) → not flagged; older than grace → flagged.
        assert!(
            classify_stale_install_artifacts(source, &[("bin", Some(source - 30))], grace)
                .is_empty()
        );
        assert_eq!(
            classify_stale_install_artifacts(source, &[("bin", Some(source - 61))], grace),
            vec!["bin"]
        );

        // Absent artifacts (not installed) never fire.
        assert!(
            classify_stale_install_artifacts(source, &[("bin", None), ("cdylib", None)], grace)
                .is_empty()
        );
    }

    #[test]
    fn stale_install_uses_source_file_mtime_not_commit_time_so_build_before_commit_is_not_stale() {
        // `#supstaledetect` regression: the dogfood flow is
        // edit → build → install → verify → THEN commit, so a freshly built
        // binary's mtime is AFTER the newest source-FILE mtime but BEFORE the
        // commit object that covers it. With the staleness basis unified onto the
        // source-FILE mtime, such a binary must NOT classify as stale — even when
        // the build→commit gap (a hypothetical later commit timestamp) exceeds the
        // grace. This is the exact false-positive the old commit-timestamp basis
        // produced (binary built 03:16, commit created 03:27, no source edits).
        let source_ts = 10_000u64;
        let grace = STALE_INSTALL_GRACE_SECS;

        // Binary built 10s after the newest source file (well inside grace).
        assert!(
            classify_stale_install_artifacts(source_ts, &[("bin", Some(source_ts + 10))], grace)
                .is_empty(),
            "a binary built just after the source edit must not be stale"
        );

        // Binary built far AFTER the source edit (e.g. an ~11-minute build→commit
        // gap's worth of slack) is even more clearly fresh — the old basis would
        // have compared this binary to the later commit timestamp and flagged it.
        assert!(
            classify_stale_install_artifacts(source_ts, &[("bin", Some(source_ts + 660))], grace)
                .is_empty(),
            "a binary newer than the source edit is fresh regardless of any later commit time"
        );

        // Genuine staleness still flags: a binary built well before the newest
        // source edit (beyond grace) means live sessions run pre-edit code.
        assert_eq!(
            classify_stale_install_artifacts(
                source_ts,
                &[("bin", Some(source_ts - grace - 1))],
                grace,
            ),
            vec!["bin"],
            "a binary older than the source edit beyond the grace must still flag"
        );
    }

    #[test]
    fn newest_artifact_mtime_uses_freshest_existing_path() {
        let dir = TempDir::new().unwrap();
        let old = dir.path().join("target/release/agent-doc");
        let fresh = dir
            .path()
            .join("target/local-install/release-local/agent-doc");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::create_dir_all(fresh.parent().unwrap()).unwrap();
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&fresh, "fresh").unwrap();

        filetime::set_file_mtime(&old, filetime::FileTime::from_unix_time(1_000, 0)).unwrap();
        filetime::set_file_mtime(&fresh, filetime::FileTime::from_unix_time(2_000, 0)).unwrap();

        assert_eq!(
            newest_artifact_mtime(&[old, fresh]),
            Some(2_000),
            "fresh local-install output should satisfy stale-install freshness"
        );
    }

    #[test]
    fn locate_agent_doc_source_repo_matches_root_and_dogfood_layout() {
        let agent_doc_manifest = "[package]\nname = \"agent-doc\"\nversion = \"0.0.0\"\n";

        // Standalone checkout: the git root itself is the crate.
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("Cargo.toml"), agent_doc_manifest).unwrap();
        assert_eq!(
            locate_agent_doc_source_repo(root.path()).as_deref(),
            Some(root.path())
        );

        // Dogfood superproject: source lives under src/agent-doc.
        let superproject = TempDir::new().unwrap();
        let src = superproject.path().join("src/agent-doc");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Cargo.toml"), agent_doc_manifest).unwrap();
        assert_eq!(locate_agent_doc_source_repo(superproject.path()), Some(src));

        // Unrelated repo (no agent-doc crate) → no warning source.
        let other = TempDir::new().unwrap();
        std::fs::write(
            other.path().join("Cargo.toml"),
            "[package]\nname = \"something-else\"\n",
        )
        .unwrap();
        assert!(locate_agent_doc_source_repo(other.path()).is_none());
    }
    #[test]
    fn preflight_output_omits_empty_claims_and_layout_issues() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        assert!(
            !json.contains("\"claims\""),
            "empty claims must be omitted from per-cycle output: {json}"
        );
        assert!(
            !json.contains("\"layout_issues\""),
            "empty layout_issues must be omitted from per-cycle output: {json}"
        );
        let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
        assert!(round_trip.claims.is_empty());
        assert!(round_trip.layout_issues.is_empty());

        // Non-empty values are still emitted and round-trip intact.
        let populated = PreflightOutput {
            claims: vec!["claimed pane %1".to_string()],
            layout_issues: vec!["stash overflow".to_string()],
            ..PreflightOutput::default()
        };
        let json = serde_json::to_string(&populated).unwrap();
        assert!(json.contains("\"claims\""), "{json}");
        assert!(json.contains("\"layout_issues\""), "{json}");
        let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.claims, vec!["claimed pane %1".to_string()]);
        assert_eq!(round_trip.layout_issues, vec!["stash overflow".to_string()]);
    }
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
    fn strike_done_queue_prompts_strikes_all_sibling_lines_for_one_resolved_id() {
        // #qstrikeall: when an id is resolved this cycle (e.g. `--review-resolve`
        // archives it to agent:done), EVERY queue line referencing that id must
        // strike — not just the leading one. A bare pin `[#id]` and a `do [#id]`
        // directive for the same resolved id are both struck, at any position,
        // even behind an unrelated live head. (The earlier single-strike report
        // was against a stale binary; this locks in the multi-strike behavior.)
        let entries = vec![
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "[#8667]".to_string(),
                multiline: false,
            }),
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#liveone]".to_string(),
                multiline: false,
            }),
            crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
                text: "do [#8667] follow-up".to_string(),
                multiline: false,
            }),
        ];
        let done_ids: std::collections::HashSet<String> =
            ["8667".to_string()].into_iter().collect();

        let (rewritten, struck) = super::strike_done_queue_head_prompts(&entries, &done_ids)
            .expect("both #8667 sibling lines must strike");
        assert_eq!(
            struck.len(),
            2,
            "both lines for the resolved id strike: {struck:?}"
        );
        assert!(matches!(
            &rewritten[0],
            crate::queue::QueueEntry::Completed(p) if p.text == "[#8667]"
        ));
        // The unrelated live head between them is preserved.
        assert!(matches!(
            &rewritten[1],
            crate::queue::QueueEntry::Prompt(p) if p.text == "do [#liveone]"
        ));
        assert!(matches!(
            &rewritten[2],
            crate::queue::QueueEntry::Completed(p) if p.text == "do [#8667] follow-up"
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
    fn ipc_dogfood_note_appends_to_exchange_and_dedupes() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );
        let diagnostic = "ipc_proof_insufficient file=/tmp/session.md source=socket_ack_content patch_id=abc invariant=live_prompt_drift_after_preflight recovery=visible_repair_required";

        let updated = super::append_ipc_dogfood_note_to_content(content, diagnostic)
            .unwrap()
            .expect("expected IPC note to append");

        assert!(updated.contains("IPC proof issue dogfood log"));
        assert!(updated.contains("Issue class: `ipc_proof_insufficient`"));
        assert!(updated.contains(diagnostic));
        assert!(
            updated.find("IPC proof issue dogfood log").unwrap()
                < updated.find("<!-- /agent:exchange -->").unwrap(),
            "note must stay inside agent:exchange"
        );
        assert!(
            updated.contains("- do [#next]\n<!-- /agent:queue -->"),
            "queue content must be preserved"
        );

        let second = super::append_ipc_dogfood_note_to_content(&updated, diagnostic).unwrap();
        assert!(second.is_none(), "same diagnostic should not duplicate");
    }

    #[test]
    fn ipc_dogfood_note_noops_without_exchange_component() {
        let content = "<!-- agent:queue -->\n- do [#next]\n<!-- /agent:queue -->\n";
        let diagnostic = "ipc_proof_insufficient file=/tmp/session.md source=file_ipc patch_id=- invariant=missing_response recovery=retry_without_disk_write";

        let updated = super::append_ipc_dogfood_note_to_content(content, diagnostic).unwrap();

        assert!(updated.is_none());
    }

    #[test]
    fn append_latest_ipc_dogfood_note_reads_matching_ops_log_entry() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let canonical = doc.canonicalize().unwrap();
        let diagnostic = format!(
            "ipc_proof_insufficient file={} source=socket_ack_content patch_id=abc invariant=live_prompt_drift_after_preflight recovery=visible_repair_required",
            canonical.display()
        );
        write_ops_log(
            &dir,
            &format!(
                "older irrelevant line\n[2026-06-23T00:00:00Z] {}\n",
                diagnostic
            ),
        );

        assert!(super::append_latest_ipc_dogfood_note(&doc).unwrap());
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("IPC proof issue dogfood log"));
        assert!(updated.contains(&diagnostic));

        assert!(!super::append_latest_ipc_dogfood_note(&doc).unwrap());
    }

    /// `#ipcqproof`: an IPC proof diagnostic appended during interrupted-cycle
    /// recovery must NOT become an unresolved prompt-bearing exchange item. The
    /// queue-consume `socket_ack_content` ACK mismatch (`live_prompt_drift_after_preflight`)
    /// and the `missing_response_probe` variant both record a fail-closed
    /// diagnostic; the appended dogfood note must classify as a binary-authored
    /// `RecoveryArtifact`, never a user `PromptTarget`, so it does not get
    /// `❯`-normalized into a prompt-only tail that forces a follow-up cycle.
    #[test]
    fn ipc_dogfood_note_is_recovery_artifact_not_prompt_bearing() {
        let before = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do [#next]\n",
            "<!-- /agent:queue -->\n",
        );

        let cases = [
            // socket ACK content mismatch on a queue-consume write.
            "ipc_proof_insufficient file=/tmp/session.md source=socket_ack_content patch_id=abc invariant=live_prompt_drift_after_preflight recovery=visible_repair_required",
            // queue-consume patch consumed without the response body present.
            "ipc_proof_insufficient file=/tmp/session.md source=socket_ack_content patch_id=- invariant=missing_response_probe recovery=retry_without_disk_write",
        ];

        for diagnostic in cases {
            let updated = super::append_ipc_dogfood_note_to_content(before, diagnostic)
                .unwrap()
                .expect("expected IPC note to append");

            // The note opens with a `### Re:` heading → RecoveryArtifact.
            assert!(
                updated.contains("### Re: IPC proof diagnostic"),
                "dogfood note must open with a ### Re: heading for {diagnostic}"
            );
            // Fail-closed recovery stays on the binary-owned path (no direct disk write).
            assert!(
                !diagnostic.contains("direct_write_fallback"),
                "IPC proof diagnostic must remain fail-closed for {diagnostic}"
            );

            // Mirrors `first_unstarted_prompt_bearing_change`: classify the diff
            // the prompt-bearing guard would see.
            let diff_text = crate::diff::unified_diff_from_contents(before, &updated)
                .expect("expected a non-empty diff after appending the note");
            let changes = crate::diff::classify_prompt_bearing_changes(&diff_text);
            assert!(
                !changes
                    .iter()
                    .any(|c| matches!(c.kind, crate::diff::PromptBearingChangeKind::PromptTarget)),
                "dogfood note must not classify as a PromptTarget for {diagnostic}: {changes:?}"
            );
            assert!(
                changes.iter().any(|c| matches!(
                    c.kind,
                    crate::diff::PromptBearingChangeKind::RecoveryArtifact
                )),
                "dogfood note must classify as a RecoveryArtifact for {diagnostic}: {changes:?}"
            );
            // No `❯` prompt-prefix normalization may be derived from the note.
            assert!(
                crate::diff::prompt_prefix_normalization_targets(&diff_text).is_empty(),
                "dogfood note must not trigger prompt-prefix normalization for {diagnostic}"
            );
            // The exchange tail is not left as a prompt-only tail.
            assert!(
                crate::session_check::prompt_only_exchange_tail(&updated).is_none(),
                "dogfood note must not leave a prompt-only exchange tail for {diagnostic}"
            );
        }
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
                in_progress: false,
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
                in_progress: false,
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
                in_progress: false,
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
                in_progress: false,
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
            in_progress: false,
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
        // backlog are recognized sync attributes and must not warn.
        for marker in [
            "<!-- agent:backlog queue -->",
            "<!-- agent:backlog queue=sync -->",
            "<!-- agent:backlog queue=append -->",
        ] {
            let content = format!("{marker}\n- [ ] [#x1] keep this\n<!-- /agent:backlog -->\n");
            assert!(
                misplaced_component_attr_warning(Path::new("session.md"), &content).is_none(),
                "recognized queue sync attr must not warn: {marker}"
            );
        }
    }
    #[test]
    fn misplaced_component_attr_warning_flags_queue_sync_attr_on_icebox() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:icebox queue=append -->\n",
            "- [ ] [#x1] parked work\n",
            "<!-- /agent:icebox -->\n",
        );
        let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
            .expect("`queue` on agent:icebox should warn");
        assert_eq!(warning.code, "misplaced_component_attr");
        assert!(warning.message.contains("agent:icebox"));
        assert!(warning.message.contains("does not auto-populate"));
        assert!(warning.message.contains("per-item enqueue"));
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
        let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
            .expect("`preset` on backlog should warn as a queue-only attribute on wrong component");
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
    fn preflight_output_includes_semantic_diff_when_set() {
        let output = PreflightOutput {
            semantic_diff: Some(SemanticDiffSummary {
                schema_version: 1,
                changed_components: vec!["queue".to_string()],
                node_events: vec![SemanticNodeEvent {
                    component: "queue".to_string(),
                    node_key: "queue:0:task:0".to_string(),
                    op: "insert".to_string(),
                    item_id: "task".to_string(),
                    before_index: None,
                    after_index: Some(0),
                    previous_node_key: None,
                    next_node_key: None,
                    before_preview: None,
                    after_preview: Some("- do [#task]".to_string()),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["semantic_diff"]["schema_version"], 1);
        assert_eq!(parsed["semantic_diff"]["changed_components"][0], "queue");
        assert_eq!(parsed["semantic_diff"]["node_events"][0]["op"], "insert");
    }
    #[test]
    fn preflight_output_omits_semantic_diff_when_none() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("semantic_diff").is_none(),
            "semantic_diff should be omitted when absent"
        );
    }
    #[test]
    fn preflight_output_semantic_merge_acks_roundtrip() {
        // #semmerge-ack-turn (Phase 4): carried acks serialize for skill
        // consumption and are omitted when empty.
        let empty = PreflightOutput::default();
        let empty_json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&empty).unwrap()).unwrap();
        assert!(
            empty_json.get("semantic_merge_acks").is_none(),
            "semantic_merge_acks omitted when empty"
        );

        let output = PreflightOutput {
            semantic_merge_acks: vec![crate::cycle_state::PendingSemanticMergeAck {
                component: "exchange".to_string(),
                id: "p3kj".to_string(),
                reason: "operator_deleted_agent_edited_node".to_string(),
                detail: "operator deleted the node the agent edited".to_string(),
                recorded_cycle_id: Some("cycle-1".to_string()),
                surfaced: true,
            }],
            ..Default::default()
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&output).unwrap()).unwrap();
        assert_eq!(parsed["semantic_merge_acks"][0]["component"], "exchange");
        assert_eq!(parsed["semantic_merge_acks"][0]["id"], "p3kj");
        assert_eq!(
            parsed["semantic_merge_acks"][0]["reason"],
            "operator_deleted_agent_edited_node"
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
                clear_threshold: 50,
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
}
