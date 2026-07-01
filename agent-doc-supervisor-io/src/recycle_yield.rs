//! Stale-binary recycle-yield request marker storage.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_supervisor::recycle_yield::{
    RECYCLE_YIELD_DIR, RecycleYieldRequest, recycle_yield_request, recycle_yield_request_is_fresh,
};
use anyhow::{Context, Result};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the recycle-yield request path for a document. Mirrors the
/// drain-owner layout: hash the document path and land the sidecar in the
/// nearest ancestor `.agent-doc/` directory.
fn recycle_yield_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir
                .join(RECYCLE_YIELD_DIR)
                .join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(RECYCLE_YIELD_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

/// Write (or refresh) a recycle-yield request for `file` with the current
/// heartbeat.
pub fn request_recycle_yield(file: &str, reason: &str) -> Result<()> {
    let path = recycle_yield_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create recycle-yield dir {}", parent.display()))?;
    }
    let request = recycle_yield_request(reason, now_secs());
    let body =
        serde_json::to_string(&request).context("failed to serialize recycle-yield request")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write recycle-yield request {}", path.display()))?;
    Ok(())
}

/// Read the raw recycle-yield request for `file` regardless of freshness.
pub fn read_recycle_yield(file: &str) -> Option<RecycleYieldRequest> {
    let path = recycle_yield_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Return the recycle-yield request iff it exists and is fresh against `now`.
pub fn fresh_recycle_yield(file: &str, now: u64) -> Option<RecycleYieldRequest> {
    let request = read_recycle_yield(file)?;
    recycle_yield_request_is_fresh(&request, now).then_some(request)
}

/// Convenience boolean: is a fresh recycle-yield request pending for `file`?
pub fn recycle_yield_pending(file: &Path) -> bool {
    fresh_recycle_yield(&file.to_string_lossy(), now_secs()).is_some()
}

/// Best-effort clear of the recycle-yield request.
pub fn clear_recycle_yield(file: &str) {
    let path = recycle_yield_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear recycle-yield request {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_then_read_roundtrips_a_fresh_request() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        request_recycle_yield(
            &file,
            agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STALE_BINARY,
        )
        .unwrap();
        let request = read_recycle_yield(&file).expect("request present after write");
        assert_eq!(
            request.reason,
            agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STALE_BINARY
        );

        assert!(fresh_recycle_yield(&file, request.requested_secs).is_some());
        assert!(
            fresh_recycle_yield(&file, request.requested_secs + 10_000).is_none(),
            "an old request must hand the loop back to a normal drain"
        );
    }

    #[test]
    fn absent_request_reads_as_none() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md").to_string_lossy().to_string();
        assert!(read_recycle_yield(&file).is_none());
        assert!(fresh_recycle_yield(&file, now_secs()).is_none());
    }

    #[test]
    fn clear_removes_the_request_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        request_recycle_yield(
            &file,
            agent_doc_supervisor::recycle_yield::RECYCLE_YIELD_STALE_BINARY,
        )
        .unwrap();
        clear_recycle_yield(&file);
        assert!(read_recycle_yield(&file).is_none());
        clear_recycle_yield(&file);
    }
}
