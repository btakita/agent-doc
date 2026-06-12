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

fn is_zero_usize(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone)]
struct SemanticComponentSnapshot {
    name: String,
    occurrence: usize,
    attrs: HashMap<String, String>,
    content: String,
    nav: SemanticNavTarget,
}

fn semantic_diff_summary(
    previous: &str,
    current: &str,
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
) -> Option<SemanticDiffSummary> {
    let mut changed_components = BTreeSet::new();
    let mut component_changes = semantic_component_changes(previous, current);
    let mut node_events = agent_doc_markdown_ast::events::diff_node_events(previous, current)
        .into_iter()
        .map(|event| {
            changed_components.insert(event.component.clone());
            SemanticNodeEvent {
                component: event.component,
                node_key: event.node_key,
                op: semantic_node_event_kind(event.kind).to_string(),
                item_id: event.item_id,
                before_index: event.before_index,
                after_index: event.after_index,
                previous_node_key: event.previous_node_key,
                next_node_key: event.next_node_key,
                before_preview: event.before.as_deref().and_then(semantic_preview),
                after_preview: event.after.as_deref().and_then(semantic_preview),
            }
        })
        .collect::<Vec<_>>();
    let prompt_changes = prompt_bearing_changes
        .iter()
        .filter_map(|change| {
            changed_components.insert("exchange".to_string());
            semantic_preview(&change.text).map(|text_preview| SemanticPromptChange {
                kind: change.kind.clone(),
                text_preview,
            })
        })
        .collect::<Vec<_>>();

    for change in &component_changes {
        changed_components.insert(change.component.clone());
    }
    node_events.sort_by(|a, b| {
        a.component
            .cmp(&b.component)
            .then_with(|| a.after_index.cmp(&b.after_index))
            .then_with(|| a.before_index.cmp(&b.before_index))
            .then_with(|| a.node_key.cmp(&b.node_key))
    });

    if component_changes.is_empty() && node_events.is_empty() && prompt_changes.is_empty() {
        return None;
    }

    component_changes.sort_by(|a, b| {
        a.component
            .cmp(&b.component)
            .then_with(|| a.occurrence.cmp(&b.occurrence))
    });

    Some(SemanticDiffSummary {
        schema_version: 1,
        changed_components: changed_components.into_iter().collect(),
        component_changes,
        node_events,
        prompt_changes,
    })
}

/// Build durable op-log records from this cycle's semantic node events
/// (`#op-scoped-drift-1`). Preflight observes a snapshot↔document diff, so every
/// node op is classified as a `user` edit (the agent's committed output already
/// lives in the snapshot). Pure so it can be unit-tested without a database.
fn build_ops_from_semantic_diff(
    document_path: &str,
    origin_session: Option<&str>,
    recorded_at: &str,
    summary: &SemanticDiffSummary,
) -> Vec<agent_doc_core::op_log::DocumentOp> {
    use agent_doc_core::op_log::{CausalClock, DocumentOp, OpSource, classify_actor};
    let actor = classify_actor(OpSource::SnapshotDiff);
    summary
        .node_events
        .iter()
        .map(|event| DocumentOp {
            document_path: document_path.to_string(),
            component: event.component.clone(),
            node_key: event.node_key.clone(),
            // Within-component node index: after-index for inserts/replaces,
            // before-index for removes. Feeds the exchange-tail narrowing in the
            // affectedness classifier (`#loop-guard-exchange-node-granularity`).
            node_index: event.after_index.or(event.before_index),
            item_id: event.item_id.clone(),
            op_kind: event.op.clone(),
            actor,
            clock: CausalClock {
                lamport: 0,
                origin_session: origin_session.map(str::to_string),
            },
            before_preview: event.before_preview.clone(),
            after_preview: event.after_preview.clone(),
            recorded_at: Some(recorded_at.to_string()),
        })
        .collect()
}

/// Persist the cycle's node ops to the durable sqlite op log. Best effort:
/// failures are logged to stderr and never propagate, so the durable substrate
/// can never block a preflight cycle.
fn persist_op_log(
    file: &Path,
    rc: &crate::graph::RunContext,
    origin_session: Option<&str>,
    summary: &SemanticDiffSummary,
) {
    if summary.node_events.is_empty() {
        return;
    }
    let Some(project_root) = rc.project_root() else {
        return;
    };
    let document_path = file.to_string_lossy().to_string();
    let recorded_at = op_log_timestamp().to_string();
    let ops = build_ops_from_semantic_diff(&document_path, origin_session, &recorded_at, summary);
    if let Err(err) = agent_doc_sqlite::op_log::append_ops(&project_root, &ops) {
        eprintln!("[preflight] op-log persist skipped: {err}");
    }
}

fn op_log_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Compute `user_intent_prompt_changes` — the set the Claude Code auto-loop
/// guard treats as "real user intent that must preempt an active queue drain".
/// The loop halts only when this is non-empty, so it must exclude everything
/// that is *not* fresh intent for the current turn:
///
/// - a synthetic queue-continuation diff (`diff_from_queue_head_only`) is queue
///   bookkeeping, never user intent;
/// - managed-component edits (queue/backlog/review/done items, activity toggles)
///   are routine session bookkeeping (`change_is_managed_state_only`);
/// - edits the affectedness classifier scoped as **independent** of the current
///   turn — i.e. targeting no address in the turn's read/write set
///   (`#queue-no-stop-unrelated-edit`). When `op_affectedness` ran and reports
///   `turn_affected == false`, every user op this cycle was independent or
///   provenance-spoofed, so the drain must not stop.
///
/// A genuine user prompt typed mid-loop edits the in-scope `exchange` tail, which
/// classifies as turn-affecting (`InputAffecting`/`OutputContended`), so
/// `turn_affected` is `true` and the prompt still preempts. When the classifier
/// did not run (`op_affectedness` is `None`, e.g. a semantic-diff parse skip),
/// this stays conservative and falls back to the managed-state filter only.
fn compute_user_intent_prompt_changes(
    prompt_bearing_changes: &[crate::diff::PromptBearingChange],
    diff_from_queue_head_only: bool,
    op_affectedness: Option<&agent_doc_core::turn_scope::CycleAffectedness>,
) -> Vec<crate::diff::PromptBearingChange> {
    if diff_from_queue_head_only {
        // Synthetic auto-queue continuation only — no user intent this cycle.
        return Vec::new();
    }
    if op_affectedness.is_some_and(|affectedness| !affectedness.turn_affected) {
        // The classifier ran and scoped every user op this cycle as independent
        // of the turn — nothing affects it, so the drain must not halt.
        return Vec::new();
    }
    prompt_bearing_changes
        .iter()
        .filter(|change| !crate::diff::change_is_managed_state_only(change))
        .cloned()
        .collect()
}

