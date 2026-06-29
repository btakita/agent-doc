//! Sidecar adapter for binary-detected queue-stall policy (`#qstallguard`).
//!
//! The pure stall classifier lives in `agent-doc-turn`; orchestration owns only
//! the one-shot continuation-pending marker IO.

use std::path::{Path, PathBuf};

use agent_doc_turn::drain_stall::ContinuationPending;
use anyhow::{Context, Result};

/// Directory (relative to the project root) holding per-document continuation
/// markers. Mirrors the sibling recycle-yield / drain-owner sidecar layout.
const DRAIN_STALL_DIR: &str = ".agent-doc/drain-stall";

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the continuation-marker path for a document.
fn marker_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(DRAIN_STALL_DIR).join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(DRAIN_STALL_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

/// Record that this committed cycle still required queue continuation.
/// Idempotent; the next preflight reconciles and clears it.
pub fn mark_continuation_pending(file: &str, cycle_id: &str) -> Result<()> {
    let path = marker_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create drain-stall dir {}", parent.display()))?;
    }
    let marker = ContinuationPending {
        cycle_id: cycle_id.to_string(),
        recorded_secs: now_secs(),
    };
    let body = serde_json::to_string(&marker).context("failed to serialize drain-stall marker")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write drain-stall marker {}", path.display()))?;
    Ok(())
}

/// Read the raw continuation-pending marker for `file`, if present.
pub fn read_continuation_pending(file: &str) -> Option<ContinuationPending> {
    let path = marker_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Convenience boolean: is a continuation-pending marker present for `file`?
pub fn continuation_pending(file: &Path) -> bool {
    read_continuation_pending(&file.to_string_lossy()).is_some()
}

/// Best-effort one-shot clear of the continuation-pending marker.
pub fn clear_continuation_pending(file: &str) {
    let path = marker_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear drain-stall marker {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_roundtrips_and_clears_one_shot() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        assert!(read_continuation_pending(&file).is_none());
        mark_continuation_pending(&file, "cycle-123").unwrap();
        let marker = read_continuation_pending(&file).expect("marker present");
        assert_eq!(marker.cycle_id, "cycle-123");

        clear_continuation_pending(&file);
        assert!(read_continuation_pending(&file).is_none());
        clear_continuation_pending(&file);
    }
}
