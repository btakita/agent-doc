//! # Module: dedupe
//!
//! ## Spec
//! - `run(file)` detects and removes duplicate consecutive response blocks
//!   within the exchange component of a template document.
//! - A duplicate is defined as two consecutive `### Re:` sections with identical
//!   headings and content (after trimming whitespace).
//! - When a duplicate is found, the second occurrence is removed.
//! - The snapshot is updated after deduplication.
//! - Any stale `.agent-doc/patches/<hash>.json` file is deleted after dedup to prevent
//!   the plugin from re-applying the removed content on next startup.
//! - Reports what was removed to stderr.
//!
//! ## Agentic Contracts
//! - Dedupe is idempotent: running it twice produces the same result.
//! - Only removes exact consecutive duplicates — non-consecutive identical
//!   blocks are preserved (they may be intentional).
//! - Never modifies user content — only agent response blocks (### Re:).

use anyhow::{Context, Result};
use std::path::Path;

pub fn run(file: &Path) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let result = dedupe_responses(&content);

    if result == content {
        eprintln!("[dedupe] no duplicates found in {}", file.display());
        return Ok(());
    }

    let removed = content.len() - result.len();
    std::fs::write(file, &result).with_context(|| format!("failed to write {}", file.display()))?;

    // Update snapshot to match
    crate::snapshot::save(file, &result)?;

    // Clean up any stale patch file so the plugin doesn't re-apply the removed content.
    // Without this, processPendingPatches() on plugin restart would re-duplicate.
    if let Ok(hash) = crate::snapshot::doc_hash(file)
        && let Some(project_root) = crate::snapshot::find_project_root(file)
    {
        let patch_file = project_root
            .join(".agent-doc/patches")
            .join(format!("{}.json", hash));
        if patch_file.exists() {
            eprintln!(
                "[dedupe] cleaning stale patch file: {}",
                patch_file.display()
            );
            if let Err(e) = std::fs::remove_file(&patch_file) {
                eprintln!("[dedupe] WARNING: failed to remove stale patch file: {}", e);
            }
        }
    }

    eprintln!(
        "[dedupe] removed {} bytes of duplicate content from {}",
        removed,
        file.display()
    );

    Ok(())
}

/// Remove consecutive duplicate `### Re:` blocks from document content.
pub fn dedupe_responses(content: &str) -> String {
    dedupe_responses_with_report(content).0
}

pub fn first_duplicate_response_heading(content: &str) -> Option<String> {
    dedupe_responses_with_report(content).1
}

fn dedupe_responses_with_report(content: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines: Vec<&str> = Vec::new();

    // Find all ### Re: block boundaries
    let mut blocks: Vec<(usize, usize)> = Vec::new(); // (start_line, end_line)
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("### Re:") {
            let start = i;
            i += 1;
            // Find end of this block (next ### Re: or end of exchange component)
            while i < lines.len()
                && !lines[i].starts_with("### Re:")
                && !lines[i].starts_with("<!-- /agent:")
            {
                i += 1;
            }
            blocks.push((start, i));
        } else {
            i += 1;
        }
    }

    if blocks.len() < 2 {
        return (content.to_string(), None);
    }

    // Find consecutive duplicates (ignoring boundary markers)
    let mut skip_ranges: Vec<(usize, usize)> = Vec::new();
    let mut first_duplicate = None;
    for pair in blocks.windows(2) {
        let (s1, e1) = pair[0];
        let (s2, e2) = pair[1];
        let block1: String = lines[s1..e1]
            .iter()
            .filter_map(|l| normalize_response_block_line(l))
            .collect::<Vec<_>>()
            .join("\n");
        let block2: String = lines[s2..e2]
            .iter()
            .filter_map(|l| normalize_response_block_line(l))
            .collect::<Vec<_>>()
            .join("\n");
        if block1 == block2 {
            eprintln!(
                "[dedupe] duplicate found: \"{}\" (lines {}-{})",
                lines[s2].trim(),
                s2 + 1,
                e2
            );
            if first_duplicate.is_none() {
                first_duplicate = Some(lines[s2].trim().to_string());
            }
            skip_ranges.push((s2, e2));
        }
    }

    if skip_ranges.is_empty() {
        return (content.to_string(), None);
    }

    // Rebuild content, skipping duplicate ranges
    for (i, line) in lines.iter().enumerate() {
        let in_skip = skip_ranges.iter().any(|(s, e)| i >= *s && i < *e);
        if !in_skip {
            result_lines.push(line);
        }
    }

    let mut result = result_lines.join("\n");
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    (result, first_duplicate)
}

fn normalize_response_block_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("<!-- agent:boundary:") {
        return None;
    }
    if trimmed.starts_with("### Re:") {
        return Some(
            trimmed
                .strip_suffix(" (HEAD)")
                .unwrap_or(trimmed)
                .to_string(),
        );
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_removes_consecutive_duplicate() {
        let content = "### Re: Foo\nContent A.\n### Re: Foo\nContent A.\n### Re: Bar\nContent B.\n";
        let result = dedupe_responses(content);
        assert_eq!(result, "### Re: Foo\nContent A.\n### Re: Bar\nContent B.\n");
        assert_eq!(
            first_duplicate_response_heading(content).as_deref(),
            Some("### Re: Foo")
        );
    }

    #[test]
    fn dedupe_preserves_non_consecutive_duplicates() {
        let content = "### Re: Foo\nContent.\n### Re: Bar\nOther.\n### Re: Foo\nContent.\n";
        let result = dedupe_responses(content);
        assert_eq!(result, content);
    }

    #[test]
    fn dedupe_treats_head_marker_as_transient() {
        let content = "\
### Re: Foo — gpt-5
Content.
<!-- agent:boundary:old -->
### Re: Foo — gpt-5 (HEAD)
Content.
<!-- agent:boundary:new -->
";
        let result = dedupe_responses(content);
        assert_eq!(
            result,
            "### Re: Foo — gpt-5\nContent.\n<!-- agent:boundary:old -->\n"
        );
        assert_eq!(
            first_duplicate_response_heading(content).as_deref(),
            Some("### Re: Foo — gpt-5 (HEAD)")
        );
    }

    #[test]
    fn dedupe_no_duplicates() {
        let content = "### Re: A\nContent 1.\n### Re: B\nContent 2.\n";
        let result = dedupe_responses(content);
        assert_eq!(result, content);
    }

    #[test]
    fn dedupe_single_block() {
        let content = "### Re: Only\nJust one.\n";
        let result = dedupe_responses(content);
        assert_eq!(result, content);
    }
}