/// Derive the TurnScope manifest for the current turn (`#op-scoped-drift-2`).
/// Resolves the driver queue node from `prompt_targets`, then builds the
/// canonical read/write sets. Returns `None` when the turn answers no prompt.
fn derive_turn_scope(
    content: &str,
    prompt_targets: &[String],
) -> Option<agent_doc_core::turn_scope::TurnScope> {
    if prompt_targets.is_empty() {
        return None;
    }
    let driver = resolve_driver_address(content, prompt_targets);
    let exchange_tail_floor = exchange_node_count(content);
    Some(
        agent_doc_core::turn_scope::TurnScope::for_driver_with_exchange_tail(
            driver,
            exchange_tail_floor,
        ),
    )
}

/// Count of `exchange` item nodes present at turn start — the tail floor for the
/// affectedness classifier (`#loop-guard-exchange-node-granularity`). An op at an
/// index at or above this count is a tail append/edit (affects the turn); below it
/// is committed history. Returns `None` when there are no exchange nodes so the
/// classifier keeps its coarse whole-component behavior.
fn exchange_node_count(content: &str) -> Option<usize> {
    let count = agent_doc_markdown_ast::mutations::all_item_nodes(content)
        .iter()
        .filter(|node| node.component == "exchange")
        .count();
    (count > 0).then_some(count)
}

/// Find the queue item node a prompt target refers to and address it.
fn resolve_driver_address(
    content: &str,
    prompt_targets: &[String],
) -> Option<agent_doc_core::turn_scope::Address> {
    let nodes = agent_doc_markdown_ast::mutations::all_item_nodes(content);
    for target in prompt_targets {
        let Some(id) = extract_target_id(target) else {
            continue;
        };
        if let Some(node) = nodes
            .iter()
            .find(|node| node.component == "queue" && node.item.id == id)
        {
            let occurrence = component_occurrence_from_node_key(&node.node_key);
            return Some(agent_doc_core::turn_scope::Address::node(
                "queue",
                occurrence,
                &node.node_key,
            ));
        }
    }
    None
}

/// Extract a backlog/queue id (`[#id]` or bare `#id`) from a prompt target.
fn extract_target_id(target: &str) -> Option<String> {
    if let Some(start) = target.find("[#") {
        let rest = &target[start + 2..];
        if let Some(close) = rest.find(']') {
            let id = &rest[..close];
            if agent_doc_core::pending::is_valid_pending_id(id) {
                return Some(id.to_string());
            }
        }
    }
    if let Some(start) = target.find('#') {
        let rest = &target[start + 1..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !id.is_empty() && agent_doc_core::pending::is_valid_pending_id(&id) {
            return Some(id);
        }
    }
    None
}

/// Component occurrence index encoded in a node key (`component:index:id:dup`).
fn component_occurrence_from_node_key(node_key: &str) -> usize {
    node_key
        .split(':')
        .nth(1)
        .and_then(|field| field.parse().ok())
        .unwrap_or(0)
}

fn semantic_component_changes(previous: &str, current: &str) -> Vec<SemanticComponentChange> {
    let before = semantic_component_snapshots("before", previous);
    let after = semantic_component_snapshots("after", current);
    let mut keys = BTreeSet::new();
    keys.extend(before.keys().cloned());
    keys.extend(after.keys().cloned());

    let mut changes = Vec::new();
    for key in keys {
        match (before.get(&key), after.get(&key)) {
            (None, Some(after_snapshot)) => changes.push(SemanticComponentChange {
                component: after_snapshot.name.clone(),
                occurrence: after_snapshot.occurrence,
                op: SemanticComponentOp::Added,
                before: None,
                after: Some(after_snapshot.nav.clone()),
            }),
            (Some(before_snapshot), None) => changes.push(SemanticComponentChange {
                component: before_snapshot.name.clone(),
                occurrence: before_snapshot.occurrence,
                op: SemanticComponentOp::Removed,
                before: Some(before_snapshot.nav.clone()),
                after: None,
            }),
            (Some(before_snapshot), Some(after_snapshot))
                if before_snapshot.content != after_snapshot.content
                    || before_snapshot.attrs != after_snapshot.attrs =>
            {
                changes.push(SemanticComponentChange {
                    component: after_snapshot.name.clone(),
                    occurrence: after_snapshot.occurrence,
                    op: SemanticComponentOp::Changed,
                    before: Some(before_snapshot.nav.clone()),
                    after: Some(after_snapshot.nav.clone()),
                });
            }
            _ => {}
        }
    }

    if let Some(change) = semantic_frontmatter_change(previous, current) {
        changes.push(change);
    }

    changes
}

fn semantic_component_snapshots(
    side: &str,
    source: &str,
) -> BTreeMap<(String, usize), SemanticComponentSnapshot> {
    let components = match crate::component::parse(source) {
        Ok(components) => components,
        Err(err) => {
            eprintln!("[preflight] semantic_diff: component parse skipped: {err}");
            return BTreeMap::new();
        }
    };
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    let mut snapshots = BTreeMap::new();
    for component in components {
        let occurrence = occurrences.entry(component.name.clone()).or_insert(0);
        let occurrence_value = *occurrence;
        *occurrence += 1;
        let nav = semantic_nav_target(
            side,
            &component.name,
            occurrence_value,
            source,
            component.open_start,
            component.close_end,
        );
        snapshots.insert(
            (component.name.clone(), occurrence_value),
            SemanticComponentSnapshot {
                name: component.name.clone(),
                occurrence: occurrence_value,
                attrs: component.attrs.clone(),
                content: component.content(source).to_string(),
                nav,
            },
        );
    }
    snapshots
}

fn semantic_frontmatter_change(previous: &str, current: &str) -> Option<SemanticComponentChange> {
    let before_span = frontmatter_span(previous);
    let after_span = frontmatter_span(current);
    let before_text = before_span.and_then(|(start, end)| previous.get(start..end));
    let after_text = after_span.and_then(|(start, end)| current.get(start..end));
    if before_text == after_text {
        return None;
    }

    let before = before_span
        .map(|(start, end)| semantic_nav_target("before", "frontmatter", 0, previous, start, end));
    let after = after_span
        .map(|(start, end)| semantic_nav_target("after", "frontmatter", 0, current, start, end));
    let op = match (before.is_some(), after.is_some()) {
        (false, true) => SemanticComponentOp::Added,
        (true, false) => SemanticComponentOp::Removed,
        _ => SemanticComponentOp::Changed,
    };
    Some(SemanticComponentChange {
        component: "frontmatter".to_string(),
        occurrence: 0,
        op,
        before,
        after,
    })
}

fn frontmatter_span(source: &str) -> Option<(usize, usize)> {
    let mut offset = 0usize;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += line.len();
        if index == 0 {
            if line.trim_end() != "---" {
                return None;
            }
            continue;
        }
        if line.trim_end() == "---" {
            return Some((0, offset));
        }
        if line_start == source.len() {
            break;
        }
    }
    None
}

