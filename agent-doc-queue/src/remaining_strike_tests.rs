//! `#remainingstrike`: a struck row's **Remaining** list follows the live
//! state of its action items' follow-up rows, and a strike never leaves an
//! operator decision written down only in a closed tree.
//!
//! Two live shapes from one session document on 2026-11-23/24, anonymized:
//!
//! - **The re-pin tree.** A worker's hand-back listed two agent action items,
//!   the first with nested detail entries. `queue report-complete` queued
//!   both as follow-up rows, joining the first item's details into one row.
//!   The hand-back's response tree landed under the second follow-up row.
//!   The first follow-up row finished (its own response `Complete.`) and was
//!   struck, then deleted; the exchange that quoted it was compacted. The
//!   tree still said `⏳ 1/2 done` and `Keep this tree`, because the joined
//!   row never matched the item's head line.
//! - **The stateless-JWT tree.** A struck question row's response listed two
//!   operator items. Their follow-up rows were finished or deleted, and the
//!   one that finished was quoted in the exchange with its lineage and the
//!   operator's nested answer (`<heading>: [#q-…] follow-up: operator: hold
//!   #7 …: We are moving to …`). The quote was not recognized, so the
//!   finished item stayed `🙋` under the struck row.

use crate::response_remaining::{
    BuriedOperatorItems, reconcile_response_remaining, struck_responses_burying_operator_items,
};

const AT: &str = "2026-11-23T20:39:10.000Z";

// ---------------------------------------------------------------------------
// The re-pin tree
// ---------------------------------------------------------------------------

const ORIGIN: &str =
    "[#setupgaps] follow-up: agent: fix the setup manifest gaps. [#q-7f1e992b915b]";
const SCRIPT_ROW: &str = "[#q-7f1e992b915b] follow-up: agent: on #12, update `scripts/check.py` at re-pin, or its stale-waiver check fails the run: Drop the cpp `--engine` substitution. Change the general `--online` substitution to match `tool check --online --engine <id>`. [#q-810c392ec541]";
const REPIN_ROW: &str = "[#q-7f1e992b915b] follow-up: agent: re-pin sdk #10 and #12 to contracts `1961c9b`. [#q-bd5a2af5f518]";

fn repin_response(state: &str, guidance: &str) -> String {
    format!(
        "  - **Response [#r-1c7433c63076]**\n\
         \x20   - **Timestamp:** {AT}\n\
         \x20   - **Summary:** I fixed both setup-manifest gaps.\n\
         \x20   - **Action items:** <!-- agent-doc:response-detail -->\n\
         \x20     - agent: on #12, update `scripts/check.py` at re-pin, or its stale-waiver check fails the run: <!-- agent-doc:response-detail -->\n\
         \x20       - Drop the cpp `--engine` substitution. <!-- agent-doc:response-detail -->\n\
         \x20       - Change the general `--online` substitution to match `tool check --online --engine <id>`. <!-- agent-doc:response-detail -->\n\
         \x20     - agent: re-pin sdk #10 and #12 to contracts `1961c9b`. <!-- agent-doc:response-detail -->\n\
         \x20   - **Details:** in the `subagent:w1` hand-back in agent:exchange.\n\
         \x20   - **State:** {state}\n\
         \x20   - **Guidance:** {guidance}\n"
    )
}

fn script_row_tree() -> String {
    format!(
        "- ### ~~{SCRIPT_ROW}~~\n\
         \x20 - **Response [#r-e39a0294043a]**\n\
         \x20   - **Timestamp:** 2026-11-23T20:49:15.000Z\n\
         \x20   - **Summary:** Done on #12 at 2700783cd.\n\
         \x20   - **State:** Complete.\n\
         \x20   - **Guidance:** 🗑️ Safe to delete this tree.\n"
    )
}

fn repin_doc(exchange: &str, script_tree: &str, response: &str) -> String {
    format!(
        "---\nqueue_active: true\n---\n\n\
         <!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n\
         <!-- agent:run id=\"main\" subagents -->\n\
         - ### ~~{ORIGIN}~~\n\
         {script_tree}\
         - ### ~~{REPIN_ROW}~~\n\
         {response}\
         - do [#later]\n\
         <!-- /agent:run -->\n"
    )
}

fn script_row_quote() -> String {
    format!("> **Queue prompt:**\n>\n> {SCRIPT_ROW}\n\n")
}

