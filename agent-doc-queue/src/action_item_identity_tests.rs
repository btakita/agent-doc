//! `#actionitemidentity`: a queue row's identity stays synced with the
//! **Action items** of its response.
//!
//! The identity is the list row without its strike wrapper (indentation plus
//! the work text, including its `[#q-…]`), the same split the strike guard
//! uses. A complete action item is struck in that list. A struck row with an
//! open item that no follow-up row carries is a stale strike when a nested
//! queue row keeps it from becoming a leaf: the wrapper comes off and the
//! identity text stays. A follow-up row keeps its own strike.

use crate::response_remaining::reconcile_response_remaining;

const MARKER: &str = "<!-- agent-doc:response-detail -->";
const QUESTION: &str = "I think we should use stateless tokens. What is the reason against it?";
const QUESTION_ID: &str = "[#q-b55e5bde5b7a]";
const CHOOSE: &str = "operator: choose one: (1) keep durable rows plus retention; (2) a stateless token with a denylist.";
const HOLD: &str = "operator: hold #7 (https://example.com/api/pull/7) until you choose. Merging it starts durable rows.";

fn question_row(struck: bool) -> String {
    let work = format!("{QUESTION} {QUESTION_ID}");
    if struck { format!("~~{work}~~") } else { work }
}

fn response(choose_struck: bool, hold_struck: bool) -> String {
    let item = |text: &str, struck: bool| {
        if struck {
            format!("        - ~~{text}~~ {MARKER}\n")
        } else {
            format!("        - {text} {MARKER}\n")
        }
    };
    format!(
        "    - **Response [#r-ddc5f0eb1a59]**\n\
         \x20     - **Timestamp:** 2026-11-24T21:38:22.000Z\n\
         \x20     - **Summary:** If the key is compromised, drop its `kid`. {MARKER}\n\
         \x20     - **Action items:** {MARKER}\n\
         {choose}\
         {hold}\
         \x20     - **Remaining:**\n\
         \x20       - 🙋 operator: choose one: (1) keep durable rows plus retention {MARKER}\n\
         \x20         - What is per-token?\n\
         \x20       - 🙋 operator: hold #7 (https://example.com/api/pull/7) until you choose {MARKER}\n\
         \x20     - **State:** Waiting on operator input.\n\
         \x20     - **Guidance:** 🙋 1/3 done, 2 yours. Only you can finish this: 2 items for you, listed under Remaining. Strike or answer each row when done.\n",
        choose = item(CHOOSE, choose_struck),
        hold = item(HOLD, hold_struck),
    )
}

fn hold_quote() -> String {
    format!(
        "> **Queue prompt:**\n>\n> https://example.com/api/pull/7: [#q-b55e5bde5b7a] follow-up: {HOLD} [operator-verify] [#q-d5003e7f665e]: We are moving to stateless tokens. [#q-29f25296f7dc]\n\n"
    )
}

fn document(queue: &str, exchange: &str) -> String {
    format!(
        "---\nqueue_active: true\n---\n\n\
         <!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n\
         <!-- agent:run id=\"main\" subagents -->\n\
         - ### https://example.com/api/pull/7\n\
         {queue}\
         - ### https://example.com/api/pull/8\n\
         <!-- /agent:run -->\n"
    )
}

fn reconciled(document: &str) -> String {
    reconcile_response_remaining(document).map_or_else(|| document.to_string(), |(text, _)| text)
}

fn question_line(document: &str) -> String {
    document
        .lines()
        .find(|line| line.contains(QUESTION) && line.contains(QUESTION_ID))
        .expect("question identity")
        .to_string()
}

fn action_line(document: &str, text: &str) -> String {
    document
        .lines()
        .find(|line| line.contains(text) && line.contains(MARKER))
        .expect("action item")
        .to_string()
}

fn question_is_executable(document: &str) -> bool {
    let components = agent_doc_element::element::parse(document).unwrap();
    let queue = components
        .iter()
        .find(|component| component.name == "queue")
        .expect("queue");
    let entries = crate::document_queue::parse(queue.content(document)).unwrap();
    crate::document_queue::executable_prompt_indexes(&entries)
        .into_iter()
        .any(|index| match &entries[index] {
            crate::document_queue::QueueEntry::Prompt(prompt) => prompt.text.contains(QUESTION),
            _ => false,
        })
}