fn semantic_nav_target(
    side: &str,
    component: &str,
    occurrence: usize,
    source: &str,
    start_byte: usize,
    end_byte: usize,
) -> SemanticNavTarget {
    let start_byte = start_byte.min(source.len());
    let end_byte = end_byte.min(source.len()).max(start_byte);
    let start_line = semantic_line_at(source, start_byte);
    let end_line = if end_byte == start_byte {
        start_line
    } else {
        semantic_line_at(source, end_byte.saturating_sub(1))
    };
    SemanticNavTarget {
        handle: format!("component:{side}:{component}:{occurrence}"),
        component: component.to_string(),
        occurrence,
        start_line,
        end_line,
        start_byte,
        end_byte,
    }
}

fn semantic_line_at(source: &str, byte: usize) -> usize {
    let end = byte.min(source.len());
    source.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

fn semantic_node_event_kind(
    kind: agent_doc_markdown_ast::events::DocumentNodeEventKind,
) -> &'static str {
    match kind {
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Insert => "insert",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Remove => "remove",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Replace => "replace",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Move => "move",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Strike => "strike",
        agent_doc_markdown_ast::events::DocumentNodeEventKind::Unstrike => "unstrike",
    }
}

fn semantic_preview(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 200;
    let mut preview = trimmed.chars().take(MAX_CHARS).collect::<String>();
    if trimmed.chars().count() > MAX_CHARS {
        preview.push_str("...");
    }
    Some(preview)
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
    let project_root = file.canonicalize().ok().and_then(|canonical| {
        snapshot::find_project_root(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let already_done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

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

        // #reviewrm: collapse identical same-id entries an interleaved finalize
        // can leave behind (the duplicate `[/] #id` pair preflight flags as
        // preset_item_id_collision). Only exact duplicates are removed; distinct
        // items that merely share an id are preserved so the ambiguity warning
        // still surfaces.
        let (after_dedupe, deduped_ids) = crate::pending::op_dedupe_identical_items(&current_body);
        if !deduped_ids.is_empty() {
            eprintln!(
                "[preflight] {}: deduped {} duplicate same-id entr{}: {}",
                surface_label,
                deduped_ids.len(),
                if deduped_ids.len() == 1 { "y" } else { "ies" },
                deduped_ids.join(", ")
            );
            current_body = after_dedupe;
            mutated = true;
        }

        if should_reap_already_done_mirrors(surface) && !already_done_ids.is_empty() {
            let (after_mirror_reap, mirror_items) =
                crate::pending::op_take_active_items_by_ids(&current_body, &already_done_ids);
            if !mirror_items.is_empty() {
                let removed_ids: Vec<String> = mirror_items.iter().map(|i| i.id.clone()).collect();
                eprintln!(
                    "[preflight] {}: reaped {} already-done mirror item(s): {}",
                    surface_label,
                    mirror_items.len(),
                    removed_ids.join(", ")
                );
                current_body = after_mirror_reap;
                mutated = true;
            }
        }

        let mut removed_items = Vec::new();
        if should_reap_ops_proof_completions(surface) {
            // #opsproof-falsepos: never auto-archive an item that was added this
            // same cycle. A brand-new add is absent from the post-commit snapshot
            // captured at cycle start; such items describe just-landed dependency
            // work and must be closed explicitly, not reaped on the cycle they
            // appear. Only apply the guard when we have a snapshot baseline to
            // compare against (untracked scaffold docs have none).
            let snapshot_baseline = snapshot_at_start
                .as_deref()
                .filter(|s| !s.trim().is_empty());
            let snapshot_ids = snapshot_baseline.map(|snap| surface_pending_ids(snap, surface));
            // `#opsproof-samecycle-add`: the snapshot baseline alone is not enough.
            // In the `write`/`finalize` path the same invocation that adds an item
            // via `--review-add` / `--pending-add*` also re-syncs the on-disk
            // snapshot, so a brand-new same-cycle add is already present in
            // `snapshot_ids` and the snapshot test cannot exclude it. Cross-check
            // the ids cycle-state recorded as added this cycle and never reap them.
            let added_this_cycle = crate::cycle_state::pending_added_ids(file);
            let ops_proof_completions: Vec<OpsProofCompletion> =
                ops_proof_completion_candidates(&current_body)
                    .into_iter()
                    .filter(|candidate| {
                        snapshot_ids
                            .as_ref()
                            .is_none_or(|ids| ids.contains(&candidate.id))
                    })
                    .filter(|candidate| !added_this_cycle.contains(&candidate.id))
                    .collect();
            if !ops_proof_completions.is_empty() {
                let evidence_by_id: HashMap<String, String> = ops_proof_completions
                    .iter()
                    .map(|candidate| (candidate.id.clone(), candidate.evidence.clone()))
                    .collect();
                let ids: HashSet<String> = ops_proof_completions
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect();
                let (after_ops_proof_reap, mut ops_proof_items) =
                    crate::pending::op_take_active_items_by_ids(&current_body, &ids);
                if !ops_proof_items.is_empty() {
                    let removed_ids: Vec<String> =
                        ops_proof_items.iter().map(|i| i.id.clone()).collect();
                    for item in &mut ops_proof_items {
                        item.state = crate::pending::PendingState::Done;
                        item.gate_type = None;
                    }
                    eprintln!(
                        "[preflight] {}: auto-completed {} ops-proof item(s): {}",
                        surface_label,
                        ops_proof_items.len(),
                        removed_ids.join(", ")
                    );
                    for item in &ops_proof_items {
                        let evidence = evidence_by_id
                            .get(&item.id)
                            .map(String::as_str)
                            .unwrap_or("ops_proof");
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "auto_complete_ops_proof file={} id={} surface={} evidence={}",
                                file.display(),
                                item.id,
                                surface_label,
                                evidence
                            ),
                        );
                    }
                    let _ = crate::cycle_state::record_pending_done_ids(file, &removed_ids);
                    let _ = crate::cycle_state::record_reaped_pending_ids(file, &removed_ids);
                    let _ = crate::cycle_state::mark_pending_mutations(file);
                    current_body = after_ops_proof_reap;
                    mutated = true;
                    removed_items.extend(ops_proof_items);
                }
            }
        }

        let (after_reap, reaped_items) = crate::pending::reap_with_items(&current_body)?;
        if !reaped_items.is_empty() {
            let removed_ids: Vec<String> = reaped_items.iter().map(|i| i.id.clone()).collect();
            eprintln!(
                "[preflight] {}: reaped {} item(s): {}",
                surface_label,
                reaped_items.len(),
                removed_ids.join(", ")
            );
            let _ = crate::cycle_state::record_reaped_pending_ids(file, &removed_ids);
            current_body = after_reap;
            mutated = true;
        }
        removed_items.extend(reaped_items);

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
    } else if is_review_component(surface) {
        "review"
    } else {
        "icebox"
    }
}

