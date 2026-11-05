//! Capture shapes outside a live waiting item (`#answeroutsidewaiting`), over
//! a sanitized fixture copied from a release session where the ledger missed
//! them:
//!
//! - an operator answer in a waiting packet that lost its waiting-item marker
//!   and now sits under a `do [#id]` queue row;
//! - a request raised for a *work* item the operator already answered.

use super::*;

const FIXTURE: &str = include_str!("../tests/fixtures/outside_waiting_session.md");
const NOW: u64 = 1_800_000_000;

fn build(content: &str) -> (Observations, Ledger) {
    let observations = extract::observe(content, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    (observations, ledger)
}

fn fact_for<'a>(facts: &'a [AnsweredFact], item: &str) -> Option<&'a AnsweredFact> {
    facts
        .iter()
        .find(|fact| fact.open_items.iter().any(|open| open == item))
}

fn packet(id: &str, queue_item: &str, request: &str) -> String {
    format!(
        "[#{id}] subagent operator request\n- *queue_item:*\n  - {queue_item}\n- *kind:* question\n- *blocking:* true\n- *request:*\n  - {request}\n- *context:*\n  - context line\n- *choices:* []\n"
    )
}

#[test]
fn an_answer_in_a_packet_outside_a_waiting_item_is_captured() {
    let (observations, ledger) = build(FIXTURE);
    let decision = ledger
        .decision_for_key(&id_key("decidetelemetrybase"))
        .expect("the queue row's id names the decision");
    let latest = decision.latest().expect("operator answer captured");
    assert_eq!(latest.answer, "Target sdk-dev. Keep 0.3.0-preview.1");
    assert!(latest.consumed, "struck answer row: {latest:?}");
    assert!(
        latest.location.contains("[#decidetelemetrybase]"),
        "{latest:?}"
    );
    // Only the operator row is an answer: the agent's reply summary nested
    // under it and the packet's generated rows are not.
    assert_eq!(decision.revisions.len(), 1, "{:?}", decision.revisions);

    // The open copy asking the same question gets the answer as a fact.
    let facts = ledger.answered_facts();
    let fact = fact_for(&facts, "tdtelemetrybase").expect("same question, same answer");
    assert_eq!(fact.answer, "Target sdk-dev. Keep 0.3.0-preview.1");
    assert_eq!(fact.tier, MatchTier::Identity);
    assert_eq!(fact.via, None);
    // It is the only open copy, so it stays open for the agent to act on.
    assert!(
        ledger
            .duplicate_closures(&observations)
            .iter()
            .all(|closure| closure.todo_id != "tdtelemetrybase")
    );
}

#[test]
fn a_reask_of_an_answer_outside_a_waiting_item_is_refused() {
    let (_, ledger) = build(FIXTURE);
    let response = packet(
        "subagent-operator-request-telemetry-base",
        "do [#decidetelemetrybase]",
        "Choose the pull request base for the telemetry annotation change.",
    );
    let reasks = ledger.reasks(&extract::observe_new_questions(&response, &[]));
    assert_eq!(reasks.len(), 1, "{reasks:?}");
    assert_eq!(reasks[0].answer, "Target sdk-dev. Keep 0.3.0-preview.1");
}

#[test]
fn follow_up_rows_under_a_queue_row_are_not_answers() {
    let (_, ledger) = build(FIXTURE);
    // `push + pr?` under `do [#tdpush]` is the operator asking, not answering.
    let push = ledger.decision_for_key(&id_key("tdpush")).unwrap();
    assert!(push.revisions.is_empty(), "{:?}", push.revisions);
    // `[#tdrelock]: authorized` is the answer; the question nested under it
    // is not a second revision.
    let relock = ledger.decision_for_key(&id_key("tdrelock")).unwrap();
    let answers: Vec<&str> = relock
        .revisions
        .iter()
        .map(|revision| revision.answer.as_str())
        .collect();
    assert_eq!(answers, vec!["authorized"]);
}

#[test]
fn a_request_raised_for_answered_work_gets_the_work_answer_attached() {
    let (observations, ledger) = build(FIXTURE);
    let request = "subagent-operator-request-tdredirect-allowlist";
    let facts = ledger.answered_facts();
    let fact = fact_for(&facts, request).expect("work answer attached to the request copy");
    assert_eq!(
        fact.answer,
        "Use the frontend standard http ports. Add a frontend PR if necessary."
    );
    assert_eq!(fact.tier, MatchTier::WorkItem);
    assert_eq!(fact.via.as_deref(), Some("tdredirect"));
    assert!(!fact.tier.acts());

    // Distinct questions stay distinct decisions.
    let request_decision = ledger.decision_for_key(&id_key(request)).unwrap();
    let work_decision = ledger.decision_for_key(&id_key("tdredirect")).unwrap();
    assert_ne!(request_decision.id, work_decision.id);
    assert!(request_decision.revisions.is_empty());
    assert_eq!(fact.decision_id, work_decision.id);

    // Attaching never closes the copy and never refuses a new question.
    assert!(
        ledger
            .duplicate_closures(&observations)
            .iter()
            .all(|closure| closure.todo_id != request)
    );
    let again = packet(
        "subagent-operator-request-tdredirect-infra",
        "[#tdredirect]: Add http://127.0.0.1:47616/demo/callback",
        "Adding `http://127.0.0.1:47616/demo/callback` needs an infra apply. Stop, append, or apply?",
    );
    assert!(
        ledger
            .reasks(&extract::observe_new_questions(&again, &[]))
            .is_empty()
    );

    let guidance = contract_guidance(&facts, &ledger.possible).unwrap();
    assert!(guidance.contains("#answeroutsidewaiting"), "{guidance}");
    assert!(guidance.contains("work item #tdredirect"), "{guidance}");
    assert!(
        guidance.contains("Use the frontend standard http ports."),
        "{guidance}"
    );
}

#[test]
fn an_answered_request_copy_gets_no_work_fact() {
    // Once the operator answers the request itself, its own answer is the
    // fact; the work item's answer is not attached on top.
    let answered = FIXTURE.replace(
        "    - **Next:** you — reply under this item with the decision or result it asks for (evidence: no answer or attempt in its packet yet) ▫️",
        "    - **Next:** you — reply under this item with the decision or result it asks for (evidence: no answer or attempt in its packet yet) ▫️\n    - Use the studio whitelist.",
    );
    let (_, ledger) = build(&answered);
    let facts = ledger.answered_facts();
    let request = "subagent-operator-request-tdredirect-allowlist";
    let own: Vec<&AnsweredFact> = facts
        .iter()
        .filter(|fact| fact.open_items.iter().any(|open| open == request))
        .collect();
    assert_eq!(own.len(), 1, "{own:?}");
    assert_eq!(own[0].answer, "Use the studio whitelist.");
    assert_eq!(own[0].via, None);
}
