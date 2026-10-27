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
    queue_continuation::required_continuation_excluding_claimed_scoped_with_presets(
        &content,
        snapshot_content.as_deref(),
        &claimed,
        &head_echo_scope_for(file, &content),
        &agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(&content, file),
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
    queue_continuation::required_continuation_excluding_claimed_scoped_with_presets(
        content,
        snapshot_content.as_deref(),
        &claimed,
        &head_echo_scope_for(file, content),
        &agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(content, file),
    )
}

/// The `#syntheticstrike` answered-head skip's echo scope for `file` whose
/// current text is `content`: the resolver the preflight catch-up residue
/// strike uses (`queue_schedule::head_echo_window` +
/// `agent_doc_git_io::revision::head_echo_scope`), so a head that strike keeps
/// is never skipped here as answered.
///
/// - `#qheadresiduescope`: only echoes written after the head's entry point,
///   read from the document's git history.
/// - GH #327 pass 4: a `loop` queue's head counts only an echo written in its
///   open pass about this queue's copy, and none while no pass is open.
///
/// The queue's window is read once, and only when a head the whole exchange
/// calls answered asks for its scope.
fn head_echo_scope_for<'a>(
    file: &'a Path,
    content: &'a str,
) -> impl Fn(&str, &str) -> agent_doc_queue::queue_head_entry::HeadEchoScope + 'a {
    let windows: std::cell::RefCell<
        std::collections::HashMap<String, agent_doc_queue::queue_head_entry::HeadEchoWindow>,
    > = Default::default();
    move |queue_id, head| {
        let window = *windows
            .borrow_mut()
            .entry(queue_id.to_string())
            .or_insert_with(|| crate::queue_schedule::head_echo_window(file, content, queue_id));
        agent_doc_git_io::revision::head_echo_scope(file, queue_id, head, window, content)
    }
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

    /// GH #327 pass 4 fixture: a drained priority `go` queue and an hourly
    /// `loop` queue, so the loop queue is the selected one. `loop_attrs` lets
    /// a control drop `loop`.
    fn loop_scope_doc(exchange: &str, main: &str, looped: &str, loop_attrs: &str) -> String {
        format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n\
<!-- agent:queue priority id=queue-1 go -->\n{main}<!-- /agent:queue -->\n\n\
<!-- agent:queue {loop_attrs} id=queue-2 -->\n{looped}<!-- /agent:queue -->\n"
        )
    }

    const LOOP_ATTRS: &str = "loop interval=\"1h\"";
    const LOOP_SCOPE_HEAD: &str = "#upgrade";
    const LOOP_SCOPE_NEXT: &str = "do [#beta]";
    const LOOP_SCOPE_T0: u64 = 1_800_000_000;

    fn loop_scope_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "test"]);
        dir
    }

    /// Commit `content` as `session.md` with committer and author time
    /// `committed_secs`.
    fn loop_scope_commit(dir: &Path, doc: &Path, content: &str, committed_secs: u64) {
        std::fs::write(doc, content).unwrap();
        let date = format!("@{committed_secs} +0000");
        for args in [
            vec!["add", "session.md"],
            vec!["-c", "commit.gpgsign=false", "commit", "-q", "-m", "cycle"],
        ] {
            let out = std::process::Command::new("git")
                .current_dir(dir)
                .env("GIT_COMMITTER_DATE", &date)
                .env("GIT_AUTHOR_DATE", &date)
                .args(&args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    /// Persist queue-2's schedule row with an open pass, as the controller's
    /// scheduler does.
    fn loop_scope_open_pass(dir: &Path, doc: &Path, opened_secs: u64) {
        let state = agent_doc_queue::queue_schedule::QueueScheduleState {
            owner_queue_id: Some("queue-2".to_string()),
            cadence: "interval=3600".to_string(),
            anchor_secs: LOOP_SCOPE_T0 - 7200,
            last_fired_due_secs: None,
            last_fired_at_secs: None,
            passes_completed: 3,
            last_pass_completed_secs: None,
            window: agent_doc_queue::queue_schedule::ScheduleWindow::Open {
                pass: 4,
                opened_secs,
                roster: vec![LOOP_SCOPE_HEAD.to_string(), LOOP_SCOPE_NEXT.to_string()],
                ran: Vec::new(),
            },
            restore_pending: None,
            coalesced_ticks: 0,
            failures: 0,
            backoff_until_secs: None,
        };
        let conn = agent_doc_sqlite::state_store::open_state_db(dir).unwrap();
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: agent_doc_hash::path_hash(doc).unwrap(),
                state_kind: format!(
                    "{}:queue-2",
                    agent_doc_queue::queue_schedule::QUEUE_SCHEDULE_STATE_KIND
                ),
                canonical_path: doc.canonicalize().unwrap().to_string_lossy().into_owned(),
                payload_json: serde_json::to_string(&state).unwrap(),
                updated_at_secs: LOOP_SCOPE_T0,
            },
        )
        .unwrap();
        let loaded =
            agent_doc_controller_io::queue_scheduler::load_schedule_state_for_queue(doc, "queue-2")
                .unwrap()
                .expect("the fixture's schedule row is readable");
        assert_eq!(loaded, state);
    }

    fn loop_scope_echo() -> String {
        format!("### Re: upgrade\n\n> **Queue prompt:** {LOOP_SCOPE_HEAD}\n\nUpgraded.\n\n")
    }

    /// The committed history GH #327 pass 4 had: both queues held `#upgrade`;
    /// a queue-1 cycle answered queue-1's copy and finalize struck it there
    /// (`#ftstrike`), leaving queue-2's copy live. Returns the current text.
    fn loop_scope_history(dir: &Path, doc: &Path, answered_secs: u64, loop_attrs: &str) -> String {
        let live = format!("- {LOOP_SCOPE_HEAD}\n- {LOOP_SCOPE_NEXT}\n");
        loop_scope_commit(
            dir,
            doc,
            &loop_scope_doc("", &format!("- {LOOP_SCOPE_HEAD}\n"), &live, loop_attrs),
            LOOP_SCOPE_T0,
        );
        let current = loop_scope_doc(
            &loop_scope_echo(),
            &format!("- ~~{LOOP_SCOPE_HEAD}~~ — auto-struck: answered this cycle (#ftstrike)\n"),
            &live,
            loop_attrs,
        );
        loop_scope_commit(dir, doc, &current, answered_secs);
        current
    }

    fn loop_scope_continuation_head(doc: &Path, current: &str) -> String {
        let from_file =
            detect_required_continuation_with(doc, |_| Ok(Some(current.to_string())), |_| false)
                .unwrap()
                .expect("an open pass's live heads require continuation")
                .head_prompt;
        let for_content = detect_required_continuation_for_content_with(
            doc,
            current,
            |_| Ok(Some(current.to_string())),
            |_| false,
        )
        .unwrap()
        .expect("an open pass's live heads require continuation")
        .head_prompt;
        assert_eq!(from_file, for_content, "both adapters resolve one scope");
        from_file
    }

    /// GH #327 pass 4 (measured shape) for `#syntheticstrike`: the echo was
    /// committed before pass 4 opened and answered queue-1's copy. The
    /// preflight residue strike keeps queue-2's live `#upgrade`; the
    /// continuation offer skipped it as answered (head-entry scope only), so
    /// the pass would move past an item it never ran.
    #[test]
    fn continuation_offers_a_loop_head_whose_echo_predates_the_pass() {
        let dir = loop_scope_project();
        let doc = dir.path().join("session.md");
        let current = loop_scope_history(dir.path(), &doc, LOOP_SCOPE_T0 + 100, LOOP_ATTRS);
        loop_scope_open_pass(dir.path(), &doc, LOOP_SCOPE_T0 + 200);

        assert_eq!(
            loop_scope_continuation_head(&doc, &current),
            LOOP_SCOPE_HEAD,
            "an echo committed before the pass opened must not skip the loop head"
        );
    }

    /// GH #327 pass 4: an echo written after the pass opened, by the write that
    /// struck the same head in another queue, answered that queue's copy. The
    /// loop queue's copy is still offered.
    #[test]
    fn continuation_offers_a_loop_head_answered_for_another_queue() {
        let dir = loop_scope_project();
        let doc = dir.path().join("session.md");
        let current = loop_scope_history(dir.path(), &doc, LOOP_SCOPE_T0 + 100, LOOP_ATTRS);
        loop_scope_open_pass(dir.path(), &doc, LOOP_SCOPE_T0 + 50);

        assert_eq!(
            loop_scope_continuation_head(&doc, &current),
            LOOP_SCOPE_HEAD,
            "an echo about queue-1's head must not skip queue-2's copy"
        );
    }

    /// GH #327 pass 4 positive control: the loop pass's own cycle answered the
    /// loop head after the pass opened, and no other queue held it. The
    /// continuation offer skips it, exactly as the residue strike strikes it.
    #[test]
    fn continuation_skips_a_loop_head_answered_in_its_own_pass() {
        let dir = loop_scope_project();
        let doc = dir.path().join("session.md");
        let live = format!("- {LOOP_SCOPE_HEAD}\n- {LOOP_SCOPE_NEXT}\n");
        loop_scope_commit(
            dir.path(),
            &doc,
            &loop_scope_doc("", "", &live, LOOP_ATTRS),
            LOOP_SCOPE_T0,
        );
        let current = loop_scope_doc(&loop_scope_echo(), "", &live, LOOP_ATTRS);
        loop_scope_commit(dir.path(), &doc, &current, LOOP_SCOPE_T0 + 100);
        loop_scope_open_pass(dir.path(), &doc, LOOP_SCOPE_T0 + 50);

        assert_eq!(
            loop_scope_continuation_head(&doc, &current),
            LOOP_SCOPE_NEXT,
            "the pass's own echo answers the loop head"
        );
    }

    /// Non-loop control: the same history in a queue without `loop` keeps the
    /// `#qheadresiduescope` entry scope only. The head was live since before
    /// the echo, so the echo answers it and the offer skips it.
    #[test]
    fn continuation_keeps_the_entry_scope_for_a_non_loop_queue() {
        let dir = loop_scope_project();
        let doc = dir.path().join("session.md");
        let current = loop_scope_history(dir.path(), &doc, LOOP_SCOPE_T0 + 100, "go");

        assert_eq!(
            loop_scope_continuation_head(&doc, &current),
            LOOP_SCOPE_NEXT,
            "a non-loop queue's head-entry scope is unchanged"
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
