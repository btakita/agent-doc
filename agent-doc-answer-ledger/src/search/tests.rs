//! `#decisionsearchmenu`: decision lookup ranking and the question cue.

use super::*;
use crate::PriorIndex;
use crate::extract::observe;
use crate::recall::OtherDocument;

const NOW: u64 = 1_800_000_000;
const HERE: &str = "/project/tasks/here.md";
const THERE: &str = "/project/tasks/there.md";

const LOCAL: &str = concat!(
    "<!-- agent:todo -->\n",
    "- [ ] [#redir] Choose the demo sign-in redirect port for local runs\n",
    "- [ ] [#cb] Choose the local callback port used by the demo login\n",
    "- [ ] [#lic] Pick the license for the published crates\n",
    "- [ ] [#build] Implement the release build step\n",
    "<!-- /agent:todo -->\n",
    "<!-- agent:queue -->\n",
    "- ~~[#cb]: Use port 3000.~~\n",
    "- ~~[#lic]: Apache-2.0 for every crate.~~\n",
    "<!-- /agent:queue -->\n",
);

const OTHER: &str = concat!(
    "<!-- agent:todo -->\n",
    "- [x] [#portpick] Which port should the demo sign-in redirect use for local runs\n",
    "<!-- /agent:todo -->\n",
    "<!-- agent:queue -->\n",
    "- ~~[#portpick]: Use port 8443.~~\n",
    "<!-- /agent:queue -->\n",
);

fn local() -> (Observations, Ledger) {
    let observations = observe(LOCAL, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    (observations, ledger)
}

fn decision_of<'a>(ledger: &'a Ledger, copy: &str) -> &'a Decision {
    ledger
        .decision_for_key(&id_key(copy))
        .unwrap_or_else(|| panic!("no decision for #{copy}: {:?}", ledger.decisions))
}

fn load_other(path: &str) -> Option<OtherDocument> {
    (path == THERE).then(|| {
        let rows = observe(OTHER, &[]);
        OtherDocument {
            display: "tasks/there.md".to_string(),
            ledger: Ledger::build(&rows, &PriorIndex::default(), NOW, None),
            rows,
        }
    })
}

#[test]
fn exact_ids_find_the_answered_decision_with_its_verbatim_answer() {
    let (_, ledger) = local();
    let decision = decision_of(&ledger, "cb");
    let revision = decision.latest().expect("answered");

    for (query, matched) in [
        ("[#cb]", SearchMatch::CopyId),
        ("#CB", SearchMatch::CopyId),
        ("cb", SearchMatch::CopyId),
        (decision.id.as_str(), SearchMatch::DecisionId),
        (revision.id.as_str(), SearchMatch::RevisionId),
    ] {
        let hits = search_ledger(&ledger, query, "tasks/here.md");
        let top = hits.first().unwrap_or_else(|| panic!("{query}: no hit"));
        assert_eq!(top.decision_id, decision.id, "{query}");
        assert_eq!(top.matched, matched, "{query}");
        assert_eq!(top.answer, "Use port 3000.", "{query}");
        assert_eq!(top.document, "tasks/here.md");
        assert!(!top.location.is_empty(), "{query}: where it was given");
        assert_eq!(top.score, 1.0);
    }
}

#[test]
fn a_decision_stamp_finds_its_decision() {
    let (_, ledger) = local();
    let decision = decision_of(&ledger, "cb");
    for query in [
        format!("⟦decision:{}⟧", decision.id),
        format!("decision:{}", decision.id.to_uppercase()),
    ] {
        let hits = search_ledger(&ledger, &query, "here");
        assert_eq!(hits.len(), 1, "{query}: {hits:?}");
        assert_eq!(hits[0].matched, SearchMatch::Stamp);
        assert_eq!(hits[0].decision_id, decision.id);
    }
    assert!(search_ledger(&ledger, "⟦decision:dec-nothing⟧", "here").is_empty());
}

