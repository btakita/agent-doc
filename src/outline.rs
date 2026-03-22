use anyhow::Result;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};
use std::path::Path;

/// A heading-delimited section of a markdown document.
struct Section {
    /// Heading text (e.g. "## User")
    heading: String,
    /// Heading depth (1 for #, 2 for ##, etc.)
    depth: usize,
    /// Line number where the heading appears (1-based)
    line: usize,
    /// Number of content lines (excluding the heading itself)
    lines: usize,
    /// Approximate token count (bytes / 4)
    tokens: usize,
}

pub fn run(file: &Path, json: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)?;
    let (_fm, body) = crate::frontmatter::parse(&content)?;

    let sections = parse_sections(body);

    if json {
        print_json(&sections);
    } else {
        print_text(&sections);
    }

    Ok(())
}

/// Collect `(byte_offset, depth, heading_text)` for every heading in `body`
/// using pulldown-cmark. Handles ATX headings (`# …`), setext headings
/// (`===` / `---` underlines), and correctly skips headings inside code blocks.
fn collect_headings(body: &str) -> Vec<(usize, usize, String)> {
    let mut headings = Vec::new();
    let parser = Parser::new_ext(body, Options::empty());
    let mut iter = parser.into_offset_iter();

    while let Some((event, range)) = iter.next() {
        if let Event::Start(Tag::Heading { level, .. }) = event {
            let depth = heading_level_to_depth(level);
            let byte_start = range.start;

            // Collect all inline text events until the matching End(Heading)
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

fn parse_sections(body: &str) -> Vec<Section> {
    let lines: Vec<&str> = body.lines().collect();
    let headings = collect_headings(body);

    // Convert byte offsets → 0-based line numbers.
    // Build a lookup: byte offset of each line start.
    let mut line_starts: Vec<usize> = Vec::with_capacity(lines.len() + 1);
    line_starts.push(0);
    for line in &lines {
        let prev = *line_starts.last().unwrap();
        line_starts.push(prev + line.len() + 1); // +1 for '\n'
    }

    // For a given byte offset find the 0-based line index.
    let byte_to_line = |byte_off: usize| -> usize {
        line_starts
            .partition_point(|&start| start <= byte_off)
            .saturating_sub(1)
    };

    // Build Section list from headings. We store line index (0-based) internally
    // and convert to 1-based at the end, matching the original behaviour.
    let mut sections: Vec<Section> = Vec::new();

    for (byte_off, depth, text) in &headings {
        let line_idx = byte_to_line(*byte_off);

        // Build the canonical heading string. For ATX headings the source line
        // starts with `#`; for setext headings the source line is plain text.
        // We reconstruct an ATX-style string so the display format is stable.
        let heading_str = {
            let src_line = lines.get(line_idx).copied().unwrap_or("").trim();
            if src_line.starts_with('#') {
                src_line.to_string()
            } else {
                // Setext heading — emit canonical ATX form
                format!("{} {}", "#".repeat(*depth), text)
            }
        };

        // Close the previous section
        if let Some(prev) = sections.last_mut() {
            let prev_line = prev.line; // 0-based
            prev.lines = line_idx - prev_line;
            let section_text = lines[prev_line + 1..line_idx].join("\n");
            prev.tokens = section_text.len().div_ceil(4);
        }

        sections.push(Section {
            heading: heading_str,
            depth: *depth,
            line: line_idx, // 0-based for now
            lines: 0,
            tokens: 0,
        });
    }

    // Close the last section
    if let Some(prev) = sections.last_mut() {
        let prev_line = prev.line;
        prev.lines = lines.len() - prev_line;
        let section_text = lines[prev_line + 1..].join("\n");
        prev.tokens = section_text.len().div_ceil(4);
    }

    // Preamble: content before the first heading
    let first_heading_line = sections.first().map_or(lines.len(), |s| s.line);
    if first_heading_line > 0 {
        let preamble_text: String = lines[..first_heading_line].join("\n");
        let preamble_tokens = preamble_text.len().div_ceil(4);
        if preamble_tokens > 0 {
            sections.insert(
                0,
                Section {
                    heading: "(preamble)".to_string(),
                    depth: 0,
                    line: 0,
                    lines: first_heading_line,
                    tokens: preamble_tokens,
                },
            );
        }
    }

    // Convert 0-based line indices to 1-based for display
    for s in &mut sections {
        s.line += 1;
    }

    sections
}

fn print_text(sections: &[Section]) {
    let total_tokens: usize = sections.iter().map(|s| s.tokens).sum();
    let total_lines: usize = sections.iter().map(|s| s.lines).sum();

    for s in sections {
        let indent = if s.depth > 1 {
            "  ".repeat(s.depth - 1)
        } else {
            String::new()
        };
        let heading = s.heading.trim_start_matches('#').trim();
        let heading_display = if heading.is_empty() {
            &s.heading
        } else {
            heading
        };
        println!(
            "{}{:<40} {:>4} lines  ~{:>5} tokens",
            indent, heading_display, s.lines, s.tokens
        );
    }
    println!("---");
    println!(
        "{:<40} {:>4} lines  ~{:>5} tokens",
        "Total", total_lines, total_tokens
    );
}

fn print_json(sections: &[Section]) {
    print!("[");
    for (i, s) in sections.iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        print!(
            r#"{{"heading":"{}","depth":{},"line":{},"lines":{},"tokens":{}}}"#,
            s.heading.replace('"', "\\\""),
            s.depth,
            s.line,
            s.lines,
            s.tokens
        );
    }
    println!("]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_headings_atx() {
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
    fn test_collect_headings_no_space_not_heading() {
        // `#NoSpace` is not a valid ATX heading per CommonMark
        let body = "#NoSpace\n\n# Real\n";
        let headings = collect_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].2, "Real");
    }

    #[test]
    fn test_collect_headings_inside_code_block_ignored() {
        let body = "```\n# Not a heading\n```\n\n# Real\n";
        let headings = collect_headings(body);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].2, "Real");
    }

    #[test]
    fn test_parse_sections_basic() {
        let body = "## User\n\nHello world\n\n## Assistant\n\nResponse here\n";
        let sections = parse_sections(body);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "## User");
        assert_eq!(sections[0].depth, 2);
        assert_eq!(sections[1].heading, "## Assistant");
        assert_eq!(sections[1].depth, 2);
    }

    #[test]
    fn test_parse_sections_with_preamble() {
        let body = "Some intro text\n\n## First\n\nContent\n";
        let sections = parse_sections(body);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "(preamble)");
        assert_eq!(sections[0].depth, 0);
        assert_eq!(sections[1].heading, "## First");
    }

    #[test]
    fn test_parse_sections_empty() {
        let body = "";
        let sections = parse_sections(body);
        assert!(sections.is_empty());
    }

    #[test]
    fn test_setext_headings() {
        // Setext-style: underlined with === (H1) or --- (H2)
        let body = "Title\n=====\n\nSome content here\n\nSection\n-------\n\nMore content\n";
        let sections = parse_sections(body);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].depth, 1);
        assert_eq!(sections[0].heading, "# Title");
        assert_eq!(sections[1].depth, 2);
        assert_eq!(sections[1].heading, "## Section");
    }

    #[test]
    fn test_heading_inside_code_block_ignored() {
        // A `#` heading inside a fenced code block must not create a section
        let body = "## Real\n\nContent\n\n```\n## Fake\n```\n\nmore\n";
        let sections = parse_sections(body);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, "## Real");
    }

    #[test]
    fn test_json_output() {
        // Just ensure it doesn't panic
        let sections = vec![Section {
            heading: "## Test".to_string(),
            depth: 2,
            line: 1,
            lines: 5,
            tokens: 20,
        }];
        print_json(&sections);
    }
}
