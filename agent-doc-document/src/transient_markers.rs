//! Pure normalization for transient agent-doc document markers.
//!
//! These helpers operate only on document text. Git staging, realtime write
//! policy, preflight, and session checks use them to compare durable content
//! while ignoring boundary markers, `(HEAD)` annotations, guard comments, and
//! managed pipeline frontmatter.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub fn strip_boundary_markers(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn code_block_byte_ranges(content: &str) -> Vec<std::ops::Range<usize>> {
    let parser = Parser::new_ext(content, Options::empty()).into_offset_iter();
    let mut ranges = Vec::new();
    let mut start: Option<usize> = None;
    for (event, range) in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                start = Some(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(s) = start.take() {
                    ranges.push(s..range.end);
                }
            }
            _ => {}
        }
    }
    ranges
}

fn is_in_code_block(ranges: &[std::ops::Range<usize>], offset: usize) -> bool {
    ranges.iter().any(|r| r.contains(&offset))
}

/// Strip ` (HEAD)` suffix from markdown heading lines and bold-text pseudo-headers.
pub fn strip_head_markers(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<&str> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !is_in_code_block(&code_ranges, offset)
            && let Some(stripped) = line.strip_suffix(" (HEAD)")
        {
            if trimmed.starts_with('#') {
                result_lines.push(stripped);
                offset += line.len() + 1;
                continue;
            }
            let without_suffix = stripped.trim_end();
            if trimmed.starts_with("**") && without_suffix.trim_start().ends_with("**") {
                result_lines.push(stripped);
                offset += line.len() + 1;
                continue;
            }
        }
        result_lines.push(line);
        offset += line.len() + 1;
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

/// Strip per-cycle guard suppression markers from durable comparisons/commits.
pub fn strip_guard_markers(content: &str) -> String {
    const MARKERS: &[&str] = &[
        "<!-- no-pending-capture -->",
        "<!-- no-pending-done-guard -->",
    ];
    let mut result_lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if MARKERS.contains(&trimmed) {
            continue;
        }
        if MARKERS.iter().any(|m| line.contains(m)) {
            let mut cleaned = line.to_string();
            for marker in MARKERS {
                cleaned = cleaned.replace(marker, "");
            }
            result_lines.push(cleaned.trim_end().to_string());
        } else {
            result_lines.push(line.to_string());
        }
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

pub fn normalize_transient_agent_doc_markers(content: &str) -> String {
    agent_doc_frontmatter::frontmatter::strip_pipeline_block_lines(&strip_guard_markers(
        &strip_head_markers(&strip_boundary_markers(content)),
    ))
}

pub fn strip_re_heading_attribution(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<String> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        if !is_in_code_block(&code_ranges, offset) {
            let trimmed = line.trim_start();
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&hash_count) && trimmed.chars().nth(hash_count) == Some(' ') {
                let after_hash = trimmed[hash_count..].trim_start();
                if after_hash.starts_with("Re:")
                    && let Some(pos) = line.rfind(" — ")
                {
                    result_lines.push(line[..pos].to_string());
                    offset += line.len() + 1;
                    continue;
                }
            }
        }
        result_lines.push(line.to_string());
        offset += line.len() + 1;
    }
    let result = result_lines.join("\n");
    if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    }
}

pub fn normalize_post_commit_re_heading_drift(content: &str) -> String {
    strip_re_heading_attribution(&normalize_transient_agent_doc_markers(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_head_markers_from_headings() {
        let input =
            "# Title\n### Re: Foo (HEAD)\nSome text with (HEAD) in it\n### Re: Bar (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(
            result,
            "# Title\n### Re: Foo\nSome text with (HEAD) in it\n### Re: Bar\n"
        );
    }

    #[test]
    fn strip_head_markers_preserves_non_heading_lines() {
        let input = "Normal line (HEAD)\n### Heading (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "Normal line (HEAD)\n### Heading\n");
    }

    #[test]
    fn strip_head_markers_bold_text() {
        let input = "**Re: Something** (HEAD)\nSome text.\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "**Re: Something**\nSome text.\n");
    }

    #[test]
    fn strip_head_markers_ignores_fenced_code_hash() {
        let input = "### Re: Answer (HEAD)\nResponse.\n```bash\n# comment (HEAD)\n```\n";
        let result = strip_head_markers(input);
        assert_eq!(
            result, "### Re: Answer\nResponse.\n```bash\n# comment (HEAD)\n```\n",
            "fenced (HEAD) must be preserved, got:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_removes_standalone_lines() {
        let input = "### Re: topic\nResponse text.\n<!-- no-pending-capture -->\nMore text.\n<!-- no-pending-done-guard -->\nEnd.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "### Re: topic\nResponse text.\nMore text.\nEnd.\n",
            "standalone guard markers should be removed:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_strips_inline_content() {
        let input = "Text with <!-- no-pending-capture --> inline.\nNormal line.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "Text with  inline.\nNormal line.\n",
            "inline guard markers should be stripped:\n{result}"
        );
    }

    #[test]
    fn strip_guard_markers_strips_trailing_on_content_line() {
        let input = "**All 39 variable products now have defaults set.** <!-- no-pending-capture -->\nNext line.\n";
        let result = strip_guard_markers(input);
        assert_eq!(
            result, "**All 39 variable products now have defaults set.**\nNext line.\n",
            "trailing guard marker should be stripped with trailing whitespace trimmed:\n{result}"
        );
    }

    #[test]
    fn normalize_transient_markers_strips_boundary_head_guard_and_pipeline() {
        let input = concat!(
            "---\n",
            "agent_doc_pipeline:\n",
            "  phase: responding\n",
            "title: test\n",
            "---\n\n",
            "### Re: topic (HEAD)\n",
            "Answer. <!-- no-pending-capture -->\n",
            "<!-- agent:boundary:abc -->\n"
        );

        assert_eq!(
            normalize_transient_agent_doc_markers(input),
            "---\ntitle: test\n---\n\n### Re: topic\nAnswer."
        );
    }

    #[test]
    fn strip_re_heading_attribution_ignores_code_blocks() {
        let input = concat!(
            "### Re: topic — gpt-5\n",
            "Response.\n",
            "```md\n",
            "### Re: literal — gpt-5\n",
            "```\n"
        );

        assert_eq!(
            strip_re_heading_attribution(input),
            "### Re: topic\nResponse.\n```md\n### Re: literal — gpt-5\n```\n"
        );
    }
}
