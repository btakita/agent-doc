//! Tests for [`super`] (`#followupmovedstate`).

use super::*;

/// A session where a struck follow-up row's response still says to keep its
/// tree while its three action items already became their own follow-up rows:
/// one struck, one live and waiting on the operator, and one completed and
/// deleted from the queue, whose row survives only as the queue-prompt quote
/// of its completion cell.
const SESSION: &str = include_str!("../tests/fixtures/followup_moved_state_session.md");

const ROOT_NEEDLE: &str =
    "- ### ~~follow up on an action item from queue item “agent-doc duplicated";

fn root_tree(document: &str) -> &str {
    let start = document.find(ROOT_NEEDLE).expect("struck root row");
    let rest = &document[start + 1..];
    let end = rest
        .find("\n- ")
        .map_or(document.len(), |at| start + 1 + at);
    &document[start..end]
}

fn root_needs_review(document: &str) -> bool {
    let line = document
        .lines()
        .position(|line| line.starts_with(ROOT_NEEDLE))
        .expect("struck root row")
        + 1;
    let status = crate::tree_status::tree_status(document, line).unwrap();
    status.needs_review.iter().any(|row| row.line == line)
}

/// The live shape: the root is struck, every action item has its own row, so
/// the root's response says the work moved instead of "keep this tree".
#[test]
fn struck_root_whose_action_items_became_rows_reads_complete() {
    assert!(
        root_tree(SESSION).contains(&format!("**Guidance:** {FOLLOW_UP_ACTION_ITEMS_GUIDANCE}")),
        "fixture keeps the inconsistent live shape"
    );
    assert!(
        root_needs_review(SESSION),
        "tree-status left the struck root in review"
    );

    let (repaired, rows) = reconcile_moved_followup_states(SESSION).expect("root repaired");
    assert_eq!(rows, 1);
    let tree = root_tree(&repaired);
    assert!(tree.contains("    - **State:** Complete.\n"), "{tree}");
    assert!(
        tree.contains(&format!(
            "    - **Guidance:** {FOLLOW_UP_MOVED_GUIDANCE_LEAD} "
        )),
        "{tree}"
    );
    assert!(!tree.contains(RESPONSE_STATE_FOLLOW_UP), "{tree}");
    assert!(
        !tree.contains("Keep this tree and follow the action items"),
        "{tree}"
    );
    assert!(
        tree.contains("Safe to delete this completed row"),
        "the usual deletion guidance follows:\n{tree}"
    );
    // The action items themselves stay in the response as its record.
    assert_eq!(
        tree.matches("<!-- agent-doc:response-detail -->").count(),
        6,
        "{tree}"
    );
    assert!(
        !root_needs_review(&repaired),
        "tree-status now treats the root as complete"
    );

    // Only the State and Guidance values change.
    let changed: Vec<_> = SESSION
        .lines()
        .zip(repaired.lines())
        .filter(|(old, new)| old != new)
        .collect();
    assert_eq!(changed.len(), 2, "{changed:#?}");
    assert_eq!(SESSION.lines().count(), repaired.lines().count());

    assert!(
        reconcile_moved_followup_states(&repaired).is_none(),
        "a second pass changes nothing"
    );
}

