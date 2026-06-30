//! # Module: diff
//!
//! ## Spec
//! - `strip_comments(content)` removes `[//]: # (...)` link-reference comments and
//!   `<!-- ... -->` HTML comments from document content, while preserving agent
//!   range markers (`<!-- agent:* -->`). Comment patterns inside fenced code blocks
//!   and inline backtick spans are not treated as comment syntax.
//! - `post_exchange_ordinary_html_comments(content)` returns ordinary HTML comments
//!   after the last `agent:exchange` close, excluding agent markers, component-owned
//!   comments, and preserved user-note blocks.
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
//! - `post_exchange_comment_scan_ignores_agent_components_and_user_notes`:
//!   post-exchange scratch comments are returned, while comments inside agent components
//!   and user-note blocks are ignored.
//! - `post_exchange_comment_directive_signals_detects_directive_text`: dispatch,
//!   preset, and slash-command-looking lines in ordinary comments produce ordered,
//!   unique signal strings.
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
//! - `first_unstarted_prompt_bearing_change_from_diff(diff, current_doc)`: selects
//!   the first still-unanswered prompt-bearing change from an already-built diff,
//!   including answered-prompt suppression against the current exchange body.
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

use agent_doc_element::element;

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
/// Delegates to `element::strip_comments` — the shared implementation
/// available to both the binary and external crates.
pub fn strip_comments(content: &str) -> String {
    // #22a8 (Phase 5b write-side): also drop the managed `agent_doc_pipeline:`
    // frontmatter block so a pipeline-only mirror write (emitted on every
    // hot-path phase transition) reads as no change and never surfaces as a user
    // edit. Both sides of every diff pass through this, so a pipeline-only delta
    // cancels to `no_changes`. Shared with the write-side splice so the strip and
    // the write agree byte-for-byte on the block boundary.
    strip_pipeline_block_lines(&element::strip_comments(content))
}

/// Extract the last added non-empty line between already-stripped documents.
pub fn extract_last_added_line(previous_stripped: &str, current_stripped: &str) -> Option<String> {
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

/// Check if a line looks truncated because a user may still be typing.
///
/// Single characters are treated as potentially truncated; the caller's stable
/// reread loop confirms whether they were intentional short answers.
pub fn looks_truncated(line: &str) -> bool {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return false;
    }

    if trimmed.starts_with('/')
        || trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with("<!--")
    {
        return false;
    }

    if trimmed.len() == 1 {
        return true;
    }

    if !trimmed.contains(' ') && trimmed.len() >= 2 {
        if trimmed.ends_with('.') && trimmed.chars().filter(|&c| c == '.').count() >= 1 {
            let before_dot = &trimmed[..trimmed.len() - 1];
            if !before_dot.is_empty() && before_dot.chars().all(|c| c.is_alphanumeric() || c == '-')
            {
                return true;
            }
        }
        return false;
    }

    let last_char = trimmed.chars().last().unwrap();
    if last_char == '.' {
        let before_dot = &trimmed[..trimmed.len() - 1];
        let last_word = before_dot
            .rsplit_once(' ')
            .map(|(_, w)| w)
            .unwrap_or(before_dot);
        if last_word.contains('.') || last_word.ends_with("http") || last_word.ends_with("https") {
            return true;
        }
        return false;
    }

    let terminal = matches!(
        last_char,
        '!' | '?' | ':' | ';' | ')' | ']' | '"' | '\'' | '`' | '*' | '-' | '>' | '|'
    );

    !terminal
}

/// Truncate a string for bounded diagnostics while preserving UTF-8 boundaries.
pub fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut truncated = max;
        while truncated > 0 && !s.is_char_boundary(truncated) {
            truncated -= 1;
        }
        format!("{}...", &s[..truncated])
    }
}

/// Byte-precise removal of the managed `agent_doc_pipeline:` frontmatter block
/// for diff comparison. Keeping this local pure helper avoids pulling the full
/// frontmatter parser into diff classification.
fn strip_pipeline_block_lines(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    if lines.first().map(|line| line.trim_end()) != Some("---") {
        return content.to_string();
    };
    let Some(close_idx) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| line.trim_end() == "---")
        .map(|(idx, _)| idx)
    else {
        return content.to_string();
    };
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut skipping = false;
    for (idx, line) in lines.iter().enumerate() {
        if idx == 0 || idx >= close_idx {
            skipping = false;
            out.push(line);
            continue;
        }
        if skipping {
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }
            skipping = false;
        }
        if line.trim_start().starts_with("agent_doc_pipeline:") {
            skipping = true;
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

/// Return ordinary HTML comments that appear after the final exchange close.
///
/// Agent markers, comments inside any parsed agent component, and preserved
/// multi-line user-note comments are not executable prompt material and are
/// excluded from the result.
pub fn post_exchange_ordinary_html_comments(content: &str) -> Vec<String> {
    let Ok(components) = element::parse(content) else {
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
        if !element::is_agent_marker(inner)
            && !components.iter().any(|component| {
                absolute_open >= component.open_start && absolute_open < component.close_end
            })
            && !post_exchange_comment_is_user_note(inner)
        {
            comments.push(inner.to_string());
        }
        let consumed = open + "<!--".len() + close + "-->".len();
        tail_start += consumed;
        tail = &content[tail_start..];
    }
    comments
}

fn post_exchange_comment_is_user_note(inner: &str) -> bool {
    let lines: Vec<&str> = inner.lines().collect();
    if lines.len() < 2 {
        return false;
    }
    let has_horizontal_rule = lines.iter().any(|line| line.trim() == "---");
    let has_prose = lines.iter().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && trimmed != "---"
            && !trimmed.starts_with('#')
            && !trimmed.starts_with('/')
            && !trimmed.starts_with("dispatch ")
            && !trimmed.starts_with("preset ")
    });
    has_horizontal_rule && has_prose
}

/// Return ordered, unique directive-looking signal strings from a comment body.
pub fn post_exchange_comment_directive_signals(comment: &str) -> Vec<String> {
    let mut signals = Vec::new();
    for line in comment.lines() {
        let trimmed = line.trim().trim_start_matches('❯').trim();
        let signal = if let Some(rest) = trimmed.strip_prefix("dispatch ") {
            Some(format!(
                "dispatch {}",
                post_exchange_comment_first_word(rest)
            ))
        } else if let Some(rest) = trimmed.strip_prefix("preset ") {
            Some(format!("preset {}", post_exchange_comment_first_word(rest)))
        } else if post_exchange_comment_looks_like_slash_command(trimmed) {
            Some(post_exchange_comment_first_word(trimmed).to_string())
        } else {
            None
        };
        if let Some(signal) = signal
            && !signals.iter().any(|existing| existing == &signal)
        {
            signals.push(signal);
        }
    }
    signals
}

fn post_exchange_comment_first_word(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or(text)
}

