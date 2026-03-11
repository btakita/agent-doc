use anyhow::{bail, Result};

/// A parsed component in a document.
///
/// Components are bounded regions marked by `<!-- agent:name -->...<!-- /agent:name -->`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    /// Byte offset of `<` in opening marker.
    pub open_start: usize,
    /// Byte offset past `>` in opening marker (includes trailing newline if present).
    pub open_end: usize,
    /// Byte offset of `<` in closing marker.
    pub close_start: usize,
    /// Byte offset past `>` in closing marker (includes trailing newline if present).
    pub close_end: usize,
}

impl Component {
    /// Extract the content between the opening and closing markers.
    #[allow(dead_code)] // public API — used by tests and future consumers
    pub fn content<'a>(&self, doc: &'a str) -> &'a str {
        &doc[self.open_end..self.close_start]
    }

    /// Replace the content between markers, returning the new document.
    /// The markers themselves are preserved.
    pub fn replace_content(&self, doc: &str, new_content: &str) -> String {
        let mut result = String::with_capacity(doc.len() + new_content.len());
        result.push_str(&doc[..self.open_end]);
        result.push_str(new_content);
        result.push_str(&doc[self.close_start..]);
        result
    }
}

/// Valid name: `[a-zA-Z0-9][a-zA-Z0-9-]*`
fn is_valid_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.as_bytes()[0];
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    name.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// True if the text inside `<!-- ... -->` is an agent component marker.
///
/// Matches `agent:NAME` (open) or `/agent:NAME` (close).
pub fn is_agent_marker(comment_text: &str) -> bool {
    let trimmed = comment_text.trim();
    if let Some(rest) = trimmed.strip_prefix("/agent:") {
        is_valid_name(rest)
    } else if let Some(rest) = trimmed.strip_prefix("agent:") {
        is_valid_name(rest)
    } else {
        false
    }
}

/// Find byte ranges of code regions (fenced code blocks + inline code spans).
/// Markers inside these ranges are treated as literal text, not component markers.
pub(crate) fn find_code_ranges(doc: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let bytes = doc.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Fenced code blocks: line starting with ``` or ~~~
        if (pos == 0 || bytes[pos - 1] == b'\n') && pos + 3 <= len {
            let fence_char = bytes[pos];
            if (fence_char == b'`' || fence_char == b'~')
                && bytes[pos + 1] == fence_char
                && bytes[pos + 2] == fence_char
            {
                let block_start = pos;
                // Skip past the opening fence line
                pos = memchr_byte(b'\n', bytes, pos).map_or(len, |p| p + 1);
                // Find closing fence
                loop {
                    if pos >= len {
                        ranges.push((block_start, len));
                        break;
                    }
                    if pos + 3 <= len
                        && bytes[pos] == fence_char
                        && bytes[pos + 1] == fence_char
                        && bytes[pos + 2] == fence_char
                    {
                        let end = memchr_byte(b'\n', bytes, pos).map_or(len, |p| p + 1);
                        ranges.push((block_start, end));
                        pos = end;
                        break;
                    }
                    pos = memchr_byte(b'\n', bytes, pos).map_or(len, |p| p + 1);
                }
                continue;
            }
        }

        // Inline code spans: `...`
        if bytes[pos] == b'`' && (pos + 1 < len && bytes[pos + 1] != b'`') {
            let span_start = pos;
            pos += 1;
            // Find closing backtick (not escaped, not newline-terminated)
            while pos < len && bytes[pos] != b'`' && bytes[pos] != b'\n' {
                pos += 1;
            }
            if pos < len && bytes[pos] == b'`' {
                ranges.push((span_start, pos + 1));
                pos += 1;
                continue;
            }
            // No closing backtick found on same line — not a code span
            continue;
        }

        pos += 1;
    }

    ranges
}

fn memchr_byte(needle: u8, haystack: &[u8], start: usize) -> Option<usize> {
    haystack[start..].iter().position(|&b| b == needle).map(|i| start + i)
}

