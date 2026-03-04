use anyhow::Result;
use similar::{ChangeTag, TextDiff};
use std::path::Path;

use crate::{component, snapshot};

/// Strip comments from document content for diff comparison.
///
/// Removes:
/// - HTML comments `<!-- ... -->` (single and multiline) — EXCEPT agent range markers
/// - Link reference comments `[//]: # (...)`
pub fn strip_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Check for link reference comment: `[//]: # (...)`
        if bytes[pos] == b'['
            && is_line_start(bytes, pos)
            && let Some(end) = match_link_ref_comment(bytes, pos)
        {
            pos = end;
            continue;
        }

        // Check for HTML comment: `<!-- ... -->`
        if pos + 4 <= len
            && &bytes[pos..pos + 4] == b"<!--"
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
    let previous = snapshot::load(doc)?.unwrap_or_default();

    let current_stripped = strip_comments(&current);
    let previous_stripped = strip_comments(&previous);

    let diff = TextDiff::from_lines(&previous_stripped, &current_stripped);
    let has_changes = diff
        .iter_all_changes()
        .any(|c| c.tag() != ChangeTag::Equal);

    if !has_changes {
        return Ok(None);
    }

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
}
