//! `agent-doc preflight <FILE>` — Run all pre-agent steps and output JSON.
//!
//! Combines recover, commit, claims-log check, diff, and document HEAD read
//! into a single call that the SKILL workflow can consume as structured data.
//!
//! ## Output (JSON to stdout)
//!
//! ```json
//! {
//!   "recovered": false,
//!   "committed": true,
//!   "claims": ["claim1"],
//!   "diff": "unified diff text or null",
//!   "no_changes": false,
//!   "document": "full document content"
//! }
//! ```
//!
//! `no_changes` is `true` when the diff is `None` (snapshot == document).
//! `diff` is `null` when `no_changes` is `true`.
//! `document` always contains the current HEAD content.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

use crate::{diff, git, recover, snapshot};

#[derive(Serialize)]
pub struct PreflightOutput {
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

/// Run the preflight sequence for a session document.
///
/// Steps (in order):
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

    // Step 3b: Wait for file to settle (mtime debounce).
    // If the file was modified very recently, the user may still be typing.
    // Wait up to 2s for the file to be idle for at least 500ms.
    {
        let debounce = std::time::Duration::from_millis(500);
        let max_wait = std::time::Duration::from_secs(2);
        let poll = std::time::Duration::from_millis(100);
        let start = std::time::Instant::now();

        loop {
            let idle_for = std::fs::metadata(file)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(debounce);

            if idle_for >= debounce {
                break;
            }
            if start.elapsed() >= max_wait {
                eprintln!("[preflight] mtime debounce timeout after {:.1}s — proceeding", start.elapsed().as_secs_f64());
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
}
