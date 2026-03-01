use anyhow::Result;
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

fn parse_sections(body: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let lines: Vec<&str> = body.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        if let Some(depth) = heading_depth(line) {
            // Close the previous section's line count
            if let Some(prev) = sections.last_mut() {
                let content_start = prev.line; // heading line (0-indexed in our tracking)
                prev.lines = i - content_start;
                let section_text = lines[content_start + 1..i].join("\n");
                prev.tokens = section_text.len().div_ceil(4); // ceil(bytes/4)
            }

            sections.push(Section {
                heading: line.to_string(),
                depth,
                line: i, // 0-indexed for internal tracking
                lines: 0,
                tokens: 0,
            });
        }
    }

    // Close the last section
    if let Some(prev) = sections.last_mut() {
        let content_start = prev.line;
        prev.lines = lines.len() - content_start;
        let section_text = lines[content_start + 1..].join("\n");
        prev.tokens = section_text.len().div_ceil(4);
    }

    // Handle content before any heading
    if sections.is_empty() || sections[0].line > 0 {
        let end = sections.first().map_or(lines.len(), |s| s.line);
        if end > 0 {
            let preamble_text: String = lines[..end].join("\n");
            let preamble_tokens = preamble_text.len().div_ceil(4);
            if preamble_tokens > 0 {
                sections.insert(
                    0,
                    Section {
                        heading: "(preamble)".to_string(),
                        depth: 0,
                        line: 0,
                        lines: end,
                        tokens: preamble_tokens,
                    },
                );
            }
        }
    }

    // Convert line numbers from 0-indexed body to 1-indexed display
    // (frontmatter offset is handled by the caller if needed)
    for s in &mut sections {
        s.line += 1; // 1-based
    }

    sections
}

fn heading_depth(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    // Must have a space after the hashes (or be just hashes at end of line)
    if hashes > 0 && (trimmed.len() == hashes || trimmed.as_bytes()[hashes] == b' ') {
        Some(hashes)
    } else {
        None
    }
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
    fn test_heading_depth() {
        assert_eq!(heading_depth("# Title"), Some(1));
        assert_eq!(heading_depth("## Section"), Some(2));
        assert_eq!(heading_depth("### Sub"), Some(3));
        assert_eq!(heading_depth("Not a heading"), None);
        assert_eq!(heading_depth("#NoSpace"), None);
        assert_eq!(heading_depth("  ## Indented"), Some(2));
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