/// The repin tree's own lines: from its root to the next top-level row.
fn repin_tree(document: &str) -> String {
    let start = document
        .find(&format!("- ### ~~{REPIN_ROW}~~"))
        .expect("repin root");
    let tree = &document[start..];
    tree[..tree.find("- do [#later]").expect("next row")].to_string()
}

fn reconciled(document: &str) -> String {
    reconcile_response_remaining(document).map_or_else(|| document.to_string(), |(text, _)| text)
}

const LEGACY_STATE: &str = "Follow-up work remains.";
const LEGACY_GUIDANCE: &str =
    "More work remains. Keep this tree and follow the action items in this response.";

#[test]
fn a_multi_line_item_is_done_when_its_joined_follow_up_row_is_struck() {
    let live = repin_doc(
        "",
        &script_row_tree(),
        &repin_response(LEGACY_STATE, LEGACY_GUIDANCE),
    );
    let tree = repin_tree(&reconciled(&live));
    assert!(tree.contains("    - **State:** Complete.\n"), "{tree}");
    assert!(tree.contains("Safe to delete this tree"), "{tree}");
    assert!(!tree.contains("**Remaining:**"), "{tree}");
    assert!(!tree.contains("⏳"), "{tree}");
}

#[test]
fn a_multi_line_item_is_done_when_its_deleted_row_is_quoted_in_the_exchange() {
    // The operator deleted the finished row; the exchange still quotes it.
    let live = repin_doc(
        &script_row_quote(),
        "",
        &repin_response(LEGACY_STATE, LEGACY_GUIDANCE),
    );
    let tree = repin_tree(&reconciled(&live));
    assert!(tree.contains("    - **State:** Complete.\n"), "{tree}");
    assert!(tree.contains("Safe to delete this tree"), "{tree}");
}

#[test]
fn a_tree_rendered_complete_stays_complete_after_its_finished_row_vanishes() {
    let live = repin_doc(
        "",
        &script_row_tree(),
        &repin_response(LEGACY_STATE, LEGACY_GUIDANCE),
    );
    let complete = reconciled(&live);
    assert!(repin_tree(&complete).contains("**State:** Complete."));
    // The operator deletes the finished row; nothing quotes it any more.
    let deleted = complete.replacen(&script_row_tree(), "", 1);
    assert_ne!(deleted, complete);
    assert_eq!(
        reconciled(&deleted),
        deleted,
        "a vanished finished row never reopens a Complete tree"
    );
    // A live row for the item does reopen it.
    let requeued = deleted.replacen(
        "- do [#later]",
        &format!("  - {SCRIPT_ROW}\n- do [#later]"),
        1,
    );
    let tree = repin_tree(&reconciled(&requeued));
    assert!(tree.contains("Follow-up work remains."), "{tree}");
}

#[test]
fn a_live_joined_row_holds_its_multi_line_item() {
    // In another top-level tree, the live joined row carries the item
    // (`#treefinish`), so this tree is finished.
    let elsewhere = repin_doc(
        "",
        &format!("- {SCRIPT_ROW}\n"),
        &repin_response(LEGACY_STATE, LEGACY_GUIDANCE),
    );
    let tree = repin_tree(&reconciled(&elsewhere));
    assert!(tree.contains("    - **State:** Complete.\n"), "{tree}");
    // Inside this tree, it is the item's one open entry: the row stands for
    // the item instead of being listed and counted a second time.
    let inside = repin_doc("", "", &repin_response(LEGACY_STATE, LEGACY_GUIDANCE)).replacen(
        "- do [#later]",
        &format!("  - {SCRIPT_ROW}\n- do [#later]"),
        1,
    );
    let tree = repin_tree(&reconciled(&inside));
    assert!(tree.contains("Follow-up work remains."), "{tree}");
    assert_eq!(tree.matches("⏳ agent: on #12").count(), 1, "{tree}");
    assert!(
        tree.contains("1/2 done") || tree.contains("2/3 done"),
        "{tree}"
    );
}

// ---------------------------------------------------------------------------
// The stateless-JWT tree
// ---------------------------------------------------------------------------

const HEADING: &str = "https://example.com/api/pull/7";
const QUESTION: &str = "I think we should use stateless tokens. What is the reason against it?";
const CHOOSE: &str = "operator: choose one: (1) keep durable per-`jti` rows plus retention; (2) a stateless token with a denylist. I rank them in that order.";
const HOLD: &str = "operator: hold #7 (https://example.com/api/pull/7) until you choose. Merging it starts durable rows that a migration would then have to carry.";

