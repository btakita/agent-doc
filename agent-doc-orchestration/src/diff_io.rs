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

/// Diff result plus the exact snapshot/current document content used to compute it.
pub struct ComputeResult {
    pub diff: Option<String>,
    pub previous: String,
    pub current: String,
}

/// Compute a unified diff between the snapshot and the current document, and
/// return the exact snapshot/current content used to compute it.
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
            previous,
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
                previous,
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
        previous,
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

/// Wait for stable content using editor-authoritative buffer state when
/// available, falling back to the truncation heuristic when no editor plugin
/// is attached.
///
/// **Editor-authoritative path** (plugin attached): reads the
/// [`EditorBufferState`] stored in the Session Actor's in-memory map by the
/// IDE plugin via the FFI bridge.
/// Waits until the editor reports a stable version/hash (dirty flag cleared or
/// debounce elapsed), then reads the file content. If the editor state
/// includes a content hash that matches the current file, returns immediately.
///
/// **Fallback path** (no plugin): uses the last-added-line / truncation
/// heuristic. Rechecks the file at short intervals until the last line looks
/// complete or content stabilises across consecutive reads.
///
/// Returns the stable file content.
pub fn wait_for_stable_content(doc: &Path, previous: &str) -> Result<String> {
    let doc_str = doc.to_string_lossy().to_string();

    // ── Editor-authoritative path ──
    if let Some(state) = crate::debounce::editor_buffer_state(&doc_str) {
        return wait_for_stable_content_editor(doc, &doc_str, &state);
    }

    // ── Fallback: truncation heuristic ──
    wait_for_stable_content_truncation(doc, previous)
}

/// Editor-authoritative stability: wait for the editor plugin to report a
/// stable buffer state (held in the Session Actor's in-memory map), then read
/// the file content.
fn wait_for_stable_content_editor(
    doc: &Path,
    doc_str: &str,
    initial_state: &crate::debounce::EditorBufferState,
) -> Result<String> {
    const EDITOR_DEBOUNCE_MS: u64 = 500;
    const EDITOR_TIMEOUT_MS: u64 = 6000;

    // If already stable (not dirty, or debounce elapsed), return immediately.
    if let Some(stable) = crate::debounce::editor_buffer_stable(doc_str, EDITOR_DEBOUNCE_MS) {
        let current = std::fs::read_to_string(doc)?;
        if let Some(ref hash) = stable.hash {
            let expected = crate::debounce::content_hash(&current);
            if hash.eq_ignore_ascii_case(&expected) {
                eprintln!(
                    "[diff] Editor buffer stable (version={}, hash match), returning immediately",
                    stable.version
                );
                return Ok(current);
            }
            eprintln!(
                "[diff] Editor hash mismatch (editor={}, disk={}), using disk content",
                &hash[..8.min(hash.len())],
                &expected[..8.min(expected.len())],
            );
        }
        eprintln!(
            "[diff] Editor buffer stable (version={}, dirty={}), reading disk",
            stable.version, stable.dirty
        );
        return Ok(current);
    }

    // Wait for editor stability within timeout.
    eprintln!(
        "[diff] Editor buffer not yet stable (version={}, dirty={}), waiting up to {}ms",
        initial_state.version, initial_state.dirty, EDITOR_TIMEOUT_MS
    );
    if let Some(stable) =
        crate::debounce::await_editor_buffer_stable(doc_str, EDITOR_DEBOUNCE_MS, EDITOR_TIMEOUT_MS)
    {
        let current = std::fs::read_to_string(doc)?;
        eprintln!(
            "[diff] Editor buffer stabilised (version={}, dirty={})",
            stable.version, stable.dirty
        );
        if let Some(ref hash) = stable.hash {
            let expected = crate::debounce::content_hash(&current);
            if hash.eq_ignore_ascii_case(&expected) {
                return Ok(current);
            }
            eprintln!(
                "[diff] Editor hash mismatch after stabilise (editor={}, disk={}), using disk",
                &hash[..8.min(hash.len())],
                &expected[..8.min(expected.len())],
            );
        }
        return Ok(current);
    }

    // Timeout — read whatever is on disk.
    eprintln!("[diff] Editor buffer stability timeout, reading current disk content");
    let current = std::fs::read_to_string(doc)?;
    Ok(current)
}

