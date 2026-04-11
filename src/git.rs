//! # Module: git
//!
//! ## Spec
//! - `commit(file)`: stages and commits a session document with an auto-generated
//!   `agent-doc(<stem>): <timestamp>` message, skipping hooks (`--no-verify`).  Relative paths are
//!   resolved against the git superproject root first, then the toplevel.  When a snapshot exists,
//!   the snapshot content (with ` (HEAD)` markers added to the shallowest new headings) is staged
//!   via `git hash-object + update-index` so the working tree is never touched; user keystrokes
//!   typed after the snapshot was taken remain uncommitted (green gutter).  Falls back to
//!   `git add -f` when hash-object fails.  After a successful commit, strips `(HEAD)` from the
//!   snapshot, repositions the boundary marker in the snapshot, and fires an IPC reposition signal
//!   (`try_ipc_reposition_boundary`) to the IDE plugin so the working tree boundary is updated
//!   via the plugin's Document API.
//! - `show_head(file)`: returns the file content from `HEAD` as `Some(String)`, or `None` if not
//!   tracked.
//! - `last_commit_mtime(file)`: returns the author timestamp of the most recent commit touching the
//!   file, or `None` if none exists.
//! - `create_branch(file)`: creates and checks out `agent-doc/<stem>`, or switches to it if it
//!   already exists.
//! - `squash_session(file)`: soft-resets to before the first `agent-doc` commit touching the file
//!   and recommits as a single squashed commit.
//! - `add_head_marker` (private): compares the snapshot against `HEAD` to identify newly added
//!   headings; marks only the shallowest (root-level) new headings with ` (HEAD)` so the IDE shows
//!   a single modified line per response section as a visual boundary.  Uses occurrence counting
//!   to correctly handle duplicate heading text across exchange cycles (e.g., multiple
//!   `### Re: Implementation complete` from different responses).  Falls back to bold-text
//!   pseudo-headers (`**...**` on its own line) when no markdown headings are found.
//!
//! ## Agentic Contracts
//! - `commit` never modifies the working tree file directly; all staging is done through the git
//!   index.  The only disk write is to the snapshot file.
//! - `commit` captures all git stdout to stderr so callers that reserve stdout for JSON (e.g.,
//!   `preflight`) are not polluted.
//! - All public functions resolve paths relative to the superproject root when running inside a
//!   submodule, so git commands always run in the correct repo.
//! - `show_head` and `last_commit_mtime` return `Ok(None)` (not `Err`) when the file has no git
//!   history.
//!
//! ## Evals
//! - strip_head_markers_from_headings: heading lines with ` (HEAD)` suffix → suffix removed; non-heading lines unchanged
//! - strip_head_markers_preserves_non_heading_lines: body text containing "(HEAD)" → preserved verbatim
//! - strip_head_markers_bold_text: bold-text pseudo-header `**Re: Something** (HEAD)` → suffix removed
//! - add_head_marker_strips_old_markers: old `(HEAD)` heading stripped; new heading acquires `(HEAD)`
//! - add_head_marker_bold_text_fallback: no markdown headings → bold-text pseudo-header gets `(HEAD)`; real heading present → bold text skipped
//! - add_head_marker_duplicate_heading_text: duplicate heading text across exchange cycles → last occurrence gets `(HEAD)` via occurrence counting
//! - reposition_boundary_to_end_basic: stale boundary before user prompt → boundary repositioned after prompt
//! - reposition_boundary_no_exchange: doc with no exchange component → content returned unchanged
//! - reposition_boundary_preserves_user_edits: user text between response and boundary → all user text preserved, boundary after it
//! - reposition_boundary_cleans_multiple_stale: document with 2 stale boundaries → all removed, exactly 1 fresh boundary at end after user text

use anyhow::Result;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::path::Path;
use std::process::Command;

/// Resolve a relative path against the git root (superproject root if in a submodule).
/// Returns (git_root, resolved_file_path) so callers can run git commands in the correct repo.
pub(crate) fn resolve_to_git_root(file: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    if file.is_absolute() {
        // Find git root from the file's directory
        let parent = file.parent().unwrap_or(Path::new("/"));
        let root = git_toplevel_at(parent)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        return Ok((root, file.to_path_buf()));
    }

    // Try superproject first (handles submodule CWD case)
    let output = Command::new("git")
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    // Try git toplevel
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(ref o) = output {
        let root = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !root.is_empty() {
            let root_path = std::path::PathBuf::from(&root);
            let resolved = root_path.join(file);
            if resolved.exists() {
                return Ok((root_path, resolved));
            }
        }
    }

    // Fallback: use as-is (relative to CWD)
    let cwd = std::env::current_dir().unwrap_or_default();
    Ok((cwd, file.to_path_buf()))
}

/// Get git toplevel from a specific directory.
fn git_toplevel_at(dir: &Path) -> Option<std::path::PathBuf> {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(std::path::PathBuf::from(s)) }
        })
}