fn question_tree(nested_under_choose: &str) -> String {
    format!(
        "- ### 🔒 {HEADING}\n\
         \x20 - %% fix(auth): always issue durable credentials %%\n\
         \x20 - ~~{QUESTION}~~\n\
         \x20   - **Response [#r-ddc5f0eb1a59]**\n\
         \x20     - **Timestamp:** 2026-11-24T21:38:22.000Z\n\
         \x20     - **Summary:** If the key is compromised, drop its `kid`. <!-- agent-doc:response-detail -->\n\
         \x20     - **Action items:** <!-- agent-doc:response-detail -->\n\
         \x20       - {CHOOSE} <!-- agent-doc:response-detail -->\n\
         \x20       - {HOLD} <!-- agent-doc:response-detail -->\n\
         \x20     - **Remaining:**\n\
         \x20       - 🙋 operator: choose one: (1) keep durable per-`jti` rows plus retention <!-- agent-doc:response-detail -->\n\
         {nested_under_choose}\
         \x20       - 🙋 operator: hold #7 (https://example.com/api/pull/7) until you choose <!-- agent-doc:response-detail -->\n\
         \x20     - **State:** Waiting on operator input.\n\
         \x20     - **Guidance:** 🙋 1/3 done, 2 yours. Only you can finish this: 2 items for you, listed under Remaining. Strike or answer each row when done.\n"
    )
}

/// The exchange quote of the dispatched answer nested under the finished
/// `hold` follow-up row: lineage, the row, then the answer.
fn hold_answer_quote() -> String {
    format!(
        "> **Queue prompt:**\n>\n> {HEADING}: [#q-b55e5bde5b7a] follow-up: {HOLD} [operator-verify] [#q-d5003e7f665e]: We are moving to stateless tokens. Update the ADRs. [#q-29f25296f7dc]\n\n"
    )
}

fn question_doc(exchange: &str, queue: &str) -> String {
    format!(
        "---\nqueue_active: true\n---\n\n\
         <!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n\
         <!-- agent:run id=\"main\" subagents -->\n{queue}\
         - ### {HEADING}/933\n\
         \x20 - %% feat: count sessions %%\n\
         <!-- /agent:run -->\n"
    )
}

fn remaining_of(document: &str) -> String {
    let start = document.find("**Remaining:**").expect("remaining");
    let tail = &document[start..];
    tail[..tail.find("**Guidance:**").expect("guidance")].to_string()
}

#[test]
fn a_finished_follow_up_quoted_with_its_lineage_and_answer_resolves_its_item() {
    let nested = "          - What is per-`jti`?\n";
    let live = question_doc(&hold_answer_quote(), &question_tree(nested));
    let updated = reconciled(&live);
    let remaining = remaining_of(&updated);
    assert!(
        !remaining.contains("🙋 operator: hold #7"),
        "the finished hold row resolved its item:\n{remaining}"
    );
    // The operator's nested question is the entry's live answer: agent work,
    // kept verbatim under its entry.
    assert!(
        remaining.contains(
            "        - ⏳ operator: choose one: (1) keep durable per-`jti` rows plus retention <!-- agent-doc:response-detail -->\n          - What is per-`jti`?\n"
        ),
        "{remaining}"
    );
    assert!(
        updated.contains("      - **State:** Follow-up work remains.\n"),
        "{updated}"
    );
    // The struck question row and the hold item are done; the answered
    // choice is still being worked.
    assert!(updated.contains("⏳ 2/3 done"), "{updated}");
}

#[test]
fn an_item_whose_live_row_was_deleted_unfinished_stays_the_operators() {
    // Nothing quotes the `choose` row: it was deleted while still open, so
    // this tree is the only place the decision is written down.
    let live = question_doc(&hold_answer_quote(), &question_tree(""));
    let remaining = remaining_of(&reconciled(&live));
    assert!(
        remaining
            .contains("🙋 operator: choose one: (1) keep durable per-`jti` rows plus retention"),
        "{remaining}"
    );
}

