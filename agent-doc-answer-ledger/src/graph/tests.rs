//! Decision-graph tests (`#decisiongraph`): node/edge projection, explicit
//! `change:` / `amend:` replies, `depends on [#id]` links, the impact walk
//! and why-was-this-decided.

use super::*;
use crate::{PriorIndex, extract};

const NOW: u64 = 1_800_000_000;

const DOC: &str = concat!(
    "---\nagent_doc_session: graph-fixture\n---\n\n",
    "## Main <!-- agent:run id=\"main\" -->\n",
    "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
    "  - 🙋 [#tdpublic] Decide whether haiven-cli is public and open source <!-- agent-doc:waiting-item -->\n",
    "    - **Next:** you — reply under this item ▫️\n",
    "    - Public and open source. Downloads come from public GitHub releases.\n",
    "    - amend: but keep the release App private\n",
    "  - 🙋 [#tdtoken] Decide whether install.sh embeds a download token <!-- agent-doc:waiting-item -->\n",
    "    - **Next:** you — reply under this item ▫️\n",
    "    - No token in install.sh; depends on [#tdpublic]\n",
    "  - 🙋 [#tdapprover] Name the approver for docs PR #88 <!-- agent-doc:waiting-item -->\n",
    "    - Add Alex + Sam, requested in 3f2a9c1\n",
    "    - change: Keep the approver as is\n",
    "  - 🙋 [#tdnotify] Decide whether to email the approvers about the review <!-- agent-doc:waiting-item -->\n",
    "    - Yes, email them.\n",
    "    - depends on [#tdapprover]\n",
    "  - 🙋 [#tdport] Choose the demo callback port <!-- agent-doc:waiting-item -->\n",
    "    - Use port 8080.\n",
    "    - Use the standard http ports.\n",
    "<!-- /agent:run -->\n\n",
    "## Todo <!-- agent:todo -->\n",
    "- [ ] [#tdpublic] Decide whether haiven-cli is public and open source\n",
    "- [ ] [#tdtoken] Decide whether install.sh embeds a download token\n",
    "- [ ] [#tdapprover] Name the approver for docs PR #88\n",
    "- [ ] [#tdnotify] Decide whether to email the approvers about the review\n",
    "- [x] [#tdrequestreview] Request review from Alex + Sam on PR #88 (source: [#tdapprover])\n",
    "- [ ] [#tdreleaseapp] Configure the release App permissions for haiven-cli (source: [#tdpublic])\n",
    "- [ ] [#tddocs] Write the download docs page (source: [#tdpublic])\n",
    "<!-- /agent:todo -->\n",
);

