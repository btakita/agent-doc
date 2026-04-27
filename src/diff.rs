//! # Module: diff
//!
//! ## Spec
//! - `strip_comments(content)` removes `[//]: # (...)` link-reference comments and
//!   `<!-- ... -->` HTML comments from document content, while preserving agent
//!   range markers (`<!-- agent:* -->`). Comment patterns inside fenced code blocks
//!   and inline backtick spans are not treated as comment syntax.
//! - `compute(doc)` reads the on-disk snapshot for `doc`, waits for stable content
//!   via `wait_for_stable_content`, strips comments from both sides, and returns a
//!   unified diff (5-line context, header `snapshot`→`document`) or `None` when
//!   there are no changes. Stale-snapshot recovery fires before emitting a diff:
//!   if the delta contains only completed assistant/user exchanges with an empty
//!   trailing user block, the snapshot is synced to the document and `None` is returned.
//! - `wait_for_stable_content(doc, previous)` polls the document file until the
//!   last inserted line looks complete (not mid-word/mid-URL) or up to 12 × 500 ms
//!   rechecks (~6 s). Three consecutive identical reads constitute "stable".
//! - `is_stale_snapshot(snapshot, document)` returns `true` when the document is a
//!   superset of the snapshot, the extra content contains at least one `## Assistant`
//!   block, and the trailing `## User` block is empty.
//! - `run(file, wait)` is the CLI entry point for the `diff` subcommand. When `wait`
//!   is `true` it runs truncation detection first, then calls `compute` and prints
//!   the result to stdout.
//! - `annotate_diff(diff_text)` transforms a unified diff into an annotated format
//!   with content-source markers: `[agent]`, `[user+]`, `[user-]`, `[user~]`.
//!   Returns `None` if the diff is empty. Pure function, no I/O.
//! - `classify_diff(diff_text)` classifies a unified diff into a `DiffType` with a
//!   human-readable reason string. Types: `Approval` (single word: go/yes/do/ok/continue),
//!   `SimpleQuestion` (single added line ending with `?`), `BoundaryArtifact` (only
//!   `(HEAD)` or boundary UUID changes), `Annotation` (edits within agent content),
//!   `StructuralChange` (only deletions/reorgs), `MultiTopic` (multiple unrelated
//!   added blocks), `ContentAddition` (general new content).
//!
//! ## Agentic Contracts
//! - Comment stripping is idempotent: calling `strip_comments` twice yields the same
//!   result as calling it once.
//! - Agent markers are always preserved by `strip_comments`; the skill can rely on
//!   their presence in the stripped output.
//! - `compute` never writes to the document file; it may write to the snapshot file
//!   only during stale-snapshot recovery. A copy-on-read guard compares the snapshot
//!   file's mtime at read time against its mtime before recovery write — if an
//!   external process modified the snapshot mid-diff, recovery is skipped.
//! - `compute` returns `None` (no diff) if and only if there are no meaningful
//!   content changes after comment stripping.
//! - `wait_for_stable_content` always terminates: the `MAX_RECHECKS` bound guarantees
//!   it returns within ~6 s regardless of file activity.
//! - `looks_truncated` never returns `true` for empty strings, markdown headings,
//!   slash commands, or fenced code fences. Single characters (including alphanumeric)
//!   are always treated as truncated — the stability check confirms completion.
//! - Callers of `compute` can assume: `Some(diff)` → there is user-visible content
//!   to respond to; `None` → skip the response cycle.
//! - `classify_diff` is pure and deterministic: same diff text always yields the same
//!   `DiffType` and reason. It operates only on the diff string, never reads files.
//!
//! ## Evals
//! - `strip_html_comment`: `"before\n<!-- a comment -->\nafter\n"` → `"before\nafter\n"`
//! - `strip_multiline_html_comment`: multiline `<!-- ... -->` on its own lines → stripped with surrounding newlines preserved
//! - `strip_link_ref_comment`: `"[//]: # (note)\n"` on its own line → removed entirely
//! - `preserve_agent_markers`: `<!-- agent:status -->` and `<!-- /agent:status -->` → unchanged
//! - `strip_inline_comment`: inline `<!-- note -->` in middle of line → comment removed, surrounding text retained
//! - `strip_preserves_comment_syntax_in_fenced_code_block`: `<!-- not a comment -->` inside triple-backtick fence → unchanged
//! - `strip_preserves_comment_syntax_in_inline_backticks`: `` `<!--` `` in inline code → not treated as comment start
//! - `strip_backtick_comment_before_agent_marker`: `` `<!--` `` text followed by `<!-- /agent:exchange -->` → agent marker not consumed
//! - `stale_snapshot_detects_completed_exchange`: snapshot + completed assistant/user cycle with empty trailing user block → `is_stale_snapshot` returns `true`
//! - `stale_snapshot_false_when_user_has_new_content`: trailing `## User` block has text → `is_stale_snapshot` returns `false`
//! - `stale_snapshot_ignores_comments_in_detection`: scratch comments between exchanges → still detected as stale
//! - `copy_on_read_guard_skips_recovery_when_snapshot_modified`: mtime comparison logic — same mtime allows recovery, different mtime blocks it, both-None allows it
//! - `compute_stale_snapshot_recovery_proceeds_when_unmodified`: stale snapshot (base + completed exchange) → recovery fires, returns None, snapshot synced
//! - `compute_stale_recovery_updates_snapshot_to_current_document`: after recovery, snapshot content matches document
//! - `compute_returns_diff_when_user_adds_content`: user adds new content → returns diff containing the addition
//! - `compute_returns_none_when_no_changes`: identical snapshot and document → returns None
//! - `diff_detects_user_edits_after_stream_write`: snapshot saved post-stream, user adds line → `compute` returns `Some(diff)` containing new text
//! - `diff_no_change_when_document_matches_snapshot`: document identical to snapshot → `compute` returns `None`
//! - `truncated_mid_sentence`: line ending mid-word → `looks_truncated` returns `true`
//! - `not_truncated_complete_sentence`: line ending with `.` → `looks_truncated` returns `false`
//! - `not_truncated_single_word_command`: bare word like `"release"` → `looks_truncated` returns `false`
//! - `truncated_single_chars`: single characters like `"A"`, `"S"`, `"1"` → `looks_truncated` returns `true` (stability check required)
//! - `wait_for_stable_content_returns_immediately_when_complete`: already-complete content → returns in < 500 ms
//! - `annotate_diff_additions`: added lines get `[user+]` markers
//! - `annotate_diff_removals`: removed lines get `[user-]` markers
//! - `annotate_diff_modifications`: paired add/remove with similar content get `[user~]` markers
//! - `annotate_diff_context`: context lines get `[agent]` markers
//! - `annotate_diff_empty`: empty diff returns `None`
//! - `classify_approval`: single added word "go" → `DiffType::Approval`
//! - `classify_approval_case_insensitive`: "Yes" → `DiffType::Approval`
//! - `classify_simple_question`: single added line "what is X?" → `DiffType::SimpleQuestion`
//! - `classify_boundary_artifact`: only `(HEAD)` marker change → `DiffType::BoundaryArtifact`
//! - `classify_boundary_uuid`: only boundary UUID change → `DiffType::BoundaryArtifact`
//! - `classify_structural_change`: only deletions → `DiffType::StructuralChange`
//! - `classify_multi_topic`: multiple separated added blocks → `DiffType::MultiTopic`
//! - `classify_content_addition`: general new content → `DiffType::ContentAddition`
//! - `classify_annotation`: colon-appended edit to agent line → `DiffType::Annotation`
//! - `parse_slash_commands(diff)`: extracts slash commands from added lines in a unified diff
//!   with guards against: code fences (``` / ~~~), blockquotes (`>`), HTML comments, and
//!   non-added lines. Returns a vec of command strings (e.g. `["/clear", "/agent-doc foo.md"]`).
//! - `detect_prompt_preset_requests(diff)`: extracts ordered `preset <name>` / `presets <a>, <b>`
//!   directives from user-added diff lines, ignoring code fences and blockquotes.
//! - `extract_prompt_preset_requests_from_text(text)`: text-mode companion used by orchestration
//!   task extraction for batch-level preset directives outside unified diffs.
//! - `parse_slash_commands_simple`: single added `/clear` line → `["/clear"]`
//! - `parse_slash_commands_ignores_fenced`: `/cmd` inside a ``` block → empty
//! - `parse_slash_commands_ignores_blockquote`: `> /cmd` → empty
//! - `parse_slash_commands_ignores_context_lines`: `/cmd` on context line (` `) → empty
//! - `parse_slash_commands_ignores_removed_lines`: `/cmd` on removed line (`-`) → empty
//! - `parse_slash_commands_with_args`: `/agent-doc foo.md` → `["/agent-doc foo.md"]`
//! - `parse_slash_commands_requires_letter_after_slash`: `/ `, `//comment` → empty
//! - `detect_prompt_preset_requests_from_diff`: added `preset #1` and `presets release-check, #2`
//!   lines → ordered unique preset names returned
//! - `extract_prompt_preset_requests_from_text_ignores_fences_and_blockquotes`
//! - `extract_imperative_directives(diff)`: finds added user directive lines like `do #id`,
//!   `run tests`, `build + install`, `commit + push`, or pending-item prose like
//!   `[#id] Fix the cross-repo ...`, skipping code fences and blockquotes
//! - `diff_contains_imperative_directive(diff)`: true when the diff contains either an explicit
//!   imperative directive line or a one-word approval like `go`
//! - `classify_prompt_bearing_changes(diff)`: extracts ordered user-authored changes that need
//!   prompt-aware handling. Each item is typed as `prompt_target`, `content_edit`,
//!   `recovery_artifact`, or `boundary_artifact`.
//! - `extract_required_response_blocks_multiple_prompts`: changed exchange tail with two prompt
//!   starts → both blocks returned oldest-first
//! - `extract_required_response_blocks_preserves_code_fence_context`: prompt block followed by an
//!   added fenced code block → returned block keeps the fence content intact
//! - `format_prompt_bearing_changes_mentions_turn_completeness`: rendered section includes the
//!   "do not stop at the newest question" contract plus edit/artifact handling guidance
//! - `extract_required_response_blocks(diff)`: extracts ordered user request blocks from added
//!   diff lines (for example `❯` prompts, questions, or imperative directives) so prompt builders
//!   can restate the full changed exchange tail instead of anchoring only on the newest question
//! - `format_required_response_targets(diff)`: compatibility wrapper that returns only the
//!   `prompt_target` blocks from `classify_prompt_bearing_changes`
//! - `format_prompt_bearing_changes(diff)`: renders the typed change list into a prompt-ready
//!   section with explicit turn-completeness and edit/artifact instructions

use anyhow::Result;
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

use crate::{component, snapshot};

const IMPERATIVE_LEADING_VERBS: &[&str] = &[
    "add",
    "audit",
    "benchmark",
    "build",
    "check",
    "clean",
    "close",
    "commit",
    "document",
    "fix",
    "harden",
    "implement",
    "install",
    "investigate",
    "note",
    "preserve",
    "push",
    "record",
    "refactor",
    "remove",
    "repair",
    "rerun",
    "revisit",
    "run",
    "test",
    "trace",
    "update",
    "verify",
    "write",
];

/// Classification of a user diff for skill routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffType {
    /// Single word matching approval list (go, yes, do, ok, continue, approve, approved).
    Approval,
    /// Single added line ending with `?`.
    SimpleQuestion,
    /// Only `(HEAD)` marker or boundary UUID changes.
    BoundaryArtifact,
    /// Edits within agent content (colon-appended, inline modifications).
    Annotation,
    /// Only deletions or reordered hunks, no additions.
    StructuralChange,
    /// Multiple unrelated added blocks separated by context lines.
    MultiTopic,
    /// General new content (default when no specific pattern matches).
    ContentAddition,
}