#[test]
fn the_nested_operator_question_is_a_queue_prompt_with_the_entry_lineage() {
    let live = question_doc("", &question_tree("          - What is per-`jti`?\n"));
    let components = agent_doc_element::element::parse(&live).unwrap();
    let queue = components
        .iter()
        .find(|component| component.name == "queue")
        .expect("queue");
    let entries = crate::document_queue::parse(queue.content(&live)).unwrap();
    let lineage = entries
        .iter()
        .enumerate()
        .find_map(|(index, entry)| match entry {
            crate::document_queue::QueueEntry::Prompt(prompt)
                if prompt.text.trim() == "What is per-`jti`?" =>
            {
                crate::document_queue::lineage_claim_text(&entries, index)
            }
            _ => None,
        })
        .expect("the nested question is a live queue prompt");
    assert!(lineage.ends_with(": What is per-`jti`?"), "{lineage}");
    assert!(lineage.contains("operator: choose one"), "{lineage}");
}

// ---------------------------------------------------------------------------
// The strike guard
// ---------------------------------------------------------------------------

fn handback_response(state: &str) -> String {
    format!(
        "    - **Response [#r-ddc5f0eb1a59]**\n\
         \x20     - **Timestamp:** 2026-11-24T21:38:22.000Z\n\
         \x20     - **Summary:** If the key is compromised, drop its `kid`. <!-- agent-doc:response-detail -->\n\
         \x20     - **Action items:** <!-- agent-doc:response-detail -->\n\
         \x20       - {CHOOSE} <!-- agent-doc:response-detail -->\n\
         \x20       - {HOLD} <!-- agent-doc:response-detail -->\n\
         \x20       - agent: rebase #7 once the operator chooses. <!-- agent-doc:response-detail -->\n\
         \x20     - **State:** {state}\n\
         \x20     - **Guidance:** {LEGACY_GUIDANCE}\n"
    )
}

fn strike_queue(followups: &str, struck: bool) -> String {
    let row = if struck {
        format!("  - ~~{QUESTION}~~\n{}", handback_response(LEGACY_STATE))
    } else {
        format!("  - {QUESTION}\n")
    };
    format!("- ### {HEADING}\n{row}{followups}")
}

#[test]
fn a_strike_that_buries_operator_items_is_reported() {
    let before = question_doc("", &strike_queue("", false));
    let after = question_doc("", &strike_queue("", true));
    assert_eq!(
        struck_responses_burying_operator_items(&before, &after),
        vec![BuriedOperatorItems {
            row: QUESTION.to_string(),
            items: vec![CHOOSE.to_string(), HOLD.to_string()],
        }],
        "both operator items would live only under the struck row; the agent item is the coordinator's to finish"
    );
}

#[test]
fn a_strike_whose_operator_items_have_follow_up_rows_is_allowed() {
    // The live shape: report-complete queued both items, naming the origin's
    // id, beside the struck row. Even when the hand-back landed on an id-less
    // twin of the origin, the rows hold the items by their text.
    let rows = format!(
        "  - 🙋 [#q-b55e5bde5b7a] follow-up: {CHOOSE} [operator-verify] [#q-f1a2b3c4d5e6]\n\
         \x20 - ~~[#q-b55e5bde5b7a] follow-up: {HOLD} [operator-verify] [#q-d5003e7f665e]~~\n"
    );
    let before = question_doc("", &strike_queue(&rows, false));
    let after = question_doc("", &strike_queue(&rows, true));
    assert_eq!(
        struck_responses_burying_operator_items(&before, &after),
        Vec::new()
    );
    // A finished row the exchange still quotes tracks its item too.
    let only_choose = format!(
        "  - 🙋 [#q-b55e5bde5b7a] follow-up: {CHOOSE} [operator-verify] [#q-f1a2b3c4d5e6]\n"
    );
    let before = question_doc(&hold_answer_quote(), &strike_queue(&only_choose, false));
    let after = question_doc(&hold_answer_quote(), &strike_queue(&only_choose, true));
    assert_eq!(
        struck_responses_burying_operator_items(&before, &after),
        Vec::new()
    );
}

#[test]
fn a_row_struck_before_or_a_complete_response_is_not_this_strikes() {
    let struck = question_doc("", &strike_queue("", true));
    assert_eq!(
        struck_responses_burying_operator_items(&struck, &struck),
        Vec::new(),
        "the row was already struck"
    );
    let before = question_doc("", &strike_queue("", false));
    let complete = struck.replacen(
        &format!("**State:** {LEGACY_STATE}"),
        "**State:** Complete.",
        1,
    );
    assert_ne!(complete, struck);
    assert_eq!(
        struck_responses_burying_operator_items(&before, &complete),
        Vec::new(),
        "a response that says Complete keeps its word"
    );
}