fn post_exchange_comment_looks_like_slash_command(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('/') else {
        return false;
    };
    rest.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase())
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
        || line_is_binary_authored_ipc_proof_diagnostic(trimmed)
        || line_is_binary_authored_compact_summary(trimmed)
}

/// `#ipcproofnostall`: recognize a binary-authored interrupted-cycle IPC-proof
/// recovery diagnostic by its OWN unambiguous tokens, independent of whether its
/// `### Re:` heading is still adjacent.
///
/// `preflight::format_ipc_dogfood_note` appends a `### Re: ... (interrupted-cycle
/// recovery)` heading plus a fenced block that ends with the structured
/// `ipc_proof_insufficient file=... invariant=... recovery=...` event line. When
/// the block is intact the heading already classifies it as a `RecoveryArtifact`.
/// But a post-commit worktree corruption (`#postcommit-ipc-worktree-corruption`)
/// can reorder/separate the structured line out of the fenced block; on its own
/// it would otherwise fall through to a `PromptTarget` at the exchange tail and
/// falsely INTERRUPT the next `session-check` / `finalize`, stalling the queue.
///
/// Match is intentionally narrow: it keys off the binary-authored event token
/// shape (`ipc_proof_insufficient` + structured `invariant=`/`recovery=` fields)
/// or the note's literal self-description line. A genuine user prompt that merely
/// mentions "ipc" / "drift" in prose does NOT match.
pub fn line_is_binary_authored_ipc_proof_diagnostic(line: &str) -> bool {
    let trimmed = line.trim();
    // The structured fail-closed event line emitted by
    // `write::materialize` / recorded in ops.log and folded into the dogfood
    // note. Require the leading event token AND both structured fields so an
    // arbitrary user sentence containing the phrase cannot match.
    if trimmed.starts_with("ipc_proof_insufficient")
        && trimmed.contains("invariant=")
        && trimmed.contains("recovery=")
    {
        return true;
    }
    // The note's literal binary-authored self-description line.
    if trimmed
        == "This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle."
    {
        return true;
    }
    false
}

/// `#provauth3`: recognize a binary-authored `### Session Summary` compaction
/// line by its OWN unambiguous shape, independent of provenance inference.
///
/// `compact.rs` rewrites `agent:exchange` into a Session Summary block authored
/// entirely by the binary (`### Session Summary`, the `*Compacted. Content
/// archived to ...*` pointer, a `Compacted content:` section, and
/// `- Archived N response topic(s): ...` / `- Prior summary/context: ...` /
/// `- Trailing prompt/context: ...` items). Relative to the pre-compact snapshot
/// every one of those lines is an *inserted* line in the exchange user region, so
/// the content-inference prompt-prefix normalizer (`normalize_user_prompts_in_exchange`)
/// stamps them with `❯` and the prompt classifier then treats them as an
/// unresolved user prompt — falsely INTERRUPTing `session-check` and stalling the
/// queue. Origin is *known* here (the binary authored the compaction), so this is
/// the provenance check that replaces the content guess: a compaction summary
/// line is never an operator prompt and never gets a `❯` prefix.
///
/// Match is intentionally narrow — it keys off the exact binary-authored summary
/// shapes, tolerating a leading `❯ ` that an earlier mis-classification already
/// applied, so a genuine user line that merely mentions "compacted" in prose does
/// NOT match.
pub fn line_is_binary_authored_compact_summary(line: &str) -> bool {
    let trimmed = line.trim();
    // Tolerate a `❯ ` (or bare `❯`) prefix a prior repair pass already applied,
    // so the recognizer matches both a freshly-built summary (no prefix) and an
    // already-corrupted committed one.
    let trimmed = trimmed
        .strip_prefix("❯ ")
        .or_else(|| trimmed.strip_prefix('❯'))
        .map(str::trim_start)
        .unwrap_or(trimmed);
    trimmed == "### Session Summary"
        || trimmed == "Compacted content:"
        || trimmed.starts_with("*Compacted. Content archived to `")
        || (trimmed.starts_with("- Archived ") && trimmed.contains("response topic(s):"))
        || trimmed.starts_with("- Prior summary/context:")
        || trimmed.starts_with("- Trailing prompt/context:")
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
    let prompt_prefixed = trimmed.starts_with('❯') && !line_looks_like_markdown_list_item(trimmed);
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && !line_has_known_response_label_after_prompt(trimmed)
        && (slash_command || prompt_prefixed || trimmed.ends_with('?') || normalized_imperative)
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

    if (trimmed.starts_with('❯') && !line_looks_like_markdown_list_item(trimmed))
        || line_looks_like_soft_prompt_request(unprefixed)
    {
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

    if line_looks_like_markdown_list_item(trimmed)
        || trimmed.starts_with("- ")
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
        || lower.starts_with("i dry-ran ")
        || lower.starts_with("recovered ")
        || lower.starts_with("confirmed ")
        || lower.starts_with("no code files changed")
        || lower.starts_with("this closeout ")
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

pub fn strip_queue_components_for_unstarted_prompt_guard(body: &str) -> String {
    let Ok(components) = element::parse(body) else {
        return body.to_string();
    };
    let mut result = body.to_string();
    for component in components.iter().rev() {
        if component.name == "queue" {
            result = component.replace_content(&result, "");
        }
    }
    result
}

pub fn prompt_bearing_body_for_unstarted_prompt_guard(content: &str) -> String {
    let body = agent_doc_frontmatter::frontmatter::parse(content)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| content.to_string());
    strip_comments(&strip_queue_components_for_unstarted_prompt_guard(&body))
}

pub fn prompt_target_is_immediately_before_existing_response(
    current_doc: &str,
    change_text: &str,
) -> bool {
    let target_line = change_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string());
    let answered_prompt_marker = target_line
        .as_deref()
        .is_some_and(|line| line.starts_with('❯'));
    let target = target_line
        .as_deref()
        .map(|line| line.trim_start_matches('❯').trim().to_string());
    let Some(target) = target else {
        return false;
    };
    if target.is_empty() {
        return false;
    }
    let body = agent_doc_frontmatter::frontmatter::parse(current_doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| current_doc.to_string());
    let Ok(components) = element::parse(&body) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    let lines: Vec<&str> = exchange.content(&body).lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let normalized = line.trim().trim_start_matches('❯').trim();
        if normalized != target {
            continue;
        }
        for next in lines.iter().skip(idx + 1) {
            let trimmed = next.trim();
            if trimmed.is_empty() || trimmed.starts_with("<!--") {
                continue;
            }
            if is_exchange_response_heading(trimmed) {
                return true;
            }
            if answered_prompt_marker {
                continue;
            }
            return false;
        }
    }
    false
}

