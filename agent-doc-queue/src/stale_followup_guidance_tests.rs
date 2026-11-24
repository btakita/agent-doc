//! `#staleguidance`: a struck row whose response still says `Follow-up work
//! remains.` after its action items became follow-up rows is re-derived by
//! every surface that reads or writes the queue, not only by preflight.
//!
//! The live shape (agent-doc-bugs, 2026-11-24): the struck
//! `[from:dogfood key=directivepos]` row's `Response [#r-5e3ae3d27dd6]` lists
//! one agent action item. It became `[#q-4009a36ee6c0] follow-up: agent: …`,
//! which a worker finished; the row was struck and then deleted from the
//! queue, so it survives only as the queue-prompt quote of its completion
//! cell. The coordinator's turns ran without preflight
//! (`queue_maintenance_skipped reason=chat_prompt_turn_without_preflight`),
//! so the one pass that rewrites the State
//! ([`super::reconcile_moved_followup_states`]) never ran: the tree kept
//! `Keep this tree and follow the action items`, and `queue tree-status`
//! read the stale State and reported `needs agent review — its response
//! lists follow-up next actions; agent-doc cannot tell who owns them`.

use super::*;
use crate::reconcile_incremental::{ReconcileInputs, queue_sync_response_chain};

const ORIGIN: &str = "- ### ~~[from:dogfood key=directivepos] fix: two finished rows still have no agent close path. #spec-test-build-install-commit-push [#q-4009a36ee6c0]~~";

const ACTION_ITEM: &str = "agent: an editor replica re-registering with its own buffer overwrites writes the binary already accepted (an `--ack-text` strike went into the shared document state, then the editor re-registered with a copy that lacked it). Re-registration must merge or reapply accepted authority writes onto the editor's buffer, never let the stale buffer win; reproduce it with a failing test.";

fn live_session() -> String {
    format!(
        concat!(
            "# Session\n\n",
            "## Run <!-- agent:run id=\"main\" subagents priority go -->\n",
            "- 🚧 [#subagent:other#1] [from:coordinator key=other] agent: other live work [#q-111111111111]\n",
            "{origin}\n",
            "  - **Response [#r-5e3ae3d27dd6]**\n",
            "    - **Timestamp:** 2026-11-24T02:35:34.000Z\n",
            "    - **Summary:** Both parts are fixed, pushed to main as `378561e5d2`, and the worktrees are removed.\n",
            "      - First, a directive now counts only in directive position. <!-- agent-doc:response-detail -->\n",
            "    - **Action items:** <!-- agent-doc:response-detail -->\n",
            "      - {item} <!-- agent-doc:response-detail -->\n",
            "    - **Details:** The full Summary, What changed, Verification, Notes and Next actions are in the `subagent:dirpos` hand-back in agent:exchange.\n",
            "    - **State:** Follow-up work remains.\n",
            "    - **Guidance:** More work remains. Keep this tree and follow the action items in this response.\n",
            "- do [#unrelated] something else\n",
            "<!-- /agent:run -->\n\n",
            "## Exchange <!-- agent:exchange patch=append subagents -->\n",
            "### Re: Subagent completed: `subagent:rereg` returned work for queue item “[#q-4009a36ee6c0] follow-up: {item} [#q-6755d6c554dd]” — agent-doc\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> [#q-4009a36ee6c0] follow-up: {item} [#q-6755d6c554dd]\n\n",
            "> **Subagent response:**\n",
            ">\n",
            "> **Summary**\n",
            ">\n",
            "> Re-registration now applies accepted writes on top of the editor's buffer.\n",
            "<!-- /agent:exchange -->\n",
        ),
        origin = ORIGIN,
        item = ACTION_ITEM,
    )
}

fn line_of(document: &str, needle: &str) -> usize {
    document
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not in:\n{document}"))
        + 1
}

/// The State and Guidance lines of the origin row's response.
fn origin_state(document: &str) -> (String, String) {
    let start = document.find("**Response [#r-5e3ae3d27dd6]**").unwrap();
    let rest = &document[start..];
    let value = |label: &str| {
        let at = rest.find(label).unwrap() + label.len();
        rest[at..].lines().next().unwrap().trim().to_string()
    };
    (value("**State:**"), value("**Guidance:**"))
}