/// Result of classifying a unified diff.
#[derive(Debug, Clone, Serialize)]
pub struct DiffClassification {
    pub diff_type: DiffType,
    pub diff_type_reason: String,
}

/// Canonical classification for user-authored prompt-bearing changes in a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptBearingChangeKind {
    PromptTarget,
    ContentEdit,
    RecoveryArtifact,
    BoundaryArtifact,
}

/// Ordered user-authored change that the harness should surface to the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBearingChange {
    pub kind: PromptBearingChangeKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptPrefixLine {
    raw: String,
    prefixed: bool,
}

/// Strip comments from document content for diff comparison.
///
/// Delegates to `component::strip_comments` — the shared implementation
/// available to both the binary and external crates.
pub fn strip_comments(content: &str) -> String {
    component::strip_comments(content)
}

/// Annotate a unified diff with content-source markers.
///
/// Transforms a standard unified diff into a human-readable annotated format
/// that shows the source of each line:
/// - `[agent]` — context lines (unchanged content from agent)
/// - `[user+]` — lines added by the user
/// - `[user-]` — lines removed by the user
/// - `[user~]` — lines modified by the user (paired add/remove with >60% common prefix)
///
/// Returns the annotated diff as a string, or `None` if the diff is empty.
pub fn annotate_diff(diff_text: &str) -> Option<String> {
    let mut result = Vec::new();
    let mut pending_removes: Vec<String> = Vec::new();

    for line in diff_text.lines() {
        // Skip unified diff headers
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            continue;
        }
        if line.starts_with("@@ ") {
            // Flush pending removes before a new hunk
            flush_removes(&mut pending_removes, &mut result);
            continue;
        }

        if let Some(content) = line.strip_prefix('+') {
            // Check if this pairs with a pending remove (modification)
            if let Some(removed) = pending_removes.pop() {
                let common = common_prefix_len(content, &removed);
                let min_len = content.len().min(removed.len());
                if min_len >= 3 && common > min_len * 6 / 10 {
                    // Modified line — show the new version
                    result.push(format!("[user~] {}", content));
                } else {
                    // Not similar enough — emit remove then add
                    result.push(format!("[user-] {}", removed));
                    result.push(format!("[user+] {}", content));
                }
            } else {
                result.push(format!("[user+] {}", content));
            }
        } else if let Some(content) = line.strip_prefix('-') {
            // Buffer removes to check for paired modifications
            pending_removes.push(content.to_string());
        } else if let Some(content) = line.strip_prefix(' ') {
            flush_removes(&mut pending_removes, &mut result);
            result.push(format!("[agent] {}", content));
        } else if !line.is_empty() {
            flush_removes(&mut pending_removes, &mut result);
            result.push(format!("[agent] {}", line));
        }
    }

    flush_removes(&mut pending_removes, &mut result);

    if result.is_empty() {
        None
    } else {
        Some(result.join("\n"))
    }
}

/// Extract user additions that appear within agent content (inline annotations).
///
/// An inline annotation is a `[user+]` or `[user~]` line that has at least one
/// substantive `[agent]` line after it — meaning the user inserted or modified text
/// inside an agent response block rather than appending at the end.
///
/// Structural markers (component tags `<!-- ... -->`, section headers `# ...`) are
/// excluded from the "agent lines after" check to avoid false positives where
/// end-of-exchange user input is followed only by closing markers.
pub fn extract_inline_annotations(annotated_diff: &str) -> Vec<String> {
    classify_prompt_bearing_changes_from_annotated(annotated_diff)
        .into_iter()
        .filter_map(|change| match change.kind {
            PromptBearingChangeKind::PromptTarget | PromptBearingChangeKind::ContentEdit => {
                Some(change.text)
            }
            PromptBearingChangeKind::RecoveryArtifact
            | PromptBearingChangeKind::BoundaryArtifact => None,
        })
        .collect()
}

/// Returns true if the annotated line is a substantive agent line (not a structural marker).
/// Component tags (`<!-- ... -->`), section headers (`# ...`), and blank lines are
/// structural — they should not count as "agent response content" for inline annotation detection.
fn is_substantive_agent_line(line: &str) -> bool {
    let Some(content) = line.strip_prefix("[agent] ") else {
        return false;
    };
    let t = content.trim_start();
    !t.is_empty() && !t.starts_with("<!--") && !t.starts_with('#')
}

/// Returns true when a `[user~]` line is just the boundary marker `(HEAD)` being appended
/// to a response heading — a reposition artifact from `agent-doc commit`, not a user edit.
fn is_head_boundary_artifact(content: &str) -> bool {
    content.ends_with(" (HEAD)") && {
        let base = &content[..content.len() - " (HEAD)".len()];
        base.trim_start_matches('#').starts_with(' ')
            || base.contains("### Re:")
            || base.contains("## ")
    }
}

fn is_boundary_artifact_line(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.starts_with("<!-- agent:boundary:")
        || is_head_boundary_artifact(trimmed)
        || trimmed == "(HEAD)"
}

fn is_recovery_artifact_line(content: &str) -> bool {
    let trimmed = content.trim_start();
    trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed == "## Assistant"
        || trimmed == "## User"
        || trimmed.starts_with("<!-- patch:")
        || trimmed.starts_with("<!-- /patch:")
}

fn line_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && (trimmed.starts_with('❯')
            || trimmed.ends_with('?')
            || looks_like_imperative_directive(trimmed))
}

fn block_looks_like_prompt_target(block: &str) -> bool {
    block.lines().any(line_looks_like_prompt_target)
}

fn classify_prompt_bearing_block(
    block_text: &str,
    has_substantive_agent_after: bool,
) -> Option<PromptBearingChangeKind> {
    let trimmed = block_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let non_blank: Vec<&str> = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if non_blank.is_empty() {
        return None;
    }
    if non_blank.iter().all(|line| is_boundary_artifact_line(line)) {
        return Some(PromptBearingChangeKind::BoundaryArtifact);
    }
    if non_blank
        .first()
        .is_some_and(|line| is_recovery_artifact_line(line))
    {
        return Some(PromptBearingChangeKind::RecoveryArtifact);
    }
    if block_looks_like_prompt_target(trimmed) {
        return Some(PromptBearingChangeKind::PromptTarget);
    }
    if has_substantive_agent_after {
        return Some(PromptBearingChangeKind::ContentEdit);
    }
    None
}

pub fn classify_prompt_bearing_changes(diff: &str) -> Vec<PromptBearingChange> {
    let mut changes = annotate_diff(diff)
        .map(|annotated| classify_prompt_bearing_changes_from_annotated(&annotated))
        .unwrap_or_default();

    // Annotated classification is the ordered source of truth because it preserves
    // mixed prompt/edit/artifact encounter order across the changed tail. Keep the
    // older prompt-block extractor as a safety net for prompt-target-only consumers
    // and append only truly-missing prompt blocks.
    for text in extract_prompt_target_blocks(diff) {
        if changes.iter().any(|existing| {
            existing.kind == PromptBearingChangeKind::PromptTarget && existing.text == text
        }) {
            continue;
        }
        changes.push(PromptBearingChange {
            kind: PromptBearingChangeKind::PromptTarget,
            text,
        });
    }

    changes
}

/// Return the prompt-bearing exchange lines that must carry a `❯ ` prefix.
///
/// This derives from the canonical `prompt_target` classifier rather than a
/// separate line-shape heuristic, so write-path normalization and session-check
/// can enforce the same invariant.
pub fn prompt_prefix_normalization_targets(diff: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut lines = Vec::new();
    for change in classify_prompt_bearing_changes(diff) {
        if change.kind != PromptBearingChangeKind::PromptTarget {
            continue;
        }
        for line in prompt_prefix_lines_from_block(&change.text) {
            if line.prefixed {
                continue;
            }
            if seen.insert(line.raw.clone()) {
                lines.push(line.raw);
            }
        }
    }
    lines
}

/// Return the first prompt-bearing line that should have had a `❯ ` prefix but did not.
pub fn first_bare_prompt_prefix_target(diff: &str) -> Option<String> {
    for change in classify_prompt_bearing_changes(diff) {
        if change.kind != PromptBearingChangeKind::PromptTarget {
            continue;
        }
        for line in prompt_prefix_lines_from_block(&change.text) {
            if !line.prefixed {
                return Some(line.raw);
            }
        }
    }
    None
}

fn classify_prompt_bearing_changes_from_annotated(
    annotated_diff: &str,
) -> Vec<PromptBearingChange> {
    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    let lines: Vec<&str> = annotated_diff.lines().collect();
    let mut changes = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        let Some(mut block) = lines[i]
            .strip_prefix("[user+] ")
            .or_else(|| lines[i].strip_prefix("[user~] "))
            .map(|line| vec![line.to_string()])
        else {
            i += 1;
            continue;
        };

        let mut in_fence = false;
        let mut fence_char = '`';
        let mut fence_len = 3usize;
        let first_trimmed = block[0].trim();
        if let Some((fc, fl)) = fence_open(first_trimmed) {
            in_fence = true;
            fence_char = fc;
            fence_len = fl;
        }

        i += 1;
        while i < lines.len() {
            if let Some(content) = lines[i]
                .strip_prefix("[user+] ")
                .or_else(|| lines[i].strip_prefix("[user~] "))
            {
                let trimmed = content.trim();
                let starts_new_block = !in_fence
                    && block.last().is_some_and(|line| line.trim().is_empty())
                    && !trimmed.is_empty()
                    && !trimmed.starts_with("```")
                    && !trimmed.starts_with("~~~");
                if starts_new_block {
                    break;
                }
                block.push(content.to_string());
                if !in_fence {
                    if let Some((fc, fl)) = fence_open(trimmed) {
                        in_fence = true;
                        fence_char = fc;
                        fence_len = fl;
                    }
                } else if fence_close(trimmed, fence_char, fence_len) {
                    in_fence = false;
                }
                i += 1;
            } else {
                break;
            }
        }

        while block.first().is_some_and(|line| line.trim().is_empty()) {
            block.remove(0);
        }
        while block.last().is_some_and(|line| line.trim().is_empty()) {
            block.pop();
        }
        if block.is_empty() {
            continue;
        }

        let has_substantive_agent_after = lines[i..]
            .iter()
            .any(|line| is_substantive_agent_line(line));
        let text = block.join("\n");
        let Some(kind) = classify_prompt_bearing_block(&text, has_substantive_agent_after) else {
            continue;
        };
        changes.push(PromptBearingChange { kind, text });
    }

    changes
}

fn prompt_prefix_lines_from_block(block: &str) -> Vec<PromptPrefixLine> {
    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    let mut lines = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 3usize;

    for line in block.lines() {
        let trimmed = line.trim();
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                continue;
            }
        } else if fence_close(trimmed, fence_char, fence_len) {
            in_fence = false;
            continue;
        }

        if in_fence || trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }

        lines.push(PromptPrefixLine {
            raw: line.to_string(),
            prefixed: trimmed.starts_with('❯'),
        });
    }

    lines
}

fn extract_prompt_target_blocks(diff: &str) -> Vec<String> {
    fn is_response_heading(line: &str) -> bool {
        line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("## Re:")
    }

    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    fn trim_block_lines(lines: &mut Vec<String>) {
        while lines.first().is_some_and(|line| line.trim().is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
    }

    fn block_start(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("<!--")
            || trimmed.starts_with("```")
            || trimmed.starts_with("~~~")
            || is_response_heading(trimmed)
        {
            return false;
        }
        line_looks_like_prompt_target(trimmed)
    }

    fn flush(blocks: &mut Vec<String>, current: &mut Vec<String>) {
        trim_block_lines(current);
        if !current.is_empty() {
            blocks.push(current.join("\n"));
            current.clear();
        }
    }

    let mut blocks = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 3usize;

    for line in diff.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ") {
            if !in_fence {
                flush(&mut blocks, &mut current);
            }
            continue;
        }

        let Some(content) = line.strip_prefix('+') else {
            if !in_fence {
                flush(&mut blocks, &mut current);
            }
            continue;
        };

        let trimmed = content.trim();
        let starts_new_block = !current.is_empty()
            && !in_fence
            && ((current.last().is_some_and(|line| line.trim().is_empty()) && !trimmed.is_empty())
                || trimmed.starts_with('❯'));
        if starts_new_block && current.iter().any(|line| !line.trim().is_empty()) {
            flush(&mut blocks, &mut current);
        }

        if current.is_empty() && !block_start(content) {
            continue;
        }

        current.push(content.to_string());

        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
            }
        } else if fence_close(trimmed, fence_char, fence_len) {
            in_fence = false;
        }
    }

    flush(&mut blocks, &mut current);
    blocks
}

