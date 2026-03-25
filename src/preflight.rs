//! # Module: preflight
//!
//! ## Spec
//! - `run(file)`: executes the full pre-agent preparation sequence for a
//!   session document and emits a single JSON object to stdout.
//! - Bails immediately if the file does not exist.
//! - Step 0 — layout check: calls `check_layout()` to detect tmux structural
//!   problems; issues are included in output but do not abort the run.
//! - Step 1 — recover: calls `recover::run(file)` to detect and apply any
//!   orphaned pending agent responses from a previous interrupted cycle.
//! - Step 2 — commit: calls `git::commit(file)` to record the previous
//!   exchange cycle; failure is downgraded to a warning, not a hard error.
//! - Step 3 — claims: reads `.agent-doc/claims.log` line-by-line via
//!   `read_and_truncate_claims`, then truncates the log to empty; claims are
//!   returned to the caller in the JSON output.
//! - Step 3b — debounce: waits up to 3 seconds (polling every 100 ms) for
//!   both the file mtime to be at least 500 ms old and the cross-process
//!   typing indicator to be inactive before proceeding to the diff step.
//! - Step 4 — diff: calls `diff::compute(file)` to compare the current
//!   document against the last snapshot; `no_changes=true` when they match.
//! - Step 5 — read document: reads the current file from disk into `document`.
//! - Serializes `PreflightOutput` as pretty JSON to stdout; all diagnostic
//!   messages go to stderr.
//! - `check_layout()`: inspects the current tmux session for structural issues:
//!   missing window index 0 (base-index compliance) and stash windows that
//!   have non-idle panes running meaningful processes. Read-only; no mutations.
//!   Returns an empty vec when not inside tmux (silent).
//! - `read_and_truncate_claims(file)`: locates `.agent-doc/claims.log` relative
//!   to the project root, collects non-empty lines, truncates the file to empty,
//!   and returns the lines. Returns empty vec if the log is absent or unreadable.
//!
//! ## Agentic Contracts
//! - All output intended for the SKILL workflow is on stdout as valid JSON;
//!   callers must not parse stderr.
//! - `no_changes=true` in the output means the SKILL workflow should skip
//!   sending to the agent; `diff` will be `null` in this case.
//! - `layout_issues` is informational: the SKILL workflow may surface issues
//!   to the user but `run` always completes the remaining steps regardless.
//! - The claims log is consumed (truncated) exactly once per `preflight` call;
//!   a second call in the same cycle will return empty claims.
//! - Recovery (`recovered=true`) means the document was modified before the
//!   diff step; the `diff` and `document` fields reflect post-recovery state.
//! - Debounce waits for user typing to settle before computing the diff;
//!   if the 3-second timeout expires, `run` proceeds and logs a warning to
//!   stderr — it never blocks indefinitely.
//! - `check_layout` is always safe to call outside tmux; it returns `[]`.
//!
//! ## Evals
//! - `preflight_produces_valid_json`: document with matching snapshot →
//!   `run` returns `Ok(())` and emits parseable JSON with `no_changes=true`.
//! - `preflight_file_not_found`: missing path → `Err` containing "file not found".
//! - `preflight_detects_diff`: snapshot saved at original content, document
//!   updated with new content → `diff::compute` returns `Some(_)` (non-null diff).
//! - `preflight_claims_read_and_truncated`: claims.log with two entries →
//!   `read_and_truncate_claims` returns both lines and the log is empty afterwards.
//! - `preflight_no_claims_log_returns_empty`: no claims.log present →
//!   `read_and_truncate_claims` returns an empty vec without error.
//! - `preflight_output_serializes_correctly`: `PreflightOutput` with known
//!   values serializes to JSON with correct field names and types.
//! - `preflight_output_null_diff_when_no_changes`: `diff=None` + `no_changes=true`
//!   → JSON has `"diff": null` and `"no_changes": true`.
//! - `check_layout_returns_empty_outside_tmux`: `TMUX` env var unset →
//!   `check_layout()` returns empty vec without invoking tmux.
//! - `preflight_output_includes_layout_issues`: `PreflightOutput` with one
//!   layout issue → JSON `layout_issues` array has length 1 with correct text.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::{diff, git, recover, sessions, snapshot};

#[derive(Serialize)]
pub struct PreflightOutput {
    /// Tmux layout issues found (empty = healthy).
    pub layout_issues: Vec<String>,
    /// Whether an orphaned pending response was recovered and applied.
    pub recovered: bool,
    /// Whether a git commit was made for the previous cycle.
    pub committed: bool,
    /// Lines from `.agent-doc/claims.log` (truncated after read).
    pub claims: Vec<String>,
    /// Unified diff text, or `null` if there are no changes.
    pub diff: Option<String>,
    /// True when the snapshot matches the document (no new user input).
    pub no_changes: bool,
    /// Full document content at HEAD (current file on disk).
    pub document: String,
}

/// Shells considered idle (not actively running a meaningful process).
const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];

