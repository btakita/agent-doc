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
//!   cycle with no recoverable response, the `git::commit(file)` path is the
//!   narrow no-op closeout that stages the snapshot only and leaves later live
//!   working-tree edits uncommitted; if that closeout still cannot prove the
//!   prior cycle is durable, preflight fails closed instead of diffing again.
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
//! - `layout_issues` is informational: the SKILL workflow may surface issues
//!   to the user but `run` always completes the remaining steps regardless.
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

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use crate::component::is_backlog_component;
use crate::{config, diff, frontmatter, git, repair, resync, sessions, snapshot};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreflightOutput {
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
    /// Canonical ordered list of user-authored changes that need prompt-aware handling.
    /// `prompt_target` items require a response, `content_edit` items are corrections
    /// the agent must incorporate, and `recovery_artifact` / `boundary_artifact`
    /// items indicate document-state cleanup rather than ordinary conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_bearing_changes: Vec<crate::diff::PromptBearingChange>,
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
}

fn is_zero_usize(n: &usize) -> bool {
    *n == 0
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
fn resolve_agent_model(frontmatter_model: Option<&str>) -> Option<String> {
    frontmatter_model.map(|m| short_model_name(m).to_string())
}

/// Trigger an automatic `resync --fix` when session-drift has been detected
/// on two consecutive preflights.
///
/// The drift counter lives at `.agent-doc/state/drift.count`. Each call either
/// increments it (drift present) or deletes it (drift absent). When the counter
/// reaches >= 2 we invoke `resync::run(true, None)` and reset it to 0 so we do
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
        if let Err(e) = resync::run(true, None) {
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

    // Get current session name.
    let session_name = match Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}"])
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => return issues, // Can't determine session — skip silently.
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
    for (session_id, entry) in registry {
        if entry.file.is_empty() {
            continue;
        }
        file_sessions
            .entry(entry.file.clone())
            .or_default()
            .push(session_id.clone());
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
                if e.to_string()
                    .contains(repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
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

    eprintln!(
        "[preflight] WARNING: previous cycle `{}` is still `{}` ({}) — attempting recovery before diff",
        state.cycle_id,
        match state.phase {
            crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
            crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
            crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
            crate::cycle_state::CyclePhase::Committed => "committed",
        },
        state.last_event
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
            if e.to_string()
                .contains(repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
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
        anyhow::bail!(
            "previous cycle `{}` is still `{}` after recovery/commit ({}){}",
            after.cycle_id,
            match after.phase {
                crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
                crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
                crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
                crate::cycle_state::CyclePhase::Committed => "committed",
            },
            after.last_event,
            marker_note
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

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Step 0a: Auto-GC (at most once per day).
    // Checks .agent-doc/gc.stamp — if missing or >24 hours old, runs lightweight GC.
    {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(root) = snapshot::find_project_root(&canonical) {
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

    // Step 0-pre: interrupted-cycle guard (#cyc1). Use exact persisted cycle
    // state instead of inferring solely from `ops.log`.
    let (recovered_prior, committed_prior) = enforce_cycle_completion(file)?;

    // Step 0: Check tmux layout health.
    eprintln!("[preflight] step 0: layout check");
    let layout_issues = check_layout();
    for issue in &layout_issues {
        eprintln!("[preflight] layout issue: {}", issue);
    }

    // Step 0b (#a014): Session drift auto-resync — when drift is detected on
    // consecutive preflights, auto-run `resync --fix` to clean the registry.
    // State lives in `.agent-doc/state/drift.count` so we only auto-fix after
    // the second consecutive detection (one false positive is tolerated).
    maybe_auto_resync_on_drift(file, &layout_issues);

    // Step 1: Recover orphaned pending responses.
    eprintln!("[preflight] step 1: repair");
    let recovered = recovered_prior
        || match repair::run(file) {
            Ok(outcome) => outcome.repaired(),
            Err(e) => {
                if e.to_string()
                    .contains(repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
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
    let (pending_reordered, pending_gated_count) = run_pending_maintenance(file)?;
    enforce_no_shadow_open_backlog(file)?;
    enforce_no_dropped_backlog(file)?;

    // Step 2: Commit previous cycle.
    eprintln!("[preflight] step 2: commit");
    let committed = committed_prior
        || match git::commit(file) {
            Ok(did_commit) => did_commit,
            Err(e) => {
                eprintln!("[preflight] commit warning: {}", e);
                false
            }
        };

    // Step 2b: Save baseline after commit (post-boundary-reposition).
    // This baseline matches the snapshot exactly, eliminating staleness.
    let baseline_file = {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        let hash = snapshot::doc_hash(&canonical).unwrap_or_else(|_| "unknown".to_string());
        let baseline_dir = snapshot::find_project_root(&canonical)
            .unwrap_or_else(|| file.parent().unwrap_or(Path::new(".")).to_path_buf())
            .join(".agent-doc/baselines");
        let _ = std::fs::create_dir_all(&baseline_dir);
        let baseline_path = baseline_dir.join(format!("{}.md", hash));
        match std::fs::read_to_string(file) {
            Ok(content) => {
                let _ = std::fs::write(&baseline_path, &content);
                eprintln!("[preflight] baseline saved: {}", baseline_path.display());
                Some(baseline_path.to_string_lossy().to_string())
            }
            Err(e) => {
                eprintln!("[preflight] failed to save baseline: {}", e);
                None
            }
        }
    };

    // Step 2c: Auto-compact if exchange component exceeds threshold.
    {
        if let Ok(content) = std::fs::read_to_string(file)
            && let Ok((fm, _)) = frontmatter::parse(&content)
            && let Some(threshold) = fm.auto_compact
            && threshold > 0
            && fm.resolve_mode().is_template()
            && let Some(comp) = crate::component::parse(&content)
                .ok()
                .and_then(|comps| comps.into_iter().find(|c| c.name == "exchange"))
        {
            let comp_content = &content[comp.open_end..comp.close_start];
            let line_count = comp_content.lines().count();
            if line_count > threshold {
                eprintln!(
                    "[preflight] step 2c: auto-compact (exchange={} lines > threshold={})",
                    line_count, threshold
                );
                if let Err(e) = crate::compact::run(file, None, Some("exchange"), None, None, false)
                {
                    eprintln!("[preflight] auto-compact warning: {}", e);
                }
            }
        }
    }

    // Step 2d: Cross-document sweep (Fix 5) — commit any other tracked docs in the same
    // project that have uncommitted snapshot content. Turns preflight into a catch-all
    // backstop: even if a previous session's commit was skipped, the next preflight
    // from any document in the project will pick it up.
    {
        let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(root) = snapshot::find_project_root(&canonical) {
            let sessions_path = root.join(".agent-doc/sessions.json");
            if let Ok(content) = std::fs::read_to_string(&sessions_path)
                && let Ok(registry) = serde_json::from_str::<
                    std::collections::HashMap<String, serde_json::Value>,
                >(&content)
            {
                for entry in registry.values() {
                    let tracked_file = entry.get("file").and_then(|v| v.as_str()).unwrap_or("");
                    if tracked_file.is_empty() {
                        continue;
                    }
                    let doc_path = root.join(tracked_file);
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
    }

    // Step 3: Read and truncate the claims log.
    eprintln!("[preflight] step 3: claims");
    let claims = read_and_truncate_claims(file);

    // Step 3b: Wait for file to settle (mtime + typing indicator debounce).
    // Check both file mtime (disk-level) and cross-process typing indicator
    // (buffer-level) to avoid picking up mid-typing edits.
    // Default: 2000ms (configurable via `agent_doc_debounce` frontmatter field).
    {
        let debounce_ms = std::fs::read_to_string(file)
            .ok()
            .and_then(|content| {
                frontmatter::parse(&content)
                    .ok()
                    .and_then(|(fm, _)| fm.debounce_ms)
            })
            .unwrap_or(2000);
        let debounce = std::time::Duration::from_millis(debounce_ms);
        let max_wait = std::time::Duration::from_secs(if debounce_ms > 3000 {
            (debounce_ms / 1000) + 1
        } else {
            3
        });
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

            let typing_active = agent_doc::debounce::is_typing_via_file(&file_str, 1500);
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
    let raw_diff = diff::compute(file)?;

    // Step 4a: Scan diff for inline `/model <x>` command and strip the matching
    // line(s) before downstream classification. The strip prevents `/model` from
    // double-emitting in `builtin_commands`.
    let global_config = config::load().unwrap_or_default();
    let harness = agent_doc::model_tier::detect_harness();
    let model_scan = raw_diff
        .as_ref()
        .map(|d| agent_doc::model_tier::scan_model_switch(d, &harness, &global_config.model));
    let mut diff_result: Option<String> = if let Some(scan) = model_scan.as_ref() {
        // Use the stripped diff for downstream consumers.
        Some(scan.stripped_diff.clone())
    } else {
        raw_diff.clone()
    };

    // Step 4b: Classify the diff for skill routing.
    let mut classification = diff_result.as_ref().map(|d| diff::classify_diff(d));
    let boundary_artifact_only = classification
        .as_ref()
        .is_some_and(|c| c.diff_type == diff::DiffType::BoundaryArtifact);
    if boundary_artifact_only {
        diff_result = None;
        classification = None;
    }

    let no_changes = diff_result.is_none();
    if !no_changes {
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

    // Step 4c: Annotate the diff with content-source markers.
    let annotated_diff = diff_result.as_ref().and_then(|d| diff::annotate_diff(d));

    // Step 4c2: Classify user-authored prompt-bearing changes across prompts, edits,
    // and response/boundary artifacts.
    let prompt_bearing_changes = diff_result
        .as_ref()
        .map(|d| diff::classify_prompt_bearing_changes(d))
        .unwrap_or_default();

    // Legacy compatibility surface for older skill consumers.
    let inline_annotations = annotated_diff
        .as_ref()
        .map(|a| diff::extract_inline_annotations(a))
        .unwrap_or_default();

    // Step 4d: Extract slash commands from user-added diff lines (classified into skill vs built-in).
    let parsed_commands = diff_result
        .as_ref()
        .map(|d| diff::parse_slash_commands_classified(d))
        .unwrap_or_else(|| diff::ParsedSlashCommands {
            skill_commands: vec![],
            builtin_commands: vec![],
        });
    let slash_commands = parsed_commands.skill_commands;
    let builtin_commands = parsed_commands.builtin_commands;
    let orchestration_request = diff_result
        .as_ref()
        .and_then(|d| diff::detect_orchestration_request(d));

    // Step 4e: Resolve model tier sources and compose effective_tier.
    // Sources (highest precedence first): inline /model command, <!-- agent:model --> component,
    // agent_doc_model_tier frontmatter, diff heuristic.
    let (
        frontmatter_tier,
        component_tier_value,
        frontmatter_env,
        frontmatter_model,
        frontmatter_prompt_presets,
    ) = match std::fs::read_to_string(file) {
        Ok(content) => {
            let (fm_tier, env_map, fm_model, prompt_presets) = frontmatter::parse(&content)
                .ok()
                .map(|(fm, _)| {
                    let resolved = fm.resolve_harness_model(&harness).map(|s| s.to_string());
                    (fm.model_tier, fm.env, resolved, fm.prompt_presets)
                })
                .unwrap_or_default();
            let comp_value = agent_doc::model_tier::extract_model_component(&content);
            (fm_tier, comp_value, env_map, fm_model, prompt_presets)
        }
        Err(_) => (None, None, Default::default(), None, Default::default()),
    };
    let component_tier = component_tier_value.as_deref().and_then(|v| {
        agent_doc::model_tier::component_value_to_tier(v, &harness, &global_config.model)
    });

    let prompt_presets_requested = diff_result
        .as_ref()
        .map(|d| diff::detect_prompt_preset_requests(d))
        .unwrap_or_default();
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
        agent_doc::model_tier::suggested_tier(diff_type_str.as_deref(), lines_added, file);

    let model_switch_name = model_scan.as_ref().and_then(|s| s.model_switch.clone());
    let model_switch_tier = model_scan.as_ref().and_then(|s| s.model_switch_tier);
    let required_tier_value = component_tier.or(frontmatter_tier);
    let effective_tier_value = agent_doc::model_tier::compose_effective_tier(
        model_switch_tier,
        component_tier,
        frontmatter_tier,
        suggested,
    );

    // Step 4f: Queue component analysis — resolve activation, consume start fences,
    // and emit queue_prompts for the skill. Runs after diff so exchange triggers
    // ("do queue") can be detected from user-added lines.
    let queue_state = run_queue_maintenance(file, diff_result.as_deref()).unwrap_or_else(|e| {
        eprintln!("[preflight] queue maintenance warning: {}", e);
        QueueState::default()
    });

    // Step 5: Scan for pending callback requests from other processes.
    let pending_callbacks = crate::callback::scan_pending_callbacks(None).unwrap_or_default();
    if !pending_callbacks.is_empty() {
        eprintln!(
            "[preflight] found {} pending callback(s)",
            pending_callbacks.len()
        );
    }

    let agent_model = resolve_agent_model(frontmatter_model.as_deref());
    let output = PreflightOutput {
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
        prompt_bearing_changes,
        inline_annotations,
        slash_commands,
        builtin_commands,
        orchestration_request,
        prompt_presets_requested,
        effective_tier: Some(effective_tier_value.to_string()),
        required_tier: required_tier_value.map(|t| t.to_string()),
        suggested_tier: Some(suggested.to_string()),
        model_switch: model_switch_name,
        model_switch_tier: model_switch_tier.map(|t| t.to_string()),
        pending_callbacks,
        env: frontmatter_env,
        pending_reordered,
        pending_gated_count,
        agent_model,
        queue_prompts: queue_state.queue_prompts,
        queue_active: queue_state.queue_active,
        queue_deferred: queue_state.queue_deferred,
        queue_start_at: queue_state.queue_start_at,
        queue_trigger: queue_state.queue_trigger,
        queue_halted: queue_state.queue_halted,
    };

    let json =
        serde_json::to_string_pretty(&output).context("failed to serialize preflight output")?;
    println!("{}", json);

    Ok(())
}

/// Run pending-component maintenance: lazy backfill, reap `[x]`, and reorder detection.
///
/// Returns `(reordered, gated_count)`:
/// - `reordered` is `true` when a reorder was detected (same ids, different order).
/// - `gated_count` is the number of items currently in `[/]` state after backfill+reap.
///
/// Any write-through (backfill / reap) is persisted and committed in the same pass.
/// Silent no-op when the document has no `agent:pending` component.
fn run_pending_maintenance(file: &Path) -> Result<(bool, usize)> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok((false, 0)),
    };
    let components = match crate::component::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok((false, 0)),
    };
    let comp = match components
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
    {
        Some(c) => c,
        None => return Ok((false, 0)),
    };
    let body = &content[comp.open_end..comp.close_start];

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let doc_id = snapshot::doc_hash(&canonical).unwrap_or_else(|_| file.display().to_string());

    let mut current_body = body.to_string();
    let mut mutated = false;
    let saw_completed_before = !completed_pending_items(&current_body).is_empty();

    // 1. Lazy backfill: assign missing hash ids and normalize checkboxes.
    let (after_backfill, changed) =
        crate::pending::backfill(&current_body, &doc_id, &std::collections::HashSet::new());
    if changed {
        eprintln!("[preflight] pending: backfilled missing hash ids / checkboxes");
        current_body = after_backfill;
        mutated = true;
    }

    // 2. Reap `[x]` items. Collect full items (not just ids) so we can
    //    archive them to `agent:pending-done` in step 2b below.
    let (after_reap, removed_items) = crate::pending::reap_with_items(&current_body);
    if !removed_items.is_empty() {
        let removed_ids: Vec<String> = removed_items.iter().map(|i| i.id.clone()).collect();
        eprintln!(
            "[preflight] pending: reaped {} item(s): {}",
            removed_items.len(),
            removed_ids.join(", ")
        );
        current_body = after_reap;
        mutated = true;
    }

    // 3. Persist any mutations to BOTH the working tree file and the snapshot.
    //    Writing to both (surgically, via component replace) keeps the two in
    //    sync so the upcoming step-2 `git::commit` stages the reaped+archived
    //    snapshot in a single commit. We no longer call `git::commit` here —
    //    see #64mb: calling commit inside maintenance produced a second commit
    //    per preflight whenever anything mutated.
    if mutated {
        // 3a. Working tree — preserves user edits outside the pending region.
        let mut new_file_content = comp.replace_content(&content, &current_body);
        if !removed_items.is_empty()
            && let Some(archived) = archive_pending_done(&new_file_content, &removed_items)
        {
            new_file_content = archived;
        }
        std::fs::write(file, &new_file_content)
            .with_context(|| format!("failed to write pending updates to {}", file.display()))?;

        // 3b. Snapshot — surgical replace of the pending (and optionally
        //     pending-done) component in the snapshot content. User edits
        //     elsewhere don't exist in the snapshot, so there's nothing to
        //     preserve beyond the existing snapshot body.
        if let Ok(Some(snap_content)) = snapshot::load(file) {
            let snap_comps = crate::component::parse(&snap_content).ok();
            if let Some(snap_pending) =
                snap_comps.and_then(|cs| cs.into_iter().find(|c| is_backlog_component(&c.name)))
            {
                let mut new_snap = snap_pending.replace_content(&snap_content, &current_body);
                if !removed_items.is_empty()
                    && let Some(archived) = archive_pending_done(&new_snap, &removed_items)
                {
                    new_snap = archived;
                }
                if let Err(e) = snapshot::save(file, &new_snap) {
                    eprintln!("[preflight] pending: snapshot sync warning: {}", e);
                }
            }
        }
    }

    if saw_completed_before {
        let persisted_content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to verify reap in {}", file.display()))?;
        ensure_no_completed_backlog_items(&persisted_content, "working tree")?;

        let snapshot_content = snapshot::load(file)?.with_context(|| {
            format!(
                "pending maintenance reaped completed backlog items in {} but the snapshot is missing",
                file.display()
            )
        })?;
        ensure_no_completed_backlog_items(&snapshot_content, "snapshot")?;
    }

    // 4. Reorder detection: compare the snapshot's pending component to the current body.
    let reordered = match snapshot::load(file).unwrap_or(None) {
        Some(snap) => {
            let snap_comp = crate::component::parse(&snap)
                .ok()
                .and_then(|comps| comps.into_iter().find(|c| is_backlog_component(&c.name)));
            if let Some(sc) = snap_comp {
                let snap_body = &snap[sc.open_end..sc.close_start];
                crate::pending::detect_reorder(snap_body, &current_body).is_some()
            } else {
                false
            }
        }
        None => false,
    };
    if reordered {
        eprintln!("[preflight] pending: reorder detected (skill must not reorder this cycle)");
    }

    // 5. Count gated items in the post-maintenance body.
    let (_, items, _) = crate::pending::parse_items(&current_body);
    let gated_count = items
        .iter()
        .filter(|i| matches!(i.state, crate::pending::PendingState::Gated))
        .count();
    if gated_count > 0 {
        eprintln!("[preflight] pending: {} gated item(s)", gated_count);
    }

    Ok((reordered, gated_count))
}

fn ensure_no_completed_backlog_items(content: &str, surface: &str) -> Result<()> {
    let components = crate::component::parse(content).with_context(|| {
        format!("failed to parse {surface} components during pending reap check")
    })?;
    let backlog = components
        .into_iter()
        .find(|component| is_backlog_component(&component.name))
        .with_context(|| {
            format!("pending reap check: {surface} is missing the backlog component")
        })?;
    let completed = completed_pending_items(backlog.content(content));
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
    anyhow::bail!("pending maintenance left completed backlog items in the {surface}: {refs}");
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

fn enforce_no_dropped_backlog(file: &Path) -> Result<()> {
    let head_content = match crate::git::show_head(file)? {
        Some(content) => content,
        None => return Ok(()),
    };
    let current_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog replay state in {}",
            file.display()
        )
    })?;
    let done_ids: std::collections::HashSet<String> = crate::cycle_state::load(file)?
        .map(|state| state.pending_done_ids.into_iter().collect())
        .unwrap_or_default();

    let report =
        crate::pending::detect_dropped_from_history(&current_content, &head_content, &done_ids)?;
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

    // Read current state
    let has_auto = crate::queue::has_auto_attr(&comp.attrs);
    let exchange_triggered = diff.map(crate::diff::detect_queue_trigger).unwrap_or(false);
    let (fm, _) = frontmatter::parse(&content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);

    let activation =
        crate::queue::resolve_activation(&entries, has_auto, exchange_triggered, persisted_active);

    let mut mutated = false;
    let mut current_content = content.clone();

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
                    frontmatter::merge_fields(&current_content, "queue_active: false")?;
            }
            // Persist to file + snapshot
            std::fs::write(file, &current_content)
                .with_context(|| format!("queue halt: failed to write {}", file.display()))?;
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
                        && let Ok(m) = frontmatter::merge_fields(&new_snap, "queue_active: false")
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
            });
        }

        // Change detection: compare head prompt between snapshot and file
        if let Ok(Some(snap_content)) = snapshot::load(file)
            && let Ok(snap_comps) = crate::component::parse(&snap_content)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let snap_body = &snap_content[snap_q.open_end..snap_q.close_start];
            if let Ok(snap_entries) = crate::queue::parse(snap_body)
                && crate::queue::detect_head_prompt_modified(
                    &snap_entries,
                    &activation.entries_after,
                )
            {
                eprintln!("[preflight] queue: halt — head prompt modified between cycles");
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
                        frontmatter::merge_fields(&current_content, "queue_active: false")?;
                }
                std::fs::write(file, &current_content)
                    .with_context(|| format!("queue halt: failed to write {}", file.display()))?;
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
                        && let Ok(m) = frontmatter::merge_fields(&ns, "queue_active: false")
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
                });
            }
        }
    }

    // Handle queue drain: if active but no prompts, clear queue_active and strip auto
    let queue_has_prompts = !crate::queue::prompts(&activation.entries_after).is_empty();
    let need_set_active = activation.active && !persisted_active;
    let need_clear_active = !activation.active && persisted_active && !activation.deferred;
    let need_strip_auto = has_auto && !queue_has_prompts;

    // Strip auto attribute from opening tag when queue drains
    if need_strip_auto {
        let comps = crate::component::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        let raw_tag = &current_content[q.open_start..q.open_end];
        let new_tag = crate::queue::strip_auto_from_tag(raw_tag);
        if new_tag != raw_tag {
            let mut rebuilt = String::with_capacity(current_content.len());
            rebuilt.push_str(&current_content[..q.open_start]);
            rebuilt.push_str(&new_tag);
            rebuilt.push_str(&current_content[q.open_end..]);
            current_content = rebuilt;
            mutated = true;
            eprintln!("[preflight] queue: stripped auto (queue drained)");
        }
    }

    // Persist queue_active state to frontmatter
    if need_set_active {
        current_content = frontmatter::merge_fields(&current_content, "queue_active: true")?;
        mutated = true;
        eprintln!("[preflight] queue: set queue_active: true");
    } else if need_clear_active {
        current_content = frontmatter::merge_fields(&current_content, "queue_active: false")?;
        mutated = true;
        eprintln!("[preflight] queue: cleared queue_active");
    }

    // Persist mutations to both file and snapshot
    if mutated {
        std::fs::write(file, &current_content)
            .with_context(|| format!("failed to write queue updates to {}", file.display()))?;

        // Update snapshot surgically
        if let Ok(Some(snap_content)) = snapshot::load(file) {
            let mut new_snap = snap_content.clone();

            // Apply queue body change to snapshot
            if (activation.consumed_start_fence || need_strip_auto)
                && let Ok(snap_comps) = crate::component::parse(&new_snap)
                && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
            {
                let new_body = crate::queue::render(&activation.entries_after);
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
                && let Ok(merged) = frontmatter::merge_fields(&new_snap, "queue_active: true")
            {
                new_snap = merged;
            } else if need_clear_active
                && let Ok(merged) = frontmatter::merge_fields(&new_snap, "queue_active: false")
            {
                new_snap = merged;
            }

            if new_snap != snap_content
                && let Err(e) = snapshot::save(file, &new_snap)
            {
                eprintln!("[preflight] queue: snapshot sync warning: {}", e);
            }
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
    })
}

/// Archive reaped pending items to `agent:pending-done` if the component
/// exists. Returns `Some(new_content)` when archival happened, `None` when
/// the archive component is absent (silent no-op per spec §3 step 3).
///
/// Entry format: `- YYYY-MM-DD [#id] text` — ISO date prefix for chronology,
/// hash preserved so the archive is grep-compatible with the live list, text
/// verbatim from the reaped item so context survives.
///
/// New entries are appended AFTER any existing archive body. The component
/// is always rendered with a trailing blank line so successive turns don't
/// pack entries onto one line.
pub(crate) fn archive_pending_done(
    content: &str,
    removed: &[crate::pending::PendingItem],
) -> Option<String> {
    if removed.is_empty() {
        return None;
    }
    let components = crate::component::parse(content).ok()?;
    let archive = components.into_iter().find(|c| c.name == "pending-done")?;
    let existing_body = &content[archive.open_end..archive.close_start];

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

    let mut new_body = existing_body.to_string();
    if !new_body.is_empty() && !new_body.ends_with('\n') {
        new_body.push('\n');
    }
    for item in removed {
        new_body.push_str(&format!("- {} [#{}] {}\n", today, item.id, item.text));
    }

    Some(archive.replace_content(content, &new_body))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

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

        let (reordered, gated_count) = run_pending_maintenance(&doc).unwrap();
        assert!(!reordered);
        assert_eq!(gated_count, 0);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(!file_after.contains("[#reap1]"));
        assert!(file_after.contains("[#keep1]"));

        let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
        assert!(!snapshot_after.contains("[#reap1]"));
        assert!(snapshot_after.contains("[#keep1]"));
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
        assert!(!recovered, "no pending response replay should be needed");
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
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
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
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
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
                file: "tasks/foo.md".to_string(),
                window: "@1".to_string(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                file: "tasks/bar.md".to_string(),
                window: "@1".to_string(),
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
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
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
                file: String::new(), // legacy entry — no file
                window: "@1".to_string(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                file: String::new(),
                window: "@1".to_string(),
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
                    .map_or(true, |a| a.is_empty())
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
        let result = resolve_agent_model(Some("claude-opus-4"));
        assert_eq!(result, Some("opus-4".to_string()));
    }

    #[test]
    fn resolve_agent_model_strips_claude_prefix_from_frontmatter() {
        let result = resolve_agent_model(Some("claude-haiku-4-5"));
        assert_eq!(result, Some("haiku-4-5".to_string()));
    }

    #[test]
    fn resolve_agent_model_preserves_short_openai_style_name() {
        let result = resolve_agent_model(Some("gpt-5"));
        assert_eq!(result, Some("gpt-5".to_string()));
    }

    #[test]
    fn resolve_agent_model_none_when_no_frontmatter() {
        // No frontmatter → None, regardless of env var state.
        let result = resolve_agent_model(None);
        assert_eq!(result, None);
    }
}