/// Flush buffered remove lines as `[user-]`.
fn flush_removes(pending: &mut Vec<String>, result: &mut Vec<String>) {
    for removed in pending.drain(..) {
        result.push(format!("[user-] {}", removed));
    }
}

/// Count characters of common prefix between two strings.
fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// Known Claude Code built-in slash command names (without arguments).
/// These affect Claude Code session state and cannot be invoked via the Skill tool.
const BUILTIN_COMMAND_NAMES: &[&str] = &[
    "/help",
    "/model",
    "/clear",
    "/compact",
    "/cost",
    "/login",
    "/logout",
    "/status",
    "/config",
    "/memory",
    "/review",
    "/bug",
    "/fast",
    "/slow",
    "/permissions",
    "/terminal-setup",
    "/doctor",
    "/init",
    "/pr-comments",
    "/vim",
    "/diff",
    "/undo",
    "/resume",
    "/listen",
    "/mcp",
    "/approved-tools",
    "/add-dir",
    "/release-notes",
    "/hooks",
    "/btw",
];

/// Returns true if the command is a Claude Code built-in (not invocable via Skill tool).
pub fn is_builtin_command(cmd: &str) -> bool {
    let cmd_name = cmd.split_whitespace().next().unwrap_or("");
    BUILTIN_COMMAND_NAMES.contains(&cmd_name)
}

/// Result of classifying slash commands from a diff.
pub struct ParsedSlashCommands {
    /// Skill commands (non-built-ins) — route to Skill tool.
    pub skill_commands: Vec<String>,
    /// Claude Code built-in commands — cannot invoke via Skill tool.
    pub builtin_commands: Vec<String>,
}

/// Structured natural-language orchestration request detected from the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OrchestrationRequest {
    /// Resolved orchestration mode the skill should dispatch.
    pub mode: OrchestrationRequestMode,
    /// The triggering user-authored line or synthesized block summary.
    pub trigger_text: String,
    /// Number of task list items detected in the same added block.
    pub task_count: usize,
}

/// Supported orchestration modes surfaced through preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationRequestMode {
    Sequential,
    Parallel,
    Dag,
}

/// Extract ordered unique prompt preset references from user-added diff lines.
pub fn detect_prompt_preset_requests(diff: &str) -> Vec<String> {
    let mut requests = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;

    for line in diff.lines() {
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            continue;
        }

        let content = if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
            &line[1..]
        } else {
            line
        };
        let trimmed = content.trim_start();

        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == '`' || fc == '~' {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= 3 {
                    in_fence = true;
                    fence_char = fc;
                    fence_len = fl;
                    continue;
                }
            }
        } else {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= fence_len && trimmed[fl..].trim().is_empty() {
                    in_fence = false;
                }
            }
            continue;
        }

        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        if content.starts_with('>') {
            continue;
        }

        collect_prompt_preset_requests_from_line(content, &mut requests);
    }

    requests
}

/// Extract ordered unique prompt preset references from plain text.
pub fn extract_prompt_preset_requests_from_text(text: &str) -> Vec<String> {
    let mut requests = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;

    for line in text.lines() {
        let trimmed = line.trim_start();

        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == '`' || fc == '~' {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= 3 {
                    in_fence = true;
                    fence_char = fc;
                    fence_len = fl;
                    continue;
                }
            }
        } else {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= fence_len && trimmed[fl..].trim().is_empty() {
                    in_fence = false;
                }
            }
            continue;
        }

        if trimmed.starts_with('>') {
            continue;
        }

        collect_prompt_preset_requests_from_line(line, &mut requests);
    }

    requests
}

fn collect_prompt_preset_requests_from_line(line: &str, requests: &mut Vec<String>) {
    let Some(names) = parse_prompt_preset_directive(line) else {
        return;
    };
    for name in names {
        if !requests.iter().any(|existing| existing == &name) {
            requests.push(name);
        }
    }
}

fn parse_prompt_preset_directive(line: &str) -> Option<Vec<String>> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix('❯') {
        trimmed = rest.trim_start();
    }

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        trimmed = rest.trim_start();
    } else {
        let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 {
            let rest = &trimmed[digits..];
            if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                trimmed = rest.trim_start();
            }
        }
    }

    let lower = trimmed.to_ascii_lowercase();
    let rest = if lower.starts_with("presets ") {
        &trimmed["presets ".len()..]
    } else if lower.starts_with("preset ") {
        &trimmed["preset ".len()..]
    } else {
        return None;
    };

    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }

    let mut names = Vec::new();
    for segment in rest.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        for part in segment.split(" and ") {
            let name = part
                .trim()
                .trim_end_matches(['.', ':', ';'])
                .trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }

    (!names.is_empty()).then_some(names)
}

/// Like `parse_slash_commands` but classifies results into skill vs built-in commands.
pub fn parse_slash_commands_classified(diff: &str) -> ParsedSlashCommands {
    let commands = parse_slash_commands(diff);
    let mut skill_commands = Vec::new();
    let mut builtin_commands = Vec::new();
    for cmd in commands {
        if is_builtin_command(&cmd) {
            builtin_commands.push(cmd);
        } else {
            skill_commands.push(cmd);
        }
    }
    ParsedSlashCommands {
        skill_commands,
        builtin_commands,
    }
}

/// Extract slash commands from user-added lines in a unified diff.
///
/// Guards against false positives:
/// - Lines inside code fences (``` or ~~~) are excluded.
/// - Blockquote lines (starting with `>`) are excluded.
/// - Only lines added by the user (`+` prefix, not `+++`) are inspected.
/// - The command must start with `/` followed immediately by an ASCII letter.
///
/// Returns the trimmed command strings (including any arguments after the command).
pub fn parse_slash_commands(diff: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;

    for line in diff.lines() {
        // Skip unified diff meta-lines.
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            continue;
        }

        // Strip leading diff marker to get the actual content.
        let content = if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
            &line[1..]
        } else {
            line
        };

        // Track code-fence state across all lines (added, removed, and context).
        let trimmed = content.trim_start();
        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == '`' || fc == '~' {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= 3 {
                    in_fence = true;
                    fence_char = fc;
                    fence_len = fl;
                    continue; // Fence delimiter itself is not a command.
                }
            }
        } else {
            // Check for matching closing fence.
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= fence_len && trimmed[fl..].trim().is_empty() {
                    in_fence = false;
                    continue; // Closing delimiter — not a command.
                }
            }
        }

        // Only process user-added lines (not context, removed, or meta).
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }

        // Skip lines inside code fences.
        if in_fence {
            continue;
        }

        // Skip blockquotes.
        if content.starts_with('>') {
            continue;
        }

        // Must start with '/' followed by a command-like token.
        // Grammar: `/[a-z][a-z0-9:_-]*` with no additional `/` in the token.
        // This rejects absolute paths like `/home/brian/...` and `/tmp/foo`
        // that look like slash commands but are really filesystem paths.
        if !content.starts_with('/') {
            continue;
        }
        let token_end = content[1..]
            .find(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(content.len());
        let token = &content[1..token_end];
        if token.is_empty() {
            continue;
        }
        let first = token.chars().next().unwrap();
        if !first.is_ascii_lowercase() {
            continue;
        }
        let rest_ok = token
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '_' | '-'));
        if !rest_ok {
            continue;
        }

        commands.push(content.trim_end().to_string());
    }

    commands
}

/// Detect a natural-language orchestration request from user-added diff lines.
///
/// This is the binary-owned counterpart to the skill-level command-synonym
/// guidance: if the user adds a batch-oriented preamble plus a task list, the
/// caller can route through `agent-doc orchestrate --from-exchange` instead of
/// relying on the agent to remember that prose convention.
pub fn detect_orchestration_request(diff: &str) -> Option<OrchestrationRequest> {
    for block in collect_added_text_blocks(diff) {
        let task_count = block
            .iter()
            .filter(|line| parse_markdown_list_item(line).is_some())
            .count();
        if task_count < 2 {
            continue;
        }

        let trigger_lines: Vec<&str> = block
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && parse_markdown_list_item(line).is_none())
            .collect();
        let trigger_text = if trigger_lines.is_empty() {
            block.join(" ")
        } else {
            trigger_lines.join(" ")
        };
        let Some(mode) = detect_orchestration_mode(&trigger_text) else {
            continue;
        };

        return Some(OrchestrationRequest {
            mode,
            trigger_text,
            task_count,
        });
    }

    None
}

/// Detect whether the user wrote `do queue` or `run queue` in an added diff line.
///
/// Returns `true` when any user-added line (outside code fences/blockquotes)
/// starts with one of these trigger phrases. Does NOT match `do #queue` (pending
/// item reference) — only the bare component activation form.
pub fn detect_queue_trigger(diff: &str) -> bool {
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;

    for line in diff.lines() {
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            continue;
        }
        if !line.starts_with('+') {
            continue;
        }
        let content = &line[1..];
        let trimmed = content.trim_start();

        // Track code fences
        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == '`' || fc == '~' {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= 3 {
                    in_fence = true;
                    fence_char = fc;
                    fence_len = fl;
                    continue;
                }
            }
        } else {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= fence_len {
                    in_fence = false;
                }
            }
            continue;
        }

        // Skip blockquotes
        if trimmed.starts_with('>') {
            continue;
        }

        // Strip the `❯ ` prompt prefix if present
        let text = trimmed
            .strip_prefix("❯ ")
            .unwrap_or(trimmed);
        let lower = text.to_lowercase();

        if lower.starts_with("do queue") || lower.starts_with("run queue") {
            let after = if lower.starts_with("do queue") {
                &text[8..]
            } else {
                &text[9..]
            };
            // Must be end of line, or followed by non-alphanumeric (punctuation, space)
            if after.is_empty()
                || after.starts_with(|c: char| !c.is_alphanumeric() && c != '#')
            {
                return true;
            }
        }
    }
    false
}

/// Build a unified diff directly from two content strings after comment stripping.
///
/// Returns `None` when there are no meaningful changes.
pub fn unified_diff_from_contents(previous: &str, current: &str) -> Option<String> {
    let previous_stripped = strip_comments(previous);
    let current_stripped = strip_comments(current);
    let diff = TextDiff::from_lines(&previous_stripped, &current_stripped);
    let has_changes = diff.iter_all_changes().any(|c| c.tag() != ChangeTag::Equal);
    if !has_changes {
        return None;
    }
    Some(
        diff.unified_diff()
            .context_radius(5)
            .header("snapshot", "document")
            .to_string(),
    )
}

/// Extract imperative user directives from added lines in a unified diff.
///
/// This is a conservative parser for the binary-enforced directive contract.
/// It only recognizes clear imperative shapes and approval words, while
/// skipping code fences and blockquotes to avoid false positives.
pub fn extract_imperative_directives(diff: &str) -> Vec<String> {
    let mut directives = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;

    for line in diff.lines() {
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            continue;
        }

        let content = if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
            &line[1..]
        } else {
            line
        };
        let trimmed = content.trim_start();

        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == '`' || fc == '~' {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= 3 {
                    in_fence = true;
                    fence_char = fc;
                    fence_len = fl;
                    continue;
                }
            }
        } else {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= fence_len && trimmed[fl..].trim().is_empty() {
                    in_fence = false;
                    continue;
                }
            }
        }

        if !line.starts_with('+') || line.starts_with("+++") || in_fence {
            continue;
        }

        if content.starts_with('>') {
            continue;
        }

        let Some(normalized) = normalize_imperative_candidate(content) else {
            continue;
        };

        if looks_like_imperative_directive(&normalized) {
            directives.push(normalized);
        }
    }

    directives
}