fn build(content: &str) -> (Ledger, DecisionGraph) {
    let observations = extract::observe(content, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    let graph = DecisionGraph::build(&ledger, &observations);
    (ledger, graph)
}

fn decision(graph: &DecisionGraph, item: &str) -> String {
    graph
        .resolve_decision(item)
        .unwrap_or_else(|| panic!("no decision for {item}: {graph:#?}"))
}

fn has_edge(graph: &DecisionGraph, from: &str, kind: EdgeKind, to: &str) -> Option<GraphEdge> {
    graph
        .edges
        .iter()
        .find(|edge| edge.from == from && edge.kind == kind && edge.to == to)
        .cloned()
}

#[test]
fn nodes_are_typed_and_work_todos_are_items_not_decisions() {
    let (_, graph) = build(DOC);
    let public = decision(&graph, "tdpublic");
    assert_eq!(graph.nodes[&public].kind, NodeKind::Decision);
    assert_eq!(graph.nodes["tdpublic"].kind, NodeKind::Question);
    assert!(has_edge(&graph, "tdpublic", EdgeKind::CopyOf, &public).is_some());
    // Work todos nobody asked about are items, with no decision of their own.
    for item in ["tdreleaseapp", "tddocs", "tdrequestreview"] {
        assert_eq!(graph.nodes[item].kind, NodeKind::Item, "{item}");
        assert!(graph.resolve_decision(item).is_none(), "{item}");
    }
    assert!(
        !graph
            .nodes
            .values()
            .any(|node| node.kind == NodeKind::Decision && node.label.contains("download docs"))
    );
    // Evidence: the commit named in the operator's answer.
    let approver = decision(&graph, "tdapprover");
    let first = &graph.revisions_of(&approver)[0].id;
    assert!(has_edge(&graph, first, EdgeKind::EvidencedBy, "commit:3f2a9c1").is_some());
    assert_eq!(graph.nodes["commit:3f2a9c1"].kind, NodeKind::Evidence);
}

#[test]
fn explicit_change_and_amend_replies_decide_the_revision_kind() {
    let (ledger, graph) = build(DOC);
    let public = decision(&graph, "tdpublic");
    let revisions = graph.revisions_of(&public);
    assert_eq!(revisions.len(), 2);
    // The lead-in is stripped from the recorded answer.
    assert_eq!(revisions[1].label, "but keep the release App private");
    let amend = graph.relation_of(&revisions[1].id).unwrap();
    assert_eq!(amend.kind, EdgeKind::Amends);
    assert_eq!(amend.source, EdgeSource::Operator);
    assert_eq!(
        amend.clause.as_deref(),
        Some("but keep the release App private")
    );

    let approver = decision(&graph, "tdapprover");
    let revisions = graph.revisions_of(&approver);
    let change = graph.relation_of(&revisions[1].id).unwrap();
    assert_eq!(change.kind, EdgeKind::Supersedes);
    assert_eq!(change.source, EdgeSource::Operator);

    // Without a lead-in the binary proposes the kind.
    let port = decision(&graph, "tdport");
    let revisions = graph.revisions_of(&port);
    let proposed = graph.relation_of(&revisions[1].id).unwrap();
    assert_eq!(proposed.kind, EdgeKind::Supersedes);
    assert_eq!(proposed.source, EdgeSource::Proposed);

    // The cycle-contract fact keeps the amended base and names the clause.
    let facts = ledger.answered_facts();
    let fact = facts
        .iter()
        .find(|fact| fact.decision_id == public)
        .unwrap();
    assert_eq!(
        fact.amends.as_deref(),
        Some("Public and open source. Downloads come from public GitHub releases.")
    );
    assert_eq!(
        fact.amended_clauses,
        vec!["but keep the release App private".to_string()]
    );
    assert_eq!(fact.change_source, Some(EdgeSource::Operator));
    assert!(fact.changed_from.is_none());
    let port_fact = facts.iter().find(|fact| fact.decision_id == port).unwrap();
    assert_eq!(port_fact.change_source, Some(EdgeSource::Proposed));
    let guidance = crate::contract_guidance(&facts, &[]).unwrap();
    assert!(
        guidance.contains("amends “Public and open source."),
        "{guidance}"
    );
    assert!(
        guidance.contains("starting the reply with `change:` or `amend:`"),
        "{guidance}"
    );
}

#[test]
fn depends_on_replies_link_decisions_and_link_only_rows_are_not_answers() {
    let (ledger, graph) = build(DOC);
    let public = decision(&graph, "tdpublic");
    let token = decision(&graph, "tdtoken");
    let approver = decision(&graph, "tdapprover");
    let notify = decision(&graph, "tdnotify");
    let edge =
        has_edge(&graph, &token, EdgeKind::DependsOn, &public).expect("token assumes public");
    assert_eq!(edge.source, EdgeSource::Operator);
    assert!(has_edge(&graph, &notify, EdgeKind::DependsOn, &approver).is_some());
    // `depends on [#tdapprover]` alone is a link, not a second answer.
    let notify_decision = ledger
        .decisions
        .iter()
        .find(|decision| decision.id == notify)
        .unwrap();
    assert_eq!(notify_decision.revisions.len(), 1);
    assert_eq!(notify_decision.revisions[0].answer, "Yes, email them.");
}

#[test]
fn a_supersede_flags_every_dependent_and_lists_actions_on_the_old_answer() {
    let (_, graph) = build(DOC);
    let approver = decision(&graph, "tdapprover");
    let notify = decision(&graph, "tdnotify");
    let impact = graph.impact(&approver, None).unwrap();
    assert_eq!(impact.change, "supersedes");
    assert_eq!(impact.change_source, EdgeSource::Operator);
    assert_eq!(
        impact.from_answer.as_deref(),
        Some("Add Alex + Sam, requested in 3f2a9c1")
    );
    assert_eq!(impact.to_answer, "Keep the approver as is");
    let flagged: BTreeMap<&str, ImpactAction> = impact
        .flagged
        .iter()
        .map(|row| (row.node.as_str(), row.action))
        .collect();
    // The closed review request was acted on under the old answer: reopen.
    assert_eq!(flagged.get("tdrequestreview"), Some(&ImpactAction::Reopen));
    // The decision that assumes this one is open: re-evaluate it.
    assert_eq!(flagged.get(notify.as_str()), Some(&ImpactAction::Flag));
    // The commit made on the old answer: the operator keeps or undoes it.
    assert!(
        impact
            .actions_taken
            .iter()
            .any(|row| row.node == "commit:3f2a9c1" && row.action == ImpactAction::KeepOrUndo),
        "{impact:#?}"
    );
    let lines = render_impact(&impact);
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("reopen #tdrequestreview")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("keep or undo: commit:3f2a9c1")),
        "{lines:?}"
    );
}