#[test]
fn a_stale_strike_unstrikes_the_row_identity_and_strikes_a_complete_action_item() {
    let queue = format!(
        "  - {row}\n{response}",
        row = question_row(true),
        response = response(false, false),
    );
    let updated = reconciled(&document(&queue, &hold_quote()));
    let identity = question_line(&updated);
    assert!(
        !identity.contains("~~"),
        "the struck identity was stale while an action item was still open:\n{identity}"
    );
    assert!(
        identity.contains(QUESTION) && identity.contains(QUESTION_ID),
        "unstriking keeps the row identity, including its id:\n{identity}"
    );
    let hold = action_line(&updated, "operator: hold #7");
    assert!(
        hold.contains("~~operator: hold #7"),
        "a complete action item is struck:\n{hold}"
    );
    let choose = action_line(&updated, "operator: choose one");
    assert!(
        !choose.contains("~~"),
        "an open action item stays unstruck:\n{choose}"
    );
    assert!(
        !question_is_executable(&updated),
        "unstriking the identity must not make the answered row runnable"
    );
    assert!(
        reconcile_response_remaining(&updated).is_none(),
        "a second pass changes nothing"
    );
}

#[test]
fn a_leaf_strike_stays_while_its_complete_action_item_is_struck() {
    // No nested queue row: removing the wrapper would make the row a leaf
    // and dispatch it again. The complete action item is still struck.
    let mut body = response(false, false);
    body = body.replace("        - What is per-token?\n", "");
    let queue = format!("  - {row}\n{body}", row = question_row(true),);
    let updated = reconciled(&document(&queue, &hold_quote()));
    let identity = question_line(&updated);
    assert!(
        identity.contains("~~") && identity.contains(QUESTION_ID),
        "a leaf strike stays so the row is not dispatched again:\n{identity}"
    );
    assert!(
        action_line(&updated, "operator: hold #7").contains("~~"),
        "the complete action item is still struck"
    );
    assert!(!action_line(&updated, "operator: choose one").contains("~~"));
}

#[test]
fn a_handoff_strike_stays_when_every_action_item_has_its_follow_up_row() {
    let rows = format!(
        "  - 🙋 [#q-b55e5bde5b7a] follow-up: {CHOOSE} [operator-verify] [#q-f1a2b3c4d5e6]\n\
         \x20 - ~~[#q-b55e5bde5b7a] follow-up: {HOLD} [operator-verify] [#q-d5003e7f665e]~~\n"
    );
    let queue = format!(
        "  - {row}\n{response}{rows}",
        row = question_row(true),
        response = response(false, false),
    );
    let updated = reconciled(&document(&queue, ""));
    let identity = question_line(&updated);
    assert!(
        identity.contains("~~"),
        "follow-up rows carry the work, so the origin strike stays:\n{identity}"
    );
    assert!(
        action_line(&updated, "operator: hold #7").contains("~~"),
        "the finished follow-up still strikes its action item"
    );
    assert!(
        !action_line(&updated, "operator: choose one").contains("~~"),
        "the live follow-up keeps its action item open"
    );
}

#[test]
fn a_struck_row_with_no_action_items_is_left_struck() {
    let queue = format!(
        "  - {row}\n\
         \x20   - **Response [#r-aabbccddeeff]**\n\
         \x20     - **Timestamp:** 2026-11-24T21:38:22.000Z\n\
         \x20     - **Summary:** Done.\n\
         \x20     - **State:** Complete.\n\
         \x20     - **Guidance:** ✅ 1/1 done. 🗑️ Safe to delete this tree.\n",
        row = question_row(true),
    );
    let source = document(&queue, "");
    let updated = reconciled(&source);
    assert!(
        question_line(&updated).contains("~~"),
        "no action items, so the strike is not this pass's:\n{}",
        question_line(&updated)
    );
}
