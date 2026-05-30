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

/// Detect "late-IPC response over-application": the working tree (`current`)
/// contains the committed `head` content plus one or more **extra copies** of
/// already-committed `### Re:` response blocks, with identical surrounding
/// scaffold and no new distinct response content.
///
/// This is the shape produced when a late IPC reposition / stale-patch replay
/// re-inserts the response that already reached `HEAD` (commit N), typically
/// wrapped in redundant `<!-- agent:boundary:* -->` markers. The next cycle's
/// `session-check` would otherwise misclassify the duplicate as a manual
/// "direct response patchback" because the body sits outside HEAD even though
/// it byte-for-byte matches HEAD's committed response.
///
/// Distinct from [`first_duplicate_response_heading`]/[`dedupe_responses`],
/// which only collapse **consecutive** identical `### Re:` blocks. The
/// over-application can leave the duplicate non-adjacent (separated by boundary
/// markers or blank lines), so we compare the *response multiset* and *scaffold*
/// against `HEAD` directly.
///
/// Returns `true` only when restoring `head` over the working tree is provably
/// safe: same non-response scaffold, the same set of distinct response blocks,
/// and `current` carries strictly more (duplicate) response blocks than `head`.
/// Any genuinely new response content, lost response content, or scaffold drift
/// returns `false` so the caller falls through to the normal patchback guard.
pub fn is_committed_response_overapplication(current: &str, head: &str) -> bool {
    let (cur_scaffold, cur_responses) = split_scaffold_and_responses(current);
    let (head_scaffold, head_responses) = split_scaffold_and_responses(head);

    // Non-response structure must match exactly (modulo transient boundary
    // markers and blank lines, which `split_scaffold_and_responses` drops).
    if cur_scaffold != head_scaffold {
        return false;
    }
    // Identical response multisets are not an over-application (any remaining
    // drift is transient-only and handled by other guards).
    if cur_responses == head_responses {
        return false;
    }
    // No new distinct response content may appear, and none may be lost.
    use std::collections::HashSet;
    let cur_set: HashSet<&String> = cur_responses.iter().collect();
    let head_set: HashSet<&String> = head_responses.iter().collect();
    if cur_set != head_set {
        return false;
    }
    // The only difference is extra (duplicate) copies in the working tree.
    cur_responses.len() > head_responses.len()
}

fn is_response_heading(trimmed: &str) -> bool {
    trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
        || trimmed == "## Assistant"
}

/// Split a document into its non-response scaffold (significant lines outside
/// any response block, with transient boundary markers and blank lines removed)
/// and the list of normalized `### Re:` response blocks. Used by
/// [`is_committed_response_overapplication`] to compare structure and response
/// content independently.
fn split_scaffold_and_responses(content: &str) -> (Vec<String>, Vec<String>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut scaffold: Vec<String> = Vec::new();
    let mut responses: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if is_response_heading(trimmed) {
            let start = i;
            i += 1;
            while i < lines.len()
                && !is_response_heading(lines[i].trim())
                && !lines[i].trim().starts_with("<!-- /agent:")
            {
                i += 1;
            }
            let block: String = lines[start..i]
                .iter()
                .filter_map(|l| normalize_response_block_line(l))
                .collect::<Vec<_>>()
                .join("\n");
            responses.push(block);
        } else {
            // Drop transient boundary markers and blank lines so reposition
            // churn around the duplicate does not read as scaffold drift.
            if !trimmed.is_empty() && !trimmed.starts_with("<!-- agent:boundary:") {
                scaffold.push(trimmed.to_string());
            }
            i += 1;
        }
    }
    (scaffold, responses)
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
            // Prefer to drop the copy whose body was corrupted with `❯ `
            // user-prompt prefixes (a multi-retry / late-IPC reposition
            // artifact), keeping the clean twin. Default to dropping the later
            // copy when neither (or both) carries the corruption.
            let b1_corrupt = block_has_prompt_prefixed_body(&lines[s1..e1]);
            let b2_corrupt = block_has_prompt_prefixed_body(&lines[s2..e2]);
            let (skip_s, skip_e) = if b1_corrupt && !b2_corrupt {
                (s1, e1)
            } else {
                (s2, e2)
            };
            eprintln!(
                "[dedupe] duplicate found: \"{}\" (lines {}-{})",
                lines[skip_s].trim(),
                skip_s + 1,
                skip_e
            );
            if first_duplicate.is_none() {
                first_duplicate = Some(lines[skip_s].trim().to_string());
            }
            skip_ranges.push((skip_s, skip_e));
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
    // Response body lines must never carry the `❯ ` user-prompt prefix; a
    // multi-retry / late-IPC reposition can wrongly prefix them. Normalize the
    // prefix away so a corrupted duplicate still compares equal to its clean
    // committed twin instead of slipping past the dedupe / over-application
    // guards as "new" content.
    let unprefixed = trimmed
        .strip_prefix('❯')
        .map(str::trim_start)
        .unwrap_or(trimmed);
    Some(unprefixed.to_string())
}