#[test]
fn an_amend_flags_only_the_rows_touching_the_amended_clause() {
    let (_, graph) = build(DOC);
    let public = decision(&graph, "tdpublic");
    let impact = graph.impact(&public, None).unwrap();
    assert_eq!(impact.change, "amends");
    let flagged: Vec<&str> = impact.flagged.iter().map(|row| row.node.as_str()).collect();
    assert_eq!(flagged, vec!["tdreleaseapp"], "{impact:#?}");
    assert_eq!(impact.flagged[0].action, ImpactAction::ReviewClause);
    // The docs page and the dependent token decision do not touch the clause.
    assert!(impact.unaffected >= 2, "{impact:#?}");
    assert!(impact.actions_taken.is_empty());
}

#[test]
fn an_initial_answer_disturbs_nothing() {
    let (_, graph) = build(DOC);
    let token = decision(&graph, "tdtoken");
    let impact = graph.impact(&token, None).unwrap();
    assert_eq!(impact.change, "initial");
    assert!(impact.flagged.is_empty() && impact.actions_taken.is_empty());
}

#[test]
fn why_walks_revisions_evidence_and_premises() {
    let (_, graph) = build(DOC);
    let token = decision(&graph, "tdtoken");
    let public = decision(&graph, "tdpublic");
    let why = graph.why(&token).unwrap();
    assert_eq!(
        why.current_answer.as_deref(),
        Some("No token in install.sh; depends on [#tdpublic]")
    );
    assert_eq!(why.revisions.len(), 1);
    assert_eq!(why.revisions[0].relation, "initial");
    assert_eq!(why.depends_on.len(), 1);
    assert_eq!(why.depends_on[0].id, public);
    assert_eq!(
        why.depends_on[0].answer.as_deref(),
        Some("but keep the release App private")
    );

    let approver = decision(&graph, "tdapprover");
    let why = graph.why(&approver).unwrap();
    assert_eq!(why.revisions.len(), 2);
    assert_eq!(
        why.revisions[0].evidence,
        vec!["commit:3f2a9c1".to_string()]
    );
    assert_eq!(why.revisions[1].relation, "supersedes");
    assert_eq!(why.revisions[1].relation_source, EdgeSource::Operator);
    assert!(why.copies.contains(&"tdapprover".to_string()));
    assert!(
        why.applies_to.contains(&"tdrequestreview".to_string()),
        "{why:#?}"
    );
    // Revision and copy ids resolve to the same decision.
    assert_eq!(
        graph.resolve_decision(&why.revisions[0].id).as_deref(),
        Some(approver.as_str())
    );
    assert_eq!(
        graph.decisions_applying_to("tdrequestreview"),
        vec![approver.clone()]
    );
    assert_eq!(
        graph.resolve_decision("[#tdapprover]").as_deref(),
        Some(approver.as_str())
    );
}

