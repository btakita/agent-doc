//! Ledger tests over a sanitized fixture shaped like the release session that
//! re-asked answered questions (`#answerbyrequestid`).

use super::*;

const FIXTURE: &str = include_str!("../tests/fixtures/release_session.md");
const NOW: u64 = 1_800_000_000;

fn build(content: &str) -> (Observations, Ledger) {
    build_with(content, &PriorIndex::default())
}

fn build_with(content: &str, prior: &PriorIndex) -> (Observations, Ledger) {
    let observations = extract::observe(content, &[]);
    let ledger = Ledger::build(&observations, prior, NOW, None);
    (observations, ledger)
}

fn fact_for<'a>(facts: &'a [AnsweredFact], item: &str) -> Option<&'a AnsweredFact> {
    facts
        .iter()
        .find(|fact| fact.open_items.iter().any(|open| open == item))
}

#[test]
fn every_copy_of_an_answered_question_gets_the_answer() {
    let (_, ledger) = build(FIXTURE);
    let facts = ledger.answered_facts();

    // Tier 1: the decision todo and the request raised for it are one decision.
    let alpha = fact_for(&facts, "tdalphahandoff").expect("alpha answered");
    assert!(
        alpha
            .open_items
            .contains(&"subagent-operator-request-tdalphahandoff-d1d4".to_string()),
        "{alpha:?}"
    );
    assert_eq!(alpha.tier, MatchTier::Identity);
    let d1d4 = fact_for(&facts, "subagent-operator-request-tdalphahandoff-d1d4").unwrap();
    assert_eq!(d1d4.decision_id, alpha.decision_id);

    let bump = fact_for(&facts, "tdcontractsbump").expect("bump answered");
    assert_eq!(bump.answer, "Keep at 0.1.0. Add the 4 new step verbs.");
    assert!(bump.location.contains("[#tdcontractsbump]"), "{bump:?}");

    let y2fn = fact_for(&facts, "y2fn").expect("cap answered");
    assert_eq!(
        y2fn.answer,
        "JWS makes the cap moot. We don't store credentials."
    );
}

#[test]
fn a_changed_answer_supersedes_and_keeps_the_old_revision() {
    let (_, ledger) = build(FIXTURE);
    let facts = ledger.answered_facts();
    let approvers = fact_for(&facts, "tdadrapprovers").unwrap();
    assert_eq!(
        approvers.answer,
        "Keep the approver as is. PR #88 already merged. Create a PR to accept all ADRs on main."
    );
    assert_eq!(
        approvers.earlier_answers,
        vec!["Add Alex + Sam".to_string()]
    );
    assert_eq!(approvers.changed_from.as_deref(), Some("Add Alex + Sam"));
    let decision = ledger.decision_for_key(&id_key("tdadrapprovers")).unwrap();
    assert_eq!(decision.revisions[1].relation, RevisionRelation::Supersedes);
    assert!(ledger.edges.contains(&Edge {
        from: decision.revisions[1].id.clone(),
        kind: EdgeKind::Supersedes,
        to: decision.revisions[0].id.clone(),
    }));

    // The change is flagged once: the index remembers it was surfaced.
    let (_, revisions) = ledger.index_rows();
    let prior = PriorIndex {
        keys: ledger.index_rows().0,
        revisions,
    };
    let (_, again) = build_with(FIXTURE, &prior);
    let facts = again.answered_facts();
    let approvers = fact_for(&facts, "tdadrapprovers").unwrap();
    assert_eq!(approvers.changed_from, None, "{approvers:?}");
    assert_eq!(
        approvers.earlier_answers,
        vec!["Add Alex + Sam".to_string()]
    );
}

#[test]
fn a_request_raised_for_work_only_applies_to_it() {
    let (_, ledger) = build(FIXTURE);
    let cap = ledger
        .decision_for_key(&id_key("subagent-operator-request-c690prov-cap"))
        .unwrap();
    let work = ledger.decision_for_key(&id_key("tdprovauth")).unwrap();
    assert_ne!(
        cap.id, work.id,
        "a work item is not the question asked for it"
    );
    assert!(ledger.edges.contains(&Edge {
        from: "subagent-operator-request-c690prov-cap".into(),
        kind: EdgeKind::AppliesTo,
        to: "tdprovauth".into(),
    }));
}