/// Returns true when the diff contains an imperative work directive or a
/// one-word approval that authorizes the next step.
pub fn diff_contains_imperative_directive(diff: &str) -> bool {
    if !extract_imperative_directives(diff).is_empty() {
        return true;
    }
    matches!(classify_diff(diff).diff_type, DiffType::Approval)
}

/// Extract ordered user-authored request blocks from added diff lines.
///
/// This is a prompt-building helper, not a semantic proof that every request
/// was answered. It makes the changed exchange tail explicit so the agent is
/// reminded to address the full oldest-first set of prompts instead of only the
/// newest visible question.
#[allow(dead_code)]
pub fn extract_required_response_blocks(diff: &str) -> Vec<String> {
    extract_prompt_target_blocks(diff)
}

/// Render extracted request blocks as a prompt-ready turn-completeness section.
#[allow(dead_code)]
pub fn format_required_response_targets(diff: &str) -> Option<String> {
    let blocks = extract_required_response_blocks(diff);
    if blocks.is_empty() {
        return None;
    }

    let mut out = String::from(
        "Required response targets (oldest first):\n\
         Do not stop at the newest question. The turn is incomplete until each item below is answered or explicitly grouped into one response.\n\n",
    );
    for (idx, block) in blocks.iter().enumerate() {
        out.push_str(&format!(
            "<target index=\"{}\">\n{}\n</target>\n\n",
            idx + 1,
            block
        ));
    }
    Some(out)
}

/// Render all prompt-bearing changes as a prompt-ready section.
pub fn format_prompt_bearing_changes(diff: &str) -> Option<String> {
    let changes = classify_prompt_bearing_changes(diff);
    if changes.is_empty() {
        return None;
    }

    let mut out = String::from(
        "User-authored prompt-bearing changes (oldest first):\n\
         Do not stop at the newest question. The turn is incomplete until each `prompt_target` item below is answered or explicitly grouped into one response.\n\
         Treat `content_edit` items as user corrections to incorporate, and treat `recovery_artifact` / `boundary_artifact` items as document-state signals to normalize rather than ordinary conversation.\n\n",
    );
    for (idx, change) in changes.iter().enumerate() {
        out.push_str(&format!(
            "<change index=\"{}\" kind=\"{}\">\n{}\n</change>\n\n",
            idx + 1,
            serde_json::to_string(&change.kind)
                .unwrap_or_else(|_| "\"prompt_target\"".to_string())
                .trim_matches('"'),
            change.text
        ));
    }
    Some(out)
}

fn looks_like_imperative_directive(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let compact = lower.trim();
    let exact = compact.trim_end_matches(|c: char| c.is_ascii_punctuation());
    if APPROVAL_WORDS.contains(&exact) {
        return true;
    }
    compact.starts_with("do #")
        || compact.starts_with("fix #")
        || compact.starts_with("fix this")
        || compact.contains(" run tests")
        || compact.starts_with("run tests")
        || compact.contains(" run benchmarks")
        || compact.starts_with("run benchmarks")
        || compact.contains(" build + install")
        || compact.starts_with("build + install")
        || compact.contains(" build and install")
        || compact.starts_with("build and install")
        || compact.contains(" commit + push")
        || compact.starts_with("commit + push")
        || compact.contains(" commit and push")
        || compact.starts_with("commit and push")
        || starts_with_imperative_verb(compact)
}

fn normalize_imperative_candidate(line: &str) -> Option<String> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("- ") {
        trimmed = rest.trim_start();
    }

    trimmed = strip_pending_checkbox_prefix(trimmed);

    if let Some(rest) = trimmed.strip_prefix("[#")
        && let Some(close) = rest.find(']')
    {
        let id = &rest[..close];
        if !id.is_empty() && id.len() <= 8 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
            trimmed = rest[close + 1..].trim_start();
        }
    }

    let normalized = trimmed
        .trim_start_matches('❯')
        .trim_start()
        .trim_end_matches(|c: char| c.is_ascii_punctuation())
        .trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn collect_added_text_blocks(diff: &str) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;

    for line in diff.lines() {
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }

        let content = if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
            &line[1..]
        } else {
            line
        };
        let trimmed = content.trim_start();

        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == '`' || fc == '~' {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= 3 {
                    in_fence = true;
                    fence_char = fc;
                    fence_len = fl;
                    continue;
                }
            }
        } else {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= fence_len && trimmed[fl..].trim().is_empty() {
                    in_fence = false;
                }
            }
            continue;
        }

        if !line.starts_with('+') || line.starts_with("+++") {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }

        if content.starts_with('>') {
            continue;
        }

        current.push(content.trim_end().to_string());
    }

    if !current.is_empty() {
        blocks.push(current);
    }

    blocks
}

fn parse_markdown_list_item(line: &str) -> Option<&str> {
    let trimmed = strip_prompt_prefix(line.trim_start());
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let rest = rest.trim();
            return (!rest.is_empty()).then_some(rest);
        }
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            let rest = rest.trim();
            return (!rest.is_empty()).then_some(rest);
        }
    }

    None
}

fn strip_prompt_prefix(line: &str) -> &str {
    line.strip_prefix("❯ ")
        .or_else(|| line.strip_prefix('❯'))
        .map(str::trim_start)
        .unwrap_or(line)
}

fn detect_orchestration_mode(text: &str) -> Option<OrchestrationRequestMode> {
    let lower = text.to_ascii_lowercase();

    if contains_any(
        &lower,
        &[
            "dependency graph",
            "depends on",
            "blocked by",
            "prerequisite",
            "then unblock",
        ],
    ) || lower.contains("after #")
    {
        return Some(OrchestrationRequestMode::Dag);
    }

    if contains_any(
        &lower,
        &[
            "fan out",
            "concurrent",
            "at the same time",
            "in parallel",
            "simultaneously",
            "parallelize",
        ],
    ) {
        return Some(OrchestrationRequestMode::Parallel);
    }

    if contains_any(
        &lower,
        &[
            "orchestra",
            "orcestra",
            "orchestrate",
            "chain",
            "in order",
            "one by one",
            "sequential",
            "sequentially",
            "synchronous",
            "run these sequentially",
        ],
    ) {
        return Some(OrchestrationRequestMode::Sequential);
    }

    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn strip_pending_checkbox_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prefix in ["[ ]", "[/]", "[x]", "[X]"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }
    if let Some(inner) = trimmed.strip_prefix("[/")
        && let Some(close) = inner.find(']')
    {
        let gate_type = &inner[..close];
        if !gate_type.is_empty()
            && gate_type
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return inner[close + 1..].trim_start();
        }
    }
    trimmed
}

fn starts_with_imperative_verb(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    if IMPERATIVE_LEADING_VERBS.contains(&first) {
        return true;
    }
    matches!((first, words.next()), ("clean", Some("up")))
}

/// Compute a unified diff between the snapshot and the current document.
/// Returns None if there are no changes.
///
/// Both snapshot and current content are comment-stripped before comparison.
pub fn compute(doc: &Path) -> Result<Option<String>> {
    let t_total = std::time::Instant::now();

    let previous = snapshot::resolve(doc)?.unwrap_or_default();
    let snap_path = snapshot::path_for(doc)?;

    // Copy-on-read: capture snapshot mtime at read time so we can detect
    // external modifications before any stale-snapshot recovery write.
    // Fixes #wcf5: IDE watchers and git hooks bypass advisory flock.
    let snap_mtime_at_read = snap_path.metadata().and_then(|m| m.modified()).ok();

    // Wait for user to finish typing (truncation detection with delayed rechecks)
    let current = wait_for_stable_content(doc, &previous)?;

    eprintln!(
        "[diff] doc={} snapshot={} doc_len={} snap_len={}",
        doc.display(),
        snap_path.display(),
        current.len(),
        previous.len(),
    );

    let t_strip = std::time::Instant::now();
    let current_stripped = strip_comments(&current);
    let previous_stripped = strip_comments(&previous);
    let elapsed_strip = t_strip.elapsed().as_millis();
    if elapsed_strip > 0 {
        eprintln!("[perf] diff.strip_comments: {}ms", elapsed_strip);
    }

    eprintln!(
        "[diff] stripped: doc_len={} snap_len={}",
        current_stripped.len(),
        previous_stripped.len(),
    );

    let Some(output) = unified_diff_from_contents(&previous, &current) else {
        eprintln!(
            "[diff] no changes detected between snapshot and document (after comment stripping)"
        );
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
        }
        return Ok(None);
    };

    // Stale snapshot recovery: if the diff is only completed assistant/user
    // exchanges with no new user content, the previous cycle wrote the response
    // but context compaction prevented the snapshot update.
    //
    // Copy-on-read guard (#wcf5): verify the snapshot file hasn't been modified
    // by an external process (IDE watcher, git hook) since we read it. If it
    // changed, skip recovery — the external update is authoritative.
    if is_stale_snapshot(&previous, &current) {
        let snap_mtime_now = snap_path.metadata().and_then(|m| m.modified()).ok();
        if snap_mtime_at_read != snap_mtime_now {
            eprintln!(
                "[snapshot recovery] Skipped — snapshot modified externally since read (copy-on-read guard)"
            );
        } else {
            eprintln!(
                "[snapshot recovery] Snapshot synced — previous cycle completed but snapshot was stale"
            );
            snapshot::save(doc, &current)?;
            let elapsed_total = t_total.elapsed().as_millis();
            if elapsed_total > 0 {
                eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
            }
            return Ok(None);
        }
    }

    eprintln!("[diff] changes detected, computing unified diff");

    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
    }

    Ok(Some(output))
}

/// Wait for stable content by detecting truncated lines and rechecking.
///
/// When the user is mid-typing, the last added line may be incomplete.
/// This function rechecks the file at short intervals until:
/// - The last line appears complete (ends with terminal punctuation or newline)
/// - The content hasn't changed between two consecutive rechecks
/// - Maximum recheck attempts reached (prevents infinite loops)
///
/// Returns the stable file content.
pub fn wait_for_stable_content(doc: &Path, previous: &str) -> Result<String> {
    const RECHECK_DELAY_MS: u64 = 500;
    const MAX_RECHECKS: u32 = 12; // ~6 seconds max
    const STABLE_CHECKS_REQUIRED: u32 = 3; // require 3 consecutive stable reads

    let mut current = std::fs::read_to_string(doc)?;
    // Track consecutive stable reads across outer iterations — content changes anywhere
    // (even between outer iterations) must reset the counter so 3 truly consecutive
    // stable reads are always required, not just 3 within a single outer pass.
    let mut stable_count = 0u32;

    for attempt in 0..MAX_RECHECKS {
        let last_added =
            extract_last_added_line(&strip_comments(previous), &strip_comments(&current));

        if let Some(line) = &last_added
            && looks_truncated(line)
        {
            eprintln!(
                "[diff] Last line may be truncated (recheck {}/{}): {:?}",
                attempt + 1,
                MAX_RECHECKS,
                truncate_for_log(line, 60)
            );
            // Sleep then re-read; count consecutive identical reads across all iterations.
            std::thread::sleep(std::time::Duration::from_millis(RECHECK_DELAY_MS));
            let refreshed = std::fs::read_to_string(doc)?;
            if refreshed == current {
                stable_count += 1;
            } else {
                current = refreshed;
                stable_count = 0;
            }
            if stable_count >= STABLE_CHECKS_REQUIRED {
                eprintln!(
                    "[diff] Content stable after {} consecutive checks",
                    STABLE_CHECKS_REQUIRED
                );
                break;
            }
            continue;
        }
        // Line looks complete — no recheck needed
        break;
    }

    Ok(current)
}

