//! Pure `agent:rules` extraction.
//!
//! `agent:rules` is operator-owned turn context. A body is a markdown list,
//! free text, or both. Several blocks in one file are all kept, in file order.
//! Nothing here reads `project.md` or decides whether a turn starts.
//!
//! ## Spec
//! - A top-level list item is one rule. Markers are `- `, `* `, `+ `, `N. `,
//!   and `N) `, with at most three leading spaces. The marker token is removed
//!   and the rest of the line is kept, including intentional spaces.
//! - A continuation stays with that item only when it is indented deeper than
//!   the marker. Nested list items are continuations, not extra rules. A blank
//!   line stays inside the item when the next non-blank line is still a
//!   continuation. A following column-0 paragraph is a new free-text rule, not
//!   a lazy continuation.
//! - Free text is one rule as written, including its internal blank lines,
//!   until the next top-level list item. Leading and trailing blank lines of a
//!   body are padding and are not part of a rule.
//! - A fence (a line whose trim starts with at least three backticks or tildes)
//!   is not split. An indented fence stays with its list item. A column-0
//!   fence ends the item and starts free text. An unclosed fence runs through
//!   the rest of the body.
//! - An empty marker line (`-`, `*`, `+`, `1.`, `1)`) is skipped. Checkbox
//!   text starts at `[ ]`. `[#id]` is ordinary text. `*emphasis*` without a
//!   space after `*` is free text.
//! - [`rules_from_document`] reads every `agent:rules` body in open-marker
//!   order. A `rules` block nested inside another `rules` block is not a
//!   second component; sibling blocks are the supported form. A `rules` block
//!   inside another component kind is included. A document that does not parse
//!   yields no rules.
//! - [`compose_rules`] concatenates project rules, then session rules. It does
//!   not deduplicate and does not let the session replace the project.

use crate::element::{self, Component};

/// How to apply a non-empty `agent_rules` list on the cycle contract.
pub const AGENT_RULES_GUIDANCE: &str = "\
Apply every agent_rules entry this turn, in order. Rules from the nearest \
.agent-doc/project.md come first; session agent:rules entries add to them and \
do not replace them. A markdown list item is one rule. Free text is one rule \
as written. Content-only edits of agent:rules do not start a turn; a real \
exchange or queue turn still carries the current rules.";

/// Guidance for `rules`, present only when the composed list is non-empty.
pub fn agent_rules_guidance(rules: &[String]) -> Option<String> {
    if rules.is_empty() {
        None
    } else {
        Some(AGENT_RULES_GUIDANCE.to_string())
    }
}

/// Project rules first, then session rules. Session entries add; they do not replace.
pub fn compose_rules(project_rules: &[String], session_rules: &[String]) -> Vec<String> {
    let mut composed = Vec::with_capacity(project_rules.len() + session_rules.len());
    composed.extend(project_rules.iter().cloned());
    composed.extend(session_rules.iter().cloned());
    composed
}

/// Rules from every top-level `agent:rules` body, in open-marker order.
///
/// A document that fails component parsing yields an empty list so a broken
/// marker cannot hide the rest of an admitted turn.
pub fn rules_from_document(doc: &str) -> Vec<String> {
    let Ok(components) = element::parse(doc) else {
        return Vec::new();
    };
    let mut rules: Vec<&Component> = components
        .iter()
        .filter(|component| component.name == "rules")
        .collect();
    rules.sort_by_key(|component| component.open_start);
    let mut extracted = Vec::new();
    for component in &rules {
        let nested_in_rules = rules.iter().any(|outer| {
            outer.open_start < component.open_start && component.close_end <= outer.close_end
        });
        if nested_in_rules {
            continue;
        }
        extracted.extend(rules_from_body(component.content(doc)));
    }
    extracted
}

/// Split one `agent:rules` body into rules.
pub fn rules_from_body(body: &str) -> Vec<String> {
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    let raw_lines: Vec<&str> = normalized.split('\n').collect();
    let mut start = 0;
    let mut end = raw_lines.len();
    while start < end && is_blank(raw_lines[start]) {
        start += 1;
    }
    while end > start && is_blank(raw_lines[end - 1]) {
        end -= 1;
    }
    if start >= end {
        return Vec::new();
    }
    let lines = &raw_lines[start..end];
    let mut rules = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if let Some(marker) = top_level_list_marker(lines[index]) {
            let (text, next) = consume_list_item(lines, index, marker);
            index = next;
            push_rule(&mut rules, text);
        } else if is_skippable_empty_marker(lines[index]) {
            index += 1;
        } else {
            let (text, next) = consume_free_text(lines, index);
            index = next;
            push_rule(&mut rules, text);
        }
    }
    rules
}

struct ListMarker {
    indent: usize,
    content_start: usize,
}

