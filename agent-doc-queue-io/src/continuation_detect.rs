//! File-backed queue-continuation detection adapters.

use anyhow::Result;
use std::path::Path;

use agent_doc_queue::queue_continuation::{self, QueueContinuation};

/// Detect whether `file` currently requires queue continuation.
///
/// This adapter owns the file read and delegates content policy to
/// `agent-doc-queue`. Callers inject snapshot loading and recycle-yield
/// detection so this IO crate stays free of orchestration snapshot/supervisor
/// internals.
pub fn detect_required_continuation_with(
    file: &Path,
    load_snapshot: impl FnOnce(&Path) -> Result<Option<String>>,
    recycle_yield_pending: impl FnOnce(&Path) -> bool,
) -> Result<Option<QueueContinuation>> {
    if recycle_yield_pending(file) {
        return Ok(None);
    }
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let snapshot_content = load_snapshot(file)?;
    queue_continuation::required_continuation(&content, snapshot_content.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn continuation_doc(head: &str) -> String {
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue_active: true\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior - gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue go -->\n- {head}\n<!-- /agent:queue -->\n"
        )
    }

    #[test]
    fn detects_required_continuation_from_file_and_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        let content = continuation_doc("do [#a]");
        std::fs::write(&doc, &content).unwrap();

        let continuation =
            detect_required_continuation_with(&doc, |_| Ok(Some(content.clone())), |_| false)
                .unwrap()
                .expect("active go queue should require continuation");

        assert_eq!(continuation.head_id.as_deref(), Some("a"));
        assert_eq!(continuation.head_prompt, "do [#a]");
    }

    #[test]
    fn suppresses_modified_head_against_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        let snapshot = continuation_doc("do [#old]");
        let current = continuation_doc("do [#new]");
        std::fs::write(&doc, &current).unwrap();

        let continuation =
            detect_required_continuation_with(&doc, |_| Ok(Some(snapshot.clone())), |_| false)
                .unwrap();

        assert!(
            continuation.is_none(),
            "operator-modified queue head should suppress continuation"
        );
    }

    #[test]
    fn recycle_yield_short_circuits_before_file_and_snapshot_reads() {
        let doc = std::path::PathBuf::from("/definitely/missing/task.md");

        let continuation = detect_required_continuation_with(
            &doc,
            |_| panic!("snapshot loader should not run while yielding for recycle"),
            |_| true,
        )
        .unwrap();

        assert!(continuation.is_none());
    }
}
