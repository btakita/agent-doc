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
