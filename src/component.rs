use anyhow::{bail, Result};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
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

    /// Append content into this component, inserting before the caret position
    /// if the caret is inside the component. Falls back to normal append if the
    /// caret is outside the component.
    ///
    /// `caret_offset`: byte offset of the caret in the document. Pass `None` for
    /// normal append behavior.
    pub fn append_with_caret(&self, doc: &str, content: &str, caret_offset: Option<usize>) -> String {
        let existing = &doc[self.open_end..self.close_start];

        if let Some(caret) = caret_offset {
            // Check if caret is inside this component
            if caret > self.open_end && caret <= self.close_start {
                // Find the line boundary before the caret
                let insert_at = doc[..caret].rfind('\n')
                    .map(|i| i + 1)
                    .unwrap_or(self.open_end);

                // Clamp to component bounds
                let insert_at = insert_at.max(self.open_end);

                let mut result = String::with_capacity(doc.len() + content.len() + 1);
                result.push_str(&doc[..insert_at]);
                result.push_str(content.trim_end());
                result.push('\n');
                result.push_str(&doc[insert_at..]);
                return result;
            }
        }

        // Normal append: add after existing content
        let mut result = String::with_capacity(doc.len() + content.len() + 1);
        result.push_str(&doc[..self.open_end]);
        result.push_str(existing.trim_end());
        result.push('\n');
        result.push_str(content.trim_end());
        result.push('\n');
        result.push_str(&doc[self.close_start..]);
        result
    }

    /// Append content into this component at the boundary marker position.
    ///
    /// Finds `<!-- agent:boundary:ID -->` inside the component. If found,
    /// inserts content at the line start of the boundary marker (replacing
    /// the marker). Falls back to normal append if the boundary is not found.
    pub fn append_with_boundary(&self, doc: &str, content: &str, boundary_id: &str) -> String {
        let boundary_marker = format!("<!-- agent:boundary:{} -->", boundary_id);
        let content_region = &doc[self.open_end..self.close_start];
        let code_ranges = find_code_ranges(doc);

        // Search for boundary marker, skipping matches inside code blocks
        let mut search_from = 0;
        let found_pos = loop {
            match content_region[search_from..].find(&boundary_marker) {
                Some(rel_pos) => {
                    let abs_pos = self.open_end + search_from + rel_pos;
                    if code_ranges.iter().any(|&(cs, ce)| abs_pos >= cs && abs_pos < ce) {
                        // Inside a code block — skip and keep searching
                        search_from += rel_pos + boundary_marker.len();
                        continue;
                    }
                    break Some(abs_pos);
                }
                None => break None,
            }
        };

        if let Some(abs_pos) = found_pos {
            // Find start of the line containing the marker
            let line_start = doc[..abs_pos]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(self.open_end)
                .max(self.open_end);

            // Find end of the marker line (including trailing newline)
            let marker_end = abs_pos + boundary_marker.len();
            let line_end = if marker_end < self.close_start
                && doc.as_bytes().get(marker_end) == Some(&b'\n')
            {
                marker_end + 1
            } else {
                marker_end
            };
            let line_end = line_end.min(self.close_start);

            // Replace the boundary marker with response content + new boundary.
            // The boundary is consumed and re-inserted, matching the binary's
            // post-patch behavior in apply_patches_with_overrides().
            let new_id = uuid::Uuid::new_v4();
            let new_marker = format!("<!-- agent:boundary:{} -->", new_id);
            let mut result = String::with_capacity(doc.len() + content.len() + new_marker.len());
            result.push_str(&doc[..line_start]);
            result.push_str(content.trim_end());
            result.push('\n');
            result.push_str(&new_marker);
            result.push('\n');
            result.push_str(&doc[line_end..]);
            return result;
        }

        // Boundary not found — fall back to normal append
        self.append_with_caret(doc, content, None)
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
///
/// Uses `pulldown-cmark` AST parsing with `offset_iter()` to accurately detect
/// code regions per the CommonMark spec.
pub fn find_code_ranges(doc: &str) -> Vec<(usize, usize)> {
    let t = std::time::Instant::now();
    let mut ranges = Vec::new();
    let parser = Parser::new_ext(doc, Options::empty());
    let mut iter = parser.into_offset_iter();
    while let Some((event, range)) = iter.next() {
        match event {
            // Inline code span: `code` or ``code``
            Event::Code(_) => {
                ranges.push((range.start, range.end));
            }
            // Fenced or indented code block: consume until End(CodeBlock)
            Event::Start(Tag::CodeBlock(_)) => {
                let block_start = range.start;
                let mut block_end = range.end;
                for (inner_event, inner_range) in iter.by_ref() {
                    block_end = inner_range.end;
                    if matches!(inner_event, Event::End(TagEnd::CodeBlock)) {
                        break;
                    }
                }
                ranges.push((block_start, block_end));
            }
            _ => {}
        }
    }
    let elapsed = t.elapsed().as_millis();
    if elapsed > 0 {
        eprintln!("[perf] find_code_ranges: {}ms", elapsed);
    }
    ranges
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
            // Skip boundary markers — these are not component markers
            if rest.starts_with("boundary:") {
                pos = close;
                continue;
            }
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
pub(crate) fn find_comment_end(bytes: &[u8], start: usize) -> Option<usize> {
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
    fn markers_in_indented_fenced_code_block_ignored() {
        // CommonMark allows up to 3 spaces before fence opener
        let doc = "\
<!-- agent:exchange -->
Content here.
<!-- /agent:exchange -->

  ```markdown
  <!-- agent:fake -->
  demo without closing tag
  ```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "exchange");
    }

    #[test]
    fn indented_fence_inside_component_ignored() {
        // Indented code block inside a component should not cause mismatched errors
        let doc = "\
<!-- agent:exchange -->
Here's how to set up:

   ```markdown
   <!-- agent:status -->
   Your status here
   ```

Done explaining.
<!-- /agent:exchange -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "exchange");
    }

    #[test]
    fn deeply_indented_fence_ignored() {
        // Tabs and many spaces should still be detected as a fence
        let doc = "\
<!-- agent:x -->
ok
<!-- /agent:x -->
      ```
      <!-- agent:y -->
      inside fence
      ```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn indented_fence_code_ranges_detected() {
        let doc = "before\n  ```\n  code\n  ```\nafter\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        assert!(doc[ranges[0].0..ranges[0].1].contains("code"));
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
        // Regression: `` `<!--` `` followed by agent marker should not confuse the parser
        let doc = "\
<!-- agent:exchange -->\n\
text `` `<!--` `` description\n\
new content here\n\
<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert!(components[0].content(doc).contains("new content here"));
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

    // --- Inline backtick code span exclusion tests ---

    #[test]
    fn single_backtick_component_tag_ignored() {
        // A component tag wrapped in single backticks should not be parsed
        let doc = "\
Use `<!-- agent:pending patch=replace -->` to mark pending sections.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
    }

    #[test]
    fn double_backtick_component_tag_ignored() {
        // A component tag wrapped in double backticks should not be parsed
        let doc = "\
Use ``<!-- agent:pending patch=replace -->`` to mark pending sections.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
    }

    #[test]
    fn component_tags_not_in_backticks_still_work() {
        // Tags outside of any backticks are parsed normally
        let doc = "\
<!-- agent:a -->
alpha
<!-- /agent:a -->
<!-- agent:b patch=append -->
beta
<!-- /agent:b -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].name, "a");
        assert_eq!(components[1].name, "b");
        assert_eq!(components[1].patch_mode(), Some("append"));
    }

    #[test]
    fn mixed_backtick_and_real_tags() {
        // Some tags in backticks (ignored), some not (parsed)
        let doc = "\
Here is an example: `<!-- agent:fake -->` and ``<!-- /agent:fake -->``.
<!-- agent:real -->
real content
<!-- /agent:real -->
Another example: `<!-- agent:also-fake patch=replace -->` is just documentation.
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
        assert_eq!(components[0].content(doc), "real content\n");
    }

    #[test]
    fn inline_code_mid_line_with_surrounding_text_ignored() {
        // Edge case: component tag inside inline code span on a line with other content
        // before and after — must not be parsed as a real component marker.
        let doc = "\
Wrap markers like `<!-- agent:status -->` in backticks to show them literally.
<!-- agent:real -->
actual content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
        assert_eq!(components[0].content(doc), "actual content\n");
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

    #[test]
    fn append_with_boundary_skips_code_block() {
        // Boundary marker inside a code block should be ignored;
        // the real marker outside should be used.
        let boundary_id = "real-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             user prompt\n\
             ```\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             ```\n\
             more user text\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result = comp.append_with_boundary(&doc, "### Re: Response\n\nContent here.", boundary_id);

        // Response should replace the REAL marker (outside code block),
        // not the one inside the code block.
        assert!(result.contains("### Re: Response"));
        assert!(result.contains("more user text"));
        // The code block example should be preserved
        assert!(result.contains(&format!("<!-- agent:boundary:{boundary_id} -->\n```")));
        // The real marker should be consumed (replaced by response)
        assert!(!result.contains(&format!("more user text\n<!-- agent:boundary:{boundary_id} -->\n<!-- /agent:exchange -->")));
    }

    #[test]
    fn append_with_boundary_no_code_block() {
        // Normal case: boundary marker not in a code block
        let boundary_id = "simple-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             user prompt\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result = comp.append_with_boundary(&doc, "### Re: Answer\n\nDone.", boundary_id);

        assert!(result.contains("### Re: Answer"));
        assert!(result.contains("user prompt"));
        // Original marker should be consumed, but a NEW boundary re-inserted
        assert!(!result.contains(&format!("agent:boundary:{boundary_id}")));
        assert!(result.contains("agent:boundary:"));
    }
}