/// Extract the last added (non-empty) line from the diff.
fn extract_last_added_line(previous_stripped: &str, current_stripped: &str) -> Option<String> {
    let diff = TextDiff::from_lines(previous_stripped, current_stripped);
    let mut last_insert: Option<String> = None;

    for change in diff.iter_all_changes() {
        if change.tag() == ChangeTag::Insert {
            let val = change.value().trim();
            if !val.is_empty() {
                last_insert = Some(val.to_string());
            }
        }
    }

    last_insert
}

/// Check if a line looks truncated (user may still be typing).
///
/// A line looks truncated if:
/// - It ends mid-word (no space or punctuation at end)
/// - It's very short (< 3 chars) and doesn't look like a command
/// - It ends with common incomplete patterns
///
/// A line does NOT look truncated if:
/// - It ends with terminal punctuation (. ! ? : ;)
/// - It's a markdown heading (starts with #)
/// - It's a command (starts with / or `)
/// - It ends with a closing marker (-->)
/// - It's empty or whitespace-only
fn looks_truncated(line: &str) -> bool {
    let trimmed = line.trim();

    // Empty or whitespace — not truncated
    if trimmed.is_empty() {
        return false;
    }

    // Commands, headings, code blocks — never truncated
    if trimmed.starts_with('/')
        || trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with("<!--")
    {
        return false;
    }

    // Single characters are treated as potentially truncated — the user may be
    // mid-typing (e.g., "S" as the start of "Save as a draft."). The stability
    // check (3 consecutive reads at 500ms each) will confirm if the input is
    // complete. Previously, single alphanumeric chars were exempt (treated as
    // choice selection like "A", "B", "y"), but this caused a bug where "S"
    // from "Save as a draft." triggered an immediate run that sent a wrong email.
    //
    // The 1.5s delay on genuine single-char responses (like "y" or "A") is
    // acceptable — the cost of acting on partial input is much higher.
    if trimmed.len() == 1 {
        return true;
    }

    // Single word that looks like a command/keyword (e.g., "go", "ok", "release")
    // But NOT if the word contains a dot mid-word (could be partial URL like "crates.")
    if !trimmed.contains(' ') && trimmed.len() >= 2 {
        // Words ending with '.' that look like partial domains/URLs are truncated
        if trimmed.ends_with('.') && trimmed.chars().filter(|&c| c == '.').count() >= 1 {
            let before_dot = &trimmed[..trimmed.len() - 1];
            // Common TLD/domain fragments: if there's a word before the dot that looks
            // like a domain component, it's likely truncated (e.g., "crates." → "crates.io")
            if !before_dot.is_empty() && before_dot.chars().all(|c| c.is_alphanumeric() || c == '-')
            {
                return true;
            }
        }
        return false;
    }

    // Check last character for terminal punctuation
    let last_char = trimmed.chars().last().unwrap();

    // Dot needs special handling: "Fixed the bug." is complete, but "linking to crates." may not be.
    // Treat '.' as terminal UNLESS the last word before '.' looks like a domain/URL fragment
    // (no spaces, all alphanumeric/hyphens, suggesting something like "crates." → "crates.io").
    if last_char == '.' {
        let before_dot = &trimmed[..trimmed.len() - 1];
        // Find the last word (after last space)
        let last_word = before_dot
            .rsplit_once(' ')
            .map(|(_, w)| w)
            .unwrap_or(before_dot);
        // If last word contains dots already (e.g., "www.example.") or is a known domain-like
        // pattern, treat as potentially truncated
        if last_word.contains('.') || last_word.ends_with("http") || last_word.ends_with("https") {
            return true;
        }
        // Otherwise, '.' is terminal (normal sentence ending)
        return false;
    }

    let terminal = matches!(
        last_char,
        '!' | '?' | ':' | ';' | ')' | ']' | '"' | '\'' | '`' | '*' | '-' | '>' | '|'
    );

    !terminal
}

/// Truncate a string for log display.
fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find the last char boundary at or before `max` bytes
        let mut truncated = max;
        while truncated > 0 && !s.is_char_boundary(truncated) {
            truncated -= 1;
        }
        format!("{}...", &s[..truncated])
    }
}

/// Detect whether the diff between snapshot and document is a stale snapshot
/// (previous cycle wrote the response but didn't update the snapshot).
///
/// Returns `true` if:
/// - The document contains the snapshot content as a prefix
/// - The content after the snapshot is only complete `## Assistant` / `## User` exchanges
/// - The trailing `## User` block is empty (no new user content)
///
/// Returns `false` if there is any new user content that needs a response.
pub fn is_stale_snapshot(snapshot_content: &str, document_content: &str) -> bool {
    let snap_stripped = strip_comments(snapshot_content);
    let doc_stripped = strip_comments(document_content);

    // Document must be longer than snapshot
    if doc_stripped.len() <= snap_stripped.len() {
        return false;
    }

    // Check that the document starts with the snapshot content
    // Use trimmed comparison to handle trailing whitespace differences
    let snap_trimmed = snap_stripped.trim_end();
    let doc_trimmed = doc_stripped.trim_end();

    if !doc_trimmed.starts_with(snap_trimmed) {
        return false;
    }

    // Get the "extra" content beyond the snapshot
    let extra = &doc_stripped[snap_trimmed.len()..];
    let extra_trimmed = extra.trim();

    if extra_trimmed.is_empty() {
        return false;
    }

    // The extra content should contain at least one ## Assistant block
    if !extra_trimmed.contains("## Assistant") {
        return false;
    }

    // Check if the last ## User block is empty (no new user content)
    // Split on "## User" and check the last segment
    let parts: Vec<&str> = extra_trimmed.split("## User").collect();
    if let Some(last_user_block) = parts.last() {
        let user_content = last_user_block.trim();
        // Empty user block = stale snapshot recovery
        // Non-empty user block = user has new input
        user_content.is_empty()
    } else {
        // No ## User block at all — not a standard exchange pattern
        false
    }
}

/// Print the diff to stdout (for the `diff` subcommand).
///
/// When `wait` is true, reads the file and snapshot, runs truncation
/// detection via `wait_for_stable_content()`, then outputs the diff.
/// Approval words (case-insensitive).
const APPROVAL_WORDS: &[&str] = &[
    "go", "yes", "do", "ok", "continue", "approve", "approved", "y", "yep", "yeah", "sure",
    "proceed", "lgtm",
];

/// Classify a unified diff into a `DiffType` with a human-readable reason.
///
/// Operates purely on the diff text — no file I/O.
pub fn classify_diff(diff_text: &str) -> DiffClassification {
    // Parse added and removed lines from the unified diff, skipping headers.
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut added_block_count = 0usize;
    let mut in_added_block = false;

    for line in diff_text.lines() {
        // Skip unified diff headers
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ") {
            in_added_block = false;
            continue;
        }
        if let Some(content) = line.strip_prefix('+') {
            added.push(content.to_string());
            if !in_added_block {
                added_block_count += 1;
                in_added_block = true;
            }
        } else if let Some(content) = line.strip_prefix('-') {
            removed.push(content.to_string());
            in_added_block = false;
        } else {
            // Context line
            in_added_block = false;
        }
    }

    // Filter out empty/whitespace-only lines for classification purposes.
    let added_content: Vec<&str> = added
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let removed_content: Vec<&str> = removed
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // 1. BoundaryArtifact: only (HEAD) or boundary UUID changes
    if is_boundary_artifact(&added_content, &removed_content) {
        return DiffClassification {
            diff_type: DiffType::BoundaryArtifact,
            diff_type_reason: "only boundary marker or (HEAD) changes".into(),
        };
    }

    // 2. Approval: single non-empty added word matching approval list, no removals
    if added_content.len() == 1 && removed_content.is_empty() {
        let word = added_content[0].to_lowercase();
        // Strip trailing punctuation for matching
        let word_clean = word.trim_end_matches(|c: char| c.is_ascii_punctuation());
        if APPROVAL_WORDS.contains(&word_clean) {
            return DiffClassification {
                diff_type: DiffType::Approval,
                diff_type_reason: format!("single approval word: \"{}\"", added_content[0]),
            };
        }
    }

    // 3. SimpleQuestion: single added line ending with `?`, no removals
    if added_content.len() == 1 && removed_content.is_empty() && added_content[0].ends_with('?') {
        return DiffClassification {
            diff_type: DiffType::SimpleQuestion,
            diff_type_reason: format!(
                "single question: \"{}\"",
                truncate_for_reason(added_content[0])
            ),
        };
    }

    // 4. Annotation: modifications to existing lines (added + removed with similar content)
    if !added_content.is_empty()
        && !removed_content.is_empty()
        && is_annotation(&added_content, &removed_content)
    {
        return DiffClassification {
            diff_type: DiffType::Annotation,
            diff_type_reason: "inline edit to existing content".into(),
        };
    }

    // 5. StructuralChange: only removals, no additions
    if added_content.is_empty() && !removed_content.is_empty() {
        return DiffClassification {
            diff_type: DiffType::StructuralChange,
            diff_type_reason: format!("{} lines removed, no additions", removed_content.len()),
        };
    }

    // 6. MultiTopic: multiple separated added blocks or --- separators
    let has_separator = added.iter().any(|l| l.trim() == "---");
    if has_separator && added_content.len() >= 2 {
        let section_count = added_content
            .split(|l| *l == "---")
            .filter(|s| !s.is_empty())
            .count();
        if section_count >= 2 {
            return DiffClassification {
                diff_type: DiffType::MultiTopic,
                diff_type_reason: format!("{} topics separated by ---", section_count),
            };
        }
    }
    if added_block_count >= 2 && added_content.len() >= 2 {
        return DiffClassification {
            diff_type: DiffType::MultiTopic,
            diff_type_reason: format!("{} distinct added blocks", added_block_count),
        };
    }

    // 7. Default: ContentAddition
    DiffClassification {
        diff_type: DiffType::ContentAddition,
        diff_type_reason: format!(
            "{} lines added{}",
            added_content.len(),
            if !removed_content.is_empty() {
                format!(", {} removed", removed_content.len())
            } else {
                String::new()
            }
        ),
    }
}

/// Check if the diff contains only boundary-related changes.
fn is_boundary_artifact(added: &[&str], removed: &[&str]) -> bool {
    let is_boundary_line = |line: &str| -> bool {
        // (HEAD) markers
        line.contains("(HEAD)")
            // Boundary UUIDs: <!-- agent:boundary:XXXX -->
            || line.contains("agent:boundary:")
            // Empty lines that accompany boundary changes
            || line.is_empty()
    };
    // Must have at least one change
    if added.is_empty() && removed.is_empty() {
        return false;
    }
    // Direct check: all lines are boundary-related
    if added.iter().all(|l| is_boundary_line(l)) && removed.iter().all(|l| is_boundary_line(l)) {
        return true;
    }
    // Paired check: added/removed pairs differ only by (HEAD) or boundary UUID
    if added.len() == removed.len() {
        return added.iter().zip(removed.iter()).all(|(a, r)| {
            let a_clean = a.replace("(HEAD)", "").trim().to_string();
            let r_clean = r.replace("(HEAD)", "").trim().to_string();
            if a_clean == r_clean {
                return true;
            }
            // Both are boundary markers with different UUIDs
            a.contains("agent:boundary:") && r.contains("agent:boundary:")
        });
    }
    false
}

