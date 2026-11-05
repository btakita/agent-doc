//! Registry overlay tests (`#decisionregistrygraph`): `decisions.md` records
//! and their `supersedes` links in the decision graph, reached by `impact`
//! and `why`.

use std::collections::BTreeMap;

use super::super::*;
use super::*;
use crate::{Ledger, PriorIndex, extract};

const NOW: u64 = 1_800_000_000;

const DOC: &str = concat!(
    "---\nagent_doc_session: registry-fixture\n---\n\n",
    "## Main <!-- agent:run id=\"main\" -->\n",
    "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n",
    "  - 🙋 [#tdshard] Decide whether to shard the cache store <!-- agent-doc:waiting-item -->\n",
    "    - **Next:** you — reply under this item ▫️\n",
    "    - Shard by tenant.\n",
    "    - depends on [#DEC-storage-v1]\n",
    "<!-- /agent:run -->\n\n",
    "## Todo <!-- agent:todo -->\n",
    "- [ ] [#tdshard] Decide whether to shard the cache store\n",
    "- [ ] [#tdmigrate] Migrate the cache to [DEC-storage-v1](decision:DEC-storage-v1)\n",
    "- [x] [#tdbackfill] Backfill rows under [DEC-storage-v1](decision:DEC-storage-v1)\n",
    "- [ ] [#tdbench] Benchmark the new store (source: [#DEC-storage-v2])\n",
    "- [ ] [#tdunrelated] Rename the CLI flag\n",
    "<!-- /agent:todo -->\n",
);

fn records() -> Vec<RegistryDecision> {
    vec![
        RegistryDecision {
            id: "DEC-storage-v1".to_string(),
            state: "superseded".to_string(),
            decision: "Store cache rows in SQLite".to_string(),
            rationale: "One file, no server.".to_string(),
            evidence_response_ids: vec!["r-aaa111".to_string()],
            updated_at: "2026-10-01T00:00:00Z".to_string(),
            supersedes: Vec::new(),
        },
        RegistryDecision {
            id: "DEC-storage-v2".to_string(),
            state: "accepted".to_string(),
            decision: "Store cache rows in Postgres".to_string(),
            rationale: "Concurrent writers.".to_string(),
            // The registry stores the bracketed lookup form.
            evidence_response_ids: vec!["[#r-bbb222]".to_string()],
            updated_at: "2026-11-01T00:00:00Z".to_string(),
            // DEC-gone is dangling: validate reports it; the graph skips it.
            supersedes: vec!["DEC-gone".to_string(), "DEC-storage-v1".to_string()],
        },
    ]
}

/// Citations as the I/O loader derives them from `decision:DEC-…` links.
fn citations(observations: &extract::Observations) -> Vec<RegistryCitation> {
    observations
        .tracked
        .iter()
        .filter_map(|item| {
            let decisions: Vec<String> = ["DEC-storage-v1", "DEC-storage-v2"]
                .into_iter()
                .filter(|id| item.text.contains(&format!("decision:{id}")))
                .map(str::to_string)
                .collect();
            (!decisions.is_empty()).then(|| RegistryCitation {
                item: item.id.clone(),
                label: item.text.clone(),
                open: item.open,
                decisions,
            })
        })
        .collect()
}

