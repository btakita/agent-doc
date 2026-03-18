use anyhow::{bail, Result};
use std::collections::HashMap;

/// A parsed component in a document.
///
/// Components are bounded regions marked by `<!-- agent:name -->...<!-- /agent:name -->`.
/// Opening tags may contain inline attributes: `<!-- agent:name key=value -->`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    /// Inline attributes parsed from the opening tag (e.g., `patch=append`).
    pub attrs: HashMap<String, String>,
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

    /// Get the patch mode from inline attributes.
    ///
    /// Checks `patch=` first, falls back to `mode=` for backward compatibility.
    pub fn patch_mode(&self) -> Option<&str> {
        self.attrs.get("patch").map(|s| s.as_str())
            .or_else(|| self.attrs.get("mode").map(|s| s.as_str()))
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
/// Matches `agent:NAME [attrs...]` (open) or `/agent:NAME` (close).
pub fn is_agent_marker(comment_text: &str) -> bool {
    let trimmed = comment_text.trim();
    if let Some(rest) = trimmed.strip_prefix("/agent:") {
        is_valid_name(rest)
    } else if let Some(rest) = trimmed.strip_prefix("agent:") {
        // Opening marker may have attributes after the name: `agent:NAME key=value`
        let name_part = rest.split_whitespace().next().unwrap_or("");
        is_valid_name(name_part)
    } else {
        false
    }
}

/// Parse `key=value` pairs from the attribute portion of an opening marker.
///
/// Given the text after `agent:NAME `, parses space-separated `key=value` pairs.
/// Values are unquoted (no quote support needed for simple mode values).
fn parse_attrs(attr_text: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    for token in attr_text.split_whitespace() {
        if let Some((key, value)) = token.split_once('=')
            && !key.is_empty()
            && !value.is_empty()
        {
            attrs.insert(key.to_string(), value.to_string());
        }
    }
    attrs
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

        // Inline code spans: `...`, ``...``, ```...``` (CommonMark §6.1)
        // A code span begins with N backticks and ends with exactly N backticks.
        if bytes[pos] == b'`' {
            let span_start = pos;
            // Count opening backticks
            let mut n = 0;
            while pos + n < len && bytes[pos + n] == b'`' {
                n += 1;
            }
            // Skip if this is a fenced code block (3+ backticks at line start)
            // — already handled above
            if n >= 3 && (span_start == 0 || bytes[span_start - 1] == b'\n') {
                pos += n;
                continue;
            }
            pos += n;
            // Find closing sequence of exactly N backticks (not more, not fewer)
            loop {
                if pos >= len {
                    break;
                }
                // Inline code spans cannot span newlines in our context
                if bytes[pos] == b'\n' {
                    break;
                }
                if bytes[pos] == b'`' {
                    let mut close_n = 0;
                    while pos + close_n < len && bytes[pos + close_n] == b'`' {
                        close_n += 1;
                    }
                    if close_n == n {
                        ranges.push((span_start, pos + close_n));
                        pos += close_n;
                        break;
                    }
                    // Wrong number of backticks — skip past them
                    pos += close_n;
                    continue;
                }
                pos += 1;
            }
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
    // Stack of (name, attrs, open_start, open_end)
    let mut stack: Vec<(String, HashMap<String, String>, usize, usize)> = Vec::new();
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
                Some((open_name, open_attrs, open_start, open_end)) => {
                    if open_name != name {
                        bail!(
                            "mismatched component: opened '{}' but closed '{}'",
                            open_name,
                            name
                        );
                    }
                    templates.push(Component {
                        name: name.to_string(),
                        attrs: open_attrs,
                        open_start,
                        open_end,
                        close_start: marker_start,
                        close_end: marker_end,
                    });
                }
                None => bail!("closing marker <!-- /agent:{} --> without matching open", name),
            }
        } else if let Some(rest) = trimmed.strip_prefix("agent:") {
            // Opening marker — may have attributes: `agent:NAME key=value`
            let mut parts = rest.splitn(2, |c: char| c.is_whitespace());
            let name = parts.next().unwrap_or("");
            let attr_text = parts.next().unwrap_or("");
            if !is_valid_name(name) {
                bail!("invalid component name: '{}'", name);
            }
            let attrs = parse_attrs(attr_text);
            stack.push((name.to_string(), attrs, marker_start, marker_end));
        }

        pos = close;
    }

    if let Some((name, _, _, _)) = stack.last() {
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

    #[test]
    fn code_ranges_double_backtick() {
        // CommonMark: `` `<!--` `` is a code span containing `<!--`
        let doc = "text `` `<!--` `` more\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let span = &doc[ranges[0].0..ranges[0].1];
        assert!(span.contains("<!--"), "double-backtick span should contain <!--: {:?}", span);
    }

    #[test]
    fn code_ranges_double_backtick_does_not_match_single() {
        // `` should not match a single ` close
        let doc = "text `` foo ` bar `` end\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let span = &doc[ranges[0].0..ranges[0].1];
        assert_eq!(span, "`` foo ` bar ``");
    }

    #[test]
    fn double_backtick_comment_before_agent_marker() {
        // Regression: `` `<!--` `` followed by agent marker should not be a huge comment
        let doc = "\
<!-- agent:exchange -->\n\
text `` `<!--` `` description\n\
new content here\n\
<!-- /agent:exchange -->\n";
        let stripped = crate::diff::strip_comments(doc);
        assert!(stripped.contains("new content here"), "content must survive stripping");
        assert!(stripped.contains("<!-- agent:exchange -->"), "agent markers must survive");
    }

    // --- Inline attribute tests ---

    #[test]
    fn parse_component_with_mode_attr() {
        let doc = "<!-- agent:exchange mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert_eq!(components[0].attrs.get("mode").map(|s| s.as_str()), Some("append"));
        assert_eq!(components[0].content(doc), "Content\n");
    }

    #[test]
    fn parse_component_with_multiple_attrs() {
        let doc = "<!-- agent:log mode=prepend timestamp=true -->\nData\n<!-- /agent:log -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "log");
        assert_eq!(components[0].attrs.get("mode").map(|s| s.as_str()), Some("prepend"));
        assert_eq!(components[0].attrs.get("timestamp").map(|s| s.as_str()), Some("true"));
    }

    #[test]
    fn parse_component_no_attrs_backward_compat() {
        let doc = "<!-- agent:status -->\nOK\n<!-- /agent:status -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "status");
        assert!(components[0].attrs.is_empty());
    }

    #[test]
    fn is_agent_marker_with_attrs() {
        assert!(is_agent_marker(" agent:exchange mode=append "));
        assert!(is_agent_marker("agent:status mode=replace"));
        assert!(is_agent_marker("agent:log mode=prepend timestamp=true"));
    }

    #[test]
    fn closing_tag_unchanged_with_attrs() {
        // Closing tags never have attributes
        let doc = "<!-- agent:status mode=replace -->\n- [x] Done\n<!-- /agent:status -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        let new_doc = components[0].replace_content(doc, "- [ ] Todo\n");
        assert!(new_doc.contains("<!-- agent:status mode=replace -->"));
        assert!(new_doc.contains("<!-- /agent:status -->"));
        assert!(new_doc.contains("- [ ] Todo"));
    }

    #[test]
    fn parse_component_with_patch_attr() {
        let doc = "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert_eq!(components[0].patch_mode(), Some("append"));
        assert_eq!(components[0].content(doc), "Content\n");
    }

    #[test]
    fn patch_attr_takes_precedence_over_mode() {
        let doc = "<!-- agent:exchange patch=replace mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), Some("replace"));
    }

    #[test]
    fn mode_attr_backward_compat() {
        let doc = "<!-- agent:exchange mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), Some("append"));
    }

    #[test]
    fn no_patch_or_mode_attr() {
        let doc = "<!-- agent:exchange -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), None);
    }

    #[test]
    fn parse_attrs_unit() {
        let attrs = parse_attrs("mode=append");
        assert_eq!(attrs.get("mode").map(|s| s.as_str()), Some("append"));

        let attrs = parse_attrs("mode=replace timestamp=true");
        assert_eq!(attrs.len(), 2);

        let attrs = parse_attrs("");
        assert!(attrs.is_empty());

        // Malformed tokens without = are ignored
        let attrs = parse_attrs("mode=append broken novalue=");
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs.get("mode").map(|s| s.as_str()), Some("append"));
    }
}