#[test]
fn cached_history_survives_and_an_operator_correction_replaces_a_proposal() {
    let (_, mut graph) = build(DOC);
    let port = decision(&graph, "tdport");
    let revisions: Vec<String> = graph
        .revisions_of(&port)
        .iter()
        .map(|node| node.id.clone())
        .collect();
    // The cache holds a stale proposal for the same pair under another kind,
    // a node the document no longer shows, and an edge to it.
    let prior_nodes = vec![Node {
        id: "r-0123456789ab".into(),
        kind: NodeKind::Evidence,
        label: String::new(),
        open: None,
        location: None,
        first_seen_at: Some(1),
        in_document: true,
    }];
    let prior_edges = vec![
        GraphEdge {
            from: revisions[1].clone(),
            kind: EdgeKind::Amends,
            to: revisions[0].clone(),
            source: EdgeSource::Proposed,
            clause: None,
        },
        GraphEdge {
            from: revisions[0].clone(),
            kind: EdgeKind::EvidencedBy,
            to: "r-0123456789ab".into(),
            source: EdgeSource::Derived,
            clause: None,
        },
        GraphEdge {
            from: "gone".into(),
            kind: EdgeKind::DependsOn,
            to: port.clone(),
            source: EdgeSource::Operator,
            clause: None,
        },
    ];
    graph.merge_prior(prior_nodes, prior_edges);
    assert_eq!(
        graph.relation_of(&revisions[1]).unwrap().kind,
        EdgeKind::Supersedes
    );
    assert!(!graph.nodes["r-0123456789ab"].in_document);
    assert!(
        has_edge(
            &graph,
            &revisions[0],
            EdgeKind::EvidencedBy,
            "r-0123456789ab"
        )
        .is_some()
    );
    // An edge whose endpoint is unknown is dropped.
    assert!(graph.edges.iter().all(|edge| edge.from != "gone"));
}

#[test]
fn evidence_refs_find_responses_and_commits_only() {
    assert_eq!(
        evidence_refs("Closed by [#r-3b817c02c2e8] in 6d5308325c; see PR #88, v0.1.0, deadbeef"),
        vec![
            "commit:6d5308325c".to_string(),
            "r-3b817c02c2e8".to_string()
        ]
    );
    assert!(evidence_refs("Keep the approver as is").is_empty());
}

#[test]
fn a_cached_operator_relation_outranks_a_rebuilt_proposal() {
    let (_, mut graph) = build(DOC);
    let port = decision(&graph, "tdport");
    let revisions: Vec<String> = graph
        .revisions_of(&port)
        .iter()
        .map(|node| node.id.clone())
        .collect();
    assert_eq!(
        graph.relation_of(&revisions[1]).unwrap().source,
        EdgeSource::Proposed
    );
    // The operator's `amend:` row was pruned; the cache still remembers it.
    graph.merge_prior(
        Vec::new(),
        vec![GraphEdge {
            from: revisions[1].clone(),
            kind: EdgeKind::Amends,
            to: revisions[0].clone(),
            source: EdgeSource::Operator,
            clause: Some("Use the standard http ports.".into()),
        }],
    );
    let relation = graph.relation_of(&revisions[1]).unwrap();
    assert_eq!(relation.kind, EdgeKind::Amends);
    assert_eq!(relation.source, EdgeSource::Operator);
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind.is_revision_relation() && edge.from == revisions[1])
            .count(),
        1
    );
}