/// An action item whose follow-up row was never placed is written down only in
/// this tree, so the tree keeps saying so.
#[test]
fn an_action_item_without_its_row_keeps_the_follow_up_state() {
    let without_operator_row = SESSION
        .lines()
        .filter(|line| !line.starts_with("- ### 🙋 follow up on an action item"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(reconcile_moved_followup_states(&without_operator_row).is_none());

    let without_ci_quote = SESSION
        .lines()
        .filter(|line| !line.starts_with("> follow up on an action item"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(reconcile_moved_followup_states(&without_ci_quote).is_none());
}

/// A live row, and a struck row whose response is complete or waiting on the
/// operator, are never rewritten.
#[test]
fn only_a_struck_rows_follow_up_response_is_rewritten() {
    let live = SESSION.replacen(
        "- ### ~~follow up on an action item from queue item “agent-doc duplicated",
        "- ### follow up on an action item from queue item “agent-doc duplicated",
        1,
    );
    let live = live.replacen("08:06Z. ⟦published⟧~~\n", "08:06Z.\n", 1);
    assert!(reconcile_moved_followup_states(&live).is_none(), "{live}");

    let waiting = SESSION.replacen(
        "**Guidance:** More work remains. Keep this tree and follow the action items in this response.",
        "**Guidance:** More work remains. Keep this tree and answer the operator request below.",
        1,
    );
    assert!(reconcile_moved_followup_states(&waiting).is_none());
}

const STRUCK_AT: &str = "2026-11-21T08:31:41.000Z";

const ORIGIN: &str = "follow up on an action item from queue item “agent-doc duplicated my prompt ` - ~~Should we integrate these?~~` above this pr…”: agent: find how a row's own claim label got written into its text.";

fn handback() -> String {
    "**Summary**\n\n\
     Traced the leak and fixed it.\n\n\
     **Verification**\n\n\
     cargo test passed.\n\n\
     **Action items**\n\n\
     - agent: find the merge step that inserts the state word; this needs a reproduction harness.\n\
     - agent: check CI for the pushed commit.\n\
     - operator: decide whether to clean up the old follow-up titles by hand.\n\n\
     **Notes**\n\n\
     No install.\n"
        .to_string()
}

fn session(rows: &str, cell: &str) -> String {
    format!(
        "---\nagent_doc_session: followup-moved-state-strike\n---\n\n\
         <!-- agent:exchange -->\n{cell}\n<!-- /agent:exchange -->\n\n\
         <!-- agent:run id=\"main\" subagents -->\n{rows}<!-- /agent:run -->\n"
    )
}

/// The strike itself renders the consistent state: report-complete placed
/// every action item as its own row before the coordinator struck the head,
/// so the head's new response says the work moved there.
#[test]
fn strike_after_report_complete_renders_the_moved_state() {
    let response = handback();
    let cell = agent_doc_document::subagent_completion::cell_with_response(
        "subagent:markerleak",
        ORIGIN,
        "markerleak-key",
        &response,
    )
    .unwrap();
    let reported = session(
        &format!("- ### ☑️ [#subagent:markerleak#1] {ORIGIN}\n"),
        &cell,
    );
    let (placed, outcome) = crate::handback_followups::plan_handback_followups(
        &reported,
        &format!("☑️ [#subagent:markerleak#1] {ORIGIN}"),
        &response,
    );
    assert_eq!(outcome.inserted.len(), 3, "{outcome:#?}\n{placed}");
    let struck_row = format!("- ### ~~{ORIGIN} ⟦published⟧~~\n");
    let struck = placed.replacen(
        &format!("- ### ☑️ [#subagent:markerleak#1] {ORIGIN}\n"),
        &struck_row,
        1,
    );
    assert_ne!(struck, placed);

    let projected =
        crate::queue_consume::attach_stopped_row_responses(&placed, &struck, "", STRUCK_AT)
            .unwrap();
    let start = projected.find(&struck_row).expect("struck row");
    let tree_end = projected[start + struck_row.len()..]
        .find("\n- ")
        .map_or(projected.len(), |at| start + struck_row.len() + at);
    let tree = &projected[start..tree_end];
    assert!(tree.contains("**Response [#r-"), "{projected}");
    // `#handbackrowstructure`: the moved items live in their follow-up rows,
    // so the slim tree does not copy them again.
    assert!(!tree.contains("**Action items:**"), "{tree}");
    assert!(tree.contains("- **Details:** "), "{tree}");
    assert!(tree.contains("- **State:** Complete.\n"), "{tree}");
    assert!(
        tree.contains(&format!("- **Guidance:** {FOLLOW_UP_MOVED_GUIDANCE_LEAD} ")),
        "{tree}"
    );
    assert!(!tree.contains(FOLLOW_UP_ACTION_ITEMS_GUIDANCE), "{tree}");
    assert!(reconcile_moved_followup_states(&projected).is_none());

    // Without the placed rows (a refused placement whose printed command has
    // not run), the struck head is the only place the action items live.
    let unplaced = reported.replacen(
        &format!("- ### ☑️ [#subagent:markerleak#1] {ORIGIN}\n"),
        &struck_row,
        1,
    );
    let projected =
        crate::queue_consume::attach_stopped_row_responses(&reported, &unplaced, "", STRUCK_AT)
            .unwrap();
    assert!(
        projected.contains(&format!("- **State:** {RESPONSE_STATE_FOLLOW_UP}\n")),
        "{projected}"
    );
    assert!(
        projected.contains(&format!(
            "- **Guidance:** {FOLLOW_UP_ACTION_ITEMS_GUIDANCE}\n"
        )),
        "{projected}"
    );

    // Once the coordinator runs the printed `queue follow-up` command, the
    // next maintenance pass updates the tree.
    let rows: String = placed
        .lines()
        .filter(|line| line.starts_with("- follow up on an action item from"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(rows.lines().count(), 3, "{placed}");
    let late = projected.replacen(
        "<!-- /agent:run -->",
        &format!("{rows}<!-- /agent:run -->"),
        1,
    );
    let (repaired, rows) = reconcile_moved_followup_states(&late)
        .unwrap_or_else(|| panic!("late placement repaired:\n{late}"));
    assert_eq!(rows, 1);
    assert!(
        repaired.contains(&format!("- **Guidance:** {FOLLOW_UP_MOVED_GUIDANCE_LEAD} ")),
        "{repaired}"
    );
}