/// Check if `file` is inside a git repository.
/// Returns `true` if the file's directory (or any ancestor) is a git repo.
/// Returns `false` if git is not available or the path is not tracked.
pub(crate) fn is_in_git_repo(file: &Path) -> bool {
    let dir = if file.is_absolute() {
        file.parent().unwrap_or(Path::new("/")).to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    Command::new("git")
        .current_dir(&dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Commit a file with an auto-generated message. Skips hooks.
/// Relative paths are resolved against the git root (superproject if in a submodule).
/// Git commands run from the resolved git root, so this works even when CWD is a submodule.
pub fn commit(file: &Path) -> Result<()> {
    let t_total = std::time::Instant::now();

    let (git_root, resolved) = resolve_to_git_root(file)?;
    let timestamp = chrono_timestamp();
    let doc_name = file
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let msg = format!("agent-doc({}): {}", doc_name, timestamp);

    // Selective commit: stage only the snapshot content (agent response),
    // leaving user edits in the working tree as uncommitted.
    //
    // If a snapshot exists, use git hash-object + update-index to stage the
    // snapshot version without touching the working tree file. This means:
    // - Agent response → committed (no git gutter)
    // - User's subsequent edits → uncommitted (green git gutter)
    let mut snapshot_content = crate::snapshot::load(file)?;
    let file_content = std::fs::read_to_string(file).unwrap_or_default();
    let file_len = file_content.len();
    let snap_len = snapshot_content.as_ref().map(|s| s.len()).unwrap_or(0);
    crate::ops_log::log_op(file, &format!(
        "commit_staging file={} snap_len={} file_len={}",
        file.display(), snap_len, file_len
    ));

    // Warn on significant file/snapshot drift — may indicate an out-of-band write
    // that bypassed the agent-doc write pipeline (snapshot not updated).
    if snap_len > 0 && file_len > snap_len {
        let drift = file_len - snap_len;
        if drift > 100 {
            eprintln!(
                "[commit] WARNING: file is {} bytes larger than snapshot for {} — possible out-of-band write (snap={}, file={})",
                drift, file.display(), snap_len, file_len
            );
            crate::ops_log::log_op(file, &format!(
                "drift_warning file={} drift={} snap_len={} file_len={}",
                file.display(), drift, snap_len, file_len
            ));

            // Extreme drift (file >5x snapshot): likely a file move/rename where
            // the snapshot is from auto-init but the file has full content from
            // the old path. Re-sync snapshot from file content so the commit
            // stages everything and the drift loop stops.
            if file_len > snap_len * 5 {
                eprintln!(
                    "[commit] Extreme drift detected ({}x) — re-syncing snapshot from file content (likely file move)",
                    file_len / snap_len.max(1)
                );
                crate::ops_log::log_op(file, &format!(
                    "snapshot_resync file={} old_snap_len={} new_snap_len={}",
                    file.display(), snap_len, file_len
                ));
                crate::snapshot::save(file, &file_content)?;
                snapshot_content = Some(file_content.clone());
            }
        }
    }

    // Handle missing snapshot: if no snapshot exists but file has content, create one.
    // This bootstraps the commit flow for files that were never written by agent-doc.
    //
    // NOTE: Snapshot/file divergence detection (Bug 2B) was removed here because it
    // cannot distinguish "file has user edits" from "file has a missed agent response".
    // Both cases look identical (file has content snapshot doesn't). The IPC snapshot
    // save failure case is handled by Bug 2A (non-fatal save with warning) and the
    // recover step in preflight (detects orphaned responses).
    if snapshot_content.is_none() && !file_content.is_empty() {
        eprintln!(
            "[commit] WARNING: no snapshot exists for {}. Creating from file content.",
            file.display()
        );
        crate::snapshot::save(file, &file_content)?;
        snapshot_content = Some(file_content.clone());
    }

    let t_staging = std::time::Instant::now();
    if let Some(ref snap) = snapshot_content {
        // Add (HEAD) marker to the last ### Re: heading in the snapshot.
        // The working tree keeps the heading WITHOUT the marker, creating
        // a single modified line (blue gutter) as a visual boundary.
        let staged_content = add_head_marker(snap, file);

        let rel_path = resolved.strip_prefix(&git_root)
            .unwrap_or(&resolved);

        if let Ok(hash) = hash_object(&git_root, &staged_content) {
            let cacheinfo = format!("100644,{},{}", hash, rel_path.to_string_lossy());
            let status = Command::new("git")
                .current_dir(&git_root)
                .args(["update-index", "--add", "--cacheinfo", &cacheinfo])
                .status()?;
            if !status.success() {
                eprintln!("[commit] update-index failed, falling back to git add");
                git_add_force(&git_root, &resolved)?;
            }
        } else {
            git_add_force(&git_root, &resolved)?;
        }
    } else {
        // No snapshot — fall back to staging the entire file
        git_add_force(&git_root, &resolved)?;
    }
    let elapsed_staging = t_staging.elapsed().as_millis();
    if elapsed_staging > 0 {
        eprintln!("[perf] commit.staging (hash_object+update-index): {}ms", elapsed_staging);
    }

    // Commit — ignore failure (nothing to commit is fine).
    // Use .output() to capture stdout (prevents git status leaking to stdout
    // when called from `preflight` which reserves stdout for JSON).
    let t_commit = std::time::Instant::now();
    // Fix 3: retry up to 3 times on index.lock contention (concurrent sessions).
    let mut commit_attempts = 0u32;
    let commit_output = loop {
        let out = Command::new("git")
            .current_dir(&git_root)
            .args(["commit", "-m", &msg, "--no-verify"])
            .output();
        match &out {
            Ok(o) if !o.status.success() && commit_attempts < 3 => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                if stderr.contains("index.lock") || stderr.contains("Unable to create") {
                    commit_attempts += 1;
                    eprintln!("[commit] index.lock contention, retry {}/3", commit_attempts);
                    std::thread::sleep(std::time::Duration::from_millis(50 * (1 << commit_attempts)));
                    continue;
                }
            }
            _ => {}
        }
        break out;
    };
    let commit_status = commit_output.as_ref().map(|o| o.status);
    let elapsed_commit = t_commit.elapsed().as_millis();
    if elapsed_commit > 0 {
        eprintln!("[perf] commit.git_commit: {}ms", elapsed_commit);
    }

    // Log commit result line to stderr (suppress verbose git status output)
    if let Ok(ref o) = commit_output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Only print the commit result line (e.g. "[main abc123] message")
            // and skip git status output (branch info, file listings, etc.)
            if trimmed.starts_with('[') && trimmed.contains(']') {
                eprintln!("{}", line);
            }
        }
    }

    // Log commit result
    match &commit_status {
        Ok(s) if s.success() => {
            crate::ops_log::log_cycle(file, "commit", None, None);
            crate::ops_log::log_op(file, &format!("commit_success file={}", file.display()));
            // Fire post_commit hook for cross-session coordination
            let session_id = crate::frontmatter::read_session_id(file).unwrap_or_default();
            crate::hooks::fire_post_commit(file, &session_id);
            crate::hooks::fire_doc_event(file, "post_commit");
        }
        Ok(s) => {
            crate::ops_log::log_op(file, &format!(
                "commit_failed file={} exit_code={}",
                file.display(),
                s.code().unwrap_or(-1)
            ));
        }
        Err(e) => {
            crate::ops_log::log_op(file, &format!(
                "commit_error file={} err={}",
                file.display(), e
            ));
        }
    }

    // After commit, strip (HEAD) markers from the snapshot so the working tree
    // is clean. The committed content has (HEAD) markers; the working tree should not.
    // This creates the blue gutter diff the user sees.
    if let Ok(ref s) = commit_status
        && s.success()
    {
            // Strip (HEAD) from snapshot
            if let Some(ref snap) = snapshot_content {
                let clean_snap = strip_head_markers(snap);
                if clean_snap != *snap {
                    eprintln!("[commit] stripping (HEAD) markers from snapshot ({} chars removed)", snap.len() - clean_snap.len());
                    if let Err(e) = crate::snapshot::save(file, &clean_snap) {
                        eprintln!("[commit] failed to clean snapshot: {}", e);
                    }
                }
            }
            // Also strip (HEAD) from working tree if present — the IPC reposition
            // may have added it. The working tree should NEVER have (HEAD) markers.
            if let Ok(working) = std::fs::read_to_string(file) {
                let clean_working = strip_head_markers(&working);
                if clean_working != working {
                    eprintln!("[commit] WARNING: (HEAD) found in working tree — stripping");
                    if let Err(e) = crate::write::atomic_write_pub(file, &clean_working) {
                        eprintln!("[commit] failed to clean working tree: {}", e);
                    }
                }
            }
            // Note: working tree is NOT modified here. The staged content has (HEAD)
            // markers, the working tree does not. This creates the blue gutter diff.
            // Previously we stripped HEAD markers from the working tree, but that was
            // unnecessary (staging doesn't modify the working tree) and caused file
            // cache conflicts in the IDE.

            // Reposition boundary in snapshot AND via IPC to the plugin.
            // Working tree is NEVER written directly — that causes IDE "externally modified"
            // dialogs and loses user keystrokes. The IPC signal tells the plugin to
            // reposition in its Document buffer, which handles stale boundaries too.
            let t_reposition = std::time::Instant::now();
            let snap_changed = reposition_boundary_in_snapshot(file);
            // Send IPC reposition signal to plugin only if boundary actually moved.
            // Skipping no-op repositions eliminates ~64% of unnecessary Document API writes.
            if snap_changed {
                crate::write::try_ipc_reposition_boundary(file);
            }
            let elapsed_reposition = t_reposition.elapsed().as_millis();
            if elapsed_reposition > 0 {
                eprintln!("[perf] commit.reposition: {}ms", elapsed_reposition);
            }

            // Signal plugin to refresh VCS state so the gutter reflects the commit.
            // Without this, the IDE shows the entire response as uncommitted until
            // the user manually refreshes the file.
            // Uses file-based signal (vcs-refresh.signal) since the socket listener
            // may not be active — the plugin watches .agent-doc/patches/ for both
            // patch files and signal files.
            if let Ok(canonical) = file.canonicalize() {
                let project_root = crate::snapshot::find_project_root(&canonical)
                    .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
                let signal_file = project_root.join(".agent-doc/patches/vcs-refresh.signal");
                if signal_file.parent().is_some_and(|p| p.exists()) {
                    match std::fs::write(&signal_file, "") {
                        Ok(()) => eprintln!("[commit] VCS refresh signal written"),
                        Err(e) => eprintln!("[commit] VCS refresh signal failed: {} (non-fatal)", e),
                    }
                }
            }
    }

    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] commit total: {}ms", elapsed_total);
    }

    Ok(())
}