/// `#decisionrevisionsource`: the index remembers who decided a revision's
/// kind, so once the operator's `change:` / `amend:` row is pruned the cycle
/// contract still reports the operator's call, not the binary's proposal.
#[test]
fn a_pruned_operator_word_stays_the_operators_call_in_the_cycle_contract() {
    let (ledger, _) = build(DOC);
    let (keys, revisions) = ledger.index_rows();
    let prior = PriorIndex {
        keys,
        revisions,
        ..PriorIndex::default()
    };
    let operator_rows: BTreeSet<(&str, &str)> = prior
        .revisions
        .iter()
        .filter(|revision| revision.relation_source == EdgeSource::Operator)
        .map(|revision| (revision.answer.as_str(), revision.relation.as_str()))
        .collect();
    assert_eq!(
        operator_rows,
        BTreeSet::from([
            ("but keep the release App private", "amends"),
            ("Keep the approver as is", "supersedes"),
        ])
    );
    let pruned = DOC
        .replace("    - amend: but keep the release App private\n", "")
        .replace("    - change: Keep the approver as is\n", "");
    let rebuild = |prior: &PriorIndex| {
        let observations = extract::observe(&pruned, &[]);
        let ledger = Ledger::build(&observations, prior, NOW + 60, None);
        let graph = DecisionGraph::build(&ledger, &observations);
        (ledger, graph)
    };
    let (ledger, graph) = rebuild(&prior);
    let public = decision(&graph, "tdpublic");
    let revisions = graph.revisions_of(&public);
    assert_eq!(revisions.len(), 2);
    assert!(!revisions[1].in_document);
    let amend = graph.relation_of(&revisions[1].id).unwrap();
    assert_eq!(
        (amend.kind, amend.source, amend.clause.as_deref()),
        (
            EdgeKind::Amends,
            EdgeSource::Operator,
            Some("but keep the release App private")
        )
    );
    let facts = ledger.answered_facts();
    let fact = facts
        .iter()
        .find(|fact| fact.decision_id == public)
        .unwrap();
    assert_eq!(fact.change_source, Some(EdgeSource::Operator));
    assert_eq!(
        fact.amends.as_deref(),
        Some("Public and open source. Downloads come from public GitHub releases.")
    );
    assert_eq!(
        fact.amended_clauses,
        vec!["but keep the release App private".to_string()]
    );
    let approver = decision(&graph, "tdapprover");
    let fact = facts
        .iter()
        .find(|fact| fact.decision_id == approver)
        .unwrap();
    assert_eq!(fact.answer, "Keep the approver as is");
    assert_eq!(fact.change_source, Some(EdgeSource::Operator));
    let change = graph
        .relation_of(&graph.revisions_of(&approver)[1].id)
        .unwrap();
    assert_eq!(
        (change.kind, change.source),
        (EdgeKind::Supersedes, EdgeSource::Operator)
    );
    // The rebuilt index keeps the operator's word for the next cycle.
    let (_, rows) = ledger.index_rows();
    assert!(
        rows.iter()
            .any(|row| row.answer == "but keep the release App private"
                && row.relation == "amends"
                && row.relation_source == EdgeSource::Operator)
    );

    // Control: an index that never recorded the source (a legacy row) falls
    // back to the binary's proposal, which is what this item fixes.
    let mut legacy = prior.clone();
    for revision in &mut legacy.revisions {
        revision.relation_source = EdgeSource::Derived;
    }
    let (ledger, _) = rebuild(&legacy);
    let fact = ledger
        .answered_facts()
        .into_iter()
        .find(|fact| fact.decision_id == public)
        .unwrap();
    assert_ne!(fact.change_source, Some(EdgeSource::Operator));
}