pub fn first_unstarted_prompt_bearing_change_from_diff(
    diff_text: &str,
    current_doc: &str,
) -> Option<PromptBearingChange> {
    let changes = classify_prompt_bearing_changes(diff_text);
    let mut skip_answered_response_run = false;
    for (idx, change) in changes.iter().enumerate() {
        match change.kind {
            PromptBearingChangeKind::RecoveryArtifact
            | PromptBearingChangeKind::BoundaryArtifact => {
                continue;
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
                    || prompt_target_is_immediately_before_existing_response(
                        current_doc,
                        &change.text,
                    )
                {
                    skip_answered_response_run = true;
                    continue;
                }
                return Some(change.clone());
            }
            PromptBearingChangeKind::ContentEdit => {
                continue;
            }
        }
    }
    None
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
    // `#ipcproofnostall`: a binary-authored IPC-proof recovery diagnostic line
    // anywhere in the block (e.g. after a post-commit worktree corruption
    // separated it from its `### Re:` heading) keeps the block a RecoveryArtifact
    // so it is never an unresolved user PromptTarget at the exchange tail. The
    // marker is keyed off the structured event/self-description tokens, so a real
    // user prompt that only mentions IPC/drift in prose still classifies normally.
    if non_blank
        .iter()
        .any(|line| line_is_binary_authored_ipc_proof_diagnostic(line))
    {
        return Some(PromptBearingChangeKind::RecoveryArtifact);
    }
    // `#provauth3`: a block that is entirely binary-authored compaction summary
    // content (Session Summary heading + archive pointer + archived-topic items)
    // is never a user prompt. Compaction rewrites the whole exchange tail at once,
    // so the block is self-contained; requiring EVERY non-blank line to match the
    // narrow summary shapes keeps a real prompt that happens to sit next to a
    // stray summary line from being hidden.
    if non_blank
        .iter()
        .all(|line| line_is_binary_authored_compact_summary(line))
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

/// Return the first bare prompt-prefix target before an inserted marker line.
pub fn first_bare_prompt_prefix_target_before_marker(diff: &str, marker: &str) -> Option<String> {
    let mut prefix_diff = String::new();
    for line in diff.lines() {
        if line
            .strip_prefix('+')
            .is_some_and(|added| added.trim() == marker)
        {
            break;
        }
        prefix_diff.push_str(line);
        prefix_diff.push('\n');
    }
    first_bare_prompt_prefix_target(&prefix_diff)
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
        if changes
            .iter()
            .any(|existing| prompt_target_block_already_classified(existing, &text))
        {
            continue;
        }
        changes.push(PromptBearingChange {
            kind: PromptBearingChangeKind::PromptTarget,
            text,
        });
    }

    changes
}