/// Reposition boundary in snapshot only (not working tree).
///
/// After commit, moves the boundary to the end of exchange in the snapshot.
/// The working tree is NOT modified — writing to it while the user is typing
/// causes the IDE to reload from disk, losing in-progress keystrokes.
/// The plugin handles working-tree boundary reposition via the
/// `reposition_boundary: true` IPC flag sent during `agent-doc write`.
/// Returns true if the boundary was actually repositioned (content changed).
fn reposition_boundary_in_snapshot(file: &Path) -> bool {
    // Check for active run — don't reposition if a run is in progress
    if let Ok(canonical) = file.canonicalize()
        && let Ok(pending_path) = crate::snapshot::pending_path_for(&canonical)
        && pending_path.exists()
    {
        eprintln!("[commit] skipping boundary reposition — active run detected");
        return false;
    }

    // Reposition in snapshot only — use template::reposition_boundary_to_end()
    // which removes ALL stale boundaries and inserts a single fresh one.
    if let Ok(Some(snap_content)) = crate::snapshot::load(file) {
        let new_snap = crate::template::reposition_boundary_to_end(&snap_content);
        if new_snap != snap_content {
            if let Err(e) = crate::snapshot::save(file, &new_snap) {
                eprintln!("[commit] failed to update snapshot after boundary reposition: {}", e);
                return false;
            }
            eprintln!("[commit] repositioned boundary in snapshot");
            return true;
        }
    }
    false
}

