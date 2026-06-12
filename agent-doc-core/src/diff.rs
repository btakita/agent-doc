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
//! - `text_line_looks_like_prompt_target("❯ **Verification:** passed")` → `false`;
//!   known assistant response labels are recognized after optional prompt glyph,
//!   list marker, and markdown emphasis normalization.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

use crate::component;

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
    // #22a8 (Phase 5b write-side): also drop the managed `agent_doc_pipeline:`
    // frontmatter block so a pipeline-only mirror write (emitted on every
    // hot-path phase transition) reads as no change and never surfaces as a user
    // edit. Both sides of every diff pass through this, so a pipeline-only delta
    // cancels to `no_changes`. Shared with the write-side splice so the strip and
    // the write agree byte-for-byte on the block boundary.
    crate::frontmatter::strip_pipeline_block_lines(&component::strip_comments(content))
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
    classify_prompt_bearing_changes_from_annotated_internal(annotated_diff, false)
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

fn is_exchange_close_marker_line(content: &str) -> bool {
    content.trim() == "<!-- /agent:exchange -->"
}

fn parse_slash_command_candidate(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix('/')?;
    let token_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let token = &rest[..token_end];
    if token.is_empty() {
        return None;
    }
    let mut chars = token.chars();
    let first = chars.next()?;
    let command_like = first.is_ascii_lowercase()
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '_' | '-'));
    command_like.then(|| trimmed.to_string())
}

fn line_looks_like_slash_command(line: &str) -> bool {
    parse_slash_command_candidate(line).is_some()
}

fn line_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    let normalized_imperative = normalize_imperative_candidate(trimmed)
        .is_some_and(|normalized| looks_like_imperative_directive(&normalized));
    let slash_command = line_looks_like_slash_command(trimmed);
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && !line_has_known_response_label_after_prompt(trimmed)
        && (slash_command
            || trimmed.starts_with('❯')
            || trimmed.ends_with('?')
            || normalized_imperative)
}

pub fn text_line_looks_like_prompt_target(line: &str) -> bool {
    line_looks_like_prompt_target(line)
}

fn block_looks_like_prompt_target(block: &str) -> bool {
    block.lines().any(line_looks_like_prompt_target)
}

fn is_exchange_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

fn normalized_prompt_preview_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || !text_line_looks_like_prompt_target(trimmed) {
        return None;
    }
    Some(trimmed.trim_start_matches('❯').trim().to_string())
}

pub fn line_looks_like_fresh_prompt_after_response(trimmed: &str) -> bool {
    if line_looks_like_plain_response_after_prompt(trimmed) {
        return false;
    }

    let unprefixed = trimmed.trim_start_matches('❯').trim();
    let lower = unprefixed.to_ascii_lowercase();
    trimmed.starts_with('❯')
        || unprefixed.ends_with('?')
        || line_looks_like_slash_command(unprefixed)
        || lower == "go"
        || lower == "continue"
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("run ")
        || lower.starts_with("rerun ")
        || lower.starts_with("build ")
        || lower.starts_with("test ")
        || lower.starts_with("commit ")
        || lower.starts_with("push ")
        || lower.starts_with("verify ")
        || lower.starts_with("investigate ")
}

pub(crate) fn line_looks_like_soft_prompt_request(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("please ")
        || lower.contains(" please ")
        || lower.starts_with("can you ")
        || lower.starts_with("could you ")
        || lower.starts_with("would you ")
        || lower.starts_with("need you to ")
}

pub fn line_looks_like_prompt_prefix_repair_start(trimmed: &str, is_target: bool) -> bool {
    let unprefixed = trimmed
        .strip_prefix("❯ ")
        .or_else(|| trimmed.strip_prefix('❯'))
        .map(str::trim_start)
        .unwrap_or(trimmed);

    if unprefixed.is_empty() || line_looks_like_plain_response_after_prompt(unprefixed) {
        return false;
    }

    is_target
        || line_looks_like_fresh_prompt_after_response(unprefixed)
        || line_looks_like_soft_prompt_request(unprefixed)
}

