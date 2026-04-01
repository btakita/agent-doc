//! # Module: ops_log
//!
//! Best-effort operational logging to `.agent-doc/logs/ops.log`.
//! Appends timestamped lines for write, commit, and snapshot operations
//! to help debug cases where responses are written but not committed.

use std::io::Write;
use std::path::Path;

/// Append a timestamped log line to `.agent-doc/logs/ops.log`.
///
/// Finds the project root by walking up from `file` (same as `snapshot::find_project_root`).
/// Best-effort: silently returns on any I/O error.
pub fn log_op(file: &Path, message: &str) {
    let _ = try_log_op(file, message);
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
        assert!(lines[0].contains("test_event"), "first line should contain message");
        assert!(lines[1].contains("second_event"), "second line should contain message");

        // Verify timestamp format [epoch_secs]
        assert!(lines[0].starts_with('['), "should start with timestamp bracket");
        assert!(lines[0].contains("] "), "should have ] separator after timestamp");
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