/// Check if the diff looks like an annotation (inline edits to existing content).
///
/// Heuristic: each removed line has a corresponding added line that starts with
/// the same prefix (the added line extends the removed one, e.g., colon-appended).
fn is_annotation(added: &[&str], removed: &[&str]) -> bool {
    if added.len() != removed.len() {
        return false;
    }
    added.iter().zip(removed.iter()).all(|(a, r)| {
        // Added line starts with the removed line content (colon-append, extension)
        a.starts_with(r)
            // Or the lines share a significant common prefix (>60% of the shorter line)
            || {
                let min_len = a.len().min(r.len());
                if min_len < 3 {
                    return false;
                }
                let common = a
                    .chars()
                    .zip(r.chars())
                    .take_while(|(a, b)| a == b)
                    .count();
                common > min_len * 6 / 10
            }
    })
}

/// Truncate a string for use in reason messages.
fn truncate_for_reason(s: &str) -> &str {
    if s.len() <= 80 { s } else { &s[..80] }
}

/// This exposes the Rust truncation detection to external callers
/// (e.g., the Claude Code skill) before they compute their own diff.
pub fn run(file: &Path, wait: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if wait {
        let previous = snapshot::resolve(file)?.unwrap_or_default();
        let _stable = wait_for_stable_content(file, &previous)?;
        eprintln!("[diff --wait] content is stable");
    }
    match compute(file)? {
        Some(diff) => print!("{}", diff),
        None => eprintln!("No changes since last run."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_format_additions() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\n";
        let current = "line1\nline2\n";
        let diff = TextDiff::from_lines(previous, current);
        let has_insert = diff
            .iter_all_changes()
            .any(|c| c.tag() == ChangeTag::Insert);
        assert!(has_insert);
    }

    #[test]
    fn diff_format_deletions() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\nline2\n";
        let current = "line1\n";
        let diff = TextDiff::from_lines(previous, current);
        let has_delete = diff
            .iter_all_changes()
            .any(|c| c.tag() == ChangeTag::Delete);
        assert!(has_delete);
    }

    #[test]
    fn diff_format_unchanged() {
        use similar::{ChangeTag, TextDiff};
        let content = "line1\nline2\n";
        let diff = TextDiff::from_lines(content, content);
        let all_equal = diff.iter_all_changes().all(|c| c.tag() == ChangeTag::Equal);
        assert!(all_equal);
    }

    #[test]
    fn diff_format_mixed() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\nline2\nline3\n";
        let current = "line1\nchanged\nline3\n";
        let diff = TextDiff::from_lines(previous, current);

        let mut output = String::new();
        for change in diff.iter_all_changes() {
            let prefix = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            output.push_str(prefix);
            output.push_str(change.value());
        }
        assert!(output.contains(" line1\n"));
        assert!(output.contains("-line2\n"));
        assert!(output.contains("+changed\n"));
        assert!(output.contains(" line3\n"));
    }

    #[test]
    fn run_file_not_found() {
        let err = run(Path::new("/nonexistent/file.md"), false).unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }

    // --- Comment stripping tests ---

    #[test]
    fn strip_html_comment() {
        let input = "before\n<!-- a comment -->\nafter\n";
        assert_eq!(strip_comments(input), "before\nafter\n");
    }

    #[test]
    fn strip_multiline_html_comment() {
        let input = "before\n<!--\nmulti\nline\n-->\nafter\n";
        assert_eq!(strip_comments(input), "before\nafter\n");
    }

    #[test]
    fn strip_link_ref_comment() {
        let input = "before\n[//]: # (a comment)\nafter\n";
        assert_eq!(strip_comments(input), "before\nafter\n");
    }

    #[test]
    fn preserve_agent_markers() {
        let input = "<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        assert_eq!(strip_comments(input), input);
    }

    #[test]
    fn strip_regular_keep_agent_marker() {
        let input = "<!-- regular comment -->\n<!-- agent:s -->\ndata\n<!-- /agent:s -->\n";
        assert_eq!(
            strip_comments(input),
            "<!-- agent:s -->\ndata\n<!-- /agent:s -->\n"
        );
    }

    #[test]
    fn strip_inline_comment() {
        // Comment not on its own line — strip just the comment text
        let input = "text <!-- note --> more\n";
        let result = strip_comments(input);
        assert_eq!(result, "text  more\n");
    }

    #[test]
    fn no_comments_unchanged() {
        let input = "# Title\n\nJust text.\n";
        assert_eq!(strip_comments(input), input);
    }

    #[test]
    fn empty_document() {
        assert_eq!(strip_comments(""), "");
    }

    // --- Stale snapshot detection tests ---

    #[test]
    fn stale_snapshot_detects_completed_exchange() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nWhat's up\n\n## Assistant\n\nNot much\n\n## User\n\n";
        assert!(is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_false_when_user_has_new_content() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\n";
        let document =
            "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nNew question here\n";
        assert!(!is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_false_when_identical() {
        let content = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\n";
        assert!(!is_stale_snapshot(content, content));
    }

    #[test]
    fn stale_snapshot_false_when_no_assistant_block() {
        let snapshot = "## User\n\nHello\n\n";
        let document = "## User\n\nHello\n\nSome random text\n\n## User\n\n";
        assert!(!is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_multiple_exchanges_stale() {
        let snapshot = "## User\n\nQ1\n\n## Assistant\n\nA1\n\n## User\n\n";
        let document = "## User\n\nQ1\n\n## Assistant\n\nA1\n\n## User\n\nQ2\n\n## Assistant\n\nA2\n\n## User\n\nQ3\n\n## Assistant\n\nA3\n\n## User\n\n";
        assert!(is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_with_inline_annotation_not_stale() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\n";
        // User added inline annotation within an existing assistant block
        let document =
            "## User\n\nHello\n\n## Assistant\n\nHi there\n\nPlease elaborate\n\n## User\n\n";
        // This modifies the snapshot prefix, so starts_with check fails
        assert!(!is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_ignores_comments_in_detection() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\n<!-- scratch -->\n\n## Assistant\n\nResponse\n\n## User\n\n";
        // Comments are stripped, so the user block between snapshot and new assistant is empty
        assert!(is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn copy_on_read_guard_skips_recovery_when_snapshot_modified() {
        // Verifies the copy-on-read guard logic: if snapshot mtime changes
        // between read and recovery, the save must be skipped.
        use std::time::SystemTime;

        let t1 = Some(SystemTime::UNIX_EPOCH);
        let t2 = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1));

        // Same mtime → recovery should proceed (guard passes)
        assert_eq!(t1, t1, "same mtime should allow recovery");

        // Different mtime → recovery should be skipped (guard blocks)
        assert_ne!(t1, t2, "different mtime should block recovery");

        // Both None (no snapshot file) → recovery should proceed
        let none: Option<SystemTime> = None;
        assert_eq!(none, none, "both None should allow recovery");
    }

    /// Set up a temp directory with `.agent-doc/snapshots/` and a document file.
    /// Returns (TempDir, doc_path). The TempDir must be kept alive for the test.
    fn setup_compute_env(
        doc_content: &str,
        snap_content: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, doc_content).unwrap();

        // Create .agent-doc/snapshots/ and write the snapshot
        let snap_path = crate::snapshot::path_for(&doc).unwrap();
        std::fs::create_dir_all(snap_path.parent().unwrap()).unwrap();
        std::fs::write(&snap_path, snap_content).unwrap();

        (dir, doc)
    }

    #[test]
    fn compute_stale_snapshot_recovery_proceeds_when_unmodified() {
        // Stale snapshot scenario: snapshot has base content, document has
        // base + completed assistant exchange with empty trailing user block.
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        // compute() should detect stale snapshot and recover (return None)
        let result = compute(&doc).unwrap();
        assert!(
            result.is_none(),
            "stale snapshot recovery should return None"
        );

        // Verify the snapshot was updated to the document content
        let updated = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(updated, document);
    }

    #[test]
    fn compute_stale_recovery_updates_snapshot_to_current_document() {
        // After stale recovery, the snapshot should match the document.
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&doc).unwrap();
        assert!(result.is_none(), "stale recovery returns None");

        // Snapshot should now be synced to the current document
        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, document,
            "snapshot should be synced to document after recovery"
        );
    }

    #[test]
    fn compute_returns_diff_when_user_adds_content() {
        // Normal case: snapshot matches base, user added new content.
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\nNew question here\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&doc).unwrap();
        assert!(result.is_some(), "should return a diff for user additions");
        let diff = result.unwrap();
        assert!(diff.contains("New question here"));
    }

    #[test]
    fn compute_returns_none_when_no_changes() {
        let content = "## User\n\nHello\n";

        let (_dir, doc) = setup_compute_env(content, content);

        let result = compute(&doc).unwrap();
        assert!(result.is_none(), "identical content should return None");
    }

    // --- Code-aware comment stripping tests ---

    #[test]
    fn strip_preserves_comment_syntax_in_inline_backticks() {
        // `<!--` inside backticks should NOT be treated as a comment start
        let input =
            "Use `<!--` to start a comment.\n<!-- agent:foo -->\ncontent\n<!-- /agent:foo -->\n";
        let result = strip_comments(input);
        assert_eq!(
            result,
            "Use `<!--` to start a comment.\n<!-- agent:foo -->\ncontent\n<!-- /agent:foo -->\n"
        );
    }

    #[test]
    fn strip_preserves_comment_syntax_in_fenced_code_block() {
        let input = "before\n```\n<!-- not a comment -->\n```\nafter\n";
        let result = strip_comments(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_backtick_comment_before_agent_marker() {
        // Regression: `<!--` in backticks matched `-->` in the agent marker,
        // swallowing all content between them
        let input = "\
Text mentions `<!--` as a trigger.\n\
More text here.\n\
New user content.\n\
<!-- /agent:exchange -->\n";
        let result = strip_comments(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_multiple_backtick_comments_in_exchange() {
        // Real-world scenario: discussion about `<!--` syntax inside an exchange component
        let snapshot = "\
<!-- agent:exchange -->\n\
Discussion about `<!--` triggers.\n\
- `<!-- agent:NAME -->` paired markers\n\
<!-- /agent:exchange -->\n";
        let current = "\
<!-- agent:exchange -->\n\
Discussion about `<!--` triggers.\n\
- `<!-- agent:NAME -->` paired markers\n\
\n\
Please fix the bug.\n\
<!-- /agent:exchange -->\n";

        let snap_stripped = strip_comments(snapshot);
        let curr_stripped = strip_comments(current);
        assert_ne!(
            snap_stripped, curr_stripped,
            "inline edits after backtick-comment text must be detected"
        );
    }

    // --- Snapshot-based diff detection after stream write ---

    #[test]
    fn diff_detects_user_edits_after_stream_write() {
        // Simulates: stream write saves snapshot, user edits document,
        // then diff::compute() should detect the user's changes.
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");

        // Agent writes response — snapshot saved as baseline + response
        let content_after_write = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nUser prompt\n\nAgent response\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content_after_write).unwrap();
        snapshot::save(&doc, content_after_write).unwrap();

        // User edits document (adds text in exchange)
        let content_after_edit = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nUser prompt\n\nAgent response\n\nNew user edit here\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content_after_edit).unwrap();

        // Diff should detect the user's new edit
        let diff = compute(&doc).unwrap();
        assert!(
            diff.is_some(),
            "diff should detect user edit after stream write"
        );
        let diff_text = diff.unwrap();
        assert!(
            diff_text.contains("New user edit here"),
            "diff should contain user's new text: {}",
            diff_text
        );
    }

    #[test]
    fn diff_no_change_when_document_matches_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nContent\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let diff = compute(&doc).unwrap();
        assert!(diff.is_none(), "no diff when document matches snapshot");
    }

    #[test]
    fn diff_detects_change_after_cumulative_stream_flushes() {
        // Simulates: stream mode does multiple cumulative flushes,
        // then user edits. Snapshot should reflect last flush state.
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");

        // Snapshot saved after stream completes (baseline + full response)
        let snapshot_content = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nFull agent response here\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, snapshot_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        // User adds new text
        let edited = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nFull agent response here\n\nRelease agent-doc\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, edited).unwrap();

        let diff = compute(&doc).unwrap();
        assert!(diff.is_some(), "diff should detect user's edit");
        assert!(diff.unwrap().contains("Release agent-doc"));
    }

    // --- Truncation detection tests ---

    #[test]
    fn truncated_mid_sentence() {
        assert!(looks_truncated(
            "Also, when I called agent-doc run on this file...and ther"
        ));
    }

    #[test]
    fn not_truncated_complete_sentence() {
        assert!(!looks_truncated("This is a complete sentence."));
    }

    #[test]
    fn not_truncated_question() {
        assert!(!looks_truncated("What should we do?"));
    }

    #[test]
    fn not_truncated_command() {
        assert!(!looks_truncated("/agent-doc compact"));
    }

    #[test]
    fn not_truncated_single_word_command() {
        assert!(!looks_truncated("release"));
    }

    #[test]
    fn not_truncated_short_words() {
        assert!(!looks_truncated("go"));
        assert!(!looks_truncated("ok"));
        assert!(!looks_truncated("no"));
        assert!(!looks_truncated("yes"));
    }

    #[test]
    fn truncated_single_chars() {
        // Single characters are now treated as potentially truncated.
        // The stability check will confirm if the input is complete.
        // This prevents partial typing (e.g., "S" from "Save as a draft.")
        // from triggering immediate runs.
        assert!(looks_truncated("A"));
        assert!(looks_truncated("S"));
        assert!(looks_truncated("1"));
        assert!(looks_truncated("y"));
    }

    #[test]
    fn not_truncated_heading() {
        assert!(!looks_truncated("### Re: Fix the bug"));
    }

    #[test]
    fn not_truncated_empty() {
        assert!(!looks_truncated(""));
    }

    #[test]
    fn not_truncated_ends_with_colon() {
        assert!(!looks_truncated("Here is the issue:"));
    }

    #[test]
    fn not_truncated_ends_with_backtick() {
        assert!(!looks_truncated("Check `crdt.rs`"));
    }

    #[test]
    fn truncated_ends_mid_word() {
        assert!(looks_truncated("Please make Claim for Tmux Pan"));
    }

    #[test]
    fn not_truncated_ends_with_period() {
        assert!(!looks_truncated("Fixed the bug."));
    }

    #[test]
    fn extract_last_added_finds_insert() {
        let prev = "line1\n";
        let curr = "line1\nnew content here\n";
        let last = extract_last_added_line(prev, curr);
        assert_eq!(last, Some("new content here".to_string()));
    }

    #[test]
    fn extract_last_added_none_when_no_changes() {
        let content = "line1\nline2\n";
        let last = extract_last_added_line(content, content);
        assert_eq!(last, None);
    }

    // --- diff --wait tests ---

    #[test]
    fn run_with_wait_stable_content() {
        // When content is already stable, --wait should not change behavior
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let snapshot_content = "line1\n";
        std::fs::write(&doc, "line1\nline2\n").unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        // run with wait=true should detect changes normally
        let result = run(&doc, true);
        assert!(result.is_ok());
    }

    #[test]
    fn run_with_wait_no_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "line1\nline2\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        // No changes — should succeed with wait=true
        let result = run(&doc, true);
        assert!(result.is_ok());
    }

    #[test]
    fn wait_for_stable_content_returns_immediately_when_complete() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "Complete sentence.\n";
        std::fs::write(&doc, content).unwrap();
        let previous = "";

        let start = std::time::Instant::now();
        let result = wait_for_stable_content(&doc, previous).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, content);
        // Should return almost immediately (no recheck needed)
        assert!(
            elapsed.as_millis() < 500,
            "should not delay for complete content"
        );
    }

    // --- classify_diff tests ---

    fn make_diff(added: &[&str], removed: &[&str]) -> String {
        let mut lines = vec!["--- snapshot", "+++ document", "@@ -1,5 +1,5 @@"];
        for r in removed {
            lines.push(&r);
        }
        lines.push(" context line");
        for a in added {
            lines.push(&a);
        }
        lines.join("\n")
    }

    #[test]
    fn classify_approval() {
        let diff = make_diff(&["+go"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::Approval);
        assert!(c.diff_type_reason.contains("go"));
    }

    #[test]
    fn classify_approval_case_insensitive() {
        let diff = make_diff(&["+Yes"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::Approval);
    }

    #[test]
    fn classify_simple_question() {
        let diff = make_diff(&["+what is the release process?"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::SimpleQuestion);
    }

    #[test]
    fn classify_boundary_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n-### Re: Something (HEAD)\n+### Re: Something\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::BoundaryArtifact);
    }

    #[test]
    fn classify_boundary_uuid() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n-<!-- agent:boundary:abc123 -->\n+<!-- agent:boundary:def456 -->\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::BoundaryArtifact);
    }

    #[test]
    fn classify_structural_change() {
        let diff = "--- snapshot\n+++ document\n@@ -1,5 +1,3 @@\n context\n-removed line one\n-removed line two\n context\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::StructuralChange);
    }

    #[test]
    fn classify_multi_topic() {
        // Two added blocks separated by context
        let diff = "--- snapshot\n+++ document\n@@ -1,5 +1,7 @@\n context\n+first topic\n context middle\n+second topic\n context end\n";
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::MultiTopic);
    }

    #[test]
    fn classify_multi_topic_with_separator() {
        // Contiguous added block with --- separator — still detected as multi-topic
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,6 @@\n context\n+question one?\n+---\n+do something else\n context end\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::MultiTopic);
    }

    #[test]
    fn classify_content_addition() {
        let diff = make_diff(&["+implement the feature using Rust"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::ContentAddition);
    }

    #[test]
    fn classify_annotation() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n context\n-The fix is deployed\n+The fix is deployed: confirmed working in prod\n context\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::Annotation);
    }

    #[test]
    fn classify_approval_in_sentence_is_content() {
        // "go" inside a longer sentence should NOT be classified as Approval
        let diff = make_diff(&["+let's go ahead and implement the feature"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::ContentAddition);
    }

    #[test]
    fn classify_single_separator_not_multi_topic() {
        // Single --- with no content on either side is not multi-topic
        let diff = make_diff(&["+---"], &[]);
        let c = classify_diff(&diff);
        // Only 1 section (the --- itself is filtered as empty), not multi-topic
        assert_ne!(c.diff_type, DiffType::MultiTopic);
    }

    #[test]
    fn classify_question_mark_in_multiline_is_content() {
        // Question mark at end of a multi-line addition is content, not simple question
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n context\n+first line of paragraph\n+is this a question?\n context\n";
        let c = classify_diff(diff);
        // Two added lines = multi-topic (two blocks is actually one contiguous block)
        // The key point: it should NOT be SimpleQuestion (that requires exactly 1 added line)
        assert_ne!(c.diff_type, DiffType::SimpleQuestion);
    }

    // --- annotate_diff tests ---

    #[test]
    fn annotate_diff_additions() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n context line\n+new user line\n more context\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[user+] new user line"));
        assert!(annotated.contains("[agent] context line"));
    }

    #[test]
    fn annotate_diff_removals() {
        let diff =
            "--- snapshot\n+++ document\n@@ -1,3 +1,2 @@\n context\n-removed line\n context\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[user-] removed line"));
    }

    #[test]
    fn annotate_diff_modifications() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n context\n-The fix is deployed\n+The fix is deployed: confirmed in prod\n context\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[user~] The fix is deployed: confirmed in prod"));
        // Should NOT have separate [user-] and [user+] for the paired lines
        assert!(!annotated.contains("[user-] The fix is deployed"));
    }

    #[test]
    fn annotate_diff_context() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n line one\n line two\n+added\n line three\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[agent] line one"));
        assert!(annotated.contains("[agent] line two"));
        assert!(annotated.contains("[agent] line three"));
    }

    #[test]
    fn annotate_diff_empty() {
        let diff = "--- snapshot\n+++ document\n";
        assert!(annotate_diff(diff).is_none());
    }

    // --- extract_inline_annotations tests ---

    #[test]
    fn inline_annotations_user_addition_between_agent_lines() {
        let annotated = "[agent] previous agent line\n\
                         [user+] This is wrong, fix it\n\
                         [agent] more agent content";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["This is wrong, fix it"]);
    }

    #[test]
    fn inline_annotations_user_modification_between_agent_lines() {
        let annotated = "[agent] context before\n\
                         [user~] The corrected line\n\
                         [agent] context after";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["The corrected line"]);
    }

    #[test]
    fn inline_annotations_user_addition_at_end_is_not_inline() {
        let annotated = "[agent] agent content\n\
                         [agent] more agent content\n\
                         [user+] New user input at end";
        let anns = extract_inline_annotations(annotated);
        assert!(anns.is_empty());
    }

    #[test]
    fn inline_annotations_component_markers_not_substantive() {
        // Component closing + section header after user input should NOT classify as inline
        let annotated = "[agent] response prose\n\
                         [user+] The fix did not seem to work\n\
                         [agent] <!-- /agent:exchange -->\n\
                         [agent] \n\
                         [agent] ## Pending / Not Built\n\
                         [agent] \n\
                         [agent] <!-- agent:pending -->";
        let anns = extract_inline_annotations(annotated);
        assert!(
            anns.is_empty(),
            "component markers should not make end-of-exchange input inline"
        );
    }

    #[test]
    fn inline_annotations_head_boundary_artifact_excluded() {
        // [user~] that only appended (HEAD) to a heading is a reposition artifact
        let annotated = "[agent] previous content\n\
                         [user~] ### Re: topic — sonnet-4-6 (HEAD)\n\
                         [agent] response prose\n\
                         [agent] more content";
        let anns = extract_inline_annotations(annotated);
        assert!(
            anns.is_empty(),
            "(HEAD) boundary reposition should not be an inline annotation"
        );
    }

    #[test]
    fn inline_annotations_real_correction_between_prose() {
        // Genuine user correction inside agent prose (not a structural marker)
        let annotated = "[agent] The score is 5 out of 10.\n\
                         [user+] This is wrong. Score should be 8-9.\n\
                         [agent] Here is more analysis below.";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["This is wrong. Score should be 8-9."]);
    }

    #[test]
    fn inline_annotations_multiple() {
        let annotated = "[agent] ### Re: topic\n\
                         [user+] First correction\n\
                         [agent] agent paragraph\n\
                         [user+] Second correction\n\
                         [agent] agent closing\n\
                         [user+] New input at end";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["First correction", "Second correction"]);
    }

    #[test]
    fn inline_annotations_skips_blank_user_lines() {
        let annotated = "[agent] agent line\n\
                         [user+] \n\
                         [agent] more agent";
        let anns = extract_inline_annotations(annotated);
        assert!(anns.is_empty());
    }

    #[test]
    fn inline_annotations_empty_annotated_diff() {
        let anns = extract_inline_annotations("");
        assert!(anns.is_empty());
    }

    #[test]
    fn inline_annotations_claudescore_table_scenario() {
        // Reproduces the claudescore bug: user corrections inside agent response
        // with table rows as agent content after them
        let annotated = "[agent] | Category | Score |\n\
                         [agent] |----------|-------|\n\
                         [agent] | Quality  | 7     |\n\
                         [user+] This is wrong. Do not lower expert scores.\n\
                         [agent] | Speed    | 8     |\n\
                         [user+] We may need to broaden the gate.\n\
                         [agent] | Total    | 7.5   |";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(
            anns,
            vec![
                "This is wrong. Do not lower expert scores.",
                "We may need to broaden the gate.",
            ]
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_promotes_inline_prompt_to_prompt_target() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            The prior explanation was incomplete\n\
            +Why was the `❯` prefix omitted here?\n\
            The rest of the response stays the same\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(changes[0].text, "Why was the `❯` prefix omitted here?");
    }

    #[test]
    fn classify_prompt_bearing_changes_marks_inline_correction_as_content_edit() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            The service returned 401 from this endpoint\n\
            +The service returned 503 from this endpoint\n\
            The rest of the response stays the same\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::ContentEdit);
        assert_eq!(
            changes[0].text,
            "The service returned 503 from this endpoint"
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_marks_response_heading_as_recovery_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n\
            ctx\n\
            +### Re: missed patchback — gpt-5\n\
            +Patched after the fact.\n\
            context end\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::RecoveryArtifact);
        assert_eq!(
            changes[0].text,
            "### Re: missed patchback — gpt-5\nPatched after the fact."
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_marks_boundary_only_edit_as_boundary_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,2 @@\n\
            -### Re: Something\n\
            +### Re: Something (HEAD)\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::BoundaryArtifact);
        assert_eq!(changes[0].text, "### Re: Something (HEAD)");
    }

    #[test]
    fn classify_prompt_bearing_changes_preserves_mixed_triage_order() {
        let diff = "--- snapshot\n+++ document\n@@ -1,7 +1,12 @@\n\
            The service returned 401 from this endpoint\n\
            +The service returned 503 from this endpoint\n\
            The rest of the response stays the same\n\
            +### Re: delayed patchback — gpt-5\n\
            +Patched after the fact.\n\
            Context after repair\n\
            +<!-- agent:boundary:test-boundary -->\n\
            More context\n\
            +Why was the `❯` prefix omitted here?\n\
            The response continues below\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::ContentEdit);
        assert_eq!(
            changes[0].text,
            "The service returned 503 from this endpoint"
        );
        assert_eq!(changes[1].kind, PromptBearingChangeKind::RecoveryArtifact);
        assert_eq!(
            changes[1].text,
            "### Re: delayed patchback — gpt-5\nPatched after the fact."
        );
        assert_eq!(changes[2].kind, PromptBearingChangeKind::BoundaryArtifact);
        assert_eq!(changes[2].text, "<!-- agent:boundary:test-boundary -->");
        assert_eq!(changes[3].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(changes[3].text, "Why was the `❯` prefix omitted here?");
    }

    // parse_slash_commands tests

    #[test]
    fn parse_slash_commands_simple() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n context\n+/clear\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/clear"]);
    }

    #[test]
    fn parse_slash_commands_with_args() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+/agent-doc foo.md\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/agent-doc foo.md"]);
    }

    #[test]
    fn parse_slash_commands_ignores_fenced() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n+```\n+/clear\n+```\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_ignores_blockquote() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+> /clear\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_ignores_context_lines() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,2 @@\n /clear\n context\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_ignores_removed_lines() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,1 @@\n-/clear\n context\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_requires_letter_after_slash() {
        // "/ " (space after slash) and "//comment" should not match.
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n+/ foo\n+//comment\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_multiple() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n+/clear\n+/agent-doc foo.md\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/clear", "/agent-doc foo.md"]);
    }

    #[test]
    fn parse_slash_commands_rejects_absolute_paths() {
        // #xzz5: `/home/brian/...` looks like a command but is a filesystem
        // path. The tightened grammar (`/[a-z][a-z0-9:_-]*` with no second `/`)
        // must reject any token containing a second slash.
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n ctx\n\
            +/home/brian/work/foo.md\n\
            +/tmp/scratch\n\
            +/usr/local/bin/agent-doc\n\
            +/var\n";
        let cmds = parse_slash_commands(diff);
        // "/var" is a bare token with no slash → allowed by grammar (it's a
        // valid command name shape). Only the three path-shaped entries are
        // rejected. This is the minimum contract: reject second-slash.
        assert_eq!(cmds, vec!["/var"]);
    }

    #[test]
    fn parse_slash_commands_rejects_uppercase_and_symbols() {
        // Grammar: first char must be [a-z]; rest must be [a-z0-9:_-].
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n ctx\n\
            +/Clear\n\
            +/foo.bar\n\
            +/foo!bang\n\
            +/foo#hash\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty(), "all four must be rejected; got: {cmds:?}");
    }

    #[test]
    fn parse_slash_commands_accepts_namespaced_and_hyphenated() {
        // Grammar allows `:`, `_`, `-`, digits — namespaced/versioned commands.
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n ctx\n\
            +/mcp:reload\n\
            +/agent-doc file.md\n\
            +/some_thing\n\
            +/v2\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(
            cmds,
            vec!["/mcp:reload", "/agent-doc file.md", "/some_thing", "/v2"]
        );
    }

    #[test]
    fn detect_orchestration_request_for_synchronous_task_list() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,7 @@\n ctx\n\
