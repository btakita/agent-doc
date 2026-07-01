//! Short-lived supervisor context-clear marker.
//!
//! The idle-queue watcher can send a `/clear` or equivalent fresh-context
//! command before draining a queue head. A supervisor recycle/restart must not
//! forget that the clear is still pending or settling, or the replacement
//! watcher can stack another clear or drain trigger into the same composer.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::queue::{
    CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS, ContextClearInFlight, context_clear_in_flight_marker,
    context_clear_in_flight_marker_active,
};
use anyhow::{Context, Result};

const CONTEXT_CLEAR_IN_FLIGHT_DIR: &str = ".agent-doc/context-clear-in-flight";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn marker_path(file: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(
        root.join(CONTEXT_CLEAR_IN_FLIGHT_DIR)
            .join(format!("{hash}.json")),
    ))
}

pub fn record_context_clear_in_flight(
    file: &Path,
    target: &str,
    harness: &str,
    command: &str,
    active_head: Option<&str>,
) -> Result<()> {
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let marker = context_clear_in_flight_marker(
        file.to_string_lossy().into_owned(),
        target,
        harness,
        command,
        active_head,
        now_secs(),
    );
    let json = serde_json::to_string_pretty(&marker).context("serialize context-clear marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn context_clear_in_flight(file: &Path) -> Result<Option<ContextClearInFlight>> {
    let Some(path) = marker_path(file)? else {
        return Ok(None);
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("read {}", path.display())),
    };
    let marker: ContextClearInFlight = match serde_json::from_str(&content) {
        Ok(marker) => marker,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            return Ok(None);
        }
    };
    if context_clear_in_flight_marker_active(
        marker.written_at,
        now_secs(),
        CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS,
    ) {
        return Ok(Some(marker));
    }
    let _ = std::fs::remove_file(&path);
    Ok(None)
}

pub fn clear_context_clear_in_flight(file: &Path) -> Result<()> {
    let Some(path) = marker_path(file)? else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_clear_marker_is_active_until_cleared() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();

        record_context_clear_in_flight(&doc, "%1", "codex", "/clear", Some("do [#a]")).unwrap();
        let marker = context_clear_in_flight(&doc).unwrap().unwrap();
        assert_eq!(marker.target, "%1");
        assert_eq!(marker.harness, "codex");
        assert_eq!(marker.command, "/clear");

        clear_context_clear_in_flight(&doc).unwrap();
        assert!(context_clear_in_flight(&doc).unwrap().is_none());
    }

    #[test]
    fn context_clear_marker_ignores_stale_payloads() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();
        let path = marker_path(&doc).unwrap().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let marker = ContextClearInFlight {
            file: doc.display().to_string(),
            target: "%1".to_string(),
            harness: "codex".to_string(),
            command: "/clear".to_string(),
            head_sha256: Some("abc".to_string()),
            head_bytes: Some(3),
            written_at: now_secs().saturating_sub(CONTEXT_CLEAR_IN_FLIGHT_TTL_SECS + 1),
        };
        std::fs::write(&path, serde_json::to_string(&marker).unwrap()).unwrap();

        assert!(context_clear_in_flight(&doc).unwrap().is_none());
        assert!(!path.exists());
    }
}