fn build() -> DecisionGraph {
    let observations = extract::observe(DOC, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    let mut graph = DecisionGraph::build(&ledger, &observations);
    graph.overlay_registry(&records(), &citations(&observations));
    graph
}

fn has_edge(graph: &DecisionGraph, from: &str, kind: EdgeKind, to: &str) -> bool {
    graph
        .edges
        .iter()
        .any(|edge| edge.from == from && edge.kind == kind && edge.to == to)
}

#[test]
fn records_become_decisions_with_one_revision_evidence_and_supersedes_links() {
    let graph = build();
    let v2 = &graph.nodes["DEC-storage-v2"];
    assert_eq!(v2.kind, NodeKind::Decision);
    assert_eq!(v2.label, "Store cache rows in Postgres");
    let revision = registry_revision_id(&records()[1]);
    assert_eq!(graph.nodes[&revision].kind, NodeKind::Revision);
    assert!(has_edge(
        &graph,
        &revision,
        EdgeKind::Answers,
        "DEC-storage-v2"
    ));
    assert!(has_edge(
        &graph,
        &revision,
        EdgeKind::EvidencedBy,
        "r-bbb222"
    ));
    assert_eq!(graph.nodes["r-bbb222"].kind, NodeKind::Evidence);
    // Decision-level supersede; the dangling target is not invented.
    assert!(has_edge(
        &graph,
        "DEC-storage-v2",
        EdgeKind::Supersedes,
        "DEC-storage-v1"
    ));
    assert!(!graph.nodes.contains_key("DEC-gone"));
    assert_eq!(
        graph.superseded_decisions("DEC-storage-v2"),
        vec!["DEC-storage-v1".to_string()]
    );
    // A decision-level edge never joins a revision chain.
    assert_eq!(graph.revisions_of("DEC-storage-v2").len(), 1);
    assert_eq!(graph.revisions_of("DEC-storage-v1").len(), 1);
    // Ids resolve verbatim, bracketed, or lowercased.
    for id in ["DEC-storage-v2", "[#DEC-storage-v2]", "#DEC-storage-v2"] {
        assert_eq!(
            graph.resolve_decision(id).as_deref(),
            Some("DEC-storage-v2"),
            "{id}"
        );
    }
    assert_eq!(
        graph.resolve_decision(&revision).as_deref(),
        Some("DEC-storage-v2")
    );
}

#[test]
fn citations_and_bracket_ids_tie_session_items_to_records() {
    let graph = build();
    assert!(has_edge(
        &graph,
        "tdmigrate",
        EdgeKind::AppliesTo,
        "DEC-storage-v1"
    ));
    assert!(has_edge(
        &graph,
        "tdbackfill",
        EdgeKind::AppliesTo,
        "DEC-storage-v1"
    ));
    assert_eq!(graph.nodes["tdbackfill"].open, Some(false));
    // `[#DEC-storage-v2]` lowercases to an item placeholder; it is folded
    // onto the record.
    assert!(!graph.nodes.contains_key("dec-storage-v2"));
    assert!(!graph.nodes.contains_key("dec-storage-v1"));
    assert!(has_edge(
        &graph,
        "tdbench",
        EdgeKind::AppliesTo,
        "DEC-storage-v2"
    ));
    // `depends on [#DEC-storage-v1]` ties the ledger decision to the record.
    let shard = graph.resolve_decision("tdshard").unwrap();
    assert!(
        has_edge(&graph, &shard, EdgeKind::DependsOn, "DEC-storage-v1"),
        "{graph:#?}"
    );
    assert_eq!(
        graph.decisions_applying_to("tdmigrate"),
        vec!["DEC-storage-v1".to_string()]
    );
}

#[test]
fn superseding_record_flags_the_replaced_records_dependents_and_old_evidence() {
    let graph = build();
    let impact = graph.impact("DEC-storage-v2", None).unwrap();
    assert_eq!(impact.change, "supersedes");
    assert_eq!(impact.change_source, EdgeSource::Derived);
    assert_eq!(
        impact.from_answer.as_deref(),
        Some("Store cache rows in SQLite")
    );
    assert_eq!(impact.to_answer, "Store cache rows in Postgres");
    assert_eq!(
        impact.superseded_decisions,
        vec!["DEC-storage-v1".to_string()]
    );
    let flagged: BTreeMap<&str, (ImpactAction, Vec<String>)> = impact
        .flagged
        .iter()
        .map(|row| (row.node.as_str(), (row.action, row.via.clone())))
        .collect();
    let old = vec!["DEC-storage-v1".to_string()];
    assert_eq!(
        flagged.get("tdmigrate"),
        Some(&(ImpactAction::Flag, old.clone()))
    );
    assert_eq!(
        flagged.get("tdbackfill"),
        Some(&(ImpactAction::Reopen, old.clone())),
        "{impact:#?}"
    );
    let shard = graph.resolve_decision("tdshard").unwrap();
    assert_eq!(
        flagged.get(shard.as_str()),
        Some(&(ImpactAction::Flag, old.clone()))
    );
    // The new record's own citing item is a dependent too.
    assert_eq!(
        flagged.get("tdbench"),
        Some(&(ImpactAction::Flag, Vec::new()))
    );
    assert!(!flagged.contains_key("tdunrelated"));
    assert!(!flagged.contains_key("DEC-storage-v1"));
    // The old record's evidence is an action taken on the old answer.
    let taken: Vec<&str> = impact
        .actions_taken
        .iter()
        .map(|row| row.node.as_str())
        .collect();
    assert_eq!(taken, vec!["r-aaa111"], "{impact:#?}");
    assert!(
        render_impact(&impact)
            .iter()
            .any(|line| line.contains("keep or undo: r-aaa111")),
    );
}

#[test]
fn the_replaced_record_alone_disturbs_nothing() {
    let graph = build();
    let impact = graph.impact("DEC-storage-v1", None).unwrap();
    assert_eq!(impact.change, "initial");
    assert!(impact.flagged.is_empty() && impact.actions_taken.is_empty());
    assert!(impact.superseded_decisions.is_empty());
}

#[test]
fn why_names_what_a_record_supersedes_and_what_supersedes_it() {
    let graph = build();
    let why = graph.why("DEC-storage-v2").unwrap();
    assert_eq!(
        why.current_answer.as_deref(),
        Some("Store cache rows in Postgres")
    );
    assert_eq!(why.revisions.len(), 1);
    assert_eq!(why.revisions[0].relation, "initial");
    assert_eq!(why.revisions[0].evidence, vec!["r-bbb222".to_string()]);
    assert_eq!(
        why.revisions[0].location.as_deref(),
        Some("decisions.md (accepted, updated 2026-11-01T00:00:00Z)")
    );
    assert_eq!(why.supersedes.len(), 1);
    assert_eq!(why.supersedes[0].id, "DEC-storage-v1");
    assert_eq!(
        why.supersedes[0].answer.as_deref(),
        Some("Store cache rows in SQLite")
    );
    assert_eq!(why.applies_to, vec!["tdbench".to_string()]);
    assert!(why.superseded_by.is_empty());

    let old = graph.why("DEC-storage-v1").unwrap();
    assert_eq!(old.superseded_by, vec!["DEC-storage-v2".to_string()]);
    assert!(old.supersedes.is_empty());
    assert_eq!(
        old.applies_to,
        vec!["tdbackfill".to_string(), "tdmigrate".to_string()]
    );
    // A ledger decision's why names the record it depends on.
    let shard = graph.resolve_decision("tdshard").unwrap();
    let why = graph.why(&shard).unwrap();
    assert!(
        why.depends_on
            .iter()
            .any(|premise| premise.id == "DEC-storage-v1"
                && premise.answer.as_deref() == Some("Store cache rows in SQLite")),
        "{why:#?}"
    );
}

#[test]
fn the_overlay_is_idempotent_and_leaves_ledger_decisions_alone() {
    let observations = extract::observe(DOC, &[]);
    let ledger = Ledger::build(&observations, &PriorIndex::default(), NOW, None);
    let bare = DecisionGraph::build(&ledger, &observations);
    let once = build();
    let mut twice = once.clone();
    twice.overlay_registry(&records(), &citations(&observations));
    assert_eq!(once, twice);
    // No registry: the overlay is a no-op.
    let mut empty = bare.clone();
    empty.overlay_registry(&[], &[]);
    assert_eq!(empty, bare);
    // The ledger decision keeps its own answer and relation.
    let shard = bare.resolve_decision("tdshard").unwrap();
    assert_eq!(
        once.impact(&shard, None).unwrap(),
        bare.impact(&shard, None).unwrap()
    );
}