/// Parse all components from a document.
///
/// Uses a stack for nesting. Returns components sorted by `open_start`.
/// Errors on unmatched open/close markers or invalid names.
/// Skips markers inside fenced code blocks and inline code spans.
pub fn parse(doc: &str) -> Result<Vec<Component>> {
    let bytes = doc.as_bytes();
    let len = bytes.len();
    let code_ranges = find_code_ranges(doc);
    let mut templates: Vec<Component> = Vec::new();
    // Stack of (name, open_start, open_end)
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut pos = 0;

    while pos + 4 <= len {
        // Look for `<!--`
        if &bytes[pos..pos + 4] != b"<!--" {
            pos += 1;
            continue;
        }

        // Skip markers inside code regions
        if code_ranges.iter().any(|&(start, end)| pos >= start && pos < end) {
            pos += 4;
            continue;
        }

        let marker_start = pos;

        // Find closing `-->`
        let close = match find_comment_end(bytes, pos + 4) {
            Some(c) => c,
            None => {
                pos += 4;
                continue;
            }
        };

        // close points to the byte after `>`
        let inner = &doc[marker_start + 4..close - 3]; // between `<!--` and `-->`
        let trimmed = inner.trim();

        // Determine end offset — consume trailing newline if present
        let mut marker_end = close;
        if marker_end < len && bytes[marker_end] == b'\n' {
            marker_end += 1;
        }

        if let Some(name) = trimmed.strip_prefix("/agent:") {
            // Closing marker
            if !is_valid_name(name) {
                bail!("invalid component name: '{}'", name);
            }
            match stack.pop() {
                Some((open_name, open_start, open_end)) => {
                    if open_name != name {
                        bail!(
                            "mismatched component: opened '{}' but closed '{}'",
                            open_name,
                            name
                        );
                    }
                    templates.push(Component {
                        name: name.to_string(),
                        open_start,
                        open_end,
                        close_start: marker_start,
                        close_end: marker_end,
                    });
                }
                None => bail!("closing marker <!-- /agent:{} --> without matching open", name),
            }
        } else if let Some(name) = trimmed.strip_prefix("agent:") {
            // Opening marker
            if !is_valid_name(name) {
                bail!("invalid component name: '{}'", name);
            }
            stack.push((name.to_string(), marker_start, marker_end));
        }

        pos = close;
    }

    if let Some((name, _, _)) = stack.last() {
        bail!(
            "unclosed component: <!-- agent:{} --> without matching close",
            name
        );
    }

    templates.sort_by_key(|t| t.open_start);
    Ok(templates)
}

