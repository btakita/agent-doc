//! Supervisor recycle-in-flight marker storage.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agent_doc_supervisor::recycle_inflight::{
    RECYCLE_INFLIGHT_DIR, RECYCLE_INFLIGHT_FILE, RecycleInflightMarker, recycle_inflight_marker,
    recycle_inflight_marker_is_fresh,
};
use anyhow::{Context, Result};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the project-scoped marker path for a document.
fn recycle_inflight_path(file: &str) -> PathBuf {
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(RECYCLE_INFLIGHT_DIR).join(RECYCLE_INFLIGHT_FILE);
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(RECYCLE_INFLIGHT_DIR)
                .join(RECYCLE_INFLIGHT_FILE);
        }
    }
}

/// Mark (or refresh) the recycle-in-flight marker for `file`'s project.
pub fn mark_recycle_inflight(file: &str, reason: &str) -> Result<()> {
    let path = recycle_inflight_path(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("failed to create recycle-inflight dir {}", parent.display())
        })?;
    }
    let marker = recycle_inflight_marker(reason, now_secs());
    let body =
        serde_json::to_string(&marker).context("failed to serialize recycle-inflight marker")?;
    std::fs::write(&path, body)
        .with_context(|| format!("failed to write recycle-inflight marker {}", path.display()))?;
    Ok(())
}

/// Read the raw recycle-in-flight marker for `file`'s project.
pub fn read_recycle_inflight(file: &str) -> Option<RecycleInflightMarker> {
    let path = recycle_inflight_path(file);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Return the marker iff it exists and is fresh against `now`.
pub fn fresh_recycle_inflight(file: &str, now: u64) -> Option<RecycleInflightMarker> {
    let marker = read_recycle_inflight(file)?;
    recycle_inflight_marker_is_fresh(&marker, now).then_some(marker)
}

/// Convenience boolean: is the project's supervisor mid-recycle right now?
pub fn recycle_inflight_pending(file: &str) -> bool {
    fresh_recycle_inflight(file, now_secs()).is_some()
}

/// Block up to `timeout` for the project's supervisor recycle to settle.
pub fn wait_for_recycle_settle(file: &str, timeout: Duration, poll: Duration) -> bool {
    if !recycle_inflight_pending(file) {
        return true;
    }
    let started = std::time::Instant::now();
    while recycle_inflight_pending(file) {
        if started.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(poll);
    }
    true
}

/// Best-effort clear of the recycle-in-flight marker.
pub fn clear_recycle_inflight(file: &str) {
    let path = recycle_inflight_path(file);
    if let Err(err) = std::fs::remove_file(&path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[agent-doc] warning: failed to clear recycle-inflight marker {}: {err}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_then_read_roundtrips_a_fresh_marker() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();

        mark_recycle_inflight(
            &file,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
        let marker = read_recycle_inflight(&file).expect("marker present after write");
        assert_eq!(
            marker.reason,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL
        );

        assert!(fresh_recycle_inflight(&file, marker.marked_secs).is_some());
        assert!(
            fresh_recycle_inflight(&file, marker.marked_secs + 10_000).is_none(),
            "an old marker must let dispatch proceed with the normal ready probe"
        );
    }

    #[test]
    fn marker_is_project_scoped_shared_across_documents() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("nested").join("b.md");
        std::fs::create_dir_all(doc_b.parent().unwrap()).unwrap();
        let doc_a = doc_a.to_string_lossy().to_string();
        let doc_b = doc_b.to_string_lossy().to_string();

        mark_recycle_inflight(
            &doc_a,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
        assert!(recycle_inflight_pending(&doc_b));
    }

    #[test]
    fn wait_for_settle_returns_immediately_when_no_recycle_pending() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        assert!(wait_for_recycle_settle(
            &file,
            Duration::from_secs(10),
            Duration::from_millis(250)
        ));
    }

    #[test]
    fn wait_for_settle_gives_up_when_recycle_never_clears() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        mark_recycle_inflight(
            &file,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
        let settled =
            wait_for_recycle_settle(&file, Duration::from_millis(0), Duration::from_millis(1));
        assert!(!settled);
    }

    #[test]
    fn clear_removes_the_marker_and_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let file = file.to_string_lossy().to_string();
        mark_recycle_inflight(
            &file,
            agent_doc_supervisor::recycle_inflight::RECYCLE_INFLIGHT_AUTO_INSTALL,
        )
        .unwrap();
        clear_recycle_inflight(&file);
        assert!(read_recycle_inflight(&file).is_none());
        clear_recycle_inflight(&file);
    }
}