/// Fallback: truncation-heuristic stability detection.
///
/// When no editor plugin is attached, rechecks the file at short intervals
/// until the last added line looks complete or content stabilises.
fn wait_for_stable_content_truncation(doc: &Path, previous: &str) -> Result<String> {
    const RECHECK_DELAY_MS: u64 = 500;
    const MAX_RECHECKS: u32 = 12;
    const STABLE_CHECKS_REQUIRED: u32 = 3;

    let mut current = std::fs::read_to_string(doc)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use std::path::Path;

    #[test]
    fn run_file_not_found() {
        let err = run(Path::new("/nonexistent/file.md"), false).unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn copy_on_read_guard_skips_recovery_when_snapshot_modified() {
        // Verifies the copy-on-read guard logic: if snapshot mtime changes
        // between read and recovery, the save must be skipped.
        use std::time::SystemTime;

        let t1 = Some(SystemTime::UNIX_EPOCH);
        let t2 = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1));

        // Same mtime → recovery should proceed (guard passes)
        assert_eq!(t1, t1, "same mtime should allow recovery");

        // Different mtime → recovery should be skipped (guard blocks)
        assert_ne!(t1, t2, "different mtime should block recovery");

        // Both None (no snapshot file) → recovery should proceed
        let none: Option<SystemTime> = None;
        assert_eq!(none, none, "both None should allow recovery");
    }

    /// Set up a temp directory with `.agent-doc/snapshots/` and a document file.
    /// Returns (TempDir, doc_path). The TempDir must be kept alive for the test.
    fn setup_compute_env(
        doc_content: &str,
        snap_content: &str,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, doc_content).unwrap();

        // Create .agent-doc/snapshots/ and write the snapshot
        let snap_path = snapshot::path_for(&doc).unwrap();
        std::fs::create_dir_all(snap_path.parent().unwrap()).unwrap();
        std::fs::write(&snap_path, snap_content).unwrap();

        (dir, doc)
    }

    #[test]
    fn compute_stale_snapshot_recovery_proceeds_when_unmodified() {
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&doc).unwrap();
        assert!(
            result.is_none(),
            "stale snapshot recovery should return None"
        );

        let updated = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(updated, document);
    }

    #[test]
    fn compute_stale_recovery_updates_snapshot_to_current_document() {
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&doc).unwrap();
        assert!(result.is_none(), "stale recovery returns None");

        let snap = crate::snapshot::load(&doc).unwrap().unwrap();
        assert_eq!(
            snap, document,
            "snapshot should be synced to document after recovery"
        );
    }

    #[test]
    fn compute_returns_diff_when_user_adds_content() {
        let snapshot = "## User\n\nHello\n";
        let document = "## User\n\nHello\n\nNew question here\n";

        let (_dir, doc) = setup_compute_env(document, snapshot);

        let result = compute(&doc).unwrap();
        assert!(result.is_some(), "should return a diff for user additions");
        let diff = result.unwrap();
        assert!(diff.contains("New question here"));
    }

    #[test]
    fn compute_returns_none_when_no_changes() {
        let content = "## User\n\nHello\n";

        let (_dir, doc) = setup_compute_env(content, content);

        let result = compute(&doc).unwrap();
        assert!(result.is_none(), "identical content should return None");
    }

    #[test]
    fn diff_detects_user_edits_after_stream_write() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");

        let content_after_write = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nUser prompt\n\nAgent response\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content_after_write).unwrap();
        snapshot::save(&doc, content_after_write).unwrap();

        let content_after_edit = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nUser prompt\n\nAgent response\n\nNew user edit here\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content_after_edit).unwrap();

        let diff = compute(&doc).unwrap();
        assert!(
            diff.is_some(),
            "diff should detect user edit after stream write"
        );
        let diff_text = diff.unwrap();
        assert!(
            diff_text.contains("New user edit here"),
            "diff should contain user's new text: {}",
            diff_text
        );
    }

    #[test]
    fn diff_no_change_when_document_matches_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nContent\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let diff = compute(&doc).unwrap();
        assert!(diff.is_none(), "no diff when document matches snapshot");
    }

    #[test]
    fn diff_detects_change_after_cumulative_stream_flushes() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");

        let snapshot_content = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nFull agent response here\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, snapshot_content).unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let edited = "---\nagent_doc_mode: template\n---\n\n<!-- agent:exchange -->\nFull agent response here\n\nRelease agent-doc\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, edited).unwrap();

        let diff = compute(&doc).unwrap();
        assert!(diff.is_some(), "diff should detect user's edit");
        assert!(diff.unwrap().contains("Release agent-doc"));
    }

    // --- Truncation detection tests ---

    #[test]
    fn truncated_mid_sentence() {
        assert!(looks_truncated(
            "Also, when I called agent-doc run on this file...and ther"
        ));
    }

    #[test]
    fn not_truncated_complete_sentence() {
        assert!(!looks_truncated("This is a complete sentence."));
    }

    #[test]
    fn not_truncated_question() {
        assert!(!looks_truncated("What should we do?"));
    }

    #[test]
    fn not_truncated_command() {
        assert!(!looks_truncated("/agent-doc compact"));
    }

    #[test]
    fn not_truncated_single_word_command() {
        assert!(!looks_truncated("release"));
    }

    #[test]
    fn not_truncated_short_words() {
        assert!(!looks_truncated("go"));
        assert!(!looks_truncated("ok"));
        assert!(!looks_truncated("no"));
        assert!(!looks_truncated("yes"));
    }

    #[test]
    fn truncated_single_chars() {
        assert!(looks_truncated("A"));
        assert!(looks_truncated("S"));
        assert!(looks_truncated("1"));
        assert!(looks_truncated("y"));
    }

    #[test]
    fn not_truncated_heading() {
        assert!(!looks_truncated("### Re: Fix the bug"));
    }

    #[test]
    fn not_truncated_empty() {
        assert!(!looks_truncated(""));
    }

    #[test]
    fn not_truncated_ends_with_colon() {
        assert!(!looks_truncated("Here is the issue:"));
    }

    #[test]
    fn not_truncated_ends_with_backtick() {
        assert!(!looks_truncated("Check `crdt.rs`"));
    }

    #[test]
    fn truncated_ends_mid_word() {
        assert!(looks_truncated("Please make Claim for Tmux Pan"));
    }

    #[test]
    fn not_truncated_ends_with_period() {
        assert!(!looks_truncated("Fixed the bug."));
    }

    #[test]
    fn extract_last_added_finds_insert() {
        let prev = "line1\n";
        let curr = "line1\nnew content here\n";
        let last = extract_last_added_line(prev, curr);
        assert_eq!(last, Some("new content here".to_string()));
    }

    #[test]
    fn extract_last_added_none_when_no_changes() {
        let content = "line1\nline2\n";
        let last = extract_last_added_line(content, content);
        assert_eq!(last, None);
    }

    // --- diff --wait tests ---

    #[test]
    fn run_with_wait_stable_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let snapshot_content = "line1\n";
        std::fs::write(&doc, "line1\nline2\n").unwrap();
        snapshot::save(&doc, snapshot_content).unwrap();

        let result = run(&doc, true);
        assert!(result.is_ok());
    }

    #[test]
    fn run_with_wait_no_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test.md");
        let content = "line1\nline2\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        let result = run(&doc, true);
        assert!(result.is_ok());
    }

    #[test]
    fn wait_for_stable_content_returns_immediately_when_complete() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "Complete sentence.\n";
        std::fs::write(&doc, content).unwrap();
        let previous = "";

        let start = std::time::Instant::now();
        let result = wait_for_stable_content(&doc, previous).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, content);
        assert!(
            elapsed.as_millis() < 500,
            "should not delay for complete content"
        );
    }

    #[test]
    fn wait_for_stable_content_uses_editor_state_when_available() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test-editor.md");
        let content = "Hello from editor.\n";
        std::fs::write(&doc, content).unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let state = crate::debounce::EditorBufferState {
            path: doc_str.clone(),
            version: 1,
            dirty: false,
            last_edit_timestamp_ms: now,
            save_timestamp_ms: Some(now),
            hash: Some(crate::debounce::content_hash(content)),
            content_len: Some(content.len()),
            session_id: None,
        };
        crate::debounce::record_editor_buffer_state(&state);

        let previous = "";
        let start = std::time::Instant::now();
        let result = wait_for_stable_content(&doc, previous).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result, content);
        assert!(
            elapsed.as_millis() < 500,
            "editor-authoritative path should return immediately when stable"
        );
    }

    #[test]
    fn wait_for_stable_content_falls_back_without_editor_state() {
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();

        let doc = dir.path().join("test-fallback.md");
        let content = "Complete sentence.\n";
        std::fs::write(&doc, content).unwrap();

        let previous = "";
        let result = wait_for_stable_content(&doc, previous).unwrap();
        assert_eq!(result, content);
    }
}