#[test]
fn same_anchors_without_an_id_overlap_only_annotate() {
    let (_, ledger) = build(FIXTURE);
    let facts = ledger.answered_facts();
    assert!(fact_for(&facts, "subagent-operator-request-c690prov-cap").is_none());
    let possible = ledger
        .possible
        .iter()
        .find(|candidate| {
            candidate
                .open_items
                .contains(&"subagent-operator-request-c690prov-cap".to_string())
        })
        .expect("gray-zone note for the cap request copy");
    assert_eq!(possible.tier, MatchTier::Anchor);
    assert_eq!(
        possible.answer,
        "JWS makes the cap moot. We don't store credentials."
    );
    let guidance = contract_guidance(&facts, &ledger.possible).unwrap();
    assert!(guidance.contains("possibly already answered"), "{guidance}");
    assert!(guidance.contains("already answered `"), "{guidance}");
    // Annotation never closes anything.
    let (observations, _) = build(FIXTURE);
    assert!(
        ledger
            .duplicate_closures(&observations)
            .iter()
            .all(|closure| closure.todo_id != "subagent-operator-request-c690prov-cap")
    );
}

#[test]
fn a_near_duplicate_with_a_different_number_is_not_merged_or_annotated() {
    let (_, ledger) = build(FIXTURE);
    let cap20 = ledger.decision_for_key(&id_key("tdcap20")).unwrap();
    let y2fn = ledger.decision_for_key(&id_key("y2fn")).unwrap();
    assert_ne!(cap20.id, y2fn.id);
    assert!(cap20.revisions.is_empty());
    assert!(
        ledger
            .possible
            .iter()
            .all(|candidate| !candidate.open_items.contains(&"tdcap20".to_string())),
        "{:?}",
        ledger.possible
    );
}