fn push_rule(rules: &mut Vec<String>, text: String) {
    if !text.trim().is_empty() {
        rules.push(text);
    }
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|byte| *byte == b' ').count()
}

fn top_level_list_marker(line: &str) -> Option<ListMarker> {
    let indent = leading_spaces(line);
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    for prefix in ["- ", "* ", "+ "] {
        if let Some(content) = rest.strip_prefix(prefix) {
            if content.trim().is_empty() {
                return None;
            }
            return Some(ListMarker {
                indent,
                content_start: indent + prefix.len(),
            });
        }
    }
    let bytes = rest.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 || index + 1 >= bytes.len() {
        return None;
    }
    if bytes[index] != b'.' && bytes[index] != b')' {
        return None;
    }
    if bytes[index + 1] != b' ' {
        return None;
    }
    if rest[index + 2..].trim().is_empty() {
        return None;
    }
    Some(ListMarker {
        indent,
        content_start: indent + index + 2,
    })
}

fn is_skippable_empty_marker(line: &str) -> bool {
    if leading_spaces(line) > 3 {
        return false;
    }
    let trimmed = line.trim();
    if matches!(trimmed, "-" | "*" | "+") {
        return true;
    }
    let bytes = trimmed.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let suffix = &trimmed[index..];
    index > 0 && (suffix == "." || suffix == ")")
}

fn fence_open(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let mut len = 1usize;
    for ch in chars {
        if ch == first {
            len += 1;
        } else {
            break;
        }
    }
    if len < 3 {
        return None;
    }
    let rest = &trimmed[len..];
    if rest.contains(first) {
        return None;
    }
    Some((first, len))
}

fn is_fence_closer(line: &str, ch: char, min_len: usize) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= min_len && trimmed.chars().all(|c| c == ch)
}

fn consume_list_item(lines: &[&str], start: usize, marker: ListMarker) -> (String, usize) {
    let mut collected = vec![lines[start][marker.content_start..].to_string()];
    let mut index = start + 1;
    let mut fence: Option<(char, usize)> = None;
    let mut pending_blanks: Vec<String> = Vec::new();
    while index < lines.len() {
        let line = lines[index];
        if let Some((ch, len)) = fence {
            collected.push(line.to_string());
            if is_fence_closer(line, ch, len) {
                fence = None;
            }
            index += 1;
            continue;
        }
        if is_blank(line) {
            pending_blanks.push(line.to_string());
            index += 1;
            continue;
        }
        if leading_spaces(line) > marker.indent {
            collected.append(&mut pending_blanks);
            if let Some(open) = fence_open(line) {
                fence = Some(open);
            }
            collected.push(line.to_string());
            index += 1;
            continue;
        }
        break;
    }
    (collected.join("\n"), index)
}

