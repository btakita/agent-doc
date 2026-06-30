//! Pure markdown outline projection.
//!
//! This module parses already-frontmatter-stripped markdown into
//! heading-delimited sections. Callers own file IO, frontmatter stripping, and
//! stdout formatting decisions.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// A heading-delimited section of a markdown document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownOutlineSection {
    /// Heading text, e.g. `## User`.
    pub heading: String,
    /// Heading depth: 1 for `#`, 2 for `##`, etc.
    pub depth: usize,
    /// One-based line number where the heading appears.
    pub line: usize,
    /// Number of lines in the section. Preserves legacy outline behavior and
    /// includes the heading line for real sections.
    pub lines: usize,
    /// Approximate token count: bytes / 4.
    pub tokens: usize,
}

/// Collect `(byte_offset, depth, heading_text)` for every heading in `body`.
///
/// Uses pulldown-cmark so ATX headings, setext headings, and headings inside
/// fenced code blocks follow CommonMark behavior.
fn collect_headings(body: &str) -> Vec<(usize, usize, String)> {
    let mut headings = Vec::new();
    let parser = Parser::new_ext(body, Options::empty());
    let mut iter = parser.into_offset_iter();

    while let Some((event, range)) = iter.next() {
        if let Event::Start(Tag::Heading { level, .. }) = event {
            let depth = heading_level_to_depth(level);
            let byte_start = range.start;

            let mut text = String::new();
            for (inner_event, _) in iter.by_ref() {
                match inner_event {
                    Event::End(pulldown_cmark::TagEnd::Heading(_)) => break,
                    Event::Text(t) | Event::Code(t) => text.push_str(&t),
                    _ => {}
                }
            }
            headings.push((byte_start, depth, text));
        }
    }

    headings
}