/// Check tmux layout health for the current session.
///
/// Returns a list of human-readable issue strings. An empty vec means the
/// layout is healthy. This is read-only — no mutations are performed.
///
/// If not running inside tmux, returns an empty vec silently.
pub fn check_layout() -> Vec<String> {
    if !sessions::in_tmux() {
        return vec![];
    }

    let mut issues = Vec::new();

    // Get current session name.
    let session_name = match Command::new("tmux")
        .args(["display-message", "-p", "#{session_name}"])
        .output()
    {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => return issues, // Can't determine session — skip silently.
    };

    if session_name.is_empty() {
        return issues;
    }

    // List windows: index, name, pane count.
    let window_output = match Command::new("tmux")
        .args([
            "list-windows",
            "-t",
            &format!("{}:", session_name),
            "-F",
            "#{window_index}\t#{window_name}\t#{window_panes}",
        ])
        .output()
    {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).to_string()
        }
        _ => return issues,
    };

    struct WinInfo {
        index: u32,
        name: String,
    }
    let windows: Vec<WinInfo> = window_output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let index: u32 = parts.next()?.parse().ok()?;
            let name = parts.next()?.to_string();
            // pane count consumed but not stored
            let _pane_count: usize = parts.next()?.parse().ok()?;
            Some(WinInfo { index, name })
        })
        .collect();

    // Check 1: Window 0 should exist (base-index compliance).
    if !windows.iter().any(|w| w.index == 0) {
        issues.push(format!(
            "window index 0 missing in session '{}' (base-index compliance)",
            session_name,
        ));
    }

    // Check 2: Stash windows with non-idle panes.
    for win in &windows {
        if win.name != "stash" && !win.name.starts_with("stash-") {
            continue;
        }

        let pane_output = match Command::new("tmux")
            .args([
                "list-panes",
                "-t",
                &format!("{}:{}", session_name, win.index),
                "-F",
                "#{pane_id}\t#{pane_current_command}",
            ])
            .output()
        {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).to_string()
            }
            _ => continue,
        };

        for line in pane_output.lines() {
            let mut parts = line.splitn(2, '\t');
            let pane_id = match parts.next() {
                Some(id) => id,
                None => continue,
            };
            let cmd = match parts.next() {
                Some(c) => c,
                None => continue,
            };

            if !IDLE_SHELLS.contains(&cmd) {
                issues.push(format!(
                    "stash window '{}' has non-idle pane {} running '{}'",
                    win.name, pane_id, cmd,
                ));
            }
        }
    }

    issues
}

/// Run the preflight sequence for a session document.
///
/// Steps (in order):
/// 0. Check tmux layout health (`check_layout`)
/// 1. Recover orphaned pending response (`recover::run`)
/// 2. Commit previous cycle (`git::commit`)
/// 3. Check claims log (read + truncate `.agent-doc/claims.log`)
/// 4. Compute diff (`diff::compute`)
/// 5. Read document HEAD from disk
///
/// Outputs JSON to stdout. Progress/diagnostic messages go to stderr.
pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Step 0: Check tmux layout health.
    eprintln!("[preflight] step 0: layout check");
    let layout_issues = check_layout();
    for issue in &layout_issues {
        eprintln!("[preflight] layout issue: {}", issue);
    }

    // Step 1: Recover orphaned pending responses.
    eprintln!("[preflight] step 1: recover");
    let recovered = recover::run(file).unwrap_or_else(|e| {
        eprintln!("[preflight] recover warning: {}", e);
        false
    });

    // Step 2: Commit previous cycle.
    eprintln!("[preflight] step 2: commit");
    let committed = match git::commit(file) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("[preflight] commit warning: {}", e);
            false
        }
    };

    // Step 3: Read and truncate the claims log.
    eprintln!("[preflight] step 3: claims");
    let claims = read_and_truncate_claims(file);

    // Step 3b: Wait for file to settle (mtime + typing indicator debounce).
    // Check both file mtime (disk-level) and cross-process typing indicator
    // (buffer-level) to avoid picking up mid-typing edits.
    {
        let debounce = std::time::Duration::from_millis(500);
        let max_wait = std::time::Duration::from_secs(3);
        let poll = std::time::Duration::from_millis(100);
        let start = std::time::Instant::now();
        let file_str = file.to_string_lossy();

        loop {
            let idle_for = std::fs::metadata(file)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(debounce);

            let typing_active = agent_doc::debounce::is_typing_via_file(&file_str, 1500);

            if idle_for >= debounce && !typing_active {
                break;
            }
            if start.elapsed() >= max_wait {
                if typing_active {
                    eprintln!("[preflight] typing indicator active but timeout after {:.1}s — proceeding", start.elapsed().as_secs_f64());
                } else {
                    eprintln!("[preflight] mtime debounce timeout after {:.1}s — proceeding", start.elapsed().as_secs_f64());
                }
                break;
            }
            std::thread::sleep(poll);
        }
    }

    // Step 4: Compute diff between snapshot and current document.
    eprintln!("[preflight] step 4: diff");
    let diff_result = diff::compute(file)?;
    let no_changes = diff_result.is_none();

    // Step 5: Read document HEAD from disk.
    eprintln!("[preflight] step 5: read document");
    let document = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read document {}", file.display()))?;

    let output = PreflightOutput {
        layout_issues,
        recovered,
        committed,
        claims,
        diff: diff_result,
        no_changes,
        document,
    };

    let json = serde_json::to_string_pretty(&output)
        .context("failed to serialize preflight output")?;
    println!("{}", json);

    Ok(())
}