#[test]
fn closed_upstream_moots_only_the_decision_that_cites_it() {
    let content = concat!(
        "---\nagent_doc_session: upstream-fixture\n---\n\n",
        "## Main <!-- agent:run id=\"main\" -->\n",
        "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
        "  - 🙋 [#tdcite] Decide whether to keep waiting on https://github.com/btakita/agent-doc/issues/395 <!-- agent-doc:waiting-item -->\n",
        "  - 🙋 [#tdbare] Answer the blocking host question (choices: Open the workspace, Wait on issue 395, Defer) <!-- agent-doc:waiting-item -->\n",
        "  - 🙋 [#tdother] Decide whether the demo callback port stays 8080 <!-- agent-doc:waiting-item -->\n",
        "  - 🙋 [#tdpull] Decide whether to merge https://github.com/btakita/agent-doc/pull/12 <!-- agent-doc:waiting-item -->\n",
        "<!-- /agent:run -->\n",
        "## Todo <!-- agent:todo -->\n",
        "- [ ] [#tdsmoke] Run the coder smoke while https://github.com/btakita/agent-doc/issues/395 is open\n",
        "<!-- /agent:todo -->\n",
    );
    let observations = extract::observe(content, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    let mut graph = DecisionGraph::build(&ledger, &observations);
    let issue = GithubUpstream::issue("btakita", "agent-doc", 395);
    let pull = GithubUpstream::pull_request("btakita", "agent-doc", 12);
    let cite = decision(&graph, "tdcite");
    let bare = decision(&graph, "tdbare");
    let other = decision(&graph, "tdother");
    let pull_decision = decision(&graph, "tdpull");
    let edge = has_edge(&graph, &issue.node_id(), EdgeKind::UpstreamOf, &cite).expect("cite edge");
    assert_eq!(edge.source, EdgeSource::Derived);
    assert_eq!(edge.clause.as_deref(), Some("open"));
    assert_eq!(graph.nodes[&issue.node_id()].kind, NodeKind::Evidence);
    // The document names issue 395 once, so "Wait on issue 395" cites it.
    assert!(has_edge(&graph, &issue.node_id(), EdgeKind::UpstreamOf, &bare).is_some());
    assert!(
        has_edge(
            &graph,
            &pull.node_id(),
            EdgeKind::UpstreamOf,
            &pull_decision
        )
        .is_some()
    );
    assert!(graph.resolve_decision("tdsmoke").is_none());

    let moot = graph.moot_by_closed_upstream(&observations, &[issue.clone()]);
    let mooted: Vec<&str> = moot.iter().map(|row| row.decision_id.as_str()).collect();
    assert!(mooted.contains(&cite.as_str()), "{moot:?}");
    assert!(mooted.contains(&bare.as_str()), "{moot:?}");
    assert!(!mooted.contains(&other.as_str()), "{moot:?}");
    assert!(!mooted.contains(&pull_decision.as_str()), "{moot:?}");
    assert!(moot.iter().all(|row| row.upstream == issue));

    let alien_content = concat!(
        "---\nagent_doc_session: alien-upstream\n---\n\n",
        "## Main <!-- agent:run id=\"main\" -->\n",
        "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
        "  - 🙋 [#tdalien] Decide whether https://github.com/other/repo/issues/395 blocks the port <!-- agent-doc:waiting-item -->\n",
        "<!-- /agent:run -->\n",
    );
    let alien_observations = extract::observe(alien_content, &[]);
    let alien_ledger = Ledger::build(&alien_observations, &PriorIndex::default(), NOW, None);
    let mut alien_graph = DecisionGraph::build(&alien_ledger, &alien_observations);
    let alien = decision(&alien_graph, "tdalien");
    let alien_moot = alien_graph.moot_by_closed_upstream(&alien_observations, &[issue.clone()]);
    assert!(alien_moot.is_empty(), "{alien_moot:?}");
    assert_ne!(alien, cite);

    let moot_pull = graph.moot_by_closed_upstream(&observations, &[pull.clone()]);
    let mooted_pull: Vec<&str> = moot_pull
        .iter()
        .map(|row| row.decision_id.as_str())
        .collect();
    assert_eq!(mooted_pull, vec![pull_decision.as_str()]);
}

#[test]
fn bare_issue_mention_stays_when_the_closed_number_is_ambiguous() {
    let content = concat!(
        "---\nagent_doc_session: bare-upstream\n---\n\n",
        "## Main <!-- agent:run id=\"main\" -->\n",
        "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
        "  - 🙋 [#tdbare] Answer the blocking host question (choices: Open the workspace, Wait on issue 395, Defer) <!-- agent-doc:waiting-item -->\n",
        "  - 🙋 [#tdother] Decide whether the demo callback port stays 8080 <!-- agent-doc:waiting-item -->\n",
        "<!-- /agent:run -->\n",
    );
    let observations = extract::observe(content, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    let mut graph = DecisionGraph::build(&ledger, &observations);
    let bare = decision(&graph, "tdbare");
    let other = decision(&graph, "tdother");
    let issue = GithubUpstream::issue("btakita", "agent-doc", 395);
    let alien = GithubUpstream::issue("other", "repo", 395);
    assert!(
        graph
            .moot_by_closed_upstream(&observations, &[issue.clone(), alien])
            .is_empty()
    );
    let moot = graph.moot_by_closed_upstream(&observations, &[issue.clone()]);
    assert_eq!(moot.len(), 1, "{moot:?}");
    assert_eq!(moot[0].decision_id, bare);
    assert_eq!(moot[0].upstream, issue);
    assert_ne!(moot[0].decision_id, other);
    assert!(has_edge(&graph, &issue.node_id(), EdgeKind::UpstreamOf, &bare).is_some());
    assert!(has_edge(&graph, &issue.node_id(), EdgeKind::UpstreamOf, &other).is_none());
}
