//! `#waitingdonetodoopen` regressions: a waiting tree must never read `✅`
//! while its own Todo or an offered decision under it is still open, and a
//! response closeout checks off the Todo of a tree that really is complete.
//!
//! The first fixture is trimmed from a live session document (haiven-dev
//! `tasks/sdk/sdk-dev.md`, 2026-11-05) whose head read
//! `🙋 ⟦✅ you: 0 · agent: 0 · done: 1⟧ [#tdadr0046approvers]` while
//! `- [ ] [#tdadr0046approvers]` stayed open in Todo, with a Next row asking
//! the operator to tick the box.

use std::collections::HashSet;

use crate::document_queue;
use crate::queue_consume::project_waiting_on_you_tree;
use crate::waiting_status::{WaitingState, project_waiting_status, waiting_status};

const ID: &str = "tdadr0046approvers";

const LIVE_ITEM: &str = concat!(
    "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n",
    "  - 🙋 ⟦✅ you: 0 · agent: 0 · done: 1⟧ [#tdadr0046approvers] Name the approver(s) for haiven-docs PR #88 (ADR 0046) <!-- agent-doc:waiting-item -->\n",
    "    - **Next:** you — check off `#tdadr0046approvers` in Todo, or reply with what remains (evidence: every row under the item is closed) ▫️\n",
    "    - **Response:** Name the approver(s) for haiven-docs PR #88 (ADR 0046) ▫️\n",
    "      - **Timestamp:** 2026-11-04T21:22-06:00 ▫️\n",
    "      - **Question:** What decision or completion result should be recorded? ▫️\n",
    "      - **Next actions:** ▫️\n",
    "        - **Action:** Name the approver(s) for haiven-docs PR #88 (ADR 0046) ▫️\n",
    "          - ~~Keep the approver as is. PR #88 already merged. Create a PR to accept all ADRs on main.~~\n",
);

const TODO: &str = "- [ ] [#tdadr0046approvers] Name the approver(s) for haiven-docs PR #88 (ADR 0046)\n- [ ] [#keepopen] An unrelated item stays open\n";

/// The same item, but the operator's reply under it offered a decision
/// (Done as-is / Add Dillon) nobody has acted on yet.
fn with_open_choices() -> String {
    LIVE_ITEM.to_string()
        + concat!(
            "    - ~~Should Dillon be named too?~~\n",
            "      - **Response [#r-3b817c02c2e8]**\n",
            "        - **Summary:** Dillon approved #88.\n",
            "          - **Choices:**\n",
            "            - **Done as-is**: Dillon's approval on #88 counts as his review. [#q-865c445b9b8a]\n",
            "            - **Add Dillon by follow-up PR**: name Luke Richey and Dillon Storm on line 76.\n",
        )
}

fn document(queue: &str, todo: &str) -> String {
    format!(
        "<!-- agent:queue id=\"main\" -->\n{queue}<!-- /agent:queue -->\n\
         <!-- agent:todo -->\n{todo}<!-- /agent:todo -->\n\
         <!-- agent:done -->\n<!-- /agent:done -->\n"
    )
}

fn open(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

fn head_index(entries: &[document_queue::QueueEntry]) -> usize {
    entries
        .iter()
        .position(crate::waiting_packet::is_waiting_item_head)
        .unwrap()
}

fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle:?} in:\n{text}"))
}

#[test]
fn a_complete_tree_with_an_open_todo_counts_the_todo_and_never_reads_done() {
    let entries = document_queue::parse(LIVE_ITEM).unwrap();
    let status = waiting_status(&entries, head_index(&entries), &open(&[ID])).unwrap();
    assert_eq!(status.state, WaitingState::Complete);
    assert_eq!((status.you, status.agent, status.done), (1, 0, 1));

    let projected =
        document_queue::render(&project_waiting_status(&entries, &open(&[ID]), None).unwrap());
    let head = line_with(&projected, "agent-doc:waiting-item");
    assert!(
        head.contains("🙋 ⟦you: 1 · agent: 0 · done: 1⟧ [#tdadr0046approvers]"),
        "{projected}"
    );
    assert!(!projected.contains('✅'), "{projected}");
    let next = line_with(&projected, "**Next:**");
    assert!(
        next.contains("the next response closeout checks off `#tdadr0046approvers`"),
        "{next}"
    );
    assert!(
        !next.contains("check off `#tdadr0046approvers` in Todo"),
        "{next}"
    );

    // One fixed point.
    let again = document_queue::parse(&projected).unwrap();
    assert!(project_waiting_status(&again, &open(&[ID]), None).is_none());

    // Only a closed Todo reads `✅`.
    let closed = waiting_status(&entries, head_index(&entries), &open(&[])).unwrap();
    assert_eq!(closed.state, WaitingState::Done);
    assert!(
        closed.token().starts_with("⟦✅ you: 0"),
        "{}",
        closed.token()
    );
}