#[test]
fn a_stamp_written_on_a_copy_is_an_identity_key() {
    let content = concat!(
        "<!-- agent:todo -->\n",
        "- [ ] [#cbcopy] Choose the callback port again ⟦decision:dec-cbport⟧\n",
        "<!-- /agent:todo -->\n",
        "<!-- agent:queue -->\n",
        "- ~~[#cbcopy]: Use port 3000.~~\n",
        "<!-- /agent:queue -->\n",
    );
    let observations = observe(content, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    let hits = search_ledger(&ledger, "decision:dec-cbport", "here");
    assert_eq!(hits.len(), 1, "{hits:?} / {:?}", ledger.decisions);
    assert_eq!(hits[0].matched, SearchMatch::Stamp);
    assert_eq!(hits[0].answer, "Use port 3000.");
}

#[test]
fn a_work_item_id_finds_the_answer_to_the_request_raised_for_it() {
    let content = concat!(
        "<!-- agent:todo -->\n",
        "- [ ] [#deploy] Deploy the demo site\n",
        "- [ ] [#subagent-operator-request-deploy-host] Choose the host for the demo site\n",
        "<!-- /agent:todo -->\n",
        "<!-- agent:queue -->\n",
        "- ~~[#subagent-operator-request-deploy-host]: Use fly.io.~~\n",
        "<!-- /agent:queue -->\n",
    );
    let observations = observe(content, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    let hits = search_ledger(&ledger, "#deploy", "here");
    assert_eq!(hits.len(), 1, "{hits:?} / {:?}", ledger.edges);
    assert_eq!(hits[0].matched, SearchMatch::AppliesTo);
    assert_eq!(hits[0].answer, "Use fly.io.");
}

#[test]
fn normalized_question_text_matches_as_the_same_question() {
    let (_, ledger) = local();
    let hits = search_ledger(
        &ledger,
        "- [ ] choose the LOCAL callback port used by the demo login?",
        "here",
    );
    assert_eq!(hits[0].matched, SearchMatch::QuestionText, "{hits:?}");
    assert_eq!(hits[0].answer, "Use port 3000.");
}

#[test]
fn wording_ranks_by_coverage_and_never_returns_unanswered_questions() {
    let (_, ledger) = local();
    let hits = search_ledger(&ledger, "callback port", "here");
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].matched, SearchMatch::Wording);
    assert_eq!(hits[0].score, 1.0);

    // The answer's words count too.
    let hits = search_ledger(&ledger, "apache license", "here");
    assert_eq!(hits[0].answer, "Apache-2.0 for every crate.");

    // `redir` is open and unanswered: nothing to look up.
    assert!(search_ledger(&ledger, "redirect sign-in", "here").is_empty());
    // Less than half the query covered, or a number the decision lacks.
    assert!(search_ledger(&ledger, "callback timeout retries budget", "here").is_empty());
    assert!(search_ledger(&ledger, "callback port 8080", "here").is_empty());
    assert!(search_ledger(&ledger, "", "here").is_empty());
}

#[test]
fn exact_matches_rank_above_wording() {
    let (_, ledger) = local();
    let lic = decision_of(&ledger, "lic").id.clone();
    let mut hits = search_ledger(&ledger, "port", "here");
    hits.extend(search_ledger(&ledger, "#lic", "here"));
    rank(&mut hits);
    assert_eq!(hits[0].decision_id, lic);
    assert_eq!(hits[0].matched, SearchMatch::CopyId);
    assert_eq!(hits[1].matched, SearchMatch::Wording);
    // One hit per decision: the better band wins.
    let mut dup = search_ledger(&ledger, "callback port", "here");
    dup.extend(search_ledger(&ledger, "#cb", "here"));
    rank(&mut dup);
    assert_eq!(dup.len(), 1);
    assert_eq!(dup[0].matched, SearchMatch::CopyId);
}