pub fn line_looks_like_targeted_prompt_prefix_repair_start(trimmed: &str, is_target: bool) -> bool {
    if !is_target {
        return false;
    }

    let unprefixed = trimmed
        .strip_prefix("❯ ")
        .or_else(|| trimmed.strip_prefix('❯'))
        .map(str::trim_start)
        .unwrap_or(trimmed);

    if unprefixed.is_empty() || line_looks_like_plain_response_after_prompt(unprefixed) {
        return false;
    }

    if trimmed.starts_with('❯') || line_looks_like_soft_prompt_request(unprefixed) {
        return true;
    }

    let lower = unprefixed.to_ascii_lowercase();
    lower == "go"
        || lower == "continue"
        || line_looks_like_slash_command(unprefixed)
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("run ")
        || lower.starts_with("rerun ")
        || lower.starts_with("build ")
        || lower.starts_with("test ")
        || lower.starts_with("commit ")
        || lower.starts_with("push ")
        || lower.starts_with("verify ")
        || lower.starts_with("investigate ")
}

pub fn line_looks_like_plain_response_after_prompt(trimmed: &str) -> bool {
    if trimmed.is_empty() {
        return false;
    }

    if line_has_known_response_label_after_prompt(trimmed) {
        return true;
    }

    if normalized_prompt_preview_line(trimmed).is_some() {
        return false;
    }

    if trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("Plan:")
        || trimmed.starts_with("Verification")
        || trimmed.starts_with("What changed:")
        || trimmed.starts_with("Follow-up:")
        || trimmed.starts_with("Backlog:")
        || trimmed.starts_with("`#")
    {
        return true;
    }

    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("i updated ")
        || lower.starts_with("i fixed ")
        || lower.starts_with("i added ")
        || lower.starts_with("i implemented ")
        || lower.starts_with("i left ")
        || lower.starts_with("updated ")
        || lower.starts_with("fixed ")
        || lower.starts_with("added ")
        || lower.starts_with("implemented ")
}

fn line_has_known_response_label_after_prompt(line: &str) -> bool {
    let Some(normalized) = normalize_response_label_candidate(line) else {
        return false;
    };
    matches!(
        normalized.as_str(),
        s if s.starts_with("Plan:")
            || s.starts_with("Verification:")
            || s.starts_with("What changed:")
            || s.starts_with("Follow-up:")
            || s.starts_with("Commit / push:")
            || s.starts_with("Backlog:")
    )
}

fn normalize_response_label_candidate(line: &str) -> Option<String> {
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

    let stripped = strip_markdown_emphasis_pair(trimmed);
    let normalized = stripped.trim_start();
    (!normalized.is_empty()).then(|| normalized.to_string())
}

fn strip_markdown_emphasis_pair(text: &str) -> String {
    for marker in ["***", "___", "**", "__", "*", "_"] {
        if let Some(rest) = text.strip_prefix(marker)
            && let Some(end) = rest.find(marker)
        {
            let label = &rest[..end];
            let tail = &rest[end + marker.len()..];
            if !label.trim().is_empty()
                && (label.trim_end().ends_with(':') || tail.trim_start().starts_with(':'))
            {
                return format!("{}{}", label, tail);
            }
        }
    }
    text.to_string()
}

pub fn prompt_change_is_already_answered(change_text: &str) -> bool {
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

    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0usize;
    let mut saw_prompt = false;
    let mut saw_response = false;

    for segment in change_text.split_inclusive('\n') {
        let line = segment.trim_end_matches('\n');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }

        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                continue;
            }
        } else {
            if fence_close(trimmed, fence_char, fence_len) {
                in_fence = false;
            }
            continue;
        }

        if is_exchange_response_heading(trimmed) {
            if saw_prompt {
                saw_response = true;
            }
            continue;
        }

        if normalized_prompt_preview_line(trimmed).is_some() {
            if saw_response && line_looks_like_fresh_prompt_after_response(trimmed) {
                return false;
            }
            saw_prompt = true;
            continue;
        }

        if !saw_prompt && line_looks_like_soft_prompt_request(trimmed) {
            saw_prompt = true;
            continue;
        }

        if saw_prompt && line_looks_like_plain_response_after_prompt(trimmed) {
            saw_response = true;
        }
    }

    saw_prompt && saw_response
}

