//! `#parentstrike` tests. The fixtures are the sdk-dev.md shape of
//! 2026-11-24, anonymized: an operator `#justdoit` reply under a `🙋`
//! operator-verify sign-in row whose follow-ups (and theirs) nested under it.

use super::*;

const SIGN_IN: &str = "🙋 [#q-bbbb00000001] follow-up: operator: sign in to the admin console. [operator-verify] [#q-bbbb00000002]";

fn live_shape(extra_child: &str) -> String {
    format!(
        concat!(
            "## Run <!-- agent:run id=main subagents -->\n",
            "- ### [#q-aaaa00000001] follow-up: agent: find why the chat message was delivered. [#q-aaaa00000002]\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Summary:** Not classified yet.\n",
            "    - **State:** Waiting on operator input.\n",
            "    - {sign_in}\n",
            "      - ~~#justdoit~~\n",
            "        - **Response [#r-1123456789ab]**\n",
            "          - **Summary:** Ran it with the signed-in session.\n",
            "          - **State:** Complete.\n",
            "      - 🙋 [#justdoit] follow-up: human (product owner): decide whether swearing is moderated. [operator-verify] [#q-cccc00000001]\n",
            "      - 📥 [#justdoit] follow-up: agent: open the config PR once that is decided. [#q-cccc00000002]\n",
            "{extra}",
            "      - 🙋 [#q-cccc00000002] follow-up: human (reviewer): approve the config PR. [operator-verify] [#q-cccc00000004]\n",
            "        - note from the operator\n",
            "- 🙋 [#q-aaaa00000002] follow-up: human (product owner): decide visibility. [operator-verify] [#q-dddd00000001]\n",
            "<!-- /agent:run -->\n",
        ),
        sign_in = SIGN_IN,
        extra = extra_child,
    )
}

const RUNNING_CHILD: &str = "      - 🚧 [#subagent:cfg#1] [#justdoit] follow-up: agent: measure the wait. [#q-cccc00000003]\n";

fn unclaimed(_: &str) -> bool {
    false
}

/// The 18 follow-up rows of the live document were children of the sign-in
/// row. Self-repair lifts each unclaimed one to be the sign-in row's
/// sibling, in order, with its own subtree; the struck `#justdoit` tree and
/// the running row (its claim is keyed by its nest vector) stay.
#[test]
fn misplaced_followups_lift_out_of_the_operator_verify_row() {
    let doc = live_shape(RUNNING_CHILD);
    let (repaired, repairs) = lift_misplaced_followups(&doc, &unclaimed);
    let expected = format!(
        concat!(
            "## Run <!-- agent:run id=main subagents -->\n",
            "- ### [#q-aaaa00000001] follow-up: agent: find why the chat message was delivered. [#q-aaaa00000002]\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Summary:** Not classified yet.\n",
            "    - **State:** Waiting on operator input.\n",
            "    - {sign_in}\n",
            "      - ~~#justdoit~~\n",
            "        - **Response [#r-1123456789ab]**\n",
            "          - **Summary:** Ran it with the signed-in session.\n",
            "          - **State:** Complete.\n",
            "      - 🚧 [#subagent:cfg#1] [#justdoit] follow-up: agent: measure the wait. [#q-cccc00000003]\n",
            "    - 🙋 [#justdoit] follow-up: human (product owner): decide whether swearing is moderated. [operator-verify] [#q-cccc00000001]\n",
            "    - 📥 [#justdoit] follow-up: agent: open the config PR once that is decided. [#q-cccc00000002]\n",
            "    - 🙋 [#q-cccc00000002] follow-up: human (reviewer): approve the config PR. [operator-verify] [#q-cccc00000004]\n",
            "      - note from the operator\n",
            "- 🙋 [#q-aaaa00000002] follow-up: human (product owner): decide visibility. [operator-verify] [#q-dddd00000001]\n",
            "<!-- /agent:run -->\n",
        ),
        sign_in = SIGN_IN,
    );
    assert_eq!(repaired, expected);
    assert_eq!(repairs.len(), 3, "{repairs:?}");
    assert!(
        repairs
            .iter()
            .all(|repair| repair.kind == SelfRepairKind::MisplacedFollowupRow)
    );
    let (again, more) = lift_misplaced_followups(&repaired, &unclaimed);
    assert_eq!(again, repaired, "a second run changes nothing");
    assert!(more.is_empty());
}

/// A row the claim ledger holds keeps its place.
#[test]
fn a_claimed_followup_keeps_its_place() {
    let doc = live_shape("");
    let claimed = |row: &str| row.contains("[#q-cccc00000001]");
    let (repaired, _) = lift_misplaced_followups(&doc, &claimed);
    assert!(
        repaired.contains(
            "      - 🙋 [#justdoit] follow-up: human (product owner): decide whether swearing is moderated. [operator-verify] [#q-cccc00000001]\n"
        ),
        "{repaired}"
    );
    assert!(
        repaired.contains(
            "    - 📥 [#justdoit] follow-up: agent: open the config PR once that is decided. [#q-cccc00000002]\n"
        ),
        "{repaired}"
    );
}

/// An operator's own group row keeps its follow-ups: it is a group label,
/// not a standalone action row.
#[test]
fn followups_under_an_operator_group_row_stay() {
    let doc = concat!(
        "<!-- agent:run id=main -->\n",
        "- parent work\n",
        "  - origin work\n",
        "  - [#q-eeee00000001] follow-up: agent: do the next step. [#q-eeee00000002]\n",
        "<!-- /agent:run -->\n",
    );
    let (repaired, repairs) = lift_misplaced_followups(doc, &unclaimed);
    assert_eq!(repaired, doc);
    assert!(repairs.is_empty());
}