fn heading_level_to_depth(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Project markdown into outline sections.
pub fn project_markdown_outline(body: &str) -> Vec<MarkdownOutlineSection> {
    let lines: Vec<&str> = body.lines().collect();
    let headings = collect_headings(body);

    let mut line_starts: Vec<usize> = Vec::with_capacity(lines.len() + 1);
    line_starts.push(0);
    for line in &lines {
        let prev = *line_starts.last().unwrap();
        line_starts.push(prev + line.len() + 1);
    }

    let byte_to_line = |byte_off: usize| -> usize {
        line_starts
            .partition_point(|&start| start <= byte_off)
            .saturating_sub(1)
    };

    let mut sections: Vec<MarkdownOutlineSection> = Vec::new();

    for (byte_off, depth, text) in &headings {
        let line_idx = byte_to_line(*byte_off);
        let heading_str = {
            let src_line = lines.get(line_idx).copied().unwrap_or("").trim();
            if src_line.starts_with('#') {
                src_line.to_string()
            } else {
                format!("{} {}", "#".repeat(*depth), text)
            }
        };

        if let Some(prev) = sections.last_mut() {
            let prev_line = prev.line;
            prev.lines = line_idx - prev_line;
            let section_text = lines[prev_line + 1..line_idx].join("\n");
            prev.tokens = section_text.len().div_ceil(4);
        }

        sections.push(MarkdownOutlineSection {
            heading: heading_str,
            depth: *depth,
            line: line_idx,
            lines: 0,
            tokens: 0,
        });
    }

    if let Some(prev) = sections.last_mut() {
        let prev_line = prev.line;
        prev.lines = lines.len() - prev_line;
        let section_text = lines[prev_line + 1..].join("\n");
        prev.tokens = section_text.len().div_ceil(4);
    }

    let first_heading_line = sections.first().map_or(lines.len(), |s| s.line);
    if first_heading_line > 0 {
        let preamble_text: String = lines[..first_heading_line].join("\n");
        let preamble_tokens = preamble_text.len().div_ceil(4);
        if preamble_tokens > 0 {
            sections.insert(
                0,
                MarkdownOutlineSection {
                    heading: "(preamble)".to_string(),
                    depth: 0,
                    line: 0,
                    lines: first_heading_line,
                    tokens: preamble_tokens,
                },
            );
        }
    }

    for section in &mut sections {
        section.line += 1;
    }

    sections
}

/// Render the human-readable outline table.
pub fn render_markdown_outline_text(sections: &[MarkdownOutlineSection]) -> String {
    let total_tokens: usize = sections.iter().map(|s| s.tokens).sum();
    let total_lines: usize = sections.iter().map(|s| s.lines).sum();
    let mut out = String::new();

    for section in sections {
        let indent = if section.depth > 1 {
            "  ".repeat(section.depth - 1)
        } else {
            String::new()
        };
        let heading = section.heading.trim_start_matches('#').trim();
        let heading_display = if heading.is_empty() {
            &section.heading
        } else {
            heading
        };
        let _ = writeln!(
            out,
            "{}{:<40} {:>4} lines  ~{:>5} tokens",
            indent, heading_display, section.lines, section.tokens
        );
    }
    out.push_str("---\n");
    let _ = writeln!(
        out,
        "{:<40} {:>4} lines  ~{:>5} tokens",
        "Total", total_lines, total_tokens
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_headings_atx() {
        let body = "# Title\n\n## Section\n\n### Sub\n";
        let headings = collect_headings(body);
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].1, 1);
        assert_eq!(headings[0].2, "Title");
        assert_eq!(headings[1].1, 2);
        assert_eq!(headings[1].2, "Section");
        assert_eq!(headings[2].1, 3);
        assert_eq!(headings[2].2, "Sub");
    }

    #[test]
    fn collect_headings_no_space_not_heading() {
        let body = "#NoSpace\n\n# Real\n";
        let headings = collect_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].2, "Real");
    }

    #[test]
    fn collect_headings_inside_code_block_ignored() {
        let body = "```\n# Not a heading\n```\n\n# Real\n";
        let headings = collect_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].2, "Real");
    }

    #[test]
    fn project_markdown_outline_basic() {
        let body = "## User\n\nHello world\n\n## Assistant\n\nResponse here\n";
        let sections = project_markdown_outline(body);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "## User");
        assert_eq!(sections[0].depth, 2);
        assert_eq!(sections[1].heading, "## Assistant");
        assert_eq!(sections[1].depth, 2);
    }

    #[test]
    fn project_markdown_outline_with_preamble() {
        let body = "Some intro text\n\n## First\n\nContent\n";
        let sections = project_markdown_outline(body);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "(preamble)");
        assert_eq!(sections[0].depth, 0);
        assert_eq!(sections[1].heading, "## First");
    }

    #[test]
    fn project_markdown_outline_empty() {
        let sections = project_markdown_outline("");
        assert!(sections.is_empty());
    }

    #[test]
    fn project_markdown_outline_setext_headings() {
        let body = "Title\n=====\n\nSome content here\n\nSection\n-------\n\nMore content\n";
        let sections = project_markdown_outline(body);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].depth, 1);
        assert_eq!(sections[0].heading, "# Title");
        assert_eq!(sections[1].depth, 2);
        assert_eq!(sections[1].heading, "## Section");
    }

    #[test]
    fn project_markdown_outline_ignores_heading_inside_code_block() {
        let body = "## Real\n\nContent\n\n```\n## Fake\n```\n\nmore\n";
        let sections = project_markdown_outline(body);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, "## Real");
    }

    #[test]
    fn outline_sections_serialize_with_stable_fields() {
        let sections = vec![MarkdownOutlineSection {
            heading: "## Test".to_string(),
            depth: 2,
            line: 1,
            lines: 5,
            tokens: 20,
        }];
        let json = serde_json::to_string(&sections).unwrap();
        assert_eq!(
            json,
            "[{\"heading\":\"## Test\",\"depth\":2,\"line\":1,\"lines\":5,\"tokens\":20}]"
        );
    }

    #[test]
    fn render_markdown_outline_text_includes_total() {
        let sections = vec![MarkdownOutlineSection {
            heading: "## Test".to_string(),
            depth: 2,
            line: 1,
            lines: 5,
            tokens: 20,
        }];
        let rendered = render_markdown_outline_text(&sections);
        assert!(rendered.contains("Test"));
        assert!(rendered.contains("Total"));
    }
}