fn assert_complete(document: &str) {
    let (state, guidance) = origin_state(document);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{document}");
    assert!(
        guidance.ends_with(FOLLOW_UP_MOVED_GUIDANCE_NOTE),
        "{guidance}\n{document}"
    );
    assert!(
        !guidance.contains("Keep this tree and follow"),
        "{guidance}"
    );
}

#[test]
fn fixture_is_the_stale_live_shape() {
    let (state, guidance) = origin_state(&live_session());
    assert_eq!(state, RESPONSE_STATE_FOLLOW_UP);
    assert_eq!(guidance, FOLLOW_UP_ACTION_ITEMS_GUIDANCE);
}

/// `queue tree-status` derives the row from the document, not from the stale
/// stored State: every action item has its follow-up row, so the struck row
/// is finished and deletable, never `needs agent review`.
#[test]
fn tree_status_rederives_a_moved_follow_up_tree() {
    let document = live_session();
    let line = line_of(&document, "key=directivepos");
    let status = crate::tree_status::tree_status(&document, line).unwrap();
    assert!(status.needs_review.is_empty(), "{status:#?}");
    assert!(
        status.deletable.iter().any(|row| row.line == line),
        "{status:#?}"
    );
    // The whole-queue view agrees.
    let status =
        crate::tree_status::tree_status(&document, line_of(&document, "## Run") + 1).unwrap();
    assert!(
        status.needs_review.iter().all(|row| row.line != line),
        "{status:#?}"
    );
}

/// An action item that never had a row is still written down only in the
/// tree, so tree-status keeps asking for review.
#[test]
fn tree_status_keeps_an_action_item_without_its_row_in_review() {
    let document = live_session().replace(
        "> [#q-4009a36ee6c0] follow-up:",
        "> [#q-4009a36ee6c0] unrelated quote:",
    );
    let line = line_of(&document, "key=directivepos");
    let status = crate::tree_status::tree_status(&document, line).unwrap();
    assert!(
        status.needs_review.iter().any(|row| row.line == line),
        "{status:#?}"
    );
}

/// Any later strike re-derives every stale moved tree in the document, so the
/// coordinator's next closeout repairs it without an operator edit.
#[test]
fn a_strike_of_any_row_rederives_stale_moved_trees() {
    let before = live_session();
    let after = before.replace(
        "- do [#unrelated] something else\n",
        "- ~~do [#unrelated] something else~~\n",
    );
    let struck = crate::queue_consume::attach_stopped_row_responses(
        &before,
        &after,
        "",
        "2026-11-24T05:00Z",
    )
    .unwrap();
    assert_complete(&struck);
    assert!(
        struck.contains("- ~~do [#unrelated] something else~~"),
        "{struck}"
    );
}

/// `agent-doc queue sync` runs the same pass.
#[test]
fn queue_sync_rederives_stale_moved_trees() {
    let inputs = ReconcileInputs {
        is_live: &|_, _| false,
        response_proven: &|_| true,
        struck_provenance: &|_| false,
        is_claimed: &|_| false,
    };
    let synced = queue_sync_response_chain(&live_session(), &inputs).document;
    assert_complete(&synced);
    assert_eq!(
        queue_sync_response_chain(&synced, &inputs).document,
        synced,
        "a second sync changes nothing"
    );
}

/// The follow-up row is linked to the action item by its origin id prefix,
/// live or struck in the queue as well as quoted in the exchange.
#[test]
fn a_live_or_struck_follow_up_row_links_the_action_item() {
    let without_quote = live_session().replace(
        "> [#q-4009a36ee6c0] follow-up:",
        "> [#q-4009a36ee6c0] unrelated quote:",
    );
    for row in [
        format!("- [#q-4009a36ee6c0] follow-up: {ACTION_ITEM} [#q-6755d6c554dd]\n"),
        format!("- ~~[#q-4009a36ee6c0] follow-up: {ACTION_ITEM} [#q-6755d6c554dd]~~\n"),
    ] {
        let document = without_quote.replace(
            "- do [#unrelated] something else\n",
            &format!("{row}- do [#unrelated] something else\n"),
        );
        let (repaired, rows) = reconcile_moved_followup_states(&document).expect("repaired");
        assert_eq!(rows, 1);
        assert_complete(&repaired);
    }
}