pub fn prompt_change_is_answered_by_later_response(
    changes: &[PromptBearingChange],
    idx: usize,
) -> bool {
    if changes
        .get(idx)
        .is_none_or(|change| change.kind != PromptBearingChangeKind::PromptTarget)
    {
        return false;
    }

    for later in changes.iter().skip(idx + 1) {
        match later.kind {
            PromptBearingChangeKind::PromptTarget => return false,
            PromptBearingChangeKind::RecoveryArtifact
            | PromptBearingChangeKind::BoundaryArtifact => {
                let heading = later
                    .text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or(later.text.as_str())
                    .trim();
                if is_exchange_response_heading(heading) {
                    return true;
                }
            }
            PromptBearingChangeKind::ContentEdit => {}
        }
    }

    false
}

fn suppress_answered_prompt_runs(changes: Vec<PromptBearingChange>) -> Vec<PromptBearingChange> {
    let mut filtered = Vec::with_capacity(changes.len());
    let mut skip_answered_response_run = false;

    for (idx, change) in changes.iter().enumerate() {
        match change.kind {
            PromptBearingChangeKind::RecoveryArtifact
            | PromptBearingChangeKind::BoundaryArtifact => {
                filtered.push(change.clone());
            }
            PromptBearingChangeKind::PromptTarget => {
                if skip_answered_response_run {
                    let preview = change
                        .text
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or(change.text.as_str())
                        .trim();
                    if !line_looks_like_fresh_prompt_after_response(preview) {
                        continue;
                    }
                }
                if prompt_change_is_already_answered(&change.text)
                    || prompt_change_is_answered_by_later_response(&changes, idx)
                {
                    skip_answered_response_run = true;
                    continue;
                }
                filtered.push(change.clone());
            }
            PromptBearingChangeKind::ContentEdit => {
                if skip_answered_response_run {
                    continue;
                }
                filtered.push(change.clone());
            }
        }
    }

    filtered
}

fn classify_prompt_bearing_block(
    block_text: &str,
    has_substantive_agent_after: bool,
    closes_exchange_tail: bool,
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
    if closes_exchange_tail
        && non_blank
            .iter()
            .all(|line| line_looks_like_plain_response_after_prompt(line))
    {
        return None;
    }
    if closes_exchange_tail {
        return Some(PromptBearingChangeKind::PromptTarget);
    }
    if has_substantive_agent_after {
        return Some(PromptBearingChangeKind::ContentEdit);
    }
    None
}

/// Return the prompt-bearing exchange lines that must carry a `❯ ` prefix.
///
/// This derives from the canonical `prompt_target` classifier rather than a
/// separate line-shape heuristic, so write-path normalization and session-check
/// can enforce the same invariant.
pub fn prompt_prefix_normalization_targets(diff: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut lines = Vec::new();
    for change in classify_prompt_bearing_changes_raw(diff) {
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
    for change in classify_prompt_bearing_changes_raw(diff) {
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
    classify_prompt_bearing_changes_from_annotated_internal(annotated_diff, true)
}

fn classify_prompt_bearing_changes_from_annotated_internal(
    annotated_diff: &str,
    promote_exchange_tail_prompts: bool,
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
        let closes_exchange_tail = lines[i..]
            .iter()
            .find_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }
                line.strip_prefix("[agent] ")
            })
            .is_some_and(is_exchange_close_marker_line);
        let text = block.join("\n");
        let Some(kind) = classify_prompt_bearing_block(
            &text,
            has_substantive_agent_after,
            promote_exchange_tail_prompts && closes_exchange_tail,
        ) else {
            continue;
        };
        changes.push(PromptBearingChange { kind, text });
    }

    changes
}