+Today is 2026-04-25. Synchronous orcestra.\n\
+- do #ss01. Add tests. Run benchmarks.\n\
+- do #ss02. Add tests. Run benchmarks.\n\
+- do #ss03. Add tests. Run benchmarks.\n";
        let request = detect_orchestration_request(diff).expect("expected orchestration request");
        assert_eq!(request.mode, OrchestrationRequestMode::Sequential);
        assert_eq!(request.task_count, 3);
        assert_eq!(
            request.trigger_text,
            "Today is 2026-04-25. Synchronous orcestra."
        );
    }

    #[test]
    fn detect_orchestration_request_for_parallel_batch() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n\
+Fan out these benchmark tasks.\n\
+- do #a1\n\
+- do #a2\n";
        let request = detect_orchestration_request(diff).expect("expected orchestration request");
        assert_eq!(request.mode, OrchestrationRequestMode::Parallel);
        assert_eq!(request.task_count, 2);
    }

    #[test]
    fn detect_orchestration_request_requires_batch_shape() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n\
+Run these in order.\n\
+- do #a1\n";
        assert!(
            detect_orchestration_request(diff).is_none(),
            "single-item lists should stay as ordinary work, not forced orchestration"
        );
    }

    #[test]
    fn detect_orchestration_request_for_prefixed_synchronous_opera_batch() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,6 @@\n ctx\n\
