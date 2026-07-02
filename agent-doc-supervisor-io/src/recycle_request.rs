//! Cross-supervisor recycle-REQUEST marker storage.
//!
//! Install fan-out writes one per served route-owned document; the owning
//! supervisor's idle loop reads it and recycles onto the freshly-installed binary
//! at its next idle boundary, then clears it once fresh.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_supervisor::recycle_request::{
    RECYCLE_REQUEST_DIR, RecycleRequest, recycle_request, recycle_request_is_fresh,
};
use anyhow::{Context, Result};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the recycle-request path for a document. Mirrors the recycle-yield
/// layout: hash the document path and land the sidecar in the nearest ancestor
/// `.agent-doc/` directory, so the request is keyed per served document.
fn recycle_request_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir
                .join(RECYCLE_REQUEST_DIR)
                .join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(RECYCLE_REQUEST_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

/// Write (or refresh) a recycle-request for `file` with the current heartbeat.
pub fn request_recycle(file: &str, reason: &str) -> Result<()> {
    let path = recycle_request_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create recycle-request dir {}", parent.display())
        })?;
    }
    let request = recycle_request(reason, now_secs());
    let body = serde_json::to_string(&request).context("failed to serialize recycle-request")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write recycle-request {}", path.display()))?;
    Ok(())
}

/// Write (or refresh) a recycle-request for a document path.
pub fn request_recycle_for_doc(file: &Path, reason: &str) -> Result<()> {
    request_recycle(&file.to_string_lossy(), reason)
}

/// Read the raw recycle-request for `file` regardless of freshness.
pub fn read_recycle_request(file: &str) -> Option<RecycleRequest> {
    let path = recycle_request_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Return the recycle-request iff it exists and is fresh against `now`.
pub fn fresh_recycle_request(file: &str, now: u64) -> Option<RecycleRequest> {
    let request = read_recycle_request(file)?;
    recycle_request_is_fresh(&request, now).then_some(request)
}

/// Convenience boolean: is a fresh recycle-request pending for `file`?
pub fn recycle_request_pending(file: &Path) -> bool {
    fresh_recycle_request(&file.to_string_lossy(), now_secs()).is_some()
}

/// Best-effort clear of the recycle-request.
pub fn clear_recycle_request(file: &str) {
    let path = recycle_request_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear recycle-request {}: {err}",
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

        request_recycle(
            &file,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT,
        )
        .unwrap();
        let request = read_recycle_request(&file).expect("request present after write");
        assert_eq!(
            request.reason,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT
        );

        assert!(fresh_recycle_request(&file, request.requested_secs).is_some());
        assert!(
            fresh_recycle_request(&file, request.requested_secs + 10_000_000).is_none(),
            "an old request must not force a stale-forever recycle"
        );
    }

    #[test]
    fn absent_request_reads_as_none() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md").to_string_lossy().to_string();
        assert!(read_recycle_request(&file).is_none());
        assert!(fresh_recycle_request(&file, now_secs()).is_none());
    }

    #[test]
    fn clear_removes_the_request_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        request_recycle(
            &file,
            agent_doc_supervisor::recycle_request::RECYCLE_REQUEST_INSTALL_FANOUT,
        )
        .unwrap();
        clear_recycle_request(&file);
        assert!(read_recycle_request(&file).is_none());
        clear_recycle_request(&file);
    }
}
