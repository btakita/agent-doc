//! Queue auto-clear hand-off probe (`#bgclearpolicy`).
//!
//! With queue auto-clear on (the default for an active `agent:queue`), the
//! in-session drain hands each item boundary to the document's supervisor so
//! the supervisor can reset the agent context before dispatching the next head.
//! This module answers "does this drain yield at this boundary?" for the three
//! in-session continuation surfaces — preflight, `session-check`, and the Codex
//! Stop hook — so they cannot disagree.
//!
//! A hand-off is only valid when the receiver can take it: the supervisor must
//! be ready (`supervisor_drain_readiness_for_doc`) and must not owe an earlier
//! hand-off (`#supdrainyieldfalsifiable`). Otherwise the in-session drain keeps
//! going in the same context — never a stall.

use std::path::Path;

use agent_doc_controller::status::SupervisorDrainReadiness;
use agent_doc_queue::queue_auto_clear;

/// One evaluation of the auto-clear hand-off for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoClearYield {
    /// The queue marker does not carry `disable-auto-clear`.
    pub enabled: bool,
    /// Supervisor readiness; `None` when it was not probed (auto-clear off or
    /// nothing left to continue).
    pub readiness: Option<SupervisorDrainReadiness>,
    /// An earlier supervisor hand-off is still undrained.
    pub handoff_overdue: bool,
    /// The in-session drain ends its turn and lets the supervisor clear and
    /// dispatch the next head.
    pub yields: bool,
}

impl AutoClearYield {
    pub fn readiness_reason(&self) -> &'static str {
        self.readiness
            .map(SupervisorDrainReadiness::reason)
            .unwrap_or("not_probed")
    }
}

/// Evaluate the hand-off for `file` whose current text is `content`.
/// `continuation_required` is the in-session continuation verdict the caller
/// would otherwise act on.
pub fn in_session_auto_clear_yield(
    file: &Path,
    content: &str,
    continuation_required: bool,
) -> AutoClearYield {
    in_session_auto_clear_yield_with(
        content,
        continuation_required,
        || agent_doc_controller_io::project_controller::supervisor_drain_readiness_for_doc(file),
        || undrained_supervisor_drain_handoff_age(file, content).is_some(),
    )
}

/// `#supdrainyieldfalsifiable` age of an undrained supervisor hand-off, judged
/// against the head the supervisor would actually dispatch: claimed and
/// subagent-held heads are skipped exactly as the supervisor's idle drain skips
/// them (`#claimeddrainwarn`). A head a worker owns is never overdue.
pub fn undrained_supervisor_drain_handoff_age(file: &Path, content: &str) -> Option<u64> {
    agent_doc_controller_io::project_controller::undrained_supervisor_drain_handoff_age(
        file,
        content,
        &crate::queue_claim::claimed_items_for_content(file, content),
    )
}

/// Record the hand-off of `content`'s next supervisor-drainable, unclaimed head
/// (`#claimeddrainwarn`); clears any earlier record when every such head is
/// claimed, so an all-claimed queue never accrues a hand-off age.
pub fn record_supervisor_drain_handoff(file: &Path, content: &str) -> Option<String> {
    agent_doc_controller_io::project_controller::record_supervisor_drain_handoff_for_content(
        file,
        content,
        &crate::queue_claim::claimed_items_for_content(file, content),
    )
}