fn classify_prompt_bearing_changes_raw(diff: &str) -> Vec<PromptBearingChange> {
    let mut changes = annotate_diff(diff)
        .map(|annotated| classify_prompt_bearing_changes_from_annotated(&annotated))
        .unwrap_or_default();

    // Annotated classification is the ordered source of truth because it preserves
    // mixed prompt/edit/artifact encounter order across the changed tail. Keep the
    // older prompt-block extractor as a safety net for prompt-target-only consumers
    // and append only truly-missing prompt blocks.
    for text in extract_prompt_target_blocks(diff) {
        if changes.iter().any(|existing| {
            existing.kind == PromptBearingChangeKind::PromptTarget
                && (existing.text == text
                    || ((existing.text.contains(&text) || text.contains(&existing.text))
                        && prompt_change_is_already_answered(&existing.text)))
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

pub fn classify_prompt_bearing_changes(diff: &str) -> Vec<PromptBearingChange> {
    suppress_answered_prompt_runs(classify_prompt_bearing_changes_raw(diff))
}

/// True when the change is entirely a managed-component state mutation:
/// queue activity toggle, queue body item (add/strike), backlog item line,
/// or done item line. These edits are routine session bookkeeping, not
/// real user prompts, so the Claude Code auto-loop guard treats them as
/// non-blocking. Plan: `#ccloopguard`.
pub fn change_is_managed_state_only(change: &PromptBearingChange) -> bool {
    text_is_managed_state_only(&change.text)
}

fn text_is_managed_state_only(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.is_empty() {
        return false;
    }
    lines.iter().all(|line| line_is_managed_state_only(line))
}

fn line_is_managed_state_only(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Queue activity comment marker (with or without `auto`)
    if trimmed.starts_with("<!-- agent:queue")
        || trimmed.starts_with("<!-- /agent:queue")
        || trimmed.starts_with("<!-- agent:backlog")
        || trimmed.starts_with("<!-- /agent:backlog")
        || trimmed.starts_with("<!-- agent:done")
        || trimmed.starts_with("<!-- /agent:done")
        || trimmed.starts_with("<!-- agent:review")
        || trimmed.starts_with("<!-- /agent:review")
    {
        return true;
    }
    // Frontmatter queue activity toggle (`queue_active: true|false`)
    if trimmed.starts_with("queue_active:") {
        return true;
    }
    // #22a8: managed `agent_doc_pipeline:` mirror block (key + indented children
    // such as `run_id:` / `step:` / `turn_id:` / `queue_task_id:`). The diff
    // already strips this block, but classify it as managed state too so any
    // residual line never reads as a user prompt.
    if trimmed.starts_with("agent_doc_pipeline:")
        || trimmed.starts_with("run_id:")
        || trimmed.starts_with("step:")
        || trimmed.starts_with("turn_id:")
        || trimmed.starts_with("queue_task_id:")
    {
        return true;
    }
    // Queue body lines: `- do ...`, `- ~do ...~` (strikethrough)
    if trimmed.starts_with("- do ") || trimmed.starts_with("- ~do ") {
        return true;
    }
    // Backlog/review/done item lines: `- [ ] [#id] ...`, `- [/]`, `- [x]`, `- [-]`
    if trimmed.starts_with("- [ ]")
        || trimmed.starts_with("- [/]")
        || trimmed.starts_with("- [x]")
        || trimmed.starts_with("- [-]")
        || trimmed.starts_with("- [?]")
    {
        return true;
    }
    // Done archive items often start with a date prefix: `- YYYY-MM-DD [#id]`
    if trimmed.len() >= 12
        && trimmed.starts_with("- ")
        && trimmed.chars().nth(2).is_some_and(|c| c.is_ascii_digit())
    {
        return true;
    }
    // Queue preset lines (`preset #foo`).
    if trimmed.starts_with("preset #") || trimmed.starts_with("preset:") {
        return true;
    }
    false
}

/// Return a copy of `diff` with user-added lines inside the current
/// `agent:queue` component removed.
///
/// Inactive queue bodies are document state, not fresh prompt input. Callers use
/// this before extracting prompt targets, slash commands, presets, and
/// imperative directives when queue activation has not resolved true.
pub fn suppress_inactive_queue_additions(diff: &str, current_content: &str) -> String {
    let ranges = queue_line_ranges(current_content);
    if ranges.is_empty() {
        return diff.to_string();
    }

    let mut current_line: Option<usize> = None;
    let mut filtered = String::new();
    for line in diff.lines() {
        if line.starts_with("@@ ") {
            current_line = hunk_current_start_line(line);
            filtered.push_str(line);
            filtered.push('\n');
            continue;
        }

        if line.starts_with("--- ") || line.starts_with("+++ ") {
            filtered.push_str(line);
            filtered.push('\n');
            continue;
        }

        if line.starts_with('+') && !line.starts_with("+++") {
            let line_no = current_line.unwrap_or(0);
            if current_line_is_in_ranges(line_no, &ranges) {
                current_line = current_line.map(|n| n + 1);
                continue;
            }
            current_line = current_line.map(|n| n + 1);
            filtered.push_str(line);
            filtered.push('\n');
            continue;
        }

        if line.starts_with(' ') {
            current_line = current_line.map(|n| n + 1);
        }

        filtered.push_str(line);
        filtered.push('\n');
    }

    filtered
}

fn queue_line_ranges(content: &str) -> Vec<(usize, usize)> {
    let Ok(components) = component::parse(content) else {
        return Vec::new();
    };
    components
        .iter()
        .filter(|component| component.name == "queue")
        .map(|component| {
            (
                line_number_at_byte(content, component.open_start),
                line_number_at_byte(content, component.close_end.saturating_sub(1)),
            )
        })
        .collect()
}

fn line_number_at_byte(content: &str, byte_offset: usize) -> usize {
    let end = byte_offset.min(content.len());
    content[..end].bytes().filter(|b| *b == b'\n').count() + 1
}

fn hunk_current_start_line(header: &str) -> Option<usize> {
    let plus = header
        .split_whitespace()
        .find(|part| part.starts_with('+'))?;
    let number = plus
        .trim_start_matches('+')
        .split(',')
        .next()
        .unwrap_or_default();
    number.parse::<usize>().ok()
}

fn current_line_is_in_ranges(line_no: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| line_no >= *start && line_no <= *end)
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
    let mut in_response_block = false;

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

        if line_looks_like_markdown_list_item(trimmed) {
            continue;
        }

        if is_exchange_response_heading(trimmed) {
            in_response_block = true;
            continue;
        }

        if in_response_block {
            if line_looks_like_targeted_prompt_prefix_repair_start(trimmed, true) {
                in_response_block = false;
            } else {
                continue;
            }
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
            let name = part.trim().trim_end_matches(['.', ':', ';']).trim();
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

/// Return the slash commands when every substantive added diff line is a slash
/// command outside fences and blockquotes. Returns `None` for mixed prompt text,
/// fenced examples, blockquotes, or diffs with no slash commands.
pub fn parse_slash_command_only_added_diff(diff: &str) -> Option<Vec<String>> {
    let mut commands = Vec::new();
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
        let was_in_fence = in_fence;
        let mut fence_delimiter = false;

        if !in_fence {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == '`' || fc == '~' {
                let fl = trimmed.chars().take_while(|&c| c == fc).count();
                if fl >= 3 {
                    in_fence = true;
                    fence_char = fc;
                    fence_len = fl;
                    fence_delimiter = true;
                }
            }
        } else {
            let fc = trimmed.chars().next().unwrap_or('\0');
            if fc == fence_char {
                let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
                if fl >= fence_len && trimmed[fl..].trim().is_empty() {
                    in_fence = false;
                    fence_delimiter = true;
                }
            }
        }

        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }

        if content.trim().is_empty() {
            continue;
        }
        if was_in_fence || fence_delimiter || trimmed.starts_with('>') {
            return None;
        }
        let command = parse_slash_command_candidate(content)?;
        commands.push(command);
    }

    (!commands.is_empty()).then_some(commands)
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
        if content.trim_start().starts_with('>') {
            continue;
        }

        // Must start with '/' followed by a command-like token.
        // Grammar: `/[a-z][a-z0-9:_-]*` with no additional `/` in the token.
        // This rejects absolute paths like `/home/brian/...` and `/tmp/foo`
        // that look like slash commands but are really filesystem paths.
        if let Some(command) = parse_slash_command_candidate(content) {
            commands.push(command);
        }
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
        let text = trimmed.strip_prefix("❯ ").unwrap_or(trimmed);
        let lower = text.to_lowercase();

        if lower.starts_with("do queue") || lower.starts_with("run queue") {
            let after = if lower.starts_with("do queue") {
                &text[8..]
            } else {
                &text[9..]
            };
            // Must be end of line, or followed by non-alphanumeric (punctuation, space)
            if after.is_empty() || after.starts_with(|c: char| !c.is_alphanumeric() && c != '#') {
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

/// Build a minimal unified diff from an in-memory prompt body.
///
/// This is used for binary-owned prompt sources that do not directly mutate the
/// session document, such as harness prompt bodies and active queue items.
pub fn synthetic_added_lines_diff(body: &str, target: &str) -> String {
    let lines = body.lines().collect::<Vec<_>>();
    let count = lines.len().max(1);
    let mut diff = format!("--- snapshot\n+++ {target}\n@@ -0,0 +1,{count} @@\n");
    if lines.is_empty() {
        diff.push_str("+\n");
        return diff;
    }
    for line in lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
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

/// Detect whether the user explicitly requested exchange compaction in added
/// diff lines.
///
/// This only matches direct imperative forms that start with `compact exchange`
/// (or `compact the exchange`) after prompt/pending normalization.
pub fn detect_exchange_compaction_request(diff: &str) -> bool {
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

        if !line.starts_with('+') || line.starts_with("+++") || in_fence || content.starts_with('>')
        {
            continue;
        }

        let Some(normalized) = normalize_imperative_candidate(content) else {
            continue;
        };
        let lower = normalized.to_ascii_lowercase();
        if lower.starts_with("compact exchange") || lower.starts_with("compact the exchange") {
            return true;
        }
    }

    false
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
        || compact.starts_with("do [#")
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
        if crate::pending::is_valid_pending_id(id) {
            trimmed = rest[close + 1..].trim_start();
        }
    }

    let normalized = trimmed
        .trim_start_matches('❯')
        .trim_start()
        .trim_end_matches(|c: char| c.is_ascii_punctuation() && c != ']')
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

pub fn line_looks_like_markdown_list_item(line: &str) -> bool {
    parse_markdown_list_item(line).is_some()
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
    let is_boundary_line =
        |line: &str| -> bool { is_boundary_artifact_line(line) || line.is_empty() };
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
            let a_trim = a.trim();
            let r_trim = r.trim();
            if a_trim == r_trim {
                return true;
            }
            if a_trim.starts_with("<!-- agent:boundary:")
                && r_trim.starts_with("<!-- agent:boundary:")
            {
                return true;
            }
            if is_head_boundary_artifact(a_trim)
                && a_trim
                    .strip_suffix(" (HEAD)")
                    .is_some_and(|base| base.trim() == r_trim)
            {
                return true;
            }
            if is_head_boundary_artifact(r_trim)
                && r_trim
                    .strip_suffix(" (HEAD)")
                    .is_some_and(|base| base.trim() == a_trim)
            {
                return true;
            }
            false
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

#[cfg(test)]
mod tests;