#[test]
fn recall_hits_map_onto_answered_decisions_here_and_elsewhere() {
    let (_, ledger) = local();
    let rows = observe(LOCAL, &[]);
    let here = SearchDocument {
        ledger: &ledger,
        rows: &rows,
        path: HERE,
        display: "tasks/here.md",
        hits_current: true,
    };
    let recall = vec![
        // The answered callback question here (line 3), the unanswered
        // redirect question (line 2: nothing to return), the answer row in
        // another document (line 5), and an unreadable document.
        RecallHit {
            path: HERE.into(),
            line: 3,
            score: 2.0,
            semantic: true,
        },
        RecallHit {
            path: HERE.into(),
            line: 2,
            score: 9.0,
            semantic: true,
        },
        RecallHit {
            path: THERE.into(),
            line: 5,
            score: 4.0,
            semantic: false,
        },
        RecallHit {
            path: "/project/x.md".into(),
            line: 1,
            score: 1.0,
            semantic: false,
        },
    ];
    let mut hits = Vec::new();
    add_recall_hits(
        &mut hits,
        std::slice::from_ref(&here),
        &recall,
        6,
        &mut load_other,
    );
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert_eq!(hits[0].document, "tasks/there.md");
    assert_eq!(hits[0].answer, "Use port 8443.");
    assert_eq!(hits[0].matched, SearchMatch::Recall);
    assert!((hits[0].score - 4.0 / 9.0).abs() < 1e-9, "{hits:?}");
    assert_eq!(hits[1].document, "tasks/here.md");
    assert_eq!(hits[1].matched, SearchMatch::RecallSemantic);

    // A stale index: its lines for this document are not trusted.
    let stale = SearchDocument {
        hits_current: false,
        ..here
    };
    let mut hits = Vec::new();
    add_recall_hits(
        &mut hits,
        std::slice::from_ref(&stale),
        &recall,
        6,
        &mut load_other,
    );
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].document, "tasks/there.md");

    // Reads are bounded.
    let mut hits = Vec::new();
    add_recall_hits(
        &mut hits,
        std::slice::from_ref(&stale),
        &recall,
        0,
        &mut |_| panic!("no read"),
    );
    assert!(hits.is_empty());
}

/// `#decisionsearchnofile`: a project search hands over every document it read;
/// a hit lands on the document it names, with no read of another copy.
#[test]
fn recall_hits_land_on_any_of_the_documents_read() {
    let (_, here_ledger) = local();
    let here_rows = observe(LOCAL, &[]);
    let there_rows = observe(OTHER, &[]);
    let there_ledger = Ledger::build(&there_rows, &PriorIndex::default(), NOW, None);
    let documents = [
        SearchDocument {
            ledger: &here_ledger,
            rows: &here_rows,
            path: HERE,
            display: "tasks/here.md",
            hits_current: true,
        },
        SearchDocument {
            ledger: &there_ledger,
            rows: &there_rows,
            path: THERE,
            display: "tasks/there.md",
            hits_current: true,
        },
    ];
    let recall = [RecallHit {
        path: THERE.into(),
        line: 5,
        score: 4.0,
        semantic: false,
    }];
    let mut hits = Vec::new();
    add_recall_hits(&mut hits, &documents, &recall, 6, &mut |_| {
        panic!("no read")
    });
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].document, "tasks/there.md");
    assert_eq!(hits[0].answer, "Use port 8443.");
}

fn registry() -> Vec<RegistryDecision> {
    let record = |id: &str, state: &str, decision: &str, supersedes: &[&str]| RegistryDecision {
        id: id.to_string(),
        state: state.to_string(),
        decision: decision.to_string(),
        rationale: "Keeps the hit rate up.".to_string(),
        evidence_response_ids: Vec::new(),
        updated_at: "2026-11-05T00:00:00Z".to_string(),
        supersedes: supersedes.iter().map(|id| id.to_string()).collect(),
    };
    vec![
        record("DEC-cache-000", "superseded", "Use a FIFO cache store", &[]),
        record(
            "DEC-cache-001",
            "accepted",
            "Use an LRU cache store",
            &["DEC-cache-000"],
        ),
        record(
            "DEC-lic-001",
            "reversed",
            "License the crates under MIT",
            &[],
        ),
    ]
}

/// `#decisionsearchnofile`: the registry answers by id, revision id, stamp,
/// text and wording, and each hit names `decisions.md`.
#[test]
fn registry_records_match_every_band() {
    let records = registry();
    let top = |query: &str| {
        search_registry(&records, query)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no hit for {query}"))
    };
    let by_id = top("[#DEC-cache-001]");
    assert_eq!(by_id.matched, SearchMatch::DecisionId);
    assert_eq!(by_id.document, "decisions.md");
    assert_eq!(by_id.answer, "Use an LRU cache store");
    assert_eq!(by_id.rationale.as_deref(), Some("Keeps the hit rate up."));
    assert_eq!(
        by_id.location,
        "decisions.md (accepted, updated 2026-11-05T00:00:00Z)"
    );
    assert!(by_id.is_current());
    assert_eq!(
        top(&crate::graph::registry_revision_id(&records[1])).matched,
        SearchMatch::RevisionId
    );
    assert_eq!(top("⟦decision:DEC-cache-001⟧").matched, SearchMatch::Stamp);
    assert_eq!(top("decision:dec-cache-001").matched, SearchMatch::Stamp);
    assert_eq!(
        top("Use an LRU cache store").matched,
        SearchMatch::QuestionText
    );
    let by_words = top("hit rate lru");
    assert_eq!(by_words.matched, SearchMatch::Wording);
    assert_eq!(by_words.decision_id, "DEC-cache-001");
    assert!(search_registry(&records, "kubernetes namespace").is_empty());
    assert!(search_registry(&records, "decision:DEC-nope").is_empty());
}