/// Does this `### Re:` block carry any body line wrongly prefixed with the
/// `❯ ` user-prompt marker? Used by [`dedupe_responses_with_report`] to drop
/// the corrupted copy of a duplicate pair in favor of its clean twin.
fn block_has_prompt_prefixed_body(block_lines: &[&str]) -> bool {
    block_lines.iter().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with("### Re:")
            && !trimmed.starts_with("<!-- agent:boundary:")
            && trimmed.starts_with('❯')
    })
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

    const HEAD_DOC: &str = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
<!-- /agent:exchange -->
";

    #[test]
    fn overapplication_detects_boundary_wrapped_duplicate() {
        // Late-IPC reposition re-inserts the committed response wrapped in a
        // second identical boundary marker. Same scaffold, same response set,
        // one extra duplicate copy → safe to restore HEAD.
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
<!-- /agent:exchange -->
";
        assert!(is_committed_response_overapplication(current, HEAD_DOC));
    }

    #[test]
    fn overapplication_detects_head_marker_variant_duplicate() {
        // The re-applied copy carries a transient `(HEAD)` annotation; it must
        // still normalize to the committed response and count as duplicate.
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:old -->
### Re: fix thing — opus-4-8 (HEAD)
Fixed it.
<!-- agent:boundary:88409761 -->
<!-- /agent:exchange -->
";
        assert!(is_committed_response_overapplication(current, HEAD_DOC));
    }

    #[test]
    fn overapplication_rejects_identical_document() {
        // No extra copies → not an over-application (transient-only drift is
        // another guard's concern).
        assert!(!is_committed_response_overapplication(HEAD_DOC, HEAD_DOC));
    }

    #[test]
    fn overapplication_rejects_new_response_content() {
        // A genuinely new response block must remain a real patchback, never a
        // duplicate we silently discard.
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: a different answer — opus-4-8
New content.
<!-- agent:boundary:newone -->
<!-- /agent:exchange -->
";
        assert!(!is_committed_response_overapplication(current, HEAD_DOC));
    }

    #[test]
    fn overapplication_rejects_scaffold_drift() {
        // A new user prompt line outside the response is scaffold drift — not a
        // safe restore-to-HEAD.
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
do [#another-thing]
<!-- /agent:exchange -->
";
        assert!(!is_committed_response_overapplication(current, HEAD_DOC));
    }

    #[test]
    fn dedupe_collapses_prompt_prefixed_corrupted_duplicate() {
        // #finalize-retry-ipc-response-duplication: a multi-retry / late-IPC
        // reposition left a duplicate response whose earlier copy had its body
        // lines wrongly prefixed with the `❯ ` user-prompt marker. Dedupe must
        // collapse to a single CLEAN block (the corrupted copy is dropped, not
        // kept), with no surviving `❯`-prefixed body line.
        let content = "\
### Re: fix thing — opus-4-8
❯ **Scope:** narrow.
❯ **Commits:** abc123.
### Re: fix thing — opus-4-8 (HEAD)
**Scope:** narrow.
**Commits:** abc123.
";
        let result = dedupe_responses(content);
        assert_eq!(
            result.matches("### Re: fix thing").count(),
            1,
            "exactly one response block must remain:\n{result}"
        );
        assert!(
            !result.contains('❯'),
            "no `❯`-prefixed body line may survive dedupe:\n{result}"
        );
        // Convergence is idempotent — a second pass is a no-op.
        assert_eq!(dedupe_responses(&result), result);
    }

    #[test]
    fn overapplication_detects_prompt_prefixed_corrupted_duplicate() {
        // The re-applied copy's body was `❯ `-prefixed by the retry; it must
        // still normalize to the committed response and count as a safe
        // restore-to-HEAD over-application.
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
❯ Fixed it.
<!-- agent:boundary:old -->
### Re: fix thing — opus-4-8 (HEAD)
Fixed it.
<!-- agent:boundary:88409761 -->
<!-- /agent:exchange -->
";
        assert!(is_committed_response_overapplication(current, HEAD_DOC));
    }

    #[test]
    fn overapplication_rejects_lost_response() {
        // Working tree dropped a committed response — not over-application.
        let head = "\
<!-- agent:exchange -->
### Re: one — opus-4-8
First.
### Re: two — opus-4-8
Second.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->
";
        let current = "\
<!-- agent:exchange -->
### Re: one — opus-4-8
First.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->
";
        assert!(!is_committed_response_overapplication(current, head));
    }
}