#[test]
fn every_open_choice_is_an_ask_and_keeps_the_tree_open() {
    let body = with_open_choices();
    let entries = document_queue::parse(&body).unwrap();
    let status = waiting_status(&entries, head_index(&entries), &open(&[ID])).unwrap();
    assert_eq!(status.state, WaitingState::Operator, "{status:?}");
    assert_eq!(status.you, 1, "{status:?}");
    assert_eq!(
        status.open_choices[0].choices,
        vec![
            "Done as-is".to_string(),
            "Add Dillon by follow-up PR".to_string()
        ]
    );
    let projected =
        document_queue::render(&project_waiting_status(&entries, &open(&[ID]), None).unwrap());
    assert!(!projected.contains('✅'), "{projected}");
    let next = line_with(&projected, "**Next:**");
    assert!(
        next.contains("choose **Done as-is** or **Add Dillon by follow-up PR**"),
        "{next}"
    );

    // A response closeout leaves the Todo open while a choice is unanswered.
    let doc = document(&body, TODO);
    let closed = project_waiting_on_you_tree(&doc, "response").unwrap();
    let out = closed.as_deref().unwrap_or(&doc);
    assert!(out.contains("- [ ] [#tdadr0046approvers]"), "{out}");

    // A pick beneath one option answers the decision.
    let picked = body.replace(
        "            - **Add Dillon by follow-up PR**: name Luke Richey and Dillon Storm on line 76.\n",
        "            - **Add Dillon by follow-up PR**: name Luke Richey and Dillon Storm on line 76.\n              - ~~pick~~\n",
    );
    let entries = document_queue::parse(&picked).unwrap();
    let status = waiting_status(&entries, head_index(&entries), &open(&[ID])).unwrap();
    assert!(status.open_choices.is_empty(), "{status:?}");
    assert_eq!(status.state, WaitingState::Complete, "{status:?}");
}

#[test]
fn choices_governed_by_a_request_are_counted_once_by_that_request() {
    let body = with_open_choices().replace(
        "          - **Choices:**\n",
        "          - **Choices:**\n            - **Request**: [#subagent-operator-request-adr0046-sibling-pick]\n",
    );
    let entries = document_queue::parse(&body).unwrap();
    let head = head_index(&entries);
    let pending = waiting_status(
        &entries,
        head,
        &open(&[ID, "subagent-operator-request-adr0046-sibling-pick"]),
    )
    .unwrap();
    assert_eq!(pending.you, 1, "{pending:?}");
    assert!(pending.open_choices.is_empty(), "{pending:?}");
    // The request answered: the decision is closed with it.
    let answered = waiting_status(&entries, head, &open(&[ID])).unwrap();
    assert_eq!(answered.state, WaitingState::Complete, "{answered:?}");
}

#[test]
fn a_response_closeout_checks_off_a_complete_tree_and_archives_it() {
    let doc = document(LIVE_ITEM, TODO);
    let out = project_waiting_on_you_tree(&doc, "response")
        .unwrap()
        .unwrap();
    assert!(out.contains("- [x] [#tdadr0046approvers]"), "{out}");
    assert!(
        out.contains("- [ ] [#keepopen]"),
        "only the complete item closes: {out}"
    );
    let receipt = line_with(&out, "archived waiting item `#tdadr0046approvers`");
    assert!(
        receipt.contains("every row under it closed, so closeout checked its Todo off"),
        "{receipt}"
    );
    assert!(
        !out.contains("**Next:** you — check off"),
        "no Next row is left asking to tick the box: {out}"
    );
    assert_eq!(
        project_waiting_on_you_tree(&out, "response").unwrap(),
        None,
        "the closeout is replay-idempotent"
    );
}

#[test]
fn a_declarative_sync_never_checks_off_a_todo() {
    let doc = document(LIVE_ITEM, TODO);
    let synced = project_waiting_on_you_tree(&doc, "").unwrap();
    let out = synced.as_deref().unwrap_or(&doc);
    assert!(out.contains("- [ ] [#tdadr0046approvers]"), "{out}");
    let head = line_with(out, "agent-doc:waiting-item");
    assert!(head.contains("⟦you: 1 · agent: 0 · done: 1⟧"), "{out}");
    assert!(!head.contains('✅'), "{out}");
}