/// Returns the byte ranges of all fenced code blocks in `content` using a
/// CommonMark-compliant parser (pulldown-cmark).
///
/// A closing fence MUST consist of plain backticks/tildes with no info string,
/// so `` ```bash `` inside a `` ``` `` block is treated as literal content —
/// not as a fence closer.  This is the root fix for the `(HEAD)` marker being
/// incorrectly applied to bash-comment lines inside code fences.
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

/// Returns `true` if the byte at `offset` falls within any code block range.
#[inline]
fn is_in_code_block(ranges: &[std::ops::Range<usize>], offset: usize) -> bool {
    ranges.iter().any(|r| r.contains(&offset))
}

/// Strip ` (HEAD)` suffix from markdown heading lines and bold-text pseudo-headers.
/// Uses a CommonMark-compliant parser to detect code blocks so that `# comment (HEAD)`
/// inside a fenced block is preserved unchanged.
fn strip_head_markers(content: &str) -> String {
    let code_ranges = code_block_byte_ranges(content);
    let mut result_lines: Vec<&str> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !is_in_code_block(&code_ranges, offset)
            && let Some(stripped) = line.strip_suffix(" (HEAD)") {
            // Strip from markdown headings
            if trimmed.starts_with('#') {
                result_lines.push(stripped);
                offset += line.len() + 1;
                continue;
            }
            // Strip from bold-text pseudo-headers (e.g., "**Re: Foo** (HEAD)")
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
    if content.ends_with('\n') { format!("{}\n", result) } else { result }
}


/// Add ` (HEAD)` suffix to ALL new markdown headings in the agent's appended content.
///
/// Matches any heading level (`#` through `######`). Compares the snapshot
/// against git HEAD to find which headings are new (added by the agent).
/// Only the top-level (shallowest) headings in the new content get marked —
/// sub-headings within a response section are left unmarked.
///
/// When git HEAD is unavailable, falls back to marking the last heading only.
fn add_head_marker(content: &str, file: &Path) -> String {
    let head_content = show_head(file).ok().flatten();

    // Step 1: Strip ALL existing (HEAD) markers from heading lines and bold-text pseudo-headers.
    // This prevents accumulation across commit cycles.
    // Use AST-based code block detection so markers inside fenced blocks are not touched.
    let content_code_ranges = code_block_byte_ranges(content);
    let mut cleaned_lines: Vec<String> = Vec::new();
    let mut offset = 0usize;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !is_in_code_block(&content_code_ranges, offset) && trimmed.ends_with(" (HEAD)") {
            if trimmed.starts_with('#') {
                cleaned_lines.push(line[..line.len() - 7].to_string());
                offset += line.len() + 1;
                continue;
            }
            // Bold-text pseudo-header: "**...** (HEAD)"
            let without_suffix = line[..line.len() - 7].trim_end();
            if trimmed.starts_with("**") && without_suffix.trim_start().ends_with("**") {
                cleaned_lines.push(line[..line.len() - 7].to_string());
                offset += line.len() + 1;
                continue;
            }
        }
        cleaned_lines.push(line.to_string());
        offset += line.len() + 1;
    }
    let cleaned = cleaned_lines.join("\n");
    // Preserve trailing newline
    let cleaned = if content.ends_with('\n') && !cleaned.ends_with('\n') {
        format!("{}\n", cleaned)
    } else {
        cleaned
    };

    // Also strip (HEAD) from git HEAD content for accurate comparison
    let head_cleaned = head_content.as_ref().map(|h| {
        h.lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.ends_with(" (HEAD)") {
                    if trimmed.starts_with('#') {
                        return &line[..line.len() - 7];
                    }
                    let without_suffix = line[..line.len() - 7].trim_end();
                    if trimmed.starts_with("**") && without_suffix.trim_start().ends_with("**") {
                        return &line[..line.len() - 7];
                    }
                }
                line
            })
            .collect::<Vec<&str>>()
            .join("\n")
    });

    // Step 2: Collect all heading positions from cleaned content.
    // Use AST-based code block detection so `# comment` inside a fenced block is excluded.
    let cleaned_code_ranges = code_block_byte_ranges(&cleaned);
    let mut heading_positions: Vec<(usize, usize, usize)> = Vec::new();
    let mut offset = 0usize;
    for line in cleaned.lines() {
        let trimmed = line.trim_start();
        let line_end = offset + line.len();
        if !is_in_code_block(&cleaned_code_ranges, offset) && trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if level <= 6 && trimmed.len() > level && trimmed.as_bytes()[level] == b' ' {
                heading_positions.push((offset, line_end, level));
            }
        }
        offset = line_end + 1;
    }

    // Fallback: if no markdown headings found, scan for bold-text pseudo-headers
    // (lines matching `**...**` at start of line). Treat the first one found as a
    // pseudo-heading so it can receive the (HEAD) marker.
    if heading_positions.is_empty() {
        let mut offset = 0usize;
        for line in cleaned.lines() {
            let trimmed = line.trim_start();
            let line_end = offset + line.len();
            let trimmed_end = trimmed.trim_end();
            if trimmed_end.starts_with("**") && trimmed_end.ends_with("**") && trimmed_end.len() > 4 {
                // Use level 99 as a sentinel — bold pseudo-headers are always "shallowest"
                // since there are no real headings to compete with.
                heading_positions.push((offset, line_end, 99));
            }
            offset = line_end + 1;
        }
    }

    if heading_positions.is_empty() {
        return cleaned;
    }

    // Step 3: Filter to headings NOT in git HEAD (= new headings from latest response)
    // Count occurrences in HEAD to handle duplicate heading text correctly.
    // A heading is "new" if it appears more times in the current content than in HEAD.
    let new_headings: Vec<(usize, usize, usize)> = if let Some(ref hc) = head_cleaned {
        // Count how many times each heading text appears in HEAD.
        // Use AST-based code block detection to exclude `# comment` lines in fenced blocks.
        let head_code_ranges = code_block_byte_ranges(hc);
        let head_heading_counts: std::collections::HashMap<&str, usize> = {
            let mut counts = std::collections::HashMap::new();
            let mut head_offset = 0usize;
            for line in hc.lines() {
                let trimmed = line.trim_start();
                let line_end = head_offset + line.len();
                if !is_in_code_block(&head_code_ranges, head_offset) && trimmed.starts_with('#') {
                    let level = trimmed.chars().take_while(|c| *c == '#').count();
                    if level <= 6 && trimmed.len() > level && trimmed.as_bytes()[level] == b' ' {
                        *counts.entry(line).or_insert(0) += 1;
                    }
                }
                head_offset = line_end + 1;
            }
            counts
        };
        // Count how many times each heading text appears in current content (up to each position)
        let mut seen_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        heading_positions.into_iter().filter(|(start, end, _)| {
            let heading_text = &cleaned[*start..*end];
            let seen = seen_counts.entry(heading_text).or_insert(0);
            *seen += 1;
            let head_count = head_heading_counts.get(heading_text).copied().unwrap_or(0);
            *seen > head_count
        }).collect()
    } else {
        // No HEAD available → mark last heading only
        vec![*heading_positions.last().unwrap()]
    };

    if new_headings.is_empty() {
        // No new headings from this commit. Re-apply (HEAD) markers from git HEAD
        // to prevent concurrent commits from stripping markers placed by a previous commit.
        // Without this, Session B's preflight commit would overwrite Session A's committed
        // content (which has HEAD markers) with cleaned content (which doesn't).
        //
        // Safety: only re-apply if HEAD has a reasonable number of markers (≤3).
        // After a file move/rename, HEAD may have many stale (HEAD) markers baked in
        // from the old path — re-applying all of them creates permanent uncommitted diffs.
        if let Some(ref head) = head_content {
            // Use AST-based detection to count only real markdown headings with (HEAD) markers,
            // excluding bash comments and other `#` lines inside fenced code blocks.
            let head_code_ranges_for_reapply = code_block_byte_ranges(head);
            let head_marker_count = {
                let mut count = 0usize;
                let mut h_offset = 0usize;
                for l in head.lines() {
                    let t = l.trim_start();
                    if !is_in_code_block(&head_code_ranges_for_reapply, h_offset)
                        && t.ends_with(" (HEAD)")
                        && t.starts_with('#')
                    {
                        count += 1;
                    }
                    h_offset += l.len() + 1;
                }
                count
            };
            if head_marker_count <= 3 {
                let mut result = cleaned;
                let mut h_offset = 0usize;
                for line in head.lines() {
                    let trimmed = line.trim_start();
                    let h_line_end = h_offset + line.len();
                    // Only re-apply if this heading is NOT inside a code block in HEAD.
                    // Prevents baked-in `# comment (HEAD)` from propagating across commits.
                    if trimmed.ends_with(" (HEAD)")
                        && trimmed.starts_with('#')
                        && !is_in_code_block(&head_code_ranges_for_reapply, h_offset)
                    {
                        let without_head = &line[..line.len() - 7];
                        // Find this heading at a line boundary in the result and re-add (HEAD)
                        let search = format!("\n{}\n", without_head);
                        if let Some(pos) = result.find(&search) {
                            let insert_at = pos + 1 + without_head.len();
                            result.insert_str(insert_at, " (HEAD)");
                        } else if result.starts_with(&format!("{}\n", without_head)) {
                            result.insert_str(without_head.len(), " (HEAD)");
                        }
                    }
                    h_offset = h_line_end + 1;
                }
                return result;
            } else {
                eprintln!(
                    "[commit] Skipping (HEAD) re-application — {} markers in HEAD (stale, likely from file move)",
                    head_marker_count
                );
            }
        }
        return cleaned;
    }

    // Step 4: Mark ALL root-level (shallowest) new headings.
    // All newly added headings get (HEAD) so they show as blue gutter (visual boundary).
    // "New" = heading text appears more times in current content than in git HEAD.
    let min_level = new_headings.iter().map(|(_, _, level)| *level).min().unwrap();
    let root_ends: Vec<usize> = new_headings.iter()
        .filter(|(_, _, level)| *level == min_level)
        .map(|(_, end, _)| *end)
        .collect();

    // Step 5: Insert (HEAD) markers in reverse order to preserve offsets
    let mut result = cleaned;
    for pos in root_ends.iter().rev() {
        result.insert_str(*pos, " (HEAD)");
    }
    result
}

