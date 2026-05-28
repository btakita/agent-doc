//! Diff I/O — snapshot-backed half of the original `diff.rs`. The pure half
//! lives in [`agent_doc_core::diff`] (Wave 4 of #adcr per `#rtx6` Option 1).
//!
//! These functions all touch `snapshot::{load, save, resolve, path_for}` and
//! must stay in the orchestration crate. Re-exported from [`crate::diff`].

use anyhow::Result;
use similar::{ChangeTag, TextDiff};
use std::path::Path;

use crate::snapshot;
use agent_doc_core::diff::{is_stale_snapshot, strip_comments, unified_diff_from_contents};

/// Diff result plus the stable current document content used to compute it.
pub struct ComputeResult {
    pub diff: Option<String>,
    pub current: String,
}

/// Compute a unified diff between the snapshot and the current document, and
/// return the stable current content used to compute it.
///
/// Both snapshot and current content are comment-stripped before comparison.
pub fn compute_with_current(doc: &Path) -> Result<ComputeResult> {
    let t_total = std::time::Instant::now();

    let previous = snapshot::resolve(doc)?.unwrap_or_default();
    let snap_path = snapshot::path_for(doc)?;

    // Copy-on-read: capture snapshot mtime at read time so we can detect
    // external modifications before any stale-snapshot recovery write.
    // Fixes #wcf5: IDE watchers and git hooks bypass advisory flock.
    let snap_mtime_at_read = snap_path.metadata().and_then(|m| m.modified()).ok();

    // Wait for user to finish typing (truncation detection with delayed rechecks)
    let current = wait_for_stable_content(doc, &previous)?;

    eprintln!(
        "[diff] doc={} snapshot={} doc_len={} snap_len={}",
        doc.display(),
        snap_path.display(),
        current.len(),
        previous.len(),
    );

    let t_strip = std::time::Instant::now();
    let current_stripped = strip_comments(&current);
    let previous_stripped = strip_comments(&previous);
    let elapsed_strip = t_strip.elapsed().as_millis();
    if elapsed_strip > 0 {
        eprintln!("[perf] diff.strip_comments: {}ms", elapsed_strip);
    }

    eprintln!(
        "[diff] stripped: doc_len={} snap_len={}",
        current_stripped.len(),
        previous_stripped.len(),
    );

    let Some(output) = unified_diff_from_contents(&previous, &current) else {
        eprintln!(
            "[diff] no changes detected between snapshot and document (after comment stripping)"
        );
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
        }
        return Ok(ComputeResult {
            diff: None,
            current,
        });
    };

    // Stale snapshot recovery: if the diff is only completed assistant/user
    // exchanges with no new user content, the previous cycle wrote the response
    // but context compaction prevented the snapshot update.
    //
    // Copy-on-read guard (#wcf5): verify the snapshot file hasn't been modified
    // by an external process (IDE watcher, git hook) since we read it. If it
    // changed, skip recovery — the external update is authoritative.
    if is_stale_snapshot(&previous, &current) {
        let snap_mtime_now = snap_path.metadata().and_then(|m| m.modified()).ok();
        if snap_mtime_at_read != snap_mtime_now {
            eprintln!(
                "[snapshot recovery] Skipped — snapshot modified externally since read (copy-on-read guard)"
            );
        } else {
            eprintln!(
                "[snapshot recovery] Snapshot synced — previous cycle completed but snapshot was stale"
            );
            snapshot::save(doc, &current)?;
            let elapsed_total = t_total.elapsed().as_millis();
            if elapsed_total > 0 {
                eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
            }
            return Ok(ComputeResult {
                diff: None,
                current,
            });
        }
    }

    eprintln!("[diff] changes detected, computing unified diff");

    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] diff.compute total: {}ms", elapsed_total);
    }

    Ok(ComputeResult {
        diff: Some(output),
        current,
    })
}

/// Compute a unified diff between the snapshot and the current document.
/// Returns None if there are no changes.
///
/// Both snapshot and current content are comment-stripped before comparison.
pub fn compute(doc: &Path) -> Result<Option<String>> {
    Ok(compute_with_current(doc)?.diff)
}

/// Wait for stable content by detecting truncated lines and rechecking.
///
/// When the user is mid-typing, the last added line may be incomplete.
/// This function rechecks the file at short intervals until:
/// - The last line appears complete (ends with terminal punctuation or newline)
/// - The content hasn't changed between two consecutive rechecks
/// - Maximum recheck attempts reached (prevents infinite loops)
///
/// Returns the stable file content.
pub fn wait_for_stable_content(doc: &Path, previous: &str) -> Result<String> {
    const RECHECK_DELAY_MS: u64 = 500;
    const MAX_RECHECKS: u32 = 12; // ~6 seconds max
    const STABLE_CHECKS_REQUIRED: u32 = 3; // require 3 consecutive stable reads

    let mut current = std::fs::read_to_string(doc)?;
    // Track consecutive stable reads across outer iterations — content changes anywhere
    // (even between outer iterations) must reset the counter so 3 truly consecutive
    // stable reads are always required, not just 3 within a single outer pass.
    let mut stable_count = 0u32;

    for attempt in 0..MAX_RECHECKS {
        let last_added =
            extract_last_added_line(&strip_comments(previous), &strip_comments(&current));

        if let Some(line) = &last_added
            && looks_truncated(line)
        {
            eprintln!(
                "[diff] Last line may be truncated (recheck {}/{}): {:?}",
                attempt + 1,
                MAX_RECHECKS,
                truncate_for_log(line, 60)
            );
            // Sleep then re-read; count consecutive identical reads across all iterations.
            std::thread::sleep(std::time::Duration::from_millis(RECHECK_DELAY_MS));
            let refreshed = std::fs::read_to_string(doc)?;
            if refreshed == current {
                stable_count += 1;
            } else {
                current = refreshed;
                stable_count = 0;
            }
            if stable_count >= STABLE_CHECKS_REQUIRED {
                eprintln!(
                    "[diff] Content stable after {} consecutive checks",
                    STABLE_CHECKS_REQUIRED
                );
                break;
            }
            continue;
        }
        // Line looks complete — no recheck needed
        break;
    }

    Ok(current)
}