fn should_reap_already_done_mirrors(surface: &str) -> bool {
    is_backlog_component(surface) || is_review_component(surface)
}

fn should_reap_ops_proof_completions(surface: &str) -> bool {
    is_backlog_component(surface) || is_review_component(surface)
}

struct OpsProofCompletion {
    id: String,
    evidence: String,
}

/// Pending item ids present in `surface` within `content`. Used to detect
/// brand-new same-cycle adds (absent from the cycle-start snapshot) so ops-proof
/// auto-completion never reaps an item on the cycle it first appears.
fn surface_pending_ids(content: &str, surface: &str) -> HashSet<String> {
    crate::component::parse(content)
        .ok()
        .and_then(|comps| {
            comps
                .into_iter()
                .find(|c| component_matches_tracked_surface(&c.name, surface))
        })
        .map(|comp| {
            let (_, items, _) = crate::pending::parse_items(comp.content(content));
            items
                .into_iter()
                .map(|item| item.id)
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn ops_proof_completion_candidates(body: &str) -> Vec<OpsProofCompletion> {
    let (_, items, _) = crate::pending::parse_items(body);
    items
        .iter()
        .filter(|item| !matches!(item.state, crate::pending::PendingState::Done))
        .filter_map(|item| {
            classify_ops_proof_completion(item).map(|evidence| OpsProofCompletion {
                id: item.id.clone(),
                evidence,
            })
        })
        .collect()
}

fn classify_ops_proof_completion(item: &crate::pending::PendingItem) -> Option<String> {
    if item.id.is_empty() {
        return None;
    }
    let text = format!("{} {}", item.text, item.continuation);
    let upper = text.to_ascii_uppercase();
    if !has_ops_completion_marker(&upper) || has_ops_completion_blocker(&upper) {
        return None;
    }

    // #opsproofgate: a live-verify / operator-drive gate must NEVER be
    // auto-completed on `evidence=commit`. A shipped commit is not proof for
    // these items — only an anchored `^[epoch] <marker>` line in ops.log
    // (driven live by the operator) is. The `#optverify` log-arbiter path
    // (`run_gate_verify`) closes them on a genuine structured emission; this
    // commit/CI prose scan must stay out of their way, or a submodule hash
    // cited in the gate text falsely archives an UNDRIVEN gate to done.
    if is_live_verify_gate(&upper) {
        return None;
    }

    // #opsproof-falsepos: an open (non-gated) actionable item must NOT be reaped
    // just because its prose cites already-landed dependency work ("the predicate
    // already shipped in abc1234"). The completion marker must be the item's own
    // leading status verb. Gated items were deliberately code-completed by the
    // agent, so a proven marker anywhere in their text legitimately closes them.
    let is_gated = matches!(item.state, crate::pending::PendingState::Gated);
    if !is_gated && !marker_is_leading_status(&upper) {
        return None;
    }

    let has_commit = contains_commit_hash(&text);
    let has_ci = contains_successful_ci_proof(&upper);
    if !has_commit && !has_ci {
        return None;
    }

    Some(
        match (has_commit, has_ci) {
            (true, true) => "commit+ci",
            (true, false) => "commit",
            (false, true) => "ci",
            (false, false) => unreachable!(),
        }
        .to_string(),
    )
}

fn has_ops_completion_marker(upper: &str) -> bool {
    ["DONE", "SHIPPED", "IMPLEMENTED", "COMPLETE", "COMPLETED"]
        .iter()
        .any(|marker| contains_ascii_word(upper, marker))
}

/// Max number of leading words (after skipping `#hashtag` tokens) that count as
/// the item's status prefix for ops-proof auto-completion.
const LEADING_STATUS_WORDS: usize = 4;

/// True when an ops-completion marker is the item's leading status verb rather
/// than a marker buried in a cited dependency clause. The leading status segment
/// is the prefix before the first clause break (`: ` or `. `), further capped to
/// the first [`LEADING_STATUS_WORDS`] words after skipping leading `#hashtag`
/// tokens. `upper` must already be ASCII-uppercased.
fn marker_is_leading_status(upper: &str) -> bool {
    has_ops_completion_marker(&leading_status_segment(upper))
}

fn leading_status_segment(upper: &str) -> String {
    let mut cut = upper.len();
    for sep in [": ", ". "] {
        if let Some(idx) = upper.find(sep) {
            cut = cut.min(idx);
        }
    }
    upper[..cut]
        .split_whitespace()
        .filter(|word| !word.starts_with('#'))
        .take(LEADING_STATUS_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_ops_completion_blocker(upper: &str) -> bool {
    const BLOCKER_PHRASES: &[&str] = &[
        "COULD NOT",
        "CAN NOT",
        "CANNOT",
        "CAN'T",
        "FALSE CLOSEOUT",
        "FOLLOW-UP",
        "FOLLOW UP",
        "FOLLOWUPS",
        "NOT DONE",
        "NOT SHIPPED",
        "NOT IMPLEMENTED",
        "SUB-PART",
        "SUBPART",
    ];
    const BLOCKER_WORDS: &[&str] = &[
        "PARTIAL",
        "REMAINING",
        "REOPENED",
        "DEFERRED",
        "BLOCKED",
        "BLOCKER",
        "TODO",
        "WIP",
        "PARTLY",
        "FAILING",
        "FAILED",
    ];

    BLOCKER_PHRASES.iter().any(|phrase| upper.contains(phrase))
        || BLOCKER_WORDS
            .iter()
            .any(|word| contains_ascii_word(upper, word))
}

/// True when an item is a live-verify / operator-drive gate whose only valid
/// completion proof is an anchored structured ops.log marker driven live by the
/// operator — never a cited commit/CI reference (`#opsproofgate`). `upper` must
/// already be ASCII-uppercased.
fn is_live_verify_gate(upper: &str) -> bool {
    const LIVE_VERIFY_PHRASES: &[&str] = &[
        "LIVE-VERIFY GATE",
        "LIVE-VERIFY ONLY",
        "LIVE VERIFY GATE",
        "LIVE VERIFY ONLY",
        "OPERATOR-DRIVE",
        "OPERATOR DRIVE",
        "OPERATOR DRIVES",
        "OPERATOR LIVE-VERIFY",
        "OPERATOR LIVE VERIFY",
    ];
    LIVE_VERIFY_PHRASES
        .iter()
        .any(|phrase| upper.contains(phrase))
}

fn contains_successful_ci_proof(upper: &str) -> bool {
    contains_ascii_word(upper, "CI")
        && ["GREEN", "PASSED", "PASSING", "SUCCESS", "SUCCEEDED"]
            .iter()
            .any(|word| contains_ascii_word(upper, word))
}

fn contains_commit_hash(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| {
            (7..=40).contains(&token.len())
                && token.chars().all(|c| c.is_ascii_hexdigit())
                && token.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'))
        })
}

fn contains_ascii_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let before = idx
            .checked_sub(1)
            .and_then(|pos| haystack.as_bytes().get(pos).copied());
        let after = haystack.as_bytes().get(idx + needle.len()).copied();
        before.is_none_or(|b| !b.is_ascii_alphanumeric())
            && after.is_none_or(|b| !b.is_ascii_alphanumeric())
    })
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

/// Opportunistic gated-review auto-verification (`#optverify` / `#optv3`).
///
/// For each gated `[/]` review item carrying a verify predicate, scan `ops.log`
/// and surface `provable` / `failed` / `pending`. When `autoverify` is true and
/// an item is `provable`, flip it `[/]→[x]` in place (persisting to both the
/// working-tree file and the snapshot, mirroring pending maintenance), so the
/// existing reap pass archives it on a later cycle. Default off — without the
/// opt-in the gate is only surfaced, never silently flipped.
///
/// Returns the per-item results for the preflight output. Best-effort: a missing
/// `ops.log`, no review component, or no predicates yields an empty vector.
fn run_gate_verify(file: &Path, autoverify: bool) -> Result<Vec<GateVerifyResult>> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let components = match crate::component::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(review) = components
        .iter()
        .find(|c| is_review_component(&c.name))
        .cloned()
    else {
        return Ok(Vec::new());
    };
    let body = review.content(&content).to_string();
    let (_, items, _) = crate::pending::parse_items(&body);

    // Gather predicate-bearing gated items.
    let predicates: Vec<(String, crate::gate_verify::GatePredicate)> = items
        .iter()
        .filter(|item| matches!(item.state, crate::pending::PendingState::Gated))
        .filter_map(|item| {
            crate::gate_verify::parse_gate_predicate(&item.text)
                .filter(|p| p.is_actionable())
                .map(|p| (item.id.clone(), p))
        })
        .collect();
    if predicates.is_empty() {
        return Ok(Vec::new());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let ops_log = snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
        .and_then(|root| std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).ok())
        .unwrap_or_default();

    let mut results = Vec::new();
    let mut to_resolve: Vec<String> = Vec::new();
    for (id, predicate) in &predicates {
        let outcome = crate::gate_verify::scan_ops_log(predicate, &ops_log);
        let (marker, at) = match &outcome {
            crate::gate_verify::VerifyOutcome::Provable { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            crate::gate_verify::VerifyOutcome::Failed { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            crate::gate_verify::VerifyOutcome::Pending => (None, None),
        };
        let status = outcome.status_str().to_string();
        let provable = matches!(outcome, crate::gate_verify::VerifyOutcome::Provable { .. });
        let auto_resolved = autoverify && provable;
        if auto_resolved {
            to_resolve.push(id.clone());
        }
        match &outcome {
            crate::gate_verify::VerifyOutcome::Provable { marker, at } => {
                eprintln!(
                    "[preflight] optverify: review #{} provable (marker {:?} @ {})",
                    id, marker, at
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "optverify review={} status=provable marker={:?} at={} auto_resolved={}",
                        id, marker, at, auto_resolved
                    ),
                );
            }
            crate::gate_verify::VerifyOutcome::Failed { marker, at } => {
                eprintln!(
                    "[preflight] optverify: review #{} FAILED (disproof {:?} @ {}) — file a bug",
                    id, marker, at
                );
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "optverify review={} status=failed marker={:?} at={}",
                        id, marker, at
                    ),
                );
            }
            crate::gate_verify::VerifyOutcome::Pending => {}
        }
        results.push(GateVerifyResult {
            id: id.clone(),
            status,
            marker,
            at,
            auto_resolved,
        });
    }

    // Opt-in transition: flip provable gates [/]→[x] in place, persisting to
    // both the working-tree file and the snapshot.
    if !to_resolve.is_empty() {
        let mut new_body = body.clone();
        for id in &to_resolve {
            new_body = crate::pending::op_done(&new_body, id)?;
        }
        let new_content = review.replace_content(&content, &new_body);
        std::fs::write(file, &new_content)
            .with_context(|| format!("failed to write {} after optverify", file.display()))?;
        // Keep the snapshot in lockstep so the upcoming commit stages the flip.
        if let Some(snap) = snapshot::load(file)?
            && let Ok(snap_comps) = crate::component::parse(&snap)
            && let Some(snap_review) = snap_comps.iter().find(|c| is_review_component(&c.name))
        {
            let snap_new = snap_review.replace_content(&snap, &new_body);
            snapshot::save(file, &snap_new)?;
        }
        eprintln!(
            "[preflight] optverify: auto-resolved {} provable gate(s): {}",
            to_resolve.len(),
            to_resolve.join(", ")
        );
    }

    Ok(results)
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
/// Collect the backlog→queue sync request from `agent:backlog`
/// (and the legacy `pending` alias) components carrying a `queue` attribute
/// (`#backlog-queue-sync-attr`). Returns the effective mode (the first
/// queue-tagged component's mode wins) and the active item ids from every
/// queue-tagged source component, in document order. Returns `None` when no
/// source component carries a recognized `queue` attribute. Icebox items are
/// intentionally excluded from component-level sync so a drained backlog cannot
/// auto-promote parked work; explicit per-item enqueue markers still work.
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

struct BacklogQueueSyncRequest {
    mode: crate::queue::BacklogQueueSyncMode,
    ids: Vec<String>,
    enqueue_ids: Vec<String>,
    priority: bool,
}

fn collect_backlog_queue_sync(
    components: &[crate::component::Component],
    content: &str,
) -> Option<BacklogQueueSyncRequest> {
    let mut mode: Option<crate::queue::BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    let mut enqueue_ids: Vec<String> = Vec::new();
    let mut priority = false;
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        enqueue_ids.extend(crate::pending::active_enqueue_item_ids(body));
        if comp.name == "icebox" {
            continue;
        }
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        priority |= comp.attrs.contains_key("priority");
        let Some(comp_mode) = crate::queue::BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        ids.extend(crate::pending::active_item_ids(body));
    }
    if mode.is_none() && !enqueue_ids.is_empty() {
        mode = Some(crate::queue::BacklogQueueSyncMode::Append);
    }
    ids.extend(enqueue_ids.iter().cloned());
    mode.map(|m| BacklogQueueSyncRequest {
        mode: m,
        ids,
        enqueue_ids,
        priority,
    })
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

/// Build an id→`after=#id` dependency map from active `agent:backlog` /
/// `agent:icebox` items for auto-dag queue ordering (`#queue-auto-dag-priority`).
/// First-seen deps win on duplicate ids across components; items with no
/// dependency tokens are omitted.
fn collect_after_deps(
    components: &[crate::component::Component],
    content: &str,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut deps = std::collections::HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, d) in crate::pending::active_item_after_deps(body) {
            if !d.is_empty() {
                deps.entry(id).or_insert(d);
            }
        }
    }
    deps
}

