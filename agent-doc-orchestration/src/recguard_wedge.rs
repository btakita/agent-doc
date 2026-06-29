//! # Module: recguard_wedge
//!
//! `#recguard-wedge-escape`: detect a *wedged* owner-pane self-invocation loop
//! and break it.
//!
//! The recursive-direct-invocation guard ([`crate::run`]) correctly refuses to
//! dispatch when `agent-doc <FILE>` runs inside the Codex pane that already owns
//! the document (re-entering the pane would deadlock). The Stop-hook redirect
//! (`#codex-self-reinvoke-prevent`, Option B) keeps the *end-of-turn*
//! continuation in-pane, but a busy agent that re-invokes the CLI **mid-turn**
//! still trips the guard. In a self-driving `agent:queue auto` loop with no
//! operator watching, the same active queue head can trip the guard every cycle
//! — the loop burns cycles re-invoking and failing without ever advancing.
//!
//! This module tracks the count of *consecutive* self-invocation guard fires for
//! the same head. A single transient self-invoke is normal; the same head
//! tripping the guard [`WEDGE_THRESHOLD`] times in a row with no advance is a
//! proven dead-loop. At that point the caller halts the runaway auto-queue
//! (`queue: stop`) so the loop stops re-firing and the operator gets one clear
//! recovery action instead of an unbounded retry storm.
//!
//! A different head (the queue advanced) resets the counter, so a healthy loop
//! that occasionally self-invokes never escalates.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Consecutive same-head self-invocation guard fires that prove a dead-loop.
/// Two transient self-invokes are tolerated; the third halts.
pub const WEDGE_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WedgeRecord {
    /// The queue head (or self-invocation detail) the count is keyed on. A new
    /// head means the loop advanced, so the counter resets.
    head: String,
    /// Consecutive self-invocation guard fires for `head`.
    count: u32,
}

fn wedge_path(file: &Path) -> Result<Option<PathBuf>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(
        root.join(".agent-doc/recguard-wedge")
            .join(format!("{hash}.json")),
    ))
}

fn read_record(path: &Path) -> Option<WedgeRecord> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Record one owner-pane self-invocation guard fire for `head` and return the
/// new consecutive count. A new head resets the count to 1. Best-effort: if the
/// project root or sidecar is unavailable the call still reports `1` so the
/// caller falls through to the normal fail-closed diagnostic.
pub fn record(file: &Path, head: &str) -> Result<u32> {
    let Some(path) = wedge_path(file)? else {
        return Ok(1);
    };
    let prior = read_record(&path);
    let count = match prior {
        Some(rec) if rec.head == head => rec.count.saturating_add(1),
        _ => 1,
    };
    let record = WedgeRecord {
        head: head.to_string(),
        count,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec(&record)?)?;
    Ok(count)
}

/// Clear the wedge counter (after a halt, or once the head advances). Best-effort
/// — a missing sidecar is success, not an error.
pub fn clear(file: &Path) -> Result<()> {
    let Some(path) = wedge_path(file)? else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// `true` once `count` reaches the wedge threshold — the same head has tripped
/// the owner-pane self-invocation guard enough times in a row to prove a
/// dead-loop the caller should break.
pub fn is_wedged(count: u32) -> bool {
    count >= WEDGE_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    #[test]
    fn record_counts_consecutive_same_head_and_halts_at_threshold() {
        let dir = setup();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        assert!(!is_wedged(1));
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 2);
        assert!(!is_wedged(2));
        let third = record(&doc, "do [#alpha]").unwrap();
        assert_eq!(third, 3);
        assert!(
            is_wedged(third),
            "third consecutive same-head fire is a wedge"
        );
    }

    #[test]
    fn record_resets_count_when_head_advances() {
        let dir = setup();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 2);
        // The queue advanced to a new head — the loop is healthy, not wedged.
        assert_eq!(
            record(&doc, "do [#beta]").unwrap(),
            1,
            "a new head resets the consecutive counter"
        );
        assert!(!is_wedged(1));
    }

    #[test]
    fn clear_removes_the_counter() {
        let dir = setup();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 2);
        clear(&doc).unwrap();
        // After a clear, counting starts over.
        assert_eq!(record(&doc, "do [#alpha]").unwrap(), 1);
        // Clearing a non-existent counter is a no-op success.
        clear(&doc).unwrap();
        clear(&doc).unwrap();
    }
}