/// [`in_session_auto_clear_yield`] with the supervisor probes injected.
pub fn in_session_auto_clear_yield_with(
    content: &str,
    continuation_required: bool,
    readiness: impl FnOnce() -> SupervisorDrainReadiness,
    handoff_overdue: impl FnOnce() -> bool,
) -> AutoClearYield {
    let enabled = queue_auto_clear::queue_auto_clear_enabled(content);
    if !(enabled && continuation_required) {
        return AutoClearYield {
            enabled,
            readiness: None,
            handoff_overdue: false,
            yields: false,
        };
    }
    let readiness = readiness();
    let handoff_overdue = readiness.is_ready() && handoff_overdue();
    AutoClearYield {
        enabled,
        readiness: Some(readiness),
        handoff_overdue,
        yields: queue_auto_clear::in_session_drain_yields_to_auto_clear(
            enabled,
            continuation_required,
            readiness.is_ready(),
            handoff_overdue,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUEUE: &str = "<!-- agent:queue go -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n";
    const QUEUE_NO_CLEAR: &str =
        "<!-- agent:queue go disable-auto-clear -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n";
    const READY: SupervisorDrainReadiness = SupervisorDrainReadiness::Ready { supervisor_pid: 7 };

    #[test]
    fn yields_to_a_ready_supervisor_by_default() {
        let decision = in_session_auto_clear_yield_with(QUEUE, true, || READY, || false);
        assert!(decision.enabled);
        assert!(decision.yields);
        assert_eq!(decision.readiness_reason(), "ready");
    }

    #[test]
    fn disable_auto_clear_keeps_the_in_session_drain_without_probing() {
        let decision = in_session_auto_clear_yield_with(
            QUEUE_NO_CLEAR,
            true,
            || panic!("readiness must not be probed when auto-clear is off"),
            || panic!("hand-off must not be probed when auto-clear is off"),
        );
        assert!(!decision.enabled);
        assert!(!decision.yields);
    }

    #[test]
    fn no_live_supervisor_or_an_overdue_handoff_keeps_the_in_session_drain() {
        let missing = in_session_auto_clear_yield_with(
            QUEUE,
            true,
            || SupervisorDrainReadiness::NoLiveSupervisor,
            || panic!("an unready supervisor has no hand-off to check"),
        );
        assert!(!missing.yields);
        assert_eq!(missing.readiness_reason(), "no_live_supervisor");

        let overdue = in_session_auto_clear_yield_with(QUEUE, true, || READY, || true);
        assert!(overdue.handoff_overdue);
        assert!(!overdue.yields);
    }

    /// Push the recorded supervisor hand-off for `head` 35 minutes into the past.
    fn backdate_handoff(root: &Path, file: &Path, head: &str) {
        let canonical = file.canonicalize().unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(root).unwrap();
        let backdated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 35 * 60;
        agent_doc_sqlite::state_store::upsert_document_runtime_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::DocumentRuntimeStateRecord {
                document_hash: agent_doc_hash::document_id_for_path(&canonical),
                state_kind: "supervisor_drain_handoff".to_string(),
                canonical_path: canonical.to_string_lossy().into_owned(),
                payload_json: format!(
                    "{{\"head_sha256\":\"{}\",\"handed_off_secs\":{backdated}}}",
                    agent_doc_hash::content_hash(head)
                ),
                updated_at_ms: backdated * 1_000,
            },
        )
        .unwrap();
    }

    #[test]
    fn auto_clear_yield_then_all_heads_claimed_reports_no_undrained_handoff() {
        // `#claimeddrainwarn` replay against the real claim ledger: the
        // auto-clear yield hands the boundary to the supervisor, then every
        // remaining head is claimed by a subagent or the coordinator. The
        // supervisor (which skips claimed heads) has nothing to drain, so the
        // hand-off must not be reported undrained — no preflight warning, and
        // the next boundary is not forced back in-session.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(
            &file,
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Queue\n\n<!-- agent:queue go -->\n- fix issue 1 now\n- fix issue 2 now\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let content = std::fs::read_to_string(&file).unwrap();

        // Head 1 is already with a subagent when the boundary yields: the
        // recorded hand-off is the head the supervisor will dispatch.
        crate::queue_claim::claim(&file, "fix issue 1 now", "subagent:one", 3_600).unwrap();
        assert_eq!(
            record_supervisor_drain_handoff(&file, &content).as_deref(),
            Some("fix issue 2 now")
        );

        // The pre-fix record (the claimed head 1), now 35 minutes old.
        backdate_handoff(dir.path(), &file, "fix issue 1 now");
        // The coordinator claims the remaining head: every head is claimed.
        crate::queue_claim::claim(&file, "fix issue 2 now", "coordinator", 3_600).unwrap();
        assert_eq!(
            undrained_supervisor_drain_handoff_age(&file, &content),
            None,
            "an all-claimed queue gives the supervisor nothing to drain"
        );
        let decision = in_session_auto_clear_yield_with(
            &content,
            true,
            || READY,
            || undrained_supervisor_drain_handoff_age(&file, &content).is_some(),
        );
        assert!(!decision.handoff_overdue);

        // An all-claimed yield records nothing, and a later release of a
        // claim does not revive the settled hand-off's age.
        assert_eq!(record_supervisor_drain_handoff(&file, &content), None);
        crate::queue_claim::release(&file, "fix issue 2 now").unwrap();
        assert_eq!(
            undrained_supervisor_drain_handoff_age(&file, &content),
            None
        );

        // Control: an unclaimed head the supervisor genuinely owes past the
        // window is still reported.
        backdate_handoff(dir.path(), &file, "fix issue 2 now");
        assert!(undrained_supervisor_drain_handoff_age(&file, &content).is_some());
    }

    #[test]
    fn nothing_to_continue_never_yields() {
        let decision = in_session_auto_clear_yield_with(
            QUEUE,
            false,
            || panic!("no continuation, no probe"),
            || panic!("no continuation, no probe"),
        );
        assert!(!decision.yields);
    }
}