fn dedup_queue_nodes_by_key(content: &str) -> Result<Option<(String, usize)>> {
    let before_nodes =
        agent_doc_markdown_ast::mutations::item_nodes(content, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to parse queue node keys: {err}")
        })?;
    let updated =
        agent_doc_markdown_ast::mutations::dedup_node_keys(content, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to dedup queue node keys: {err}")
        })?;
    if updated == content {
        return Ok(None);
    }
    let after_nodes =
        agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue").map_err(|err| {
            anyhow::anyhow!("queue maintenance: failed to parse deduped queue node keys: {err}")
        })?;
    let dropped = before_nodes.len().saturating_sub(after_nodes.len());
    Ok(Some((updated, dropped)))
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
    let mut source_queue_priority = false;
    let mut queue_tag_attrs_normalized = false;

    let raw_queue_tag = &current_content[comp.open_start..comp.open_end];
    let normalized_queue_tag = crate::queue::normalize_queue_tag_attrs(raw_queue_tag);
    if normalized_queue_tag != raw_queue_tag {
        let mut rebuilt = String::with_capacity(current_content.len());
        rebuilt.push_str(&current_content[..comp.open_start]);
        rebuilt.push_str(&normalized_queue_tag);
        rebuilt.push_str(&current_content[comp.open_end..]);
        current_content = rebuilt;
        mutated = true;
        queue_tag_attrs_normalized = true;
        eprintln!("[preflight] queue: normalized malformed queue marker attributes");
    }

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

    // Backlog→queue sync (#backlog-queue-sync-attr): when an `agent:backlog`
    // component carries a `queue` attribute, regenerate the queue `do [#id]`
    // prompts from its active items BEFORE activation so a freshly synced queue
    // can auto-activate on the same cycle. `agent:icebox` is intentionally not a
    // component-level sync source; parked work must be moved to backlog or
    // explicitly marked for enqueue. Per-item enqueue markers
    // (#queue-enqueue-action) append marked ids without requiring the component
    // attribute.
    if let Some(sync_request) = collect_backlog_queue_sync(&components, &content) {
        let mode = sync_request.mode;
        source_queue_priority = sync_request.priority;
        let enqueue_ids: std::collections::HashSet<String> = sync_request
            .enqueue_ids
            .iter()
            .map(|id| id.trim().to_ascii_lowercase())
            .collect();
        let mut backlog_ids = sync_request.ids;
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
        // #backlog-queue-sync-pending-add-amplification (decision B/C): while the
        // queue is already running (persisted-active auto-loop), do NOT promote
        // freshly-added backlog items into the live queue. Re-mirroring on every
        // cycle injected each new `--pending-add` as a `do [#id]` head, growing
        // the queue unboundedly and tripping pending_done_guard on each finalize.
        // Restrict the sync to ids already present as queue heads so captured
        // follow-ups wait for the NEXT activation instead of joining mid-loop. A
        // fresh activation (queue not yet active) still mirrors the full backlog.
        let persisted_active_incoming = frontmatter::parse(&content)
            .map(|(fm, _)| fm.queue_active.unwrap_or(false))
            .unwrap_or(false);
        // `#backlog-queue-empty-active-repopulate`: gate the empty-active-queue
        // repopulation on the queue's `go` control. `go` (frontmatter `queue: go`
        // or a marker-side `go`/`start` token, both → `QueueControl::Start`) opts
        // into continuous-backlog-loop: when the live queue is fully drained (0
        // un-struck prompts), repopulate from the full active backlog instead of
        // holding. Without `go` (a plain persisted-active queue), keep the
        // drain-then-stop hold. Amplification can't occur with 0 live prompts, and
        // `active_item_ids` returns only Open `[ ]` items, so processed items
        // (marked `[/]`/`[x]` per the `do #id` closeout rule) drop out and the
        // loop converges when no Open backlog item remains.
        let queue_go_mode = matches!(
            crate::queue::marker_control(&comp.attrs),
            Some(agent_doc_core::frontmatter::QueueControl::Start)
        ) || frontmatter::parse(&content)
            .ok()
            .and_then(|(fm, _)| fm.queue)
            .and_then(|q| agent_doc_core::frontmatter::QueueControl::parse(&q))
            .map(|c| matches!(c, agent_doc_core::frontmatter::QueueControl::Start))
            .unwrap_or(false);
        // `#backlog-queue-attr-populates-in-go-mode`: a plain persisted-active
        // queue (no `go`/`start`) still holds freshly-added backlog ids out of the
        // running loop to avoid mid-loop amplification. But a `go`-mode queue
        // (`queue: go`/`start`) is an explicit continuous-backlog-loop opt-in: the
        // `queue` backlog attribute is *supposed* to populate the queue, so fresh
        // backlog ids append immediately (not only when the queue fully drains).
        // Append/Prepend stay idempotent (existing + struck `Completed` ids are
        // never re-added) and processed items drop out of `active_item_ids` once
        // marked `[/]`/`[x]`, so the queue stays bounded by the open backlog.
        if persisted_active_incoming && !queue_go_mode {
            let existing_queue_ids: std::collections::HashSet<String> = entries
                .iter()
                .filter_map(queue_entry_do_id)
                .map(|id| id.to_ascii_lowercase())
                .collect();
            let before = backlog_ids.len();
            backlog_ids.retain(|id| {
                let key = id.trim().to_ascii_lowercase();
                existing_queue_ids.contains(&key) || enqueue_ids.contains(&key)
            });
            let held = before - backlog_ids.len();
            if held > 0 {
                eprintln!(
                    "[preflight] queue: held {held} freshly-added backlog id(s) out of the active auto-loop \
                     (they sync at the next activation; #backlog-queue-sync-pending-add-amplification)"
                );
            }
        } else if persisted_active_incoming && queue_go_mode {
            eprintln!(
                "[preflight] queue: go-mode active queue — appending fresh backlog `queue`-attr id(s) \
                 (continuous-backlog-loop; #backlog-queue-attr-populates-in-go-mode)"
            );
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
                        "{}: a backlog/pending queue sync request populated an empty queue. \
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
    // Also runs when the rank map is empty so a `__prioritized__` manual pin
    // (#queue-manual-priority-override) still floats to the top of the queue even
    // when no backlog item carries a `priority` attribute.
    if comp.attrs.contains_key("priority") || source_queue_priority {
        let rank = collect_backlog_priority_ranks(&components, &content);
        if let Ok(Some(snap_content)) = snapshot::load(file)
            && let Ok(snap_components) = crate::component::parse(&snap_content)
            && let Some(snap_queue) = snap_components.iter().find(|c| c.name == "queue")
        {
            let snap_body = &snap_content[snap_queue.open_end..snap_queue.close_start];
            if let Ok(snap_entries) = crate::queue::parse(snap_body) {
                if let Some(pinned) =
                    crate::queue::annotate_operator_priority_reorders(&snap_entries, &entries)
                {
                    let new_body = crate::queue::render(&pinned);
                    current_content = {
                        let comps = crate::component::parse(&current_content)?;
                        let q = comps.iter().find(|c| c.name == "queue").unwrap();
                        q.replace_content(&current_content, &new_body)
                    };
                    eprintln!(
                        "[preflight] queue: pinned manually reordered prompt(s) with operator priority"
                    );
                    entries = pinned;
                    mutated = true;
                }
                // #7r2s: a brand-new queue line the operator just typed (absent from
                // the snapshot, not one the binary appended from the backlog this
                // cycle) carries no pin, so the priority sort below would sink it
                // under `queue`-attr backlog items. Auto-pin it with operator
                // priority so it stays at its authored slot.
                let synced_set: std::collections::HashSet<String> =
                    synced_queue_ids.iter().cloned().collect();
                if let Some(pinned_new) = crate::queue::annotate_manual_queue_additions(
                    &snap_entries,
                    &entries,
                    &synced_set,
                ) {
                    let new_body = crate::queue::render(&pinned_new);
                    current_content = {
                        let comps = crate::component::parse(&current_content)?;
                        let q = comps.iter().find(|c| c.name == "queue").unwrap();
                        q.replace_content(&current_content, &new_body)
                    };
                    eprintln!(
                        "[preflight] queue: auto-pinned manually-added prompt(s) with operator priority (#7r2s)"
                    );
                    entries = pinned_new;
                    mutated = true;
                }
            }
        }
        // Auto-dag (#queue-auto-dag-priority): order by `after=#id` dependency
        // graph first (a blocker outranks a pin); fall back to the plain
        // pin+priority sort when there are no dependency edges.
        let deps = collect_after_deps(&components, &content);
        let sorted = crate::queue::sort_prompts_by_dag(&entries, &rank, &deps)
            .map(|s| ("auto-dag dependency order (blockers + pins)", s))
            .or_else(|| {
                crate::queue::sort_prompts_by_priority(&entries, &rank)
                    .map(|s| ("backlog priority (operator pins position-locked)", s))
            });
        if let Some((how, sorted)) = sorted {
            let sorted = crate::queue::annotate_agent_priority_promotions(&entries, &sorted)
                .unwrap_or(sorted);
            let new_body = crate::queue::render(&sorted);
            current_content = {
                let comps = crate::component::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            eprintln!("[preflight] queue: sorted do-prompts by {how}");
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

    // Collapse duplicated queue nodes by durable AST node key, never by prompt
    // text. This keeps intentional repeated `do [#id]` prompts executable while
    // preserving a structural cleanup point for true duplicate node-key replay
    // residue from IPC/snapshot drift.
    // #queue-completed-items-escape-below-component: a post-commit CRDT/boundary
    // merge can displace struck queue items past `<!-- /agent:queue -->` into the
    // neighbouring parking-lot comment, where they render invisibly and
    // accumulate as orphaned residue. Drop any such displaced struck-queue line
    // (outside every agent component span) before the rest of queue maintenance.
    if let Some(repaired) =
        crate::template::repair_queue_struck_items_escaped_below_marker(&current_content)
    {
        current_content = repaired;
        mutated = true;
        eprintln!(
            "[preflight] queue: removed displaced struck queue item(s) below the closing marker"
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "queue_escape_repair file={} reason=struck_items_below_close_marker",
                file.display()
            ),
        );
    }

    if let Some((deduped_content, dropped)) = dedup_queue_nodes_by_key(&current_content)? {
        current_content = deduped_content;
        let comps = crate::component::parse(&current_content)?;
        if let Some(q) = comps.iter().find(|c| c.name == "queue") {
            let body = &current_content[q.open_end..q.close_start];
            activation.entries_after = crate::queue::parse(body)
                .context("queue maintenance: failed to parse AST-deduped queue")?;
        }
        mutated = true;
        eprintln!("[preflight] queue: collapsed {dropped} duplicate queue node-key(s)");
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
                current_content = frontmatter::merge_queue_state(&current_content, false)?;
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
                    eprintln!(
                        "[preflight] queue: pause — head prompt modified mid-edit (buffer not settled); not grabbing a half-typed head"
                    );
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
                        current_content = frontmatter::merge_queue_state(&current_content, false)?;
                    }
                    std::fs::write(file, &current_content).with_context(|| {
                        format!("queue halt: failed to write {}", file.display())
                    })?;
                    converge_live_buffer_queue_shape(
                        file,
                        &current_content,
                        project_root.as_deref(),
                    );
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
    let need_clear_non_auto_residue =
        !has_auto && !activation.active && !activation.deferred && drained_residue;
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
        let new_tag =
            crate::queue::strip_control_from_tag(&crate::queue::strip_auto_from_tag(raw_tag));
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

        if queue_tag_attrs_normalized
            && let Ok(snap_comps) = crate::component::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let raw_tag = &new_snap[snap_q.open_start..snap_q.open_end];
            let normalized_tag = crate::queue::normalize_queue_tag_attrs(raw_tag);
            if normalized_tag != raw_tag {
                let mut rebuilt = String::with_capacity(new_snap.len());
                rebuilt.push_str(&new_snap[..snap_q.open_start]);
                rebuilt.push_str(&normalized_tag);
                rebuilt.push_str(&new_snap[snap_q.open_end..]);
                new_snap = rebuilt;
            }
        }

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
        if need_set_active && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, true) {
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
/// attribute, and `queue:` frontmatter to disk + snapshot. When a live
/// IPC listener owns the document it keeps its own working buffer; without this
/// push it overwrites the disk write on its next flush — re-adding stale queue
/// body lines, `auto`, and `queue_active: true` — and the snapshot/HEAD drift
/// regenerates on every preflight (`#adoc-queue-ipc-buffer-divergence`). A
/// content-only IPC patch cannot converge an opening-tag attribute or
/// frontmatter, so we send a dedicated convergence message carrying the queue
/// body, desired `auto` state, and canonical queue frontmatter. Best-effort: a
/// missing listener or send failure is logged, never fatal — the disk/snapshot
/// write remains the source of truth.
fn converge_live_buffer_queue_shape(file: &Path, content: &str, project_root: Option<&Path>) {
    let Some(root) = project_root else {
        return;
    };
    if !crate::ipc_socket::is_listener_active(root) {
        return;
    }
    let (want_auto, queue_body) = match crate::component::parse(content) {
        Ok(comps) => comps
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| {
                (
                    crate::queue::has_auto_attr(&q.attrs),
                    Some(q.content(content).to_string()),
                )
            })
            .unwrap_or((false, None)),
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
    // #queue-active-deprecated-line-stuck: converge with the CANONICAL `queue:`
    // control, never the deprecated `queue_active:` line. Emitting the legacy form
    // here re-introduced `queue_active: true` into the live route-owned buffer on
    // every preflight (the buffer then flushed it back to disk, undoing the
    // repair-step migration that drops it). The canonical key is the sole queue
    // control; readers still fold it onto `queue_active` in memory.
    let fm_yaml = format!("queue: {}", if queue_active { "start" } else { "stop" });
    match crate::ipc_socket::send_queue_convergence(
        root,
        &canonical.to_string_lossy(),
        want_auto,
        Some(&fm_yaml),
        queue_body.as_deref(),
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
