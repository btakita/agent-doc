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
//!   only during stale-snapshot recovery.
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
//! - `parse_slash_commands_simple`: single added `/clear` line → `["/clear"]`
//! - `parse_slash_commands_ignores_fenced`: `/cmd` inside a ``` block → empty
//! - `parse_slash_commands_ignores_blockquote`: `> /cmd` → empty
//! - `parse_slash_commands_ignores_context_lines`: `/cmd` on context line (` `) → empty
//! - `parse_slash_commands_ignores_removed_lines`: `/cmd` on removed line (`-`) → empty
//! - `parse_slash_commands_with_args`: `/agent-doc foo.md` → `["/agent-doc foo.md"]`
//! - `parse_slash_commands_requires_letter_after_slash`: `/ `, `//comment` → empty

use anyhow::Result;
use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use std::path::Path;

use crate::{component, snapshot};

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

        // Must start with '/' followed by an ASCII letter.
        if !content.starts_with('/') {
            continue;
        }
        if !content[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }

        commands.push(content.trim_end().to_string());
    }

    commands
}

/// Compute a unified diff between the snapshot and the current document.
/// Returns None if there are no changes.
///
/// Both snapshot and current content are comment-stripped before comparison.
pub fn compute(doc: &Path) -> Result<Option<String>> {
    let t_total = std::time::Instant::now();

    let previous = snapshot::resolve(doc)?.unwrap_or_default();
    let snap_path = snapshot::path_for(doc)?;

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

    let diff = TextDiff::from_lines(&previous_stripped, &current_stripped);
    let has_changes = diff
        .iter_all_changes()
        .any(|c| c.tag() != ChangeTag::Equal);

    if !has_changes {
        eprintln!("[diff] no changes detected between snapshot and document (after comment stripping)");
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
        }
        return Ok(None);
    }

    // Stale snapshot recovery: if the diff is only completed assistant/user
    // exchanges with no new user content, the previous cycle wrote the response
    // but context compaction prevented the snapshot update.
    if is_stale_snapshot(&previous, &current) {
        eprintln!("[snapshot recovery] Snapshot synced — previous cycle completed but snapshot was stale");
        snapshot::save(doc, &current)?;
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
        }
        return Ok(None);
    }

    eprintln!("[diff] changes detected, computing unified diff");

    // Use unified diff with 5 lines of context around each change.
    // This provides surrounding context while keeping the diff focused.
    let output = diff
        .unified_diff()
        .context_radius(5)
        .header("snapshot", "document")
        .to_string();

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
        let last_added = extract_last_added_line(&strip_comments(previous), &strip_comments(&current));

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
                eprintln!("[diff] Content stable after {} consecutive checks", STABLE_CHECKS_REQUIRED);
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
            if !before_dot.is_empty() && before_dot.chars().all(|c| c.is_alphanumeric() || c == '-') {
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
        let last_word = before_dot.rsplit_once(' ').map(|(_, w)| w).unwrap_or(before_dot);
        // If last word contains dots already (e.g., "www.example.") or is a known domain-like
        // pattern, treat as potentially truncated
        if last_word.contains('.') || last_word.ends_with("http") || last_word.ends_with("https") {
            return true;
        }
        // Otherwise, '.' is terminal (normal sentence ending)
        return false;
    }

    let terminal = matches!(last_char, '!' | '?' | ':' | ';' | ')' | ']' | '"' | '\'' | '`' | '*' | '-' | '>' | '|');

    !terminal
}

/// Truncate a string for log display.
fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
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
    if added_content.len() == 1 && removed_content.is_empty() && added_content[0].ends_with('?')
    {
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
    if s.len() <= 80 {
        s
    } else {
        &s[..80]
    }
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
        let has_insert = diff.iter_all_changes().any(|c| c.tag() == ChangeTag::Insert);
        assert!(has_insert);
    }

    #[test]
    fn diff_format_deletions() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\nline2\n";
        let current = "line1\n";
        let diff = TextDiff::from_lines(previous, current);
        let has_delete = diff.iter_all_changes().any(|c| c.tag() == ChangeTag::Delete);
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
        let document = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nNew question here\n";
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
        let document = "## User\n\nHello\n\n## Assistant\n\nHi there\n\nPlease elaborate\n\n## User\n\n";
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

    // --- Code-aware comment stripping tests ---

    #[test]
    fn strip_preserves_comment_syntax_in_inline_backticks() {
        // `<!--` inside backticks should NOT be treated as a comment start
        let input = "Use `<!--` to start a comment.\n<!-- agent:foo -->\ncontent\n<!-- /agent:foo -->\n";
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
        assert!(diff.is_some(), "diff should detect user edit after stream write");
        let diff_text = diff.unwrap();
        assert!(diff_text.contains("New user edit here"), "diff should contain user's new text: {}", diff_text);
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
        assert!(looks_truncated("Also, when I called agent-doc run on this file...and ther"));
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
        assert!(elapsed.as_millis() < 500, "should not delay for complete content");
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
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,2 @@\n context\n-removed line\n context\n";
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
        let diff =
            "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n+/clear\n+/agent-doc foo.md\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/clear", "/agent-doc foo.md"]);
    }
}