/// A record that no longer stands stays findable, but says so and never
/// outranks the record that replaced it in its band, even on a better score.
#[test]
fn a_record_that_no_longer_stands_is_labelled_and_ranks_last() {
    let records = registry();
    let by_id = &search_registry(&records, "DEC-cache-000")[0];
    assert!(!by_id.is_current());
    assert_eq!(by_id.state.as_deref(), Some("superseded"));
    assert_eq!(by_id.superseded_by, ["DEC-cache-001"]);
    assert!(!search_registry(&records, "DEC-lic-001")[0].is_current());

    let hits = search_registry(&records, "fifo lru cache store");
    let ids: Vec<&str> = hits.iter().map(|hit| hit.decision_id.as_str()).collect();
    assert_eq!(ids, ["DEC-cache-001", "DEC-cache-000"], "{hits:?}");
    assert_eq!(hits[0].score, hits[1].score, "{hits:?}");

    // The replaced record covers more of the query, and still ranks second.
    let hits = search_registry(&records, "fifo cache store");
    assert_eq!(hits[0].decision_id, "DEC-cache-001", "{hits:?}");
    assert_eq!(hits[1].decision_id, "DEC-cache-000", "{hits:?}");
    assert!(hits[0].score < hits[1].score, "{hits:?}");
}

#[test]
fn the_question_cue_fires_only_when_the_turn_is_about_to_ask() {
    let observations = observe(LOCAL, &[]);
    let items = question_item_ids(&observations);
    assert!(items.contains("redir") && items.contains("cb") && items.contains("lic"));
    assert!(
        !items.contains("build"),
        "a work item is not a question: {items:?}"
    );

    let cue = |prompts: &[&str], answered: usize, pushed: bool| -> Option<QuestionCue> {
        question_cue(&CueInputs {
            prompts,
            question_items: &items,
            answered_decisions: answered,
            answers_pushed: pushed,
        })
    };
    // Ordinary work turn: no menu.
    assert_eq!(cue(&["do [#build]"], 2, false), None);
    assert_eq!(cue(&["Which file holds the release step?"], 2, false), None);
    assert_eq!(cue(&["I authorize you to do this."], 2, false), None);
    assert_eq!(cue(&[], 2, false), None);
    // The prompt names an open question.
    assert_eq!(
        cue(&["do [#build]", "do [#redir]"], 2, false),
        Some(QuestionCue::PromptNamesQuestion("redir".into()))
    );
    // The prompt asks for a decision.
    for prompt in [
        "Choose a cache size",
        "give me choices for the layout",
        "ask me before you merge",
        "decide the retry budget",
    ] {
        assert_eq!(
            cue(&[prompt], 2, false),
            Some(QuestionCue::PromptAsksDecision),
            "{prompt}"
        );
    }
    // Answers already pushed for open copies.
    assert_eq!(
        cue(&["do [#build]"], 1, true),
        Some(QuestionCue::AnsweredCopiesOpen)
    );
    // Nothing answered anywhere: nothing to look up.
    assert_eq!(cue(&["do [#redir]"], 0, false), None);
}

#[test]
fn the_menu_is_short_and_names_the_lookup_commands() {
    let menu = lookup_menu("tasks/plan.md");
    assert!(
        menu.contains("agent-doc decision search tasks/plan.md"),
        "{menu}"
    );
    assert!(
        menu.contains("agent-doc decision answered tasks/plan.md"),
        "{menu}"
    );
    assert!(
        menu.contains("agent-doc decision why tasks/plan.md <id>"),
        "{menu}"
    );
    assert!(menu.len() < 420, "menu grew to {} bytes", menu.len());
}