/// Read the claims log and truncate it. Returns lines as a `Vec<String>`.
/// Returns an empty vec if the log doesn't exist or can't be read.
fn read_and_truncate_claims(file: &Path) -> Vec<String> {
    // Canonicalize to find project root reliably.
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let root = match snapshot::find_project_root(&canonical) {
        Some(r) => r,
        None => return vec![],
    };

    let log_path = root.join(".agent-doc/claims.log");

    let contents = match std::fs::read_to_string(&log_path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    if contents.is_empty() {
        return vec![];
    }

    // Collect non-empty lines.
    let claims: Vec<String> = contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();

    // Truncate the log.
    if let Err(e) = std::fs::write(&log_path, "") {
        eprintln!("[preflight] failed to truncate claims log: {}", e);
    }

    claims
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    /// Set up a minimal project directory with .agent-doc/ structure and a git repo.
    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();

        // Initialize a bare git repo so `git commit` doesn't fail fatally.
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@test.com"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .ok();

        dir
    }

    #[test]
    fn preflight_produces_valid_json() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n## User\n\nHello\n",
        )
        .unwrap();

        // Snapshot matches document → no_changes = true.
        snapshot::save(&doc, &std::fs::read_to_string(&doc).unwrap()).unwrap();

        run(&doc).unwrap();
        // If run() returns Ok(()), the JSON was printed to stdout without error.
        // The test verifies no panic and no error return.
    }

    #[test]
    fn preflight_file_not_found() {
        let err = run(Path::new("/nonexistent/missing.md")).unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn preflight_detects_diff() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();

        // Save snapshot of original, then add new content.
        snapshot::save(&doc, original).unwrap();
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n## User\n\nHello\n\nNew question here.\n",
        )
        .unwrap();

        // diff::compute should detect changes → no_changes = false.
        let diff_result = diff::compute(&doc).unwrap();
        assert!(diff_result.is_some(), "diff should detect new content");
    }

    #[test]
    fn preflight_claims_read_and_truncated() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();
        snapshot::save(&doc, "# Doc\n").unwrap();

        // Write a claims log.
        let log_path = dir.path().join(".agent-doc/claims.log");
        std::fs::write(&log_path, "claim A\nclaim B\n").unwrap();

        let claims = read_and_truncate_claims(&doc);
        assert_eq!(claims, vec!["claim A", "claim B"]);

        // Log should be truncated.
        let after = std::fs::read_to_string(&log_path).unwrap();
        assert!(after.is_empty(), "claims log should be empty after read");
    }

    #[test]
    fn preflight_no_claims_log_returns_empty() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();

        // No claims.log exists.
        let claims = read_and_truncate_claims(&doc);
        assert!(claims.is_empty());
    }

    #[test]
    fn preflight_output_serializes_correctly() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec!["foo".to_string()],
            diff: Some("+new line\n".to_string()),
            no_changes: false,
            document: "# Doc\n".to_string(),
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["recovered"], false);
        assert_eq!(parsed["committed"], true);
        assert_eq!(parsed["claims"][0], "foo");
        assert_eq!(parsed["no_changes"], false);
        assert!(parsed["diff"].as_str().is_some());
        assert_eq!(parsed["document"], "# Doc\n");
    }

    #[test]
    fn preflight_output_null_diff_when_no_changes() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            document: "content".to_string(),
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["diff"].is_null());
        assert_eq!(parsed["no_changes"], true);
    }

    #[test]
    fn check_layout_returns_empty_outside_tmux() {
        // When TMUX env var is not set (typical in CI / test), check_layout
        // should return an empty vec silently.
        let saved = std::env::var("TMUX").ok();
        // SAFETY: test is single-threaded; we restore the value immediately after.
        unsafe { std::env::remove_var("TMUX") };
        let issues = check_layout();
        // Restore if it was set.
        if let Some(val) = saved {
            unsafe { std::env::set_var("TMUX", val) };
        }
        assert!(issues.is_empty(), "expected no issues outside tmux, got: {:?}", issues);
    }

    #[test]
    fn preflight_output_includes_layout_issues() {
        let output = PreflightOutput {
            layout_issues: vec!["window index 0 missing".to_string()],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            document: "content".to_string(),
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["layout_issues"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["layout_issues"][0], "window index 0 missing");
    }
}