/// Extract the last added (non-empty) line from the diff.
fn extract_last_added_line(previous_stripped: &str, current_stripped: &str) -> Option<String> {
    let diff = TextDiff::from_lines(previous_stripped, current_stripped);
    let mut last_insert: Option<String> = None;

    for change in diff.iter_all_changes() {
        if change.tag() == ChangeTag::Insert {
            let val = change.value().trim();
            if !val.is_empty() {
                last_insert = Some(val.to_string());
            }
        }
    }

    last_insert
}

/// Check if a line looks truncated (user may still be typing).
///
/// A line looks truncated if:
/// - It ends mid-word (no space or punctuation at end)
/// - It's very short (< 3 chars) and doesn't look like a command
/// - It ends with common incomplete patterns
///
/// A line does NOT look truncated if:
/// - It ends with terminal punctuation (. ! ? : ;)
/// - It's a markdown heading (starts with #)
/// - It's a command (starts with / or `)
/// - It ends with a closing marker (-->)
/// - It's empty or whitespace-only
fn looks_truncated(line: &str) -> bool {
    let trimmed = line.trim();

    // Empty or whitespace — not truncated
    if trimmed.is_empty() {
        return false;
    }

    // Commands, headings, code blocks — never truncated
    if trimmed.starts_with('/')
        || trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with("<!--")
    {
        return false;
    }

    // Single characters are treated as potentially truncated — the user may be
    // mid-typing (e.g., "S" as the start of "Save as a draft."). The stability
    // check (3 consecutive reads at 500ms each) will confirm if the input is
    // complete. Previously, single alphanumeric chars were exempt (treated as
    // choice selection like "A", "B", "y"), but this caused a bug where "S"
    // from "Save as a draft." triggered an immediate run that sent a wrong email.
    //
    // The 1.5s delay on genuine single-char responses (like "y" or "A") is
    // acceptable — the cost of acting on partial input is much higher.
    if trimmed.len() == 1 {
        return true;
    }

    // Single word that looks like a command/keyword (e.g., "go", "ok", "release")
    // But NOT if the word contains a dot mid-word (could be partial URL like "crates.")
    if !trimmed.contains(' ') && trimmed.len() >= 2 {
        // Words ending with '.' that look like partial domains/URLs are truncated
        if trimmed.ends_with('.') && trimmed.chars().filter(|&c| c == '.').count() >= 1 {
            let before_dot = &trimmed[..trimmed.len() - 1];
            // Common TLD/domain fragments: if there's a word before the dot that looks
            // like a domain component, it's likely truncated (e.g., "crates." → "crates.io")
            if !before_dot.is_empty() && before_dot.chars().all(|c| c.is_alphanumeric() || c == '-')
            {
                return true;
            }
        }
        return false;
    }

    // Check last character for terminal punctuation
    let last_char = trimmed.chars().last().unwrap();

    // Dot needs special handling: "Fixed the bug." is complete, but "linking to crates." may not be.
    // Treat '.' as terminal UNLESS the last word before '.' looks like a domain/URL fragment
    // (no spaces, all alphanumeric/hyphens, suggesting something like "crates." → "crates.io").
    if last_char == '.' {
        let before_dot = &trimmed[..trimmed.len() - 1];
        // Find the last word (after last space)
        let last_word = before_dot
            .rsplit_once(' ')
            .map(|(_, w)| w)
            .unwrap_or(before_dot);
        // If last word contains dots already (e.g., "www.example.") or is a known domain-like
        // pattern, treat as potentially truncated
        if last_word.contains('.') || last_word.ends_with("http") || last_word.ends_with("https") {
            return true;
        }
        // Otherwise, '.' is terminal (normal sentence ending)
        return false;
    }

    let terminal = matches!(
        last_char,
        '!' | '?' | ':' | ';' | ')' | ']' | '"' | '\'' | '`' | '*' | '-' | '>' | '|'
    );

    !terminal
}

/// Truncate a string for log display.
fn truncate_for_log(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        // Find the last char boundary at or before `max` bytes
        let mut truncated = max;
        while truncated > 0 && !s.is_char_boundary(truncated) {
            truncated -= 1;
        }
        format!("{}...", &s[..truncated])
    }
}

/// This exposes the Rust truncation detection to external callers
/// (e.g., the Claude Code skill) before they compute their own diff.
pub fn run(file: &Path, wait: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if wait {
        let previous = snapshot::resolve(file)?.unwrap_or_default();
        let _stable = wait_for_stable_content(file, &previous)?;
        eprintln!("[diff --wait] content is stable");
    }
    match compute(file)? {
        Some(diff) => print!("{}", diff),
        None => eprintln!("No changes since last run."),
    }
    Ok(())
}
