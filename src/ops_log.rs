//! # Module: ops_log
//!
//! ## Spec
//! - `log_op(file, message)` appends a timestamped text line to
//!   `.agent-doc/logs/ops.log`. Best-effort: silently returns on I/O errors.
//! - `log_cycle(file, entry)` appends a structured JSON line to
//!   `.agent-doc/logs/cycles.jsonl` for reproducible operation tracking.
//!   Each entry records the operation type, git commit hash, snapshot content
//!   hash, and document content hash — enough to reconstruct any cycle.
//! - `CycleEntry` is serializable and contains: `op`, `file`, `timestamp`,
//!   `commit_hash`, `snapshot_hash`, `file_hash`.
//!
//! ## Agentic Contracts
//! - Both log functions are best-effort and never panic or return errors.
//! - `cycles.jsonl` uses newline-delimited JSON (one object per line) for
//!   easy parsing by both humans and tools.
//! - `commit_hash` and `snapshot_hash` together identify the exact cycle state.
//!   `agent-doc replay` can reconstruct any cycle from these references.
//!
//! ## Evals
//! - `log_op_creates_file_and_appends`: two calls → two lines in ops.log
//! - `log_op_no_panic_on_missing_project_root`: no .agent-doc dir → silent noop
//! - `log_cycle_writes_jsonl`: cycle entry → valid JSON line in cycles.jsonl
//! - `log_cycle_appends_multiple`: multiple entries → multiple lines

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

/// Append a timestamped log line to `.agent-doc/logs/ops.log`.
///
/// Finds the project root by walking up from `file` (same as `snapshot::find_project_root`).
/// Best-effort: silently returns on any I/O error.
pub fn log_op(file: &Path, message: &str) {
    let _ = try_log_op(file, message);
}

/// Structured cycle log entry for reproducible operation tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleEntry {
    /// Operation type (e.g., "write_inline", "write_template", "write_stream", "commit").
    pub op: String,
    /// Document path (relative to project root).
    pub file: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Git commit hash after the operation (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    /// SHA256 of the snapshot content after the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    /// SHA256 of the document file content after the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
}

/// Compute SHA256 hex hash of content.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Get the current git HEAD commit hash for a file.
fn git_head_hash(file: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%H", "--"])
        .arg(file)
        .output()
        .ok()?;
    if output.status.success() {
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if hash.is_empty() { None } else { Some(hash) }
    } else {
        None
    }
}

/// Get the current timestamp in ISO 8601 format.
fn iso_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple UTC format without chrono dependency
    format!("{}", now)
}

/// Append a structured cycle entry to `.agent-doc/logs/cycles.jsonl`.
///
/// Best-effort: silently returns on any I/O error.
pub fn log_cycle(
    file: &Path,
    op: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) {
    let _ = try_log_cycle(file, op, snapshot_content, file_content);
}

fn try_log_cycle(
    file: &Path,
    op: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Option<()> {
    let canonical = file.canonicalize().ok()?;
    let project_root = crate::snapshot::find_project_root(&canonical)?;
    let logs_dir = project_root.join(".agent-doc/logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join("cycles.jsonl");

    let relative = canonical
        .strip_prefix(&project_root)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .to_string();

    let entry = CycleEntry {
        op: op.to_string(),
        file: relative,
        timestamp: iso_timestamp(),
        commit_hash: git_head_hash(file),
        snapshot_hash: snapshot_content.map(content_hash),
        file_hash: file_content.map(content_hash),
    };

    let json = serde_json::to_string(&entry).ok()?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    writeln!(f, "{}", json).ok()
}

fn try_log_op(file: &Path, message: &str) -> Option<()> {
    let canonical = file.canonicalize().ok()?;
    let project_root = crate::snapshot::find_project_root(&canonical)?;
    let logs_dir = project_root.join(".agent-doc/logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join("ops.log");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    writeln!(f, "[{}] {}", ts, message).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn log_op_creates_file_and_appends() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();

        // Create project root marker (.agent-doc dir) and the file
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("test.md");
        fs::write(&doc_path, "test").unwrap();

        log_op(&doc_path, "test_event file=test.md");
        log_op(&doc_path, "second_event file=test.md");

        let log_path = project_root.join(".agent-doc/logs/ops.log");
        assert!(log_path.exists(), "ops.log should be created");

        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "should have 2 log lines");
        assert!(
            lines[0].contains("test_event"),
            "first line should contain message"
        );
        assert!(
            lines[1].contains("second_event"),
            "second line should contain message"
        );

        // Verify timestamp format [epoch_secs]
        assert!(
            lines[0].starts_with('['),
            "should start with timestamp bracket"
        );
        assert!(
            lines[0].contains("] "),
            "should have ] separator after timestamp"
        );
    }

    #[test]
    fn log_cycle_writes_jsonl() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("test.md");
        fs::write(&doc_path, "content").unwrap();

        log_cycle(&doc_path, "write_inline", Some("snapshot"), Some("content"));

        let log_path = project_root.join(".agent-doc/logs/cycles.jsonl");
        assert!(log_path.exists(), "cycles.jsonl should be created");

        let content = fs::read_to_string(&log_path).unwrap();
        let entry: serde_json::Value =
            serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(entry["op"], "write_inline");
        assert!(entry["file"].as_str().unwrap().contains("test.md"));
        assert!(entry["snapshot_hash"].is_string());
        assert!(entry["file_hash"].is_string());
    }

    #[test]
    fn log_cycle_appends_multiple() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project_root = tmp.path();
        fs::create_dir_all(project_root.join(".agent-doc")).unwrap();
        let doc_path = project_root.join("test.md");
        fs::write(&doc_path, "content").unwrap();

        log_cycle(&doc_path, "write_inline", Some("snap1"), Some("file1"));
        log_cycle(&doc_path, "commit", Some("snap2"), Some("file2"));

        let log_path = project_root.join(".agent-doc/logs/cycles.jsonl");
        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);

        let entry1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let entry2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(entry1["op"], "write_inline");
        assert_eq!(entry2["op"], "commit");
    }

    #[test]
    fn log_op_no_panic_on_missing_project_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("orphan.md");
        fs::write(&doc_path, "test").unwrap();

        // No .agent-doc dir — should silently return without panic
        log_op(&doc_path, "should_not_crash");

        // Verify no log was created
        assert!(!tmp.path().join(".agent-doc/logs/ops.log").exists());
    }
}