fn consume_free_text(lines: &[&str], start: usize) -> (String, usize) {
    let mut collected = Vec::new();
    let mut index = start;
    let mut fence: Option<(char, usize)> = None;
    while index < lines.len() {
        let line = lines[index];
        if let Some((ch, len)) = fence {
            collected.push(line.to_string());
            if is_fence_closer(line, ch, len) {
                fence = None;
            }
            index += 1;
            continue;
        }
        if index > start
            && (top_level_list_marker(line).is_some() || is_skippable_empty_marker(line))
        {
            break;
        }
        if let Some(open) = fence_open(line) {
            fence = Some(open);
        }
        collected.push(line.to_string());
        index += 1;
    }
    while collected.last().is_some_and(|line| is_blank(line)) {
        collected.pop();
    }
    (collected.join("\n"), index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_free_text_and_mixed_bodies_are_rules() {
        assert_eq!(
            rules_from_body("- alpha\n* beta\n+ gamma\n1. delta\n2) epsilon\n"),
            vec!["alpha", "beta", "gamma", "delta", "epsilon"]
        );
        assert_eq!(
            rules_from_body("Intro line.\n\nMore intro.\n"),
            vec!["Intro line.\n\nMore intro."]
        );
        assert_eq!(
            rules_from_body("Intro line.\n\nMore intro.\n\n- one\n- two\n\nOutro.\n"),
            vec!["Intro line.\n\nMore intro.", "one", "two", "Outro."]
        );
    }

    #[test]
    fn continuations_stay_with_the_item_and_column_zero_starts_a_new_rule() {
        assert_eq!(
            rules_from_body("- parent\n  - child\n    detail\n"),
            vec!["parent\n  - child\n    detail"]
        );
        assert_eq!(
            rules_from_body("- parent\n\n  still parent\n"),
            vec!["parent\n\n  still parent"]
        );
        assert_eq!(
            rules_from_body("- first\n\nsecond paragraph\n"),
            vec!["first", "second paragraph"]
        );
        assert_eq!(rules_from_body("-  two\n"), vec![" two"]);
        assert_eq!(rules_from_body("   - item\n"), vec!["item"]);
        assert_eq!(rules_from_body("    - code\n"), vec!["    - code"]);
    }

    #[test]
    fn empty_markers_checkboxes_ids_and_emphasis_follow_the_marker_rules() {
        assert_eq!(
            rules_from_body("- real\n\n-\n\n* \n\n- next\n"),
            vec!["real", "next"]
        );
        assert_eq!(rules_from_body("- [ ] box\n"), vec!["[ ] box"]);
        assert_eq!(
            rules_from_body("- [#sample] keep the token\n"),
            vec!["[#sample] keep the token"]
        );
        assert_eq!(
            rules_from_body("*emphasis* stays prose\n"),
            vec!["*emphasis* stays prose"]
        );
        assert_eq!(rules_from_body("* item\n"), vec!["item"]);
        assert_eq!(
            rules_from_body("---\n\nstill one\n"),
            vec!["---\n\nstill one"]
        );
        assert!(rules_from_body("\n\n").is_empty());
        assert_eq!(
            rules_from_body("- alpha\r\n- beta\r\n"),
            vec!["alpha", "beta"]
        );
    }

    #[test]
    fn fences_do_not_split_rules() {
        let body = "\
See the sample.\n\
\n\
```\n\
- not a rule\n\
```\n\
\n\
- real\n";
        assert_eq!(
            rules_from_body(body),
            vec![
                "See the sample.\n\n```\n- not a rule\n```".to_string(),
                "real".to_string(),
            ]
        );

        let indented = "- parent\n  ```\n- not a sibling\n  ```\n- sibling\n";
        assert_eq!(
            rules_from_body(indented),
            vec![
                "parent\n  ```\n- not a sibling\n  ```".to_string(),
                "sibling".to_string(),
            ]
        );

        let column_zero = "\
- parent\n\
```\n\
- inside fence\n\
```\n";
        assert_eq!(
            rules_from_body(column_zero),
            vec!["parent".to_string(), "```\n- inside fence\n```".to_string()]
        );
    }

    #[test]
    fn multiple_blocks_compose_in_file_order_without_replacing() {
        let doc = "\
<!-- agent:rules -->\n\
- first\n\
<!-- /agent:rules -->\n\
<!-- agent:rules -->\n\
Second block stays free text.\n\
\n\
Including the blank line.\n\
<!-- /agent:rules -->\n";
        assert_eq!(
            rules_from_document(doc),
            vec![
                "first".to_string(),
                "Second block stays free text.\n\nIncluding the blank line.".to_string(),
            ]
        );

        let project = "\
<!-- agent:rules -->\n\
- p1\n\
<!-- /agent:rules -->\n\
<!-- agent:rules -->\n\
- p2\n\
<!-- /agent:rules -->\n";
        let session = "\
<!-- agent:rules -->\n\
- s1\n\
<!-- /agent:rules -->\n";
        assert_eq!(
            compose_rules(&rules_from_document(project), &rules_from_document(session)),
            vec!["p1".to_string(), "p2".to_string(), "s1".to_string()]
        );
        assert_eq!(
            compose_rules(&["same".into()], &["same".into(), "more".into()]),
            vec!["same", "same", "more"]
        );
    }

    #[test]
    fn nested_rules_are_not_a_second_component_and_other_parents_still_count() {
        let nested = "\
<!-- agent:rules -->\n\
- outer\n\
<!-- agent:rules -->\n\
- inner\n\
<!-- /agent:rules -->\n\
<!-- /agent:rules -->\n";
        let rules = rules_from_document(nested);
        assert_eq!(
            rules.iter().filter(|rule| rule.as_str() == "inner").count(),
            1
        );
        assert!(rules.iter().any(|rule| rule == "outer"));

        let inside_exchange = "\
<!-- agent:exchange -->\n\
<!-- agent:rules -->\n\
- inside exchange\n\
<!-- /agent:rules -->\n\
<!-- /agent:exchange -->\n";
        assert_eq!(
            rules_from_document(inside_exchange),
            vec!["inside exchange".to_string()]
        );
    }

    #[test]
    fn parse_failures_and_fenced_markers_yield_no_rules() {
        assert!(rules_from_document("<!-- agent:rules -->\n- visible\n").is_empty());
        assert!(rules_from_document("<!-- agent:rules -->\n<!-- /agent:rules -->\n").is_empty());
        let fenced = "\
```\n\
<!-- agent:rules -->\n\
- not a component\n\
<!-- /agent:rules -->\n\
```\n";
        assert!(rules_from_document(fenced).is_empty());
    }

    #[test]
    fn guidance_is_omitted_when_there_are_no_rules() {
        assert_eq!(agent_rules_guidance(&[]), None);
        assert_eq!(
            agent_rules_guidance(&["one".to_string()]).as_deref(),
            Some(AGENT_RULES_GUIDANCE)
        );
    }
}
