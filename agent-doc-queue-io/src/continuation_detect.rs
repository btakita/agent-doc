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
    // `#queueclaim`: a head claimed by a worker outside the in-session loop
    // (a dispatched subagent) is in flight, not drainable.
    let claimed = crate::subagent_dispatch::out_of_loop_items_for_content(file, &content);
    // `#qheadresiduescope`: the answered-head skip counts only echoes written
    // after each head's entry point, read from the document's git history.
    queue_continuation::required_continuation_excluding_claimed_scoped(
        &content,
        snapshot_content.as_deref(),
        &claimed,
        &|queue_id, head| {
            agent_doc_git_io::revision::free_text_head_entry_anchor_exchange(file, queue_id, head)
        },
    )
}

/// Detect whether already-resolved current document content requires queue
/// continuation.
pub fn detect_required_continuation_for_content_with(
    file: &Path,
    content: &str,
    load_snapshot: impl FnOnce(&Path) -> Result<Option<String>>,
    recycle_yield_pending: impl FnOnce(&Path) -> bool,
) -> Result<Option<QueueContinuation>> {
    if recycle_yield_pending(file) {
        return Ok(None);
    }
    let snapshot_content = load_snapshot(file)?;
    let claimed = crate::subagent_dispatch::out_of_loop_items_for_content(file, content);
    // `#qheadresiduescope`: the answered-head skip counts only echoes written
    // after each head's entry point, read from the document's git history.
    queue_continuation::required_continuation_excluding_claimed_scoped(
        content,
        snapshot_content.as_deref(),
        &claimed,
        &|queue_id, head| {
            agent_doc_git_io::revision::free_text_head_entry_anchor_exchange(file, queue_id, head)
        },
    )
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
    fn queue_subagent_head_is_dispatch_work_not_inline_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        let content = continuation_doc("do [#a]").replace(
            "<!-- agent:queue go -->",
            "<!-- agent:queue subagents go -->",
        );
        std::fs::write(&doc, &content).unwrap();

        let continuation = detect_required_continuation_for_content_with(
            &doc,
            &content,
            |_| Ok(Some(content.clone())),
            |_| false,
        )
        .unwrap();

        assert!(
            continuation.is_none(),
            "an unclaimed subagent-intent head must be dispatched, not drained inline"
        );
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

    const REPORT: &str = "the session list flickers when a second pane opens the same document";

    fn residue_scope_doc(exchange: &str, queue: &str) -> String {
        format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue: go\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue go -->\n{queue}- do [#beta]\n<!-- /agent:queue -->\n"
        )
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit(dir: &Path, doc: &Path, content: &str, message: &str) {
        std::fs::write(doc, content).unwrap();
        git(dir, &["add", "task.md"]);
        git(
            dir,
            &["-c", "commit.gpgsign=false", "commit", "-q", "-m", message],
        );
    }

    /// `#qheadresiduescope`: the operator reported a bug, a cycle answered and
    /// struck it, then the operator typed the same report again and a later
    /// closeout answered something else. The continuation-head skip matched the
    /// re-typed head against the old echo and left it out of the offer. Scoped
    /// to echoes after its re-entry (read from git history), it is the next
    /// head. Covers both re-entry shapes: the old struck row deleted, and kept.
    #[test]
    fn continuation_offers_a_retyped_report_answered_only_before_reentry() {
        for keep_struck_row in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            git(dir.path(), &["init", "-q"]);
            git(dir.path(), &["config", "user.email", "test@example.com"]);
            git(dir.path(), &["config", "user.name", "test"]);
            let doc = dir.path().join("task.md");
            let report_row = format!("- {REPORT}\n");
            let struck_row = format!("- ~~{REPORT}~~\n");
            commit(
                dir.path(),
                &doc,
                &residue_scope_doc("", &report_row),
                "operator reports the bug",
            );
            let answered = format!("### Re: flicker\n\n> **Queue prompt:** {REPORT}\n\nFixed.\n\n");
            commit(
                dir.path(),
                &doc,
                &residue_scope_doc(&answered, &struck_row),
                "cycle answers and strikes it",
            );
            let retyped = if keep_struck_row {
                format!("{struck_row}{report_row}")
            } else {
                report_row.clone()
            };
            let later = format!(
                "{answered}### Re: other\n\n> **Queue prompt:** rename the export button\n\nRenamed.\n\n"
            );
            let current = residue_scope_doc(&later, &retyped);
            commit(dir.path(), &doc, &current, "operator re-types the report");

            let continuation =
                detect_required_continuation_with(&doc, |_| Ok(Some(current.clone())), |_| false)
                    .unwrap()
                    .expect("an active go queue with live heads requires continuation");
            assert_eq!(
                continuation.head_prompt, REPORT,
                "keep_struck_row={keep_struck_row}: a re-typed report answered only \
                 before it re-entered the queue must be offered"
            );
        }
    }

    /// `#qheadresiduescope` counterpart: a re-typed report answered by an echo
    /// written after its re-entry (whose strike was missed) is still skipped.
    #[test]
    fn continuation_skips_a_retyped_report_answered_after_reentry() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "test"]);
        let doc = dir.path().join("task.md");
        let report_row = format!("- {REPORT}\n");
        let answered = format!("### Re: flicker\n\n> **Queue prompt:** {REPORT}\n\nFixed.\n\n");
        commit(
            dir.path(),
            &doc,
            &residue_scope_doc(&answered, &format!("- ~~{REPORT}~~\n")),
            "first answer, struck",
        );
        commit(
            dir.path(),
            &doc,
            &residue_scope_doc(&answered, &report_row),
            "operator re-types the report",
        );
        let answered_again = format!(
            "{answered}### Re: flicker again\n\n> **Queue prompt:** {REPORT}\n\nFixed the second path.\n\n"
        );
        let current = residue_scope_doc(&answered_again, &report_row);
        commit(dir.path(), &doc, &current, "answered again; strike missed");

        let continuation =
            detect_required_continuation_with(&doc, |_| Ok(Some(current.clone())), |_| false)
                .unwrap()
                .expect("the id-backed head still requires continuation");
        assert_eq!(continuation.head_prompt, "do [#beta]");
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