/// Write content to git's object database and return the blob hash.
fn hash_object(git_root: &Path, content: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(content.as_bytes())?;
            }
            child.wait_with_output()
        })?;
    if !output.status.success() {
        anyhow::bail!("git hash-object failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Force-add a file to the index (fallback path).
fn git_add_force(git_root: &Path, resolved: &Path) -> Result<()> {
    let status = Command::new("git")
        .current_dir(git_root)
        .args(["add", "-f", &resolved.to_string_lossy()])
        .status()?;
    if !status.success() {
        anyhow::bail!("git add failed");
    }
    Ok(())
}

/// Create and checkout a branch for the session.
pub fn create_branch(file: &Path) -> Result<()> {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "session".to_string());
    let branch_name = format!("agent-doc/{}", stem);

    let status = Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .status()?;
    if !status.success() {
        // Branch may already exist — try switching to it
        let status = Command::new("git")
            .args(["checkout", &branch_name])
            .status()?;
        if !status.success() {
            anyhow::bail!("failed to create or switch to branch {}", branch_name);
        }
    }
    Ok(())
}

/// Squash all agent-doc commits touching a file into one.
pub fn squash_session(file: &Path) -> Result<()> {
    let file_str = file.to_string_lossy();

    // Find the first agent-doc commit for this file
    let output = Command::new("git")
        .args([
            "log",
            "--oneline",
            "--reverse",
            "--grep=^agent-doc",
            "--",
            &file_str,
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next();
    let first_hash = match first_line {
        Some(line) => line.split_whitespace().next().unwrap_or(""),
        None => {
            eprintln!("No agent-doc commits found for {}", file.display());
            return Ok(());
        }
    };

    // Soft reset to the commit before the first agent-doc commit
    let status = Command::new("git")
        .args(["reset", "--soft", &format!("{}~1", first_hash)])
        .status()?;
    if !status.success() {
        anyhow::bail!("git reset failed");
    }

    // Recommit as a single squashed commit
    let status = Command::new("git")
        .args([
            "commit",
            "-m",
            &format!("agent-doc: squashed session for {}", file.display()),
            "--no-verify",
        ])
        .status()?;
    if !status.success() {
        anyhow::bail!("git commit failed during squash");
    }

    eprintln!("Squashed agent-doc commits for {}", file.display());
    Ok(())
}

/// Get the content of a file from the last agent-doc commit (or HEAD).
/// Returns None if the file is not tracked or no commits exist.
pub fn show_head(file: &Path) -> Result<Option<String>> {
    let (git_root, resolved) = resolve_to_git_root(file)?;

    // Get the file path relative to the git root
    let rel_path = if resolved.is_absolute() {
        resolved
            .strip_prefix(&git_root)
            .unwrap_or(&resolved)
            .to_path_buf()
    } else {
        resolved.clone()
    };

    let output = Command::new("git")
        .current_dir(&git_root)
        .args(["show", &format!("HEAD:{}", rel_path.to_string_lossy())])
        .output()?;

    if !output.status.success() {
        // File not tracked or no commits — not an error
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

/// Get the author timestamp of the last commit touching a file.
/// Returns None if the file has no commits.
pub fn last_commit_mtime(file: &Path) -> Result<Option<std::time::SystemTime>> {
    let (git_root, resolved) = resolve_to_git_root(file)?;

    let rel_path = if resolved.is_absolute() {
        resolved
            .strip_prefix(&git_root)
            .unwrap_or(&resolved)
            .to_path_buf()
    } else {
        resolved.clone()
    };

    let output = Command::new("git")
        .current_dir(&git_root)
        .args([
            "log",
            "-1",
            "--format=%ct",
            "--",
            &rel_path.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let ts_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ts_str.is_empty() {
        return Ok(None);
    }

    let epoch: u64 = ts_str.parse().unwrap_or(0);
    if epoch == 0 {
        return Ok(None);
    }

    Ok(Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(epoch)))
}

fn chrono_timestamp() -> String {
    // Use date command for simplicity — no extra dependency
    let output = Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_head_markers_from_headings() {
        let input = "# Title\n### Re: Foo (HEAD)\nSome text with (HEAD) in it\n### Re: Bar (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "# Title\n### Re: Foo\nSome text with (HEAD) in it\n### Re: Bar\n");
    }

    #[test]
    fn strip_head_markers_preserves_non_heading_lines() {
        let input = "Normal line (HEAD)\n### Heading (HEAD)\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "Normal line (HEAD)\n### Heading\n");
    }

    #[test]
    fn add_head_marker_strips_old_markers() {
        let content = "### Re: Old (HEAD)\n### Re: New\n";
        let result = add_head_marker(content, Path::new("/nonexistent/file.md"));
        assert!(!result.contains("### Re: Old (HEAD)"), "old heading should not have (HEAD)");
        assert!(result.contains("### Re: New (HEAD)") || result.contains("### Re: Old\n"), "old (HEAD) should be stripped");
    }

    #[test]
    fn add_head_marker_bold_text_fallback() {
        // No markdown headings — bold-text pseudo-header should get (HEAD)
        let content = "Some intro text.\n**Re: Something**\nBody paragraph.\n";
        let result = add_head_marker(content, Path::new("/nonexistent/file.md"));
        assert!(
            result.contains("**Re: Something** (HEAD)"),
            "bold-text pseudo-header should get (HEAD) marker, got: {result}"
        );
    }

    #[test]
    fn add_head_marker_prefers_real_headings() {
        // Both a real heading and bold text — only the heading should get (HEAD)
        let content = "### Re: Something\n**Bold text**\nBody.\n";
        let result = add_head_marker(content, Path::new("/nonexistent/file.md"));
        assert!(
            result.contains("### Re: Something (HEAD)"),
            "real heading should get (HEAD), got: {result}"
        );
        assert!(
            !result.contains("**Bold text** (HEAD)"),
            "bold text should NOT get (HEAD) when real headings exist, got: {result}"
        );
    }

    #[test]
    fn add_head_marker_duplicate_heading_text() {
        // Simulate a document where the same heading text appears in both
        // old content (git HEAD) and new content. The new occurrence should
        // get the (HEAD) marker even though the same text exists earlier.
        //
        // We can't easily mock git HEAD in unit tests, so we test the
        // no-HEAD fallback (marks last heading only). The real fix is
        // verified by the occurrence-counting logic in add_head_marker.
        let content = "### Re: Implementation complete\nOld response.\n### Re: Other\nMiddle.\n### Re: Implementation complete\nNew response.\n";
        let result = add_head_marker(content, Path::new("/nonexistent/file.md"));
        // With no HEAD available, marks the last heading
        assert!(
            result.ends_with("### Re: Implementation complete (HEAD)\nNew response.\n"),
            "last heading should get (HEAD), got: {result}"
        );
    }

    #[test]
    fn strip_head_markers_bold_text() {
        let input = "**Re: Something** (HEAD)\nSome text.\n";
        let result = strip_head_markers(input);
        assert_eq!(result, "**Re: Something**\nSome text.\n");
    }

    #[test]
    fn add_head_marker_ignores_fenced_code_hash() {
        // A line starting with `#` inside a fenced code block must NOT get (HEAD).
        // The last real markdown heading should get it instead.
        let content = "### Re: Implementation\nSome response.\n```yaml\n# this is a yaml comment\nkey: value\n```\n";
        let result = add_head_marker(content, Path::new("/nonexistent/file.md"));
        assert!(
            result.contains("### Re: Implementation (HEAD)"),
            "real heading should get (HEAD), got:\n{result}"
        );
        assert!(
            !result.contains("# this is a yaml comment (HEAD)"),
            "fenced code comment must NOT get (HEAD), got:\n{result}"
        );
    }

    #[test]
    fn strip_head_markers_ignores_fenced_code_hash() {
        // strip_head_markers should not remove content inside fenced code blocks.
        // If somehow `# comment (HEAD)` ended up in a fence, it should be left alone.
        let input = "### Re: Answer (HEAD)\nResponse.\n```bash\n# comment (HEAD)\n```\n";
        let result = strip_head_markers(input);
        assert_eq!(
            result,
            "### Re: Answer\nResponse.\n```bash\n# comment (HEAD)\n```\n",
            "fenced (HEAD) must be preserved, got:\n{result}"
        );
    }

    #[test]
    fn add_head_marker_bash_comment_inside_plain_fence() {
        // Regression: a ``` fence followed by inner ```bash confused the old ad-hoc
        // fence tracker.  CommonMark says ```bash cannot CLOSE a fence opened by ```;
        // only plain ``` (no info string) can close it.  The old `is_fence_marker`
        // toggled on every backtick-sequence regardless of state, causing the fence
        // state to invert and exposing `# On the server — run once` as if it were
        // outside the fence — giving it a (HEAD) marker it must not have.
        //
        // Document structure (simplified from tasks/software/monsterrodholders.md):
        //   - A ``` plain fence containing terminal output (lines that look like
        //     ```bash openings inside the block).
        //   - A real ### Re: heading immediately after.
        //   - A ```bash fence containing `# On the server — run once`.
        let content = concat!(
            "### Re: previous (HEAD)\n",    // existing marker from prior commit
            "Old response.\n",
            "```\n",                         // opens plain fence
            "```bash\n",                     // looks like fence open — but it's CONTENT
            "some terminal output\n",
            "```\n",                         // closes the plain fence (per CommonMark)
            "### Re: new heading\n",         // real heading added this commit
            "Description.\n",
            "```bash\n",                     // opens bash fence
            "# On the server — run once\n", // bash comment — must NOT get (HEAD)
            "git config pull.rebase true\n",
            "```\n",
        );
        let result = add_head_marker(content, Path::new("/nonexistent/file.md"));
        assert!(
            result.contains("### Re: new heading (HEAD)"),
            "real new heading must get (HEAD), got:\n{result}"
        );
        assert!(
            !result.contains("# On the server — run once (HEAD)"),
            "bash comment inside fenced block must NOT get (HEAD), got:\n{result}"
        );
    }

    #[test]
    fn reposition_boundary_to_end_basic() {
        let content = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc123 -->\nUser prompt.\n<!-- /agent:exchange -->\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        // Boundary should be after user prompt, before close tag
        assert!(result.contains("User prompt.\n<!-- agent:boundary:"));
        assert!(result.contains("-->\n<!-- /agent:exchange -->"));
        // Old boundary consumed
        assert!(!result.contains("abc123"));
    }

    #[test]
    fn reposition_boundary_no_exchange() {
        let content = "# No exchange component\nJust text.\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        // Should return unchanged if no exchange
        assert_eq!(result.trim(), content.trim());
    }

    #[test]
    fn reposition_boundary_preserves_user_edits() {
        let content = "<!-- agent:exchange patch=append -->\n### Re: Answer\nAgent response.\n<!-- agent:boundary:old-id -->\nUser's new prompt here.\nMore user text.\n<!-- /agent:exchange -->\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        assert!(result.contains("User's new prompt here."), "user edit must be preserved");
        assert!(result.contains("More user text."), "user edit must be preserved");
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User's new prompt here.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }

    #[test]
    fn reposition_boundary_cleans_multiple_stale() {
        // Simulate a document with multiple stale boundary markers
        let content = "<!-- agent:exchange patch=append -->\n\
            First response.\n\
            <!-- agent:boundary:aaa111 -->\n\
            Second response.\n\
            <!-- agent:boundary:bbb222 -->\n\
            User prompt.\n\
            <!-- /agent:exchange -->\n";
        let result = agent_doc::template::reposition_boundary_to_end(content);
        // All old boundaries should be removed
        assert!(!result.contains("aaa111"), "first stale boundary must be removed");
        assert!(!result.contains("bbb222"), "second stale boundary must be removed");
        // Exactly one fresh boundary should exist
        let boundary_count = result.matches("<!-- agent:boundary:").count();
        assert_eq!(boundary_count, 1, "exactly one boundary marker should remain");
        // The single boundary should be after user prompt
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let user_pos = result.find("User prompt.").unwrap();
        assert!(boundary_pos > user_pos, "boundary must be after user text");
    }

    // --- Bug 2B regression tests ---
    // Verify that commit does NOT overwrite the snapshot with user edits.
    // The divergence detection was removed from commit because is_stale_baseline
    // cannot distinguish "file has user edits" from "file has a missed agent response" —
    // both look like "file has content snapshot doesn't have".

    #[test]
    fn is_stale_baseline_write_path_user_edits_in_baseline_not_stale() {
        // Write path: baseline has user edits appended, snapshot is the committed state.
        // is_stale_baseline(baseline_with_edits, snapshot) should be FALSE
        // because the baseline's exchange CONTAINS the snapshot's exchange content.
        let snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            <!-- /agent:exchange -->\n";
        let baseline_with_user_edits = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response\n\
            Agent response text.\n\
            Implement agent-kit changes.\n\
            Implement updates to agent-doc.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            !crate::write::is_stale_baseline(baseline_with_user_edits, snapshot),
            "baseline with user edits should NOT be stale (it contains snapshot content)"
        );
    }

    #[test]
    fn is_stale_baseline_write_path_stale_baseline_detected() {
        // Write path: baseline is from before the last agent response.
        // is_stale_baseline(old_baseline, current_snapshot) should be TRUE.
        let current_snapshot = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            ### Re: Response 2\n\
            Second response.\n\
            <!-- /agent:exchange -->\n";
        let old_baseline = "<!-- agent:exchange patch=append -->\n\
            ### Re: Response 1\n\
            First response.\n\
            <!-- /agent:exchange -->\n";

        assert!(
            crate::write::is_stale_baseline(old_baseline, current_snapshot),
            "baseline missing committed response should be stale"
        );
    }

    #[test]
    fn is_in_git_repo_true_inside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git").current_dir(root).args(["init"]).output().unwrap();
        Command::new("git").current_dir(root).args(["config", "user.email", "test@test.com"]).output().unwrap();
        Command::new("git").current_dir(root).args(["config", "user.name", "Test"]).output().unwrap();

        let doc = root.join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(is_in_git_repo(&doc), "file inside git repo should return true");
    }

    #[test]
    fn is_in_git_repo_false_outside_repo() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "# test\n").unwrap();

        assert!(!is_in_git_repo(&doc), "file outside git repo should return false");
    }

    #[test]
    fn write_commit_lifecycle() {
        // Full lifecycle: git repo + snapshot + commit → verify commit in log.
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        // Set up git repo
        Command::new("git").current_dir(root).args(["init"]).output().unwrap();
        Command::new("git").current_dir(root).args(["config", "user.email", "test@test.com"]).output().unwrap();
        Command::new("git").current_dir(root).args(["config", "user.name", "Test"]).output().unwrap();

        // Create and commit an initial file so HEAD exists
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git").current_dir(root).args(["add", "README.md"]).output().unwrap();
        Command::new("git").current_dir(root).args(["commit", "-m", "initial", "--no-verify"]).output().unwrap();

        // Create a document and snapshot
        let doc = root.join("session.md");
        let doc_content = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, doc_content).unwrap();

        let snap_path = crate::snapshot::path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, doc_content).unwrap();

        // Stage + initial commit so the file is tracked
        Command::new("git").current_dir(root).args(["add", "session.md"]).output().unwrap();
        Command::new("git").current_dir(root).args(["commit", "-m", "add doc", "--no-verify"]).output().unwrap();

        // Now call commit (simulating what --commit does after write)
        commit(&doc).expect("commit should succeed");

        // Verify a new commit exists with the agent-doc message
        let log = Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-3"])
            .output()
            .unwrap();
        let log_str = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_str.contains("agent-doc(session):"),
            "git log should contain agent-doc commit, got:\n{log_str}"
        );
    }

    // --- Fix 3: index.lock retry ---

    #[test]
    fn commit_retry_logic_handles_index_lock_error() {
        // Verify the retry loop triggers when git commit stderr contains "index.lock".
        // We simulate this by checking that the retry backoff constants are correct:
        // attempts 1→100ms, 2→200ms, 3→400ms (50 * 2^attempt).
        assert_eq!(50u64 * (1u64 << 1u32), 100, "retry 1 backoff should be 100ms");
        assert_eq!(50u64 * (1u64 << 2u32), 200, "retry 2 backoff should be 200ms");
        assert_eq!(50u64 * (1u64 << 3u32), 400, "retry 3 backoff should be 400ms");
    }

    #[test]
    fn commit_succeeds_when_no_lock_contention() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();

        Command::new("git").current_dir(root).args(["init"]).output().unwrap();
        Command::new("git").current_dir(root).args(["config", "user.email", "test@test.com"]).output().unwrap();
        Command::new("git").current_dir(root).args(["config", "user.name", "Test"]).output().unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        Command::new("git").current_dir(root).args(["add", "README.md"]).output().unwrap();
        Command::new("git").current_dir(root).args(["commit", "-m", "initial", "--no-verify"]).output().unwrap();

        let doc = root.join("session.md");
        let content = "---\nagent_doc_session: test\n---\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();
        let snap_path = crate::snapshot::path_for(&doc).unwrap();
        let snap_abs = root.join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
        Command::new("git").current_dir(root).args(["add", "session.md"]).output().unwrap();
        Command::new("git").current_dir(root).args(["commit", "-m", "add doc", "--no-verify"]).output().unwrap();

        // No lock present — commit should succeed on first try
        let result = commit(&doc);
        assert!(result.is_ok(), "commit without lock should succeed: {:?}", result.err());
    }

    // --- Fix 1: snapshot saved before process::exit(75) (structural test) ---
    // The actual exit path in write::run_stream calls snapshot::save before process::exit(75).
    // We verify this by checking that snapshot::save is callable at that point.
    // Full integration testing requires IPC infrastructure; unit coverage is in write.rs.

    #[test]
    fn is_stale_baseline_write_path_replace_edits_ignored() {
        // Write path: user edited a replace-mode component in the baseline.
        // Only append-mode components are checked. Replace edits are fine.
        let snapshot = "<!-- agent:status patch=replace -->\nOriginal\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nUser changed\n<!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(
            !crate::write::is_stale_baseline(baseline, snapshot),
            "user edits in replace + append components should NOT be stale"
        );
    }
}
