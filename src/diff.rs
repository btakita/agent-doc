use anyhow::Result;
use similar::{ChangeTag, TextDiff};
use std::path::Path;

use crate::{component, snapshot};

/// Strip comments from document content for diff comparison.
///
/// Removes:
/// - HTML comments `<!-- ... -->` (single and multiline) — EXCEPT agent range markers
/// - Link reference comments `[//]: # (...)`
///
/// Skips `<!--` sequences inside fenced code blocks and inline backtick spans
/// to prevent code examples containing `<!--` from being misinterpreted as
/// comment starts.
pub fn strip_comments(content: &str) -> String {
    let code_ranges = component::find_code_ranges(content);
    let in_code = |pos: usize| code_ranges.iter().any(|&(start, end)| pos >= start && pos < end);

    let mut result = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Check for link reference comment: `[//]: # (...)`
        if bytes[pos] == b'['
            && !in_code(pos)
            && is_line_start(bytes, pos)
            && let Some(end) = match_link_ref_comment(bytes, pos)
        {
            pos = end;
            continue;
        }

        // Check for HTML comment: `<!-- ... -->`
        if pos + 4 <= len
            && &bytes[pos..pos + 4] == b"<!--"
            && !in_code(pos)
            && let Some((end, inner)) = match_html_comment(content, pos)
        {
            if component::is_agent_marker(inner) {
                // Preserve agent markers — copy them through
                result.push_str(&content[pos..end]);
                pos = end;
            } else {
                // Strip the comment (and trailing newline if on its own line)
                let mut skip_to = end;
                if is_line_start(bytes, pos) && skip_to < len && bytes[skip_to] == b'\n' {
                    skip_to += 1;
                }
                pos = skip_to;
            }
            continue;
        }

        result.push(content[pos..].chars().next().unwrap());
        pos += content[pos..].chars().next().unwrap().len_utf8();
    }

    result
}

/// True if `pos` is at the start of a line (pos == 0 or bytes[pos-1] == '\n').
fn is_line_start(bytes: &[u8], pos: usize) -> bool {
    pos == 0 || bytes[pos - 1] == b'\n'
}

/// Match `[//]: # (...)` starting at `pos`. Returns byte offset past the line end.
fn match_link_ref_comment(bytes: &[u8], pos: usize) -> Option<usize> {
    let prefix = b"[//]: # (";
    let len = bytes.len();
    if pos + prefix.len() > len {
        return None;
    }
    if &bytes[pos..pos + prefix.len()] != prefix {
        return None;
    }
    // Find closing `)` then end of line
    let mut i = pos + prefix.len();
    while i < len && bytes[i] != b')' && bytes[i] != b'\n' {
        i += 1;
    }
    if i < len && bytes[i] == b')' {
        i += 1; // past `)`
        if i < len && bytes[i] == b'\n' {
            i += 1; // consume newline
        }
        Some(i)
    } else {
        None
    }
}

/// Match `<!-- ... -->` starting at `pos`. Returns (end_offset, inner_text).
fn match_html_comment(content: &str, pos: usize) -> Option<(usize, &str)> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = pos + 4; // past `<!--`
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            let inner = &content[pos + 4..i];
            return Some((i + 3, inner));
        }
        i += 1;
    }
    None
}

/// Compute a unified diff between the snapshot and the current document.
/// Returns None if there are no changes.
///
/// Both snapshot and current content are comment-stripped before comparison.
pub fn compute(doc: &Path) -> Result<Option<String>> {
    let current = std::fs::read_to_string(doc)?;
    let snap_path = snapshot::path_for(doc)?;
    let previous = snapshot::resolve(doc)?.unwrap_or_default();

    eprintln!(
        "[diff] doc={} snapshot={} doc_len={} snap_len={}",
        doc.display(),
        snap_path.display(),
        current.len(),
        previous.len(),
    );

    let current_stripped = strip_comments(&current);
    let previous_stripped = strip_comments(&previous);

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
        return Ok(None);
    }

    // Stale snapshot recovery: if the diff is only completed assistant/user
    // exchanges with no new user content, the previous cycle wrote the response
    // but context compaction prevented the snapshot update.
    if is_stale_snapshot(&previous, &current) {
        eprintln!("[snapshot recovery] Snapshot synced — previous cycle completed but snapshot was stale");
        snapshot::save(doc, &current)?;
        return Ok(None);
    }

    eprintln!("[diff] changes detected, computing unified diff");

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
    Ok(Some(output))
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
pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    match compute(file)? {
        Some(diff) => print!("{}", diff),
        None => eprintln!("No changes since last submit."),
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
        let err = run(Path::new("/nonexistent/file.md")).unwrap_err();
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
}