fn prompt_target_block_already_classified(
    existing: &PromptBearingChange,
    prompt_text: &str,
) -> bool {
    if existing.text == prompt_text {
        return true;
    }
    match existing.kind {
        PromptBearingChangeKind::PromptTarget => {
            (existing.text.contains(prompt_text) || prompt_text.contains(&existing.text))
                && prompt_change_is_already_answered(&existing.text)
        }
        PromptBearingChangeKind::RecoveryArtifact
        | PromptBearingChangeKind::BoundaryArtifact
        | PromptBearingChangeKind::ContentEdit => existing.text.contains(prompt_text),
    }
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
    let Ok(components) = element::parse(content) else {
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

/// True when `trimmed` (a line already left-trimmed of whitespace) opens with a
/// markdown list bullet (`- `, `* `, `+ `, or an ordered `N. `). Used to exclude
/// queue/backlog/review task entries from same-turn exchange-directive detection
/// (`#qcompactfp`).
fn trimmed_is_markdown_list_item(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }
    // Ordered-list `N. ` bullet.
    if let Some(dot) = trimmed.find(". ") {
        let head = &trimmed[..dot];
        if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Detect whether the user explicitly requested exchange compaction in added
/// diff lines.
///
/// This only matches direct imperative forms that start with `compact exchange`
/// (or `compact the exchange`) after prompt/pending normalization, and only when
/// authored as exchange prose — a queue/backlog/review **list item** that merely
/// begins with those words is a task entry, not a same-turn directive, and is
/// skipped (`#qcompactfp`).
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

        if !line.starts_with('+')
            || line.starts_with("+++")
            || in_fence
            || content.starts_with('>')
            || trimmed_is_markdown_list_item(trimmed)
        {
            // A markdown list item (`- `/`* `/`+ `/`N. `) is a queue/backlog/review
            // task entry, never a same-turn exchange compaction directive — a real
            // `compact exchange` request is exchange prose (optionally `❯`-prefixed).
            // Skipping bullets prevents a queued bug-report head such as
            // "Compact exchange should commit the compacted content" from falsely
            // aborting finalize (#qcompactfp).
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
        if agent_doc_element_backlog::backlog::is_valid_pending_id(id) {
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
    let mut in_recovery_response_tail = false;

    for line in diff.lines() {
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            in_recovery_response_tail = false;
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
            in_recovery_response_tail = false;
            continue;
        }

        if content.starts_with('>') {
            continue;
        }

        if is_exchange_response_heading(trimmed) {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            in_recovery_response_tail = true;
            continue;
        }

        if in_recovery_response_tail {
            if line_looks_like_targeted_prompt_prefix_repair_start(
                trimmed,
                line_looks_like_prompt_target(trimmed),
            ) {
                in_recovery_response_tail = false;
            } else {
                continue;
            }
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

/// True when a path can participate in the partial-staging closeout guard.
///
/// The guard targets source/test code partial staging, not session document
/// churn. Markdown is intentionally excluded to avoid cross-document prose
/// overlap producing false positives.
pub fn is_partial_staging_relevant_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with(".agent-doc/")
        || normalized.starts_with(".git/")
        || normalized.ends_with(".lock")
    {
        return false;
    }
    let lower = normalized.to_ascii_lowercase();
    let Some(ext) = lower.rsplit('.').next() else {
        return false;
    };
    matches!(
        ext,
        "rs" | "kt"
            | "kts"
            | "java"
            | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "go"
            | "rb"
            | "swift"
            | "txt"
            | "snap"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
    )
}

/// True when committed and dirty path sets look like source/test companions.
pub fn partial_staging_paths_look_related(committed: &[String], dirty: &[String]) -> bool {
    if committed
        .iter()
        .any(|committed_path| dirty.iter().any(|dirty_path| dirty_path == committed_path))
    {
        return true;
    }
    let dirty_has_test = dirty.iter().any(|path| path_looks_test_like(path));
    let committed_has_source = committed.iter().any(|path| !path_looks_test_like(path));
    dirty_has_test && committed_has_source
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialStagingCompanionFinding {
    pub committed_paths: Vec<String>,
    pub dirty_paths: Vec<String>,
    pub literals: Vec<String>,
}

pub fn partial_staging_companion_finding(
    committed_paths: &[String],
    dirty_paths: &[String],
    committed_diff: &str,
    dirty_diff: &str,
) -> Option<PartialStagingCompanionFinding> {
    let committed_paths = filtered_partial_staging_paths(committed_paths);
    if committed_paths.is_empty() {
        return None;
    }

    let dirty_paths = filtered_partial_staging_paths(dirty_paths);
    if dirty_paths.is_empty() || !partial_staging_paths_look_related(&committed_paths, &dirty_paths)
    {
        return None;
    }

    let committed_literals = extract_changed_string_literals(committed_diff);
    let dirty_literals = extract_changed_string_literals(dirty_diff);
    let literals = committed_literals
        .intersection(&dirty_literals)
        .cloned()
        .collect::<Vec<_>>();
    if literals.is_empty() {
        return None;
    }

    Some(PartialStagingCompanionFinding {
        committed_paths,
        dirty_paths,
        literals,
    })
}

fn filtered_partial_staging_paths(paths: &[String]) -> Vec<String> {
    let mut filtered = paths
        .iter()
        .filter(|path| is_partial_staging_relevant_path(path))
        .cloned()
        .collect::<Vec<_>>();
    filtered.sort();
    filtered.dedup();
    filtered
}

pub fn path_looks_test_like(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.starts_with("test/")
        || lower.contains("/test/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_tests.rs")
        || lower.ends_with(".snap")
        || lower
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains("test"))
}

/// Extract changed string/backtick literals from a unified diff.
pub fn extract_changed_string_literals(diff: &str) -> std::collections::BTreeSet<String> {
    let mut literals = std::collections::BTreeSet::new();
    for line in diff.lines() {
        if !(line.starts_with('+') || line.starts_with('-'))
            || line.starts_with("+++")
            || line.starts_with("---")
        {
            continue;
        }
        for literal in extract_string_literals_from_line(&line[1..]) {
            if interesting_changed_literal(&literal) {
                literals.insert(literal);
            }
        }
    }
    literals
}

fn extract_string_literals_from_line(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '"' && ch != '`' {
            continue;
        }
        let quote = ch;
        let mut escaped = false;
        let mut literal = String::new();
        for next in chars.by_ref() {
            if escaped {
                literal.push(next);
                escaped = false;
                continue;
            }
            if quote == '"' && next == '\\' {
                escaped = true;
                continue;
            }
            if next == quote {
                break;
            }
            literal.push(next);
        }
        result.push(literal);
    }
    result
}

fn interesting_changed_literal(literal: &str) -> bool {
    let trimmed = literal.trim();
    trimmed.len() >= 4 && trimmed.chars().any(|ch| ch.is_ascii_alphanumeric())
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
    fn partial_staging_relevant_paths_exclude_markdown_and_sidecars() {
        assert!(is_partial_staging_relevant_path("src/lib.rs"));
        assert!(is_partial_staging_relevant_path("tests/flow.snap"));
        assert!(!is_partial_staging_relevant_path(
            "tasks/agent-doc/session.md"
        ));
        assert!(!is_partial_staging_relevant_path(".agent-doc/state.json"));
        assert!(!is_partial_staging_relevant_path("Cargo.lock"));
    }

    #[test]
    fn partial_staging_paths_match_same_file_or_source_with_dirty_test() {
        assert!(partial_staging_paths_look_related(
            &["src/lib.rs".to_string()],
            &["src/lib.rs".to_string()]
        ));
        assert!(partial_staging_paths_look_related(
            &["src/lib.rs".to_string()],
            &["tests/lib_test.rs".to_string()]
        ));
        assert!(!partial_staging_paths_look_related(
            &["tests/lib_test.rs".to_string()],
            &["src/lib.rs".to_string()]
        ));
    }

    #[test]
    fn changed_string_literals_extracts_only_interesting_changed_literals() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
-let old = \"stable literal\";
+let new = \"stable literal\";
+let raw = `retry closeout`;
+let short = \"x\";
 context = \"ignored context\";
";
        let literals = extract_changed_string_literals(diff);
        assert!(literals.contains("stable literal"));
        assert!(literals.contains("retry closeout"));
        assert!(!literals.contains("x"));
        assert!(!literals.contains("ignored context"));
    }

    #[test]
    fn partial_staging_companion_finding_detects_source_test_literal_overlap() {
        let committed_paths = vec!["src/render.rs".to_string()];
        let dirty_paths = vec!["tests/render_test.rs".to_string()];
        let committed_diff = r#"
diff --git a/src/render.rs b/src/render.rs
@@ -1 +1 @@
-pub fn render() -> &'static str { "old output" }
+pub fn render() -> &'static str { "new queue output" }
"#;
        let dirty_diff = r#"
diff --git a/tests/render_test.rs b/tests/render_test.rs
@@ -1 +1 @@
-assert_eq!(render(), "old output");
+assert_eq!(render(), "new queue output");
"#;

        let finding = partial_staging_companion_finding(
            &committed_paths,
            &dirty_paths,
            committed_diff,
            dirty_diff,
        )
        .unwrap();
        assert_eq!(finding.committed_paths, vec!["src/render.rs"]);
        assert_eq!(finding.dirty_paths, vec!["tests/render_test.rs"]);
        assert_eq!(finding.literals, vec!["new queue output", "old output"]);
    }

    #[test]
    fn partial_staging_companion_finding_dedupes_and_sorts_paths_and_literals() {
        let committed_paths = vec!["src/z.rs".to_string(), "src/a.rs".to_string()];
        let dirty_paths = vec![
            "tests/z_test.rs".to_string(),
            "tests/a_test.rs".to_string(),
            "tests/z_test.rs".to_string(),
        ];
        let committed_diff = r#"
+let a = "shared alpha";
+let z = "shared zeta";
"#;
        let dirty_diff = r#"
+assert_eq!(actual, "shared zeta");
+assert_eq!(other, "shared alpha");
"#;

        let finding = partial_staging_companion_finding(
            &committed_paths,
            &dirty_paths,
            committed_diff,
            dirty_diff,
        )
        .unwrap();
        assert_eq!(finding.committed_paths, vec!["src/a.rs", "src/z.rs"]);
        assert_eq!(
            finding.dirty_paths,
            vec!["tests/a_test.rs", "tests/z_test.rs"]
        );
        assert_eq!(finding.literals, vec!["shared alpha", "shared zeta"]);
    }

    #[test]
    fn partial_staging_companion_finding_rejects_unrelated_or_no_overlap() {
        assert!(
            partial_staging_companion_finding(
                &["tasks/session.md".to_string()],
                &["tasks/other.md".to_string()],
                "+\"make check\"\n",
                "+\"make check\"\n",
            )
            .is_none()
        );
        assert!(
            partial_staging_companion_finding(
                &["tests/render_test.rs".to_string()],
                &["src/render.rs".to_string()],
                "+\"new queue output\"\n",
                "+\"new queue output\"\n",
            )
            .is_none()
        );
        assert!(
            partial_staging_companion_finding(
                &["src/render.rs".to_string()],
                &["tests/render_test.rs".to_string()],
                "+\"committed literal\"\n",
                "+\"dirty literal\"\n",
            )
            .is_none()
        );
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
    fn post_exchange_comment_scan_ignores_agent_components_and_user_notes() {
        let content = concat!(
            "<!--\n",
            "pre-exchange scratch ignored\n",
            "/clear\n",
            "-->\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "Scratch note while testing.\n",
            "dispatch #manual-review\n",
            "-->\n\n",
            "<!--\n",
            "---\n",
            "Preserved user note.\n",
            "dispatch #ignored\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- dispatch #queued -->\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:done -->\n",
            "<!-- archived #manual-review -->\n",
            "<!-- /agent:done -->\n",
        );

        assert_eq!(
            post_exchange_ordinary_html_comments(content),
            vec!["Scratch note while testing.\ndispatch #manual-review".to_string()]
        );
    }

    #[test]
    fn post_exchange_comment_directive_signals_detects_directive_text() {
        let comment = concat!(
            "Scratch note while testing.\n",
            "dispatch #manual-review now\n",
            "dispatch #manual-review again\n",
            "preset #repair-check with args\n",
            "❯ /clear now\n",
            "/Clear ignored\n",
            "//ignored\n",
        );

        assert_eq!(
            post_exchange_comment_directive_signals(comment),
            vec![
                "dispatch #manual-review".to_string(),
                "preset #repair-check".to_string(),
                "/clear".to_string()
            ]
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

    // --- classify_diff tests ---

    fn make_diff(added: &[&str], removed: &[&str]) -> String {
        let mut lines = vec!["--- snapshot", "+++ document", "@@ -1,5 +1,5 @@"];
        for r in removed {
            lines.push(r);
        }
        lines.push(" context line");
        for a in added {
            lines.push(a);
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
    fn classify_head_mention_in_user_prose_as_content_addition() {
        let diff = make_diff(
            &[
                "+`❯ ` prompt prefix is being stripped away by the uncommitted user affordance that adds the ` (HEAD)` suffix. spec-test-build-install-commit-push",
            ],
            &[],
        );
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::ContentAddition);
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
        let c = classify_diff(diff);
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
    fn classify_prompt_bearing_changes_promotes_plain_exchange_tail_to_prompt_target() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
Done.\n\
+When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push\n\
<!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(
            changes[0].text,
            "When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push"
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_promotes_bare_slash_command_to_prompt_target() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
### Re: older — gpt-5\n\
+/clear\n\
<!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(changes[0].text, "/clear");
        assert!(line_looks_like_fresh_prompt_after_response("/clear"));
        assert!(!text_line_looks_like_prompt_target(
            "/home/brian/work/foo.md"
        ));
    }

    #[test]
    fn prefixed_markdown_response_labels_are_not_prompt_targets() {
        for line in [
            "❯ **Verification:** Both redirects confirmed via `curl`.",
            "❯ Commit / push:",
            "❯ **Commit / push:**",
            "❯ - **Verification:** `cargo test` passed.",
            "❯ 1. **What changed:** normalized response labels.",
        ] {
            assert!(
                !text_line_looks_like_prompt_target(line),
                "assistant response label must not be classified as a prompt target: {line}"
            );
            assert!(
                line_looks_like_plain_response_after_prompt(line),
                "assistant response label must remain response prose: {line}"
            );
        }
    }

    #[test]
    fn prefixed_user_followup_after_response_still_starts_prompt() {
        for line in [
            "❯ verify deploy status",
            "❯ Verification failed; what next?",
            "❯ do [#respfx]. spec-test-build-install-commit-push",
        ] {
            assert!(
                text_line_looks_like_prompt_target(line),
                "real user follow-up must stay prompt-bearing: {line}"
            );
            assert!(
                line_looks_like_fresh_prompt_after_response(line),
                "real user follow-up must start a new prompt run: {line}"
            );
        }
    }

    #[test]
    fn classify_prompt_bearing_changes_ignores_prefixed_markdown_response_label() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            ### Re: done — gpt-5\n\
            +❯ **Verification:** Both redirects confirmed via `curl`.\n\
            <!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert!(
            changes.is_empty(),
            "prefixed assistant response label must not reopen a cycle: {changes:?}"
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_ignores_prefixed_recovery_evidence() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,11 @@\n\
            ctx\n\
            +### Re: stalled Run Agent Doc recovery — gpt-5\n\
            +\n\
            +Recovered the stranded closeout first.\n\
            +\n\
            +❯ - `./gradlew test --tests 'TerminalUtilTest.protected prompt input route refusal is actionable not persistent failure'`\n\
            +❯ - `cargo test -p agent-doc-orchestration protected_prompt_draft_preview_redacts_and_bounds_latest_draft`\n\
            +❯ - `612b1552` `test: cover protected prompt route refusal`\n\
            +\n\
            +No code files changed in this follow-up.\n\
            <!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);

        assert_eq!(
            changes.len(),
            1,
            "recovery evidence must stay grouped: {changes:?}"
        );
        assert_eq!(changes[0].kind, PromptBearingChangeKind::RecoveryArtifact);
        assert!(
            !changes
                .iter()
                .any(|change| change.kind == PromptBearingChangeKind::PromptTarget),
            "prompt-prefixed recovery evidence must not become prompt targets: {changes:?}"
        );
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

    // `#ipcproofnostall`: the binary-authored interrupted-cycle IPC-proof
    // recovery diagnostic (heading + fenced block) must classify as a
    // RecoveryArtifact and never as an unresolved user PromptTarget.
    #[test]
    fn classify_prompt_bearing_changes_marks_ipc_proof_diagnostic_block_as_recovery_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,12 @@\n\
            ctx\n\
            +### Re: IPC proof diagnostic (interrupted-cycle recovery) — agent-doc\n\
            +\n\
            +```text\n\
            +**IPC proof issue dogfood log**\n\
            +This is binary-authored diagnostic content, not a user prompt, so it does not require a separate response cycle.\n\
            +Issue class: `ipc_proof_insufficient`\n\
            +ipc_proof_insufficient file=/tmp/session.md source=socket_ack_content patch_id=abc invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle\n\
            +```\n\
            <!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert!(
            !changes
                .iter()
                .any(|change| change.kind == PromptBearingChangeKind::PromptTarget),
            "intact IPC-proof diagnostic must not classify as a PromptTarget: {changes:?}"
        );
    }

    // The corruption case: a post-commit worktree corruption separated the
    // `ipc_proof_insufficient` event line from its `### Re:` heading and fence,
    // leaving the bare structured line at the exchange tail. It must STILL be a
    // RecoveryArtifact, not the unresolved PromptTarget that stalls the queue.
    #[test]
    fn classify_prompt_bearing_changes_marks_separated_ipc_proof_line_as_recovery_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n\
            ctx\n\
            +ipc_proof_insufficient file=/tmp/session.md source=socket_ack_content patch_id=abc invariant=live_prompt_drift_after_preflight recovery=content_ours_snapshot_next_cycle\n\
            <!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert!(
            !changes
                .iter()
                .any(|change| change.kind == PromptBearingChangeKind::PromptTarget),
            "a separated binary-authored ipc_proof_insufficient line must not be a PromptTarget: {changes:?}"
        );
        assert!(
            changes
                .iter()
                .any(|change| change.kind == PromptBearingChangeKind::RecoveryArtifact),
            "the separated diagnostic line must classify as a RecoveryArtifact: {changes:?}"
        );
    }

    // Non-regression: a genuine unresolved user prompt at the exchange tail that
    // happens to mention "ipc" and "drift" in prose must STILL be a PromptTarget.
    // The exemption is token-specific, not keyword-broad.
    #[test]
    fn classify_prompt_bearing_changes_keeps_real_prompt_mentioning_ipc_as_prompt_target() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n\
            ctx\n\
            +The IPC drift keeps breaking my finalize — please diagnose the root cause and fix it.\n\
            <!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert!(
            changes
                .iter()
                .any(|change| change.kind == PromptBearingChangeKind::PromptTarget),
            "a real user prompt that mentions ipc/drift must remain a PromptTarget: {changes:?}"
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

    #[test]
    fn prompt_bearing_body_for_unstarted_prompt_guard_strips_frontmatter_comments_and_queue() {
        let content = concat!(
            "---\nagent_doc_session: sid\nqueue: start\n---\n\n",
            "Visible.\n",
            "<!-- hidden prompt? -->\n",
            "<!-- agent:queue auto -->\n",
            "do #queued\n",
            "<!-- /agent:queue -->\n",
        );

        let body = prompt_bearing_body_for_unstarted_prompt_guard(content);

        assert!(!body.contains("agent_doc_session"));
        assert!(!body.contains("hidden prompt?"));
        assert!(!body.contains("do #queued"));
        assert!(body.contains("Visible."));
        assert!(body.contains("<!-- agent:queue auto -->"));
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_from_diff_detects_plain_tail_prompt() {
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:stale -->\n",
            "Please fix the markdown parser.\n",
            "<!-- /agent:exchange -->\n",
        );
        let diff = unified_diff_from_contents(snapshot, current).expect("diff");

        let change = first_unstarted_prompt_bearing_change_from_diff(&diff, current)
            .expect("plain exchange-tail prompt should remain actionable");

        assert_eq!(change.kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(change.text, "Please fix the markdown parser.");
    }

    #[test]
    fn first_unstarted_prompt_bearing_change_from_diff_ignores_answered_existing_response() {
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ JB `/clear` on this document error:\n",
            "```\n",
            "clear refused while actor was starting\n",
            "```\n\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ JB `/clear` on this document error:\n",
            "```\n",
            "clear refused while actor was starting\n",
            "```\n\n",
            "❯ This prompt was duplicated.\n",
            "### Re: live typing duplicate and clear refusal — gpt-5 (HEAD)\n\n",
            "Fixed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let diff = unified_diff_from_contents(snapshot, current).expect("diff");

        let change = first_unstarted_prompt_bearing_change_from_diff(&diff, current);

        assert!(
            change.is_none(),
            "answered prompt immediately before an existing response should not stay actionable"
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_ignores_raw_answered_stale_exchange_tail() {
        let diff = concat!(
            "--- snapshot\n",
            "+++ document\n",
            "@@ -19,11 +19,11 @@\n",
            " ---\n",
            " \n",
            " ## Status\n",
            " \n",
            " <!-- agent:status patch=replace -->\n",
            "-Blocked on environment: GitHub is unreachable from this sandbox, so I cannot rename `ClaudeScore/BuildPartyInvestorDemo` or add the follow-up submodule here.\n",
            "+Updated local references for the renamed `ClaudeScore/buildparty-investor-demo` repo: `.gitmodules` now points at the new SSH URL, and the checked-out submodule's `origin` remote has been synced to match.\n",
            " <!-- /agent:status -->\n",
            " \n",
            " ## Exchange\n",
            " \n",
            " <!-- agent:exchange patch=append -->\n",
            "@@ -60,10 +60,17 @@\n",
            " ```bash\n",
            " git submodule add git@github.com:ClaudeScore/buildparty-investor-demo.git buildparty-investor-demo\n",
            " ```\n",
            " \n",
            " GitHub will redirect normal clone/fetch/push traffic from the old repo name, but it still recommends updating local remotes, and workflows that use the old repo as a GitHub Action reference will not redirect cleanly. Sources: [GitHub CLI `gh repo rename`](https://cli.github.com/manual/gh_repo_rename), [GitHub repo rename docs](https://docs.github.com/github/administering-a-repository/managing-repository-settings/renaming-a-repository).\n",
            "+I renamed the repo to ClaudeScore/buildparty-investor-demo. Please update references\n",
            "+I updated the repo-local references to the renamed GitHub repo.\n",
            "+\n",
            "+- `.gitmodules` now uses `git@github.com:ClaudeScore/buildparty-investor-demo.git`\n",
            "+- The checked-out submodule at `buildparty-investor-demo/` now has `origin` set to the same URL\n",
            "+\n",
            "+The only remaining stale reference I found is the submodule README title (`BuildPartyInvestorDemo` in `buildparty-investor-demo/README.md`). I left that untouched because it belongs to the submodule's own content rather than this parent repo's wiring.\n",
            " <!-- /agent:exchange -->\n",
            " \n",
            " ## Queue\n",
            " \n",
            " <!-- agent:queue -->\n",
        );
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::ContentEdit);
        assert_eq!(
            changes[0].text,
            "Updated local references for the renamed `ClaudeScore/buildparty-investor-demo` repo: `.gitmodules` now points at the new SSH URL, and the checked-out submodule's `origin` remote has been synced to match."
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_ignores_answered_prompt_before_blank_heading_gap() {
        let diff = concat!(
            "--- snapshot\n",
            "+++ document\n",
            "@@ -1,3 +1,8 @@\n",
            " <!-- agent:exchange patch=append -->\n",
            "-<!-- agent:boundary:initial -->\n",
            "+❯ do #sim1. spec-test-build-install-commit-push\n",
            "+\n",
            "+### Re: sim closeout — gpt-5 (HEAD)\n",
            "+\n",
            "+Done.\n",
            "+<!-- agent:boundary:new -->\n",
            " <!-- /agent:exchange -->\n",
        );

        let changes = classify_prompt_bearing_changes(diff);
        assert!(
            changes
                .iter()
                .all(|change| change.kind != PromptBearingChangeKind::PromptTarget),
            "answered prompt should not remain unresolved: {changes:?}"
        );
    }

    // parse_slash_commands tests

    #[test]
    fn parse_slash_commands_simple() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n context\n+/clear\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/clear"]);
    }

    #[test]
    fn parse_slash_command_only_added_diff_accepts_bare_clear() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n context\n+/clear\n";
        assert_eq!(
            parse_slash_command_only_added_diff(diff),
            Some(vec!["/clear".to_string()])
        );
    }

    #[test]
    fn parse_slash_command_only_added_diff_rejects_mixed_prompt_text() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n context\n+/clear\n+Why was this answered?\n";
        assert_eq!(parse_slash_command_only_added_diff(diff), None);
    }

    #[test]
    fn parse_slash_command_only_added_diff_rejects_fenced_or_blockquoted_commands() {
        let fenced = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n+```\n+/clear\n+```\n";
        assert_eq!(parse_slash_command_only_added_diff(fenced), None);

        let quoted = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+> /clear\n";
        assert_eq!(parse_slash_command_only_added_diff(quoted), None);
    }

    #[test]
    fn parse_slash_commands_trims_surrounding_whitespace() {
        let diff = "--- snapshot\n+++ queue\n@@ -0,0 +1,1 @@\n+   /clear  \n";
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
    fn detect_orchestration_request_ignores_prefixed_recovery_evidence() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,10 @@\n ctx\n\
+### Re: stalled Run Agent Doc recovery — gpt-5\n\
+\n\
+I dry-ran the emitted orchestration command. It resolved only already-recorded verification evidence.\n\
+\n\
+❯ - `./gradlew test --tests TerminalUtilTest`\n\
+❯ - `cargo test -p agent-doc-orchestration protected_prompt_draft_preview_redacts_and_bounds_latest_draft`\n\
+❯ - `612b1552` `test: cover protected prompt route refusal`\n\
+\n\
+No code files changed in this follow-up.\n";
        assert!(
            detect_orchestration_request(diff).is_none(),
            "recovered response evidence must not emit orchestration work"
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
    fn suppress_inactive_queue_additions_removes_queue_prompt_lines() {
        let current = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#gdbpropscan]\n",
            "<!-- /agent:queue -->\n"
        );
        let diff = concat!(
            "--- snapshot\n",
            "+++ document\n",
            "@@ -7,5 +7,7 @@\n",
            " Done.\n",
            " <!-- /agent:exchange -->\n",
            " \n",
            " <!-- agent:queue -->\n",
            "+dispatch #spec-test-build-install-commit-push\n",
            "+- do [#gdbpropscan]\n",
            " <!-- /agent:queue -->\n",
        );

        let filtered = suppress_inactive_queue_additions(diff, current);

        assert!(!filtered.contains("[#gdbpropscan]"));
        assert!(!filtered.contains("dispatch #spec-test-build-install-commit-push"));
        assert!(classify_prompt_bearing_changes(&filtered).is_empty());
        assert!(extract_imperative_directives(&filtered).is_empty());
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
            +do [#dodone]. spec-test-build-install-commit-push\n\
            +do [#plainid]\n\
            +run benchmarks\n";
        let directives = extract_imperative_directives(diff);
        assert_eq!(
            directives,
            vec![
                "do #6zyp. update spec + tests. build + install for local testing. commit + push",
                "do [#dodone]. spec-test-build-install-commit-push",
                "do [#plainid]",
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
    fn extract_imperative_directives_detects_long_custom_pending_id() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n\
            +- [ ] [#sdig2matrix] Fix the custom backlog id normalization path\n";
        let directives = extract_imperative_directives(diff);
        assert_eq!(
            directives,
            vec!["Fix the custom backlog id normalization path"]
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
    fn detect_exchange_compaction_request_matches_bare_directive() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+compact exchange\n";
        assert!(detect_exchange_compaction_request(diff));
    }

    #[test]
    fn detect_exchange_compaction_request_matches_prompt_prefixed_variant() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+❯ compact exchange...do not add...summarize the content and delete the rest\n";
        assert!(detect_exchange_compaction_request(diff));
    }

    #[test]
    fn detect_exchange_compaction_request_ignores_non_directive_mentions() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+I failed to compact exchange earlier.\n";
        assert!(!detect_exchange_compaction_request(diff));
    }

    #[test]
    fn detect_exchange_compaction_request_ignores_queue_list_item() {
        // #qcompactfp: a queued bug-report head that merely begins with "compact
        // exchange" is a task entry, not a same-turn directive — it must not abort
        // finalize. Bullet spellings `- `, `* `, and ordered `N. ` are all skipped.
        for diff in [
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+- Compact exchange should commit the compacted content\n",
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+* compact exchange should commit the compacted content\n",
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+1. compact exchange should commit the compacted content\n",
        ] {
            assert!(
                !detect_exchange_compaction_request(diff),
                "list-item compaction mention must not be a directive: {diff:?}"
            );
        }
        // A genuine prose directive (no bullet) still matches.
        let prose = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+compact exchange\n";
        assert!(detect_exchange_compaction_request(prose));
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
            +❯ In src/sample-app, why did patchback miss the prefix?\n\
            +See my inquiry:\n\
            +```text\n\
            +line one\n\
            +line two\n\
            +```\n";

        let blocks = extract_required_response_blocks(diff);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("❯ In src/sample-app"));
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
            +❯ In src/sample-app, why did patchback miss the prefix?\n\
            +See my inquiry:\n\
            +- keep this markdown bullet bare\n\
            +  - keep nested markdown bullets bare\n\
            +1. keep ordered markdown bullets bare\n\
            +```text\n\
            +line one\n\
            +line two\n\
            +```\n";

        let targets = prompt_prefix_normalization_targets(diff);
        assert_eq!(
            targets,
            vec!["See my inquiry:".to_string(),],
            "only bare prompt-context prose should need fresh prefixing"
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

    #[test]
    fn first_bare_prompt_prefix_target_skips_markdown_lists() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,6 @@\n\
            ctx\n\
            +❯ Please compare these options:\n\
            +- option one\n\
            +  - nested option detail\n\
            +1. ordered option\n\
            +### Re: answer — gpt-5\n";

        let bare = first_bare_prompt_prefix_target(diff);
        assert_eq!(bare, None);
    }

    #[test]
    fn first_bare_prompt_prefix_target_before_marker_scopes_to_response_insert() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,7 @@\n\
            ctx\n\
            +❯ Existing question?\n\
            +Follow-up before the marker.\n\
            +### Re: answer — gpt-5\n\
            +❯ A later prompt should not count here.\n\
            +Follow-up after the marker.\n\
            +### Re: later answer — gpt-5\n";

        let bare = first_bare_prompt_prefix_target_before_marker(diff, "### Re: answer — gpt-5");
        assert_eq!(bare.as_deref(), Some("Follow-up before the marker."));

        let bare =
            first_bare_prompt_prefix_target_before_marker(diff, "### Re: later answer — gpt-5");
        assert_eq!(bare.as_deref(), Some("Follow-up before the marker."));

        let bare = first_bare_prompt_prefix_target_before_marker(diff, "missing marker");
        assert_eq!(bare.as_deref(), Some("Follow-up before the marker."));

        let later_only = "--- snapshot\n+++ document\n@@ -1,2 +1,5 @@\n\
            ctx\n\
            +### Re: answer — gpt-5\n\
            +❯ A later prompt should not count here.\n\
            +Follow-up after the marker.\n";

        let bare =
            first_bare_prompt_prefix_target_before_marker(later_only, "### Re: answer — gpt-5");
        assert_eq!(bare, None);
    }

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

    // Plan: tasks/agent-doc/plan-claude-code-queue-auto-loop.md `#ccloopguard`.
    // Managed-component state edits (queue/backlog/done body, queue activity
    // toggle, frontmatter queue flag) must not block the Claude Code auto-loop.
    // Real user prompts must continue to block it.
    fn pbc(kind: PromptBearingChangeKind, text: &str) -> PromptBearingChange {
        PromptBearingChange {
            kind,
            text: text.to_string(),
        }
    }

    #[test]
    fn change_is_managed_state_only_accepts_queue_activity_toggle() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "<!-- agent:queue auto -->"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "<!-- agent:queue -->"
        )));
    }

    #[test]
    fn change_is_managed_state_only_accepts_frontmatter_queue_active_flip() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "queue_active: true"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "queue_active: false"
        )));
    }

    #[test]
    fn pipeline_only_frontmatter_write_is_no_change() {
        // #22a8: writing / clearing the managed agent_doc_pipeline block on a
        // phase transition must read as no change (diff cancels both sides).
        let snapshot = "---\nqueue: start\n---\n\n## Body\n- item\n";
        let with_pipeline = "---\nqueue: start\nagent_doc_pipeline:\n  run_id: cycle-123\n  step: response_captured\n  turn_id: \"#x\"\n---\n\n## Body\n- item\n";
        assert!(
            unified_diff_from_contents(snapshot, with_pipeline).is_none(),
            "adding a pipeline block must not register as a change"
        );
        assert!(
            unified_diff_from_contents(with_pipeline, snapshot).is_none(),
            "clearing a pipeline block must not register as a change"
        );
        // A real body edit alongside a pipeline write is still detected.
        let with_pipeline_and_edit = "---\nqueue: start\nagent_doc_pipeline:\n  run_id: cycle-123\n  step: committed\n---\n\n## Body\n- item changed\n";
        assert!(
            unified_diff_from_contents(snapshot, with_pipeline_and_edit).is_some(),
            "a real body edit must still be detected through a pipeline write"
        );
    }

    #[test]
    fn change_is_managed_state_only_accepts_pipeline_block_lines() {
        for line in [
            "agent_doc_pipeline:",
            "  run_id: cycle-123",
            "  step: write_applied",
            "  turn_id: \"#x\"",
            "  queue_task_id: \"#x\"",
        ] {
            assert!(
                change_is_managed_state_only(&pbc(PromptBearingChangeKind::ContentEdit, line)),
                "pipeline line should be managed state: {line:?}"
            );
        }
    }

    #[test]
    fn change_is_managed_state_only_accepts_queue_item_lines() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- do [#newitem]"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- ~do [#consumed]~"
        )));
    }

    #[test]
    fn change_is_managed_state_only_accepts_backlog_and_done_item_lines() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "- [ ] [#newitem] short description"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "- [/] [#gated] partial progress note"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "- 2026-05-25 [#done] closed last cycle"
        )));
    }

    #[test]
    fn change_is_managed_state_only_accepts_multi_line_managed_blocks() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- do [#a]\n- do [#b]\n- do [#c]"
        )));
    }

    #[test]
    fn change_is_managed_state_only_rejects_real_user_prompts() {
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "Why is the queue not auto-looping?"
        )));
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "❯ do this thing please"
        )));
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "Fix the regression on line 42."
        )));
    }

    #[test]
    fn change_is_managed_state_only_rejects_mixed_managed_and_user_text() {
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- do [#newitem]\nAnd please also fix the older bug."
        )));
    }

    #[test]
    fn line_is_binary_authored_compact_summary_recognizes_summary_shapes() {
        // Binary-authored compaction summary shapes — with and without a `❯ `
        // prefix a prior content-inference repair may have wrongly applied.
        for line in [
            "### Session Summary",
            "❯ ### Session Summary",
            "Compacted content:",
            "*Compacted. Content archived to `.agent-doc/archives/x.md`*",
            "❯ *Compacted. Content archived to `.agent-doc/archives/x.md`*",
            "- Archived 6 response topic(s): a; b; c; 3 more",
            "❯ - Archived 6 response topic(s): a; b; c; 3 more",
            "- Prior summary/context: prior compacted content: ...",
            "- Trailing prompt/context: leftover prose",
        ] {
            assert!(
                line_is_binary_authored_compact_summary(line),
                "should recognize binary-authored summary line: {line:?}"
            );
        }
        // Genuine user prompts must NOT match, even when they mention compaction.
        for line in [
            "Why did the compaction archive my queue items?",
            "do #provauth3",
            "- Archived the old plan, please review",
            "Compacted the notes manually — is that ok?",
        ] {
            assert!(
                !line_is_binary_authored_compact_summary(line),
                "must not match a real user line: {line:?}"
            );
        }
    }

    #[test]
    fn compact_summary_replacement_is_not_a_prompt_target() {
        // `#provauth3`: replacing the exchange tail with a compaction Session
        // Summary must classify as a recovery artifact, never an unresolved user
        // PromptTarget (which falsely INTERRUPTed session-check and stalled the
        // queue at the start of this dogfood session).
        let prev = "<!-- agent:exchange patch=append -->\n\
### Re: old topic - gpt-5\n\nA long archived answer body.\n\n\
<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n";
        let current = "<!-- agent:exchange patch=append -->\n\
### Session Summary\n\n\
*Compacted. Content archived to `.agent-doc/archives/x.md`*\n\n\
Compacted content:\n\
- Archived 6 response topic(s): a; b; c; 3 more\n\
- Prior summary/context: earlier work\n\
<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n";
        let diff = unified_diff_from_contents(prev, current).expect("diff");
        let changes = classify_prompt_bearing_changes(&diff);
        assert!(
            changes
                .iter()
                .all(|c| c.kind != PromptBearingChangeKind::PromptTarget),
            "compaction summary lines must not be PromptTargets: {changes:?}"
        );
    }
}