#[test]
fn duplicate_copies_close_without_racing_the_queue() {
    let (observations, ledger) = build(FIXTURE);
    let closures = ledger.duplicate_closures(&observations);
    let closed: Vec<&str> = closures
        .iter()
        .map(|closure| closure.todo_id.as_str())
        .collect();
    // Consumed queue answer, no live row: the stale request copy closes.
    assert!(
        closed.contains(&"subagent-operator-request-tdgodot-allowlist"),
        "{closures:?}"
    );
    // A live queue row still carries `Pat Owner` to the worker: not yet.
    assert!(
        !closed.contains(&"subagent-operator-request-tdalphahandoff-d1d4"),
        "{closures:?}"
    );
    // Answers in an item's own packet are its dispatch path: never closed.
    assert!(!closed.contains(&"tdcontractsbump"));
    assert!(!closed.contains(&"tdalphahandoff"));
    assert!(!closed.contains(&"y2fn"));

    // Once the queue row is consumed, the request copy closes and the decision
    // todo stays open for the agent to act on.
    let consumed = FIXTURE.replace(
        "- ### 🙋📥 [#subagent-operator-request-tdalphahandoff-d1d4]: Pat Owner",
        "- ### ~~[#subagent-operator-request-tdalphahandoff-d1d4]: Pat Owner~~",
    );
    let (observations, ledger) = build(&consumed);
    let closures = ledger.duplicate_closures(&observations);
    let d1d4 = closures
        .iter()
        .find(|closure| closure.todo_id == "subagent-operator-request-tdalphahandoff-d1d4")
        .expect("closed");
    assert_eq!(d1d4.kept_open.as_deref(), Some("tdalphahandoff"));

    let rewritten = apply_duplicate_closures(&consumed, &closures)
        .unwrap()
        .unwrap();
    let todo_line = rewritten
        .lines()
        .find(|line| line.contains("[#subagent-operator-request-tdalphahandoff-d1d4] Name"))
        .unwrap();
    assert!(todo_line.starts_with("- [x]"), "{todo_line}");
    assert!(todo_line.contains("already answered `"), "{todo_line}");
    assert!(rewritten.contains("- [ ] [#tdalphahandoff]"));
    // Idempotent.
    let (observations, ledger) = build(&rewritten);
    let again = ledger.duplicate_closures(&observations);
    assert!(
        apply_duplicate_closures(&rewritten, &again)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_follow_up_asked_after_the_answer_is_not_closed() {
    let consumed = FIXTURE.replace(
        "- ### 🙋📥 [#subagent-operator-request-tdalphahandoff-d1d4]: Pat Owner",
        "- ### ~~[#subagent-operator-request-tdalphahandoff-d1d4]: Pat Owner~~",
    );
    let (_, first) = build(&consumed);
    let (mut keys, revisions) = first.index_rows();
    // The request copy was first seen after the latest answer.
    for key in &mut keys {
        if key.key == id_key("subagent-operator-request-tdalphahandoff-d1d4") {
            key.first_seen_at = NOW + 100;
        }
    }
    let prior = PriorIndex { keys, revisions };
    let observations = extract::observe(&consumed, &[]);
    let ledger = Ledger::build(&observations, &prior, NOW + 200, None);
    let closures = ledger.duplicate_closures(&observations);
    // It was answered directly by the consumed queue row, so it still closes.
    assert!(
        closures
            .iter()
            .any(|closure| closure.todo_id.ends_with("-d1d4"))
    );

    // A brand-new request copy with no direct answer stays open.
    let follow_up = consumed.replace(
        "<!-- /agent:todo -->",
        "- [ ] [#subagent-operator-request-tdalphahandoff-tag] Name the tag for the Alpha release.\n<!-- /agent:todo -->",
    );
    let mut prior = prior;
    prior.keys.push(PriorKey {
        key: id_key("subagent-operator-request-tdalphahandoff-tag"),
        decision_id: ledger
            .decision_for_key(&id_key("tdalphahandoff"))
            .unwrap()
            .id
            .clone(),
        first_seen_at: NOW + 300,
    });
    let observations = extract::observe(&follow_up, &[]);
    let ledger = Ledger::build(&observations, &prior, NOW + 400, None);
    assert!(
        ledger
            .duplicate_closures(&observations)
            .iter()
            .all(|closure| !closure.todo_id.ends_with("-tag"))
    );
}

#[test]
fn an_operator_same_as_reply_merges_once_and_closes_the_copy() {
    let merged = FIXTURE.replace(
        "    - **Next:** you — answer: Choose the api live-credential cap N ▫️",
        "    - **Next:** you — answer: Choose the api live-credential cap N ▫️\n    - same as [#y2fn]",
    );
    let (observations, ledger) = build(&merged);
    let cap = ledger
        .decision_for_key(&id_key("subagent-operator-request-c690prov-cap"))
        .unwrap();
    let y2fn = ledger.decision_for_key(&id_key("y2fn")).unwrap();
    assert_eq!(cap.id, y2fn.id);
    assert_eq!(cap.tier, MatchTier::OperatorMerge);
    let closures = ledger.duplicate_closures(&observations);
    let closure = closures
        .iter()
        .find(|closure| closure.todo_id == "subagent-operator-request-c690prov-cap")
        .expect("merged copy closes");
    assert_eq!(closure.kept_open.as_deref(), Some("y2fn"));
    // The merge is durable: the index keeps both keys in one decision even
    // after the operator's merge row is gone.
    let (keys, revisions) = ledger.index_rows();
    let (_, later) = build_with(FIXTURE, &PriorIndex { keys, revisions });
    assert_eq!(
        later
            .decision_for_key(&id_key("subagent-operator-request-c690prov-cap"))
            .unwrap()
            .id,
        later.decision_for_key(&id_key("y2fn")).unwrap().id
    );
}

#[test]
fn reasks_are_refused_unless_they_cite_the_answer() {
    let (_, ledger) = build(FIXTURE);
    let packet = |id: &str, queue_item: &str, request: &str| {
        format!(
            "[#{id}] subagent operator request\n- *queue_item:*\n  - {queue_item}\n- *kind:* question\n- *blocking:* true\n- *request:*\n  - {request}\n- *context:*\n  - context line\n- *choices:* []\n"
        )
    };
    // A new request against the answered decision item.
    let response = packet(
        "subagent-operator-request-alpha-again",
        "do [#tdalphahandoff]",
        "Name the Alpha release owner and cutoff.",
    );
    let new = extract::observe_new_questions(&response, &[]);
    let reasks = ledger.reasks(&new);
    assert_eq!(reasks.len(), 1, "{reasks:?}");
    let message = reask_refusal(&reasks);
    assert!(
        message.contains("Use the Recommended bundle") || message.contains("Pat Owner"),
        "{message}"
    );

    // Citing the recorded answer lets a genuine follow-up through.
    let cited = response.replace(
        "context line",
        "already answered: Pat Owner owns it; the tag name is still open",
    );
    assert!(
        ledger
            .reasks(&extract::observe_new_questions(&cited, &[]))
            .is_empty()
    );

    // An exact copy of an answered todo under a new id.
    let todo_add = vec![
        "[#tdbumpagain] Decide whether to bump the contracts suite revision 0.1.0 -> 0.2.0 for the null-expiry scenarios. SDK runners need the 4 new step verbs".to_string(),
    ];
    let reasks = ledger.reasks(&extract::observe_new_questions("", &todo_add));
    assert_eq!(reasks.len(), 1, "{reasks:?}");
    assert_eq!(reasks[0].answer, "Keep at 0.1.0. Add the 4 new step verbs.");

    // A question raised for unanswered work, and an unrelated question, pass.
    let unrelated = packet(
        "subagent-operator-request-prov-branch",
        "do [#tdprovauth]",
        "Which branch should the credential change target?",
    );
    assert!(
        ledger
            .reasks(&extract::observe_new_questions(&unrelated, &[]))
            .is_empty()
    );
    let near = vec!["[#tdcap30] Choose the ADR 0041 live-credential cap N (5 or 30), the eviction rule (oldest-issued or least-recently-used), and the brief recipients.".to_string()];
    assert!(
        ledger
            .reasks(&extract::observe_new_questions("", &near))
            .is_empty()
    );
}

#[test]
fn answers_survive_in_the_index_after_their_rows_are_pruned() {
    let (_, ledger) = build(FIXTURE);
    let (keys, revisions) = ledger.index_rows();
    let allowlist = ledger
        .decision_for_key(&id_key("subagent-operator-request-tdgodot-allowlist"))
        .unwrap()
        .id
        .clone();
    let pruned = FIXTURE.replace(
        "- ### ~~[#subagent-operator-request-tdgodot-allowlist]: Use the standard http ports.~~\n",
        "",
    );
    let (_, later) = build_with(&pruned, &PriorIndex { keys, revisions });
    let decision = later
        .decision_for_key(&id_key("subagent-operator-request-tdgodot-allowlist"))
        .unwrap();
    assert_eq!(decision.id, allowlist, "stable decision id");
    let latest = decision.latest().expect("remembered answer");
    assert_eq!(latest.answer, "Use the standard http ports.");
    assert!(!latest.in_document);
    let facts = later.answered_facts();
    assert!(fact_for(&facts, "subagent-operator-request-tdgodot-allowlist").is_some());
}

#[test]
fn archived_packets_still_answer_a_later_copy() {
    let reopened = FIXTURE.replace(
        "<!-- /agent:todo -->",
        "- [ ] [#tdbranch2] Choose the target branch for the telemetry pull request: sdk-dev or dev\n<!-- /agent:todo -->",
    );
    let (_, ledger) = build(&reopened);
    let facts = ledger.answered_facts();
    let fact = fact_for(&facts, "tdbranch2").expect("archived answer matches by text");
    assert_eq!(fact.answer, "Target sdk-dev. Keep 0.3.0-preview.1");
    assert!(fact.location.starts_with("done archive"), "{fact:?}");
}

#[test]
fn stable_decision_ids_come_from_the_index() {
    let prior = PriorIndex {
        keys: vec![PriorKey {
            key: id_key("y2fn"),
            decision_id: "dec-legacy".into(),
            first_seen_at: 1,
        }],
        revisions: Vec::new(),
    };
    let (_, ledger) = build_with(FIXTURE, &prior);
    assert_eq!(
        ledger.decision_for_key(&id_key("y2fn")).unwrap().id,
        "dec-legacy"
    );
}

struct FixedNeighbours;

impl SemanticNeighbours for FixedNeighbours {
    fn similarity(&self, a: &str, b: &str) -> Option<f64> {
        let pair = format!("{a} | {b}");
        (pair.contains("redirect") && pair.contains("callback")).then_some(0.9)
    }
}

#[test]
fn semantic_neighbours_only_annotate() {
    let content = concat!(
        "<!-- agent:todo -->\n",
        "- [ ] [#redir] Choose the demo sign-in redirect port for local runs\n",
        "- [ ] [#cb] Choose the local callback port used by the demo login\n",
        "<!-- /agent:todo -->\n",
        "<!-- agent:queue -->\n",
        "- ~~[#cb]: Use port 3000.~~\n",
        "<!-- /agent:queue -->\n",
    );
    let observations = extract::observe(content, &[]);
    let ledger = Ledger::build(
        &observations,
        &PriorIndex::default(),
        NOW,
        Some(&FixedNeighbours),
    );
    assert!(fact_for(&ledger.answered_facts(), "redir").is_none());
    let note = ledger
        .possible
        .iter()
        .find(|candidate| candidate.open_items.contains(&"redir".to_string()))
        .expect("neighbour note");
    assert_eq!(note.tier, MatchTier::Semantic);
    assert!(
        ledger
            .duplicate_closures(&observations)
            .iter()
            .all(|c| c.todo_id != "redir")
    );
    assert_eq!(ledger.near_duplicates.len(), 1);
}