fn sign_in_start(doc: &str) -> usize {
    doc.find(&format!("    - {SIGN_IN}")).unwrap()
}

/// Closing the finished sign-in row lifts its open follow-ups out first, so
/// only the row itself is struck and every open row stays open.
#[test]
fn parent_close_lifts_open_followups_before_the_strike() {
    let doc = live_shape("");
    let plan = plan_parent_close(&doc, sign_in_start(&doc), &unclaimed).unwrap();
    assert!(plan.open_rows.is_empty(), "{plan:?}");
    assert_eq!(plan.parent, SIGN_IN);
    assert_eq!(plan.lifted.len(), 3, "{plan:?}");
    assert!(
        plan.document.contains(&format!(
            concat!(
                "    - {}\n",
                "      - ~~#justdoit~~\n",
                "        - **Response [#r-1123456789ab]**\n",
                "          - **Summary:** Ran it with the signed-in session.\n",
                "          - **State:** Complete.\n",
                "    - 🙋 [#justdoit] follow-up: human (product owner): decide whether swearing is moderated. [operator-verify] [#q-cccc00000001]\n",
                "    - 📥 [#justdoit] follow-up: agent: open the config PR once that is decided. [#q-cccc00000002]\n",
                "    - 🙋 [#q-cccc00000002] follow-up: human (reviewer): approve the config PR. [operator-verify] [#q-cccc00000004]\n",
                "      - note from the operator\n",
            ),
            SIGN_IN
        )),
        "{}",
        plan.document
    );
}

/// Running work under the row, or an operator's own sub-prompt, keeps the
/// parent open. The plan names each such row and changes nothing.
#[test]
fn parent_close_names_rows_that_keep_the_parent_open() {
    let doc = live_shape(concat!(
        "      - 🚧 [#subagent:cfg#1] [#justdoit] follow-up: agent: measure the wait. [#q-cccc00000003]\n",
        "      - rerun the smoke test on sbx\n",
    ));
    let plan = plan_parent_close(&doc, sign_in_start(&doc), &unclaimed).unwrap();
    assert_eq!(plan.document, doc);
    assert!(plan.lifted.is_empty());
    assert_eq!(
        plan.open_rows,
        vec![
            "🚧 [#subagent:cfg#1] [#justdoit] follow-up: agent: measure the wait. [#q-cccc00000003]"
                .to_string(),
            "rerun the smoke test on sbx".to_string(),
        ]
    );
}

/// Placement: a follow-up of `#justdoit`, the operator's reply under the
/// sign-in row, lands after the sign-in row's subtree at its depth.
#[test]
fn hoisted_slot_leaves_the_standalone_parent() {
    let body = concat!(
        "- ### [#q-aaaa00000001] follow-up: agent: find it. [#q-aaaa00000002]\n",
        "  - **Response [#r-0123456789ab]**\n",
        "    - 🙋 [#q-bbbb00000001] follow-up: operator: sign in. [operator-verify] [#q-bbbb00000002]\n",
        "      - 🚧 [#subagent:justdoit#1] #justdoit\n",
        "- later row\n",
    );
    let origin = body
        .lines()
        .position(|line| line.contains("#justdoit"))
        .unwrap();
    let (end, prefix) = hoisted_followup_slot(body, origin).unwrap();
    assert_eq!(&body[end..], "- later row\n");
    assert_eq!(prefix, "    ");
}

/// The queue sync chain (preflight, `queue sync`, the controller's
/// reconcile) runs the lift, and no later pass in the chain puts the rows
/// back: the sign-in row is a leaf again and the chain is a fixed point.
#[test]
fn queue_sync_chain_rehomes_the_live_shape() {
    use crate::reconcile_incremental::{
        ReconcileInputs, queue_sync_response_chain, reconcile_full,
    };
    let is_live = |_: &str, _: u64| true;
    let response_proven = |_: &str| false;
    let struck_provenance = |_: &str| false;
    let inputs = ReconcileInputs {
        is_live: &is_live,
        response_proven: &response_proven,
        struck_provenance: &struck_provenance,
        is_claimed: &unclaimed,
    };
    let doc = live_shape("");
    for synced in [
        queue_sync_response_chain(&doc, &inputs).document,
        reconcile_full(&doc, &inputs).document,
    ] {
        let rows = rows_of(&synced);
        let sign_in = rows
            .iter()
            .position(|row| {
                row.item
                    .is_some_and(|item| item.contains("[#q-bbbb00000001]"))
            })
            .unwrap_or_else(|| panic!("{synced}"));
        for id in [
            "[#q-cccc00000001]",
            "[#q-cccc00000002]",
            "[#q-cccc00000004]",
        ] {
            let at = rows
                .iter()
                .position(|row| {
                    row.item
                        .is_some_and(|item| item.ends_with(id) && is_followup_row(item))
                })
                .unwrap_or_else(|| panic!("{id} stays a live row: {synced}"));
            assert_eq!(
                direct_parent(&rows, at).and_then(|parent| rows[parent].item),
                direct_parent(&rows, sign_in).and_then(|parent| rows[parent].item),
                "{id} is the sign-in row's sibling:\n{synced}"
            );
        }
        assert_eq!(
            queue_sync_response_chain(&synced, &inputs).document,
            synced,
            "a second sync changes nothing"
        );
    }
}