/// Find the end of an HTML comment (`-->`), returning byte offset past `>`.
fn find_comment_end(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = start;
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            return Some(i + 3);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_range() {
        let doc = "before\n<!-- agent:status -->\nHello\n<!-- /agent:status -->\nafter\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "status");
        assert_eq!(ranges[0].content(doc), "Hello\n");
    }

    #[test]
    fn nested_ranges() {
        let doc = "\
<!-- agent:outer -->
<!-- agent:inner -->
content
<!-- /agent:inner -->
<!-- /agent:outer -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 2);
        // Sorted by open_start — outer first
        assert_eq!(ranges[0].name, "outer");
        assert_eq!(ranges[1].name, "inner");
        assert_eq!(ranges[1].content(doc), "content\n");
    }

    #[test]
    fn siblings() {
        let doc = "\
<!-- agent:a -->
alpha
<!-- /agent:a -->
<!-- agent:b -->
beta
<!-- /agent:b -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].name, "a");
        assert_eq!(ranges[0].content(doc), "alpha\n");
        assert_eq!(ranges[1].name, "b");
        assert_eq!(ranges[1].content(doc), "beta\n");
    }

    #[test]
    fn no_ranges() {
        let doc = "# Just a document\n\nWith no range templates.\n";
        let ranges = parse(doc).unwrap();
        assert!(ranges.is_empty());
    }

    #[test]
    fn unmatched_open_error() {
        let doc = "<!-- agent:orphan -->\nContent\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("unclosed component"));
    }

    #[test]
    fn unmatched_close_error() {
        let doc = "Content\n<!-- /agent:orphan -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("without matching open"));
    }

    #[test]
    fn mismatched_names_error() {
        let doc = "<!-- agent:foo -->\n<!-- /agent:bar -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("mismatched"));
    }

    #[test]
    fn invalid_name() {
        let doc = "<!-- agent:-bad -->\n<!-- /agent:-bad -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("invalid component name"));
    }

    #[test]
    fn name_validation() {
        assert!(is_valid_name("status"));
        assert!(is_valid_name("my-section"));
        assert!(is_valid_name("a1"));
        assert!(is_valid_name("A"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("-bad"));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name("has_underscore"));
    }

    #[test]
    fn content_extraction() {
        let doc = "<!-- agent:x -->\nfoo\nbar\n<!-- /agent:x -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges[0].content(doc), "foo\nbar\n");
    }

    #[test]
    fn replace_roundtrip() {
        let doc = "before\n<!-- agent:s -->\nold\n<!-- /agent:s -->\nafter\n";
        let ranges = parse(doc).unwrap();
        let new_doc = ranges[0].replace_content(doc, "new\n");
        assert_eq!(
            new_doc,
            "before\n<!-- agent:s -->\nnew\n<!-- /agent:s -->\nafter\n"
        );
        // Re-parse should work
        let ranges2 = parse(&new_doc).unwrap();
        assert_eq!(ranges2.len(), 1);
        assert_eq!(ranges2[0].content(&new_doc), "new\n");
    }

    #[test]
    fn is_agent_marker_yes() {
        assert!(is_agent_marker(" agent:status "));
        assert!(is_agent_marker("/agent:status"));
        assert!(is_agent_marker("agent:my-thing"));
        assert!(is_agent_marker(" /agent:A1 "));
    }

    #[test]
    fn is_agent_marker_no() {
        assert!(!is_agent_marker("just a comment"));
        assert!(!is_agent_marker("agent:"));
        assert!(!is_agent_marker("/agent:"));
        assert!(!is_agent_marker("agent:-bad"));
        assert!(!is_agent_marker("some agent:fake stuff"));
    }

    #[test]
    fn regular_comments_ignored() {
        let doc = "<!-- just a comment -->\n<!-- agent:x -->\ndata\n<!-- /agent:x -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn multiline_comment_ignored() {
        let doc = "\
<!--
multi
line
comment
-->
<!-- agent:s -->
content
<!-- /agent:s -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "s");
    }

    #[test]
    fn empty_content() {
        let doc = "<!-- agent:empty --><!-- /agent:empty -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].content(doc), "");
    }

    #[test]
    fn markers_in_fenced_code_block_ignored() {
        let doc = "\
<!-- agent:real -->
content
<!-- /agent:real -->
```markdown
<!-- agent:fake -->
this is just an example
<!-- /agent:fake -->
```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "real");
    }

    #[test]
    fn markers_in_inline_code_ignored() {
        let doc = "\
Use `<!-- agent:example -->` markers for components.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "real");
    }

    #[test]
    fn markers_in_tilde_fence_ignored() {
        let doc = "\
<!-- agent:x -->
data
<!-- /agent:x -->
~~~
<!-- agent:y -->
example
<!-- /agent:y -->
~~~
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn code_ranges_detected() {
        let doc = "before\n```\ncode\n```\nafter `inline` end\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 2);
        // Fenced block
        assert!(doc[ranges[0].0..ranges[0].1].contains("code"));
        // Inline span
        assert!(doc[ranges[1].0..ranges[1].1].contains("inline"));
    }
}