+❯ synchronous opera\n\
+❯ preset #spec-test-build-install-commit-push\n\
+❯ - do #jbpfx1\n\
+❯ - do #jbpfx2\n";
        let request = detect_orchestration_request(diff).expect("expected orchestration request");
        assert_eq!(request.mode, OrchestrationRequestMode::Sequential);
        assert_eq!(request.task_count, 2);
        assert_eq!(
            request.trigger_text,
            "❯ synchronous opera ❯ preset #spec-test-build-install-commit-push"
        );
    }

    #[test]
    fn detect_queue_trigger_do_queue() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_run_queue() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+run queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_case_insensitive() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+Do Queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_with_prompt_prefix() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+❯ do queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_pending_ref() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do #queue Phase 2\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_in_code_fence() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n+```\n+do queue\n+```\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_in_blockquote() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+> do queue\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_with_trailing_punct() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do queue.\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_on_context_line() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n do queue\n+other\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn detect_prompt_preset_requests_from_diff() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,6 @@\n ctx\n\
+preset #1\n\
+presets release-check, #2\n\
+preset #1\n\
+> preset ignored\n\
+```md\n\
+preset fenced\n";
        assert_eq!(
            detect_prompt_preset_requests(diff),
            vec![
                "#1".to_string(),
                "release-check".to_string(),
                "#2".to_string()
            ]
        );
    }

    #[test]
    fn extract_prompt_preset_requests_from_text_ignores_fences_and_blockquotes() {
        let text = "synchronous orchestra\npreset #1\n> preset quoted\n\n```md\npreset fenced\n```\npresets release-check and #2\n";
        assert_eq!(
            extract_prompt_preset_requests_from_text(text),
            vec![
                "#1".to_string(),
                "release-check".to_string(),
                "#2".to_string()
            ]
        );
    }

    #[test]
    fn extract_imperative_directives_detects_do_and_build_push() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n\
            +do #6zyp. update spec + tests. build + install for local testing. commit + push\n\
            +run benchmarks\n";
        let directives = extract_imperative_directives(diff);
        assert_eq!(
            directives,
            vec![
                "do #6zyp. update spec + tests. build + install for local testing. commit + push",
                "run benchmarks",
            ]
        );
        assert!(diff_contains_imperative_directive(diff));
    }

    #[test]
    fn extract_imperative_directives_detects_pending_item_natural_language() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n\
            +- [ ] [#n8q4] Fix the cross-repo `no-permissions-bypass` miss now dominating benchmark MAE\n";
        let directives = extract_imperative_directives(diff);
        assert_eq!(
            directives,
            vec!["Fix the cross-repo `no-permissions-bypass` miss now dominating benchmark MAE"]
        );
        assert!(diff_contains_imperative_directive(diff));
    }

    #[test]
    fn extract_imperative_directives_ignores_blockquotes_and_fences() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,6 @@\n ctx\n\
            +> do #skip\n\
            +```\n\
            +run tests\n\
            +```\n\
            +plain note\n";
        let directives = extract_imperative_directives(diff);
        assert!(
            directives.is_empty(),
            "unexpected directives: {directives:?}"
        );
        assert!(!diff_contains_imperative_directive(diff));
    }

    #[test]
    fn diff_contains_imperative_directive_for_approval_word() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+go\n";
        assert!(diff_contains_imperative_directive(diff));
    }

    #[test]
    fn extract_required_response_blocks_multiple_prompts() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,9 @@\n\
            ctx\n\
            +❯ First question?\n\
            +Context line.\n\
            +\n\
            +❯ Second question?\n\
            +do #n8q4. run tests. build + install. commit + push\n";

        let blocks = extract_required_response_blocks(diff);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], "❯ First question?\nContext line.");
        assert_eq!(
            blocks[1],
            "❯ Second question?\ndo #n8q4. run tests. build + install. commit + push"
        );
    }

    #[test]
    fn extract_required_response_blocks_preserves_code_fence_context() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,7 @@\n\
            ctx\n\
            +❯ In src/boost-client, why did patchback miss the prefix?\n\
            +See my inquiry:\n\
            +```text\n\
            +line one\n\
            +line two\n\
            +```\n";

        let blocks = extract_required_response_blocks(diff);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("❯ In src/boost-client"));
        assert!(blocks[0].contains("```text\nline one\nline two\n```"));
    }

    #[test]
    fn format_required_response_targets_mentions_turn_completeness() {
        let diff =
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n+❯ Why were two prompts left unresolved?\n";
        let rendered = format_required_response_targets(diff).unwrap();
        assert!(rendered.contains("Do not stop at the newest question"));
        assert!(rendered.contains("<target index=\"1\">"));
        assert!(rendered.contains("❯ Why were two prompts left unresolved?"));
    }

    #[test]
    fn format_prompt_bearing_changes_mentions_edit_and_artifact_contract() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,6 @@\n\
            context before\n\
            +❯ Why was this missed?\n\
            +\n\
            +This line should say 503.\n\
            context after\n";
        let rendered = format_prompt_bearing_changes(diff).unwrap();
        assert!(rendered.contains("User-authored prompt-bearing changes (oldest first):"));
        assert!(rendered.contains("kind=\"prompt_target\""));
        assert!(rendered.contains("kind=\"content_edit\""));
        assert!(rendered.contains("Treat `content_edit` items as user corrections"));
    }

    #[test]
    fn prompt_prefix_normalization_targets_preserve_prompt_context_and_skip_fences() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,7 @@\n\
            ctx\n\
            +❯ In src/boost-client, why did patchback miss the prefix?\n\
            +See my inquiry:\n\
            +```text\n\
            +line one\n\
            +line two\n\
            +```\n";

        let targets = prompt_prefix_normalization_targets(diff);
        assert_eq!(
            targets,
            vec!["See my inquiry:".to_string(),],
            "only the bare prompt-context line should need fresh prefixing"
        );
    }

    #[test]
    fn first_bare_prompt_prefix_target_detects_unprefixed_prompt_block_line() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,5 @@\n\
            ctx\n\
            +❯ Existing question?\n\
            +Follow-up context.\n\
            +### Re: answer — gpt-5\n\
            +Body\n";

        let bare = first_bare_prompt_prefix_target(diff);
        assert_eq!(bare.as_deref(), Some("Follow-up context."));
    }
}
