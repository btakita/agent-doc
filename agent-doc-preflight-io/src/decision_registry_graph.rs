//! Load the project `decisions.md` registry into a session document's
//! decision graph (`#decisionregistrygraph`).
//!
//! The registry at the document's project root is read with
//! `agent-doc-decision-io` (its records and their `Supersedes:` links), the
//! session items citing a record (`[DEC-x](decision:DEC-x)`) are found with
//! the same `decision:DEC-…` parser `decision validate` uses, and both are
//! projected by [`DecisionGraph::overlay_registry`]. Read-only: nothing is
//! written to the document, the registry, or the `state.db` graph cache, so
//! the registry stays its own authority. A missing registry is a no-op; an
//! unreadable one is reported on stderr and skipped.

use std::collections::BTreeMap;
use std::path::Path;

use agent_doc_answer_ledger::extract::Observations;
use agent_doc_answer_ledger::graph::{DecisionGraph, RegistryCitation, RegistryDecision};

/// Registry records as graph input.
pub fn registry_decisions(registry: &agent_doc_decision_io::Registry) -> Vec<RegistryDecision> {
    registry
        .records
        .values()
        .map(|record| RegistryDecision {
            id: record.id.clone(),
            state: record.state.as_str().to_string(),
            decision: record.decision.clone(),
            rationale: record.rationale.clone(),
            evidence_response_ids: record.evidence_response_ids.clone(),
            updated_at: record.updated_at.clone(),
            supersedes: record.supersedes.clone(),
        })
        .collect()
}

/// Session items (question copies first, then todo / backlog / review /
/// icebox items) whose text cites a registry record.
pub fn registry_citations(observations: &Observations) -> Vec<RegistryCitation> {
    let mut by_item: BTreeMap<String, RegistryCitation> = BTreeMap::new();
    let questions = observations.questions.iter().filter_map(|question| {
        question
            .id
            .as_ref()
            .map(|id| (id.clone(), question.text.as_str(), question.open))
    });
    let tracked = observations
        .tracked
        .iter()
        .map(|item| (item.id.clone(), item.text.as_str(), item.open));
    for (item, text, open) in questions.chain(tracked) {
        let decisions = agent_doc_decision_io::decision_references(text);
        if decisions.is_empty() {
            continue;
        }
        let citation =
            by_item
                .entry(item.to_ascii_lowercase())
                .or_insert_with(|| RegistryCitation {
                    item: item.clone(),
                    label: text.to_string(),
                    open,
                    decisions: Vec::new(),
                });
        citation.open |= open;
        citation.decisions.extend(decisions);
        citation.decisions.sort();
        citation.decisions.dedup();
    }
    by_item.into_values().collect()
}

/// Overlay the registry at `file`'s project root onto `graph`.
pub fn overlay_project_registry(
    file: &Path,
    graph: &mut DecisionGraph,
    observations: &Observations,
) {
    let Some(root) = agent_doc_project_root_io::project_root_containing(file) else {
        return;
    };
    if !agent_doc_decision_io::registry_path(&root).is_file() {
        return;
    }
    match agent_doc_decision_io::read_registry(&root) {
        Ok(registry) => graph.overlay_registry(
            &registry_decisions(&registry),
            &registry_citations(observations),
        ),
        Err(err) => eprintln!(
            "[decision graph] {} unreadable ({err:#}); using the session document only",
            agent_doc_decision_io::registry_path(&root).display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_answer_ledger::EdgeKind;
    use agent_doc_answer_ledger::graph::{ImpactAction, NodeKind};
    use agent_doc_decision_io::{DecisionRecord, DecisionState, Registry};

    fn record(id: &str, state: DecisionState, text: &str, supersedes: &[&str]) -> DecisionRecord {
        DecisionRecord {
            id: id.to_string(),
            state,
            decision: text.to_string(),
            rationale: "Because.".to_string(),
            evidence_response_ids: vec![format!("[#r-{:0>12x}]", id.len())],
            updated_at: "2026-11-05T00:00:00Z".to_string(),
            supersedes: supersedes.iter().map(|id| id.to_string()).collect(),
        }
    }

    fn project(registry: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        if let Some(registry) = registry {
            std::fs::write(dir.path().join("decisions.md"), registry).unwrap();
        }
        let doc = dir.path().join("plan.md");
        let content = concat!(
            "---\nagent_doc_session: registry-io\n---\n\n",
            "## Todo <!-- agent:todo -->\n",
            "- [ ] [#tdmigrate] Migrate rows per [DEC-store-v1](decision:DEC-store-v1)\n",
            "- [ ] [#tdother] Rename the flag\n",
            "<!-- /agent:todo -->\n",
        )
        .to_string();
        std::fs::write(&doc, &content).unwrap();
        (dir, doc, content)
    }

    fn registry_markdown() -> String {
        let mut records = BTreeMap::new();
        for record in [
            record("DEC-store-v1", DecisionState::Superseded, "Use SQLite", &[]),
            record(
                "DEC-store-v2",
                DecisionState::Accepted,
                "Use Postgres",
                &["DEC-store-v1"],
            ),
        ] {
            records.insert(record.id.clone(), record);
        }
        agent_doc_decision_io::render_registry(&Registry { records })
    }

    #[test]
    fn build_graph_loads_the_project_registry_and_its_supersedes_links() {
        let (_dir, doc, content) = project(Some(&registry_markdown()));
        let (_, graph) = crate::answer_ledger::build_graph(&doc, &content);
        assert_eq!(graph.nodes["DEC-store-v2"].kind, NodeKind::Decision);
        assert!(graph.edges.iter().any(|edge| edge.from == "DEC-store-v2"
            && edge.kind == EdgeKind::Supersedes
            && edge.to == "DEC-store-v1"));
        let impact = graph.impact("DEC-store-v2", None).unwrap();
        assert_eq!(impact.change, "supersedes");
        assert_eq!(impact.from_answer.as_deref(), Some("Use SQLite"));
        assert!(
            impact
                .flagged
                .iter()
                .any(|row| row.node == "tdmigrate" && row.action == ImpactAction::Flag),
            "{impact:#?}"
        );
        assert!(!impact.flagged.iter().any(|row| row.node == "tdother"));
        // The registry's `[#r-…]` evidence meets the bare graph evidence node.
        let old_evidence = format!("r-{:0>12x}", "DEC-store-v1".len());
        assert!(
            impact
                .actions_taken
                .iter()
                .any(|row| row.node == old_evidence && row.action == ImpactAction::KeepOrUndo),
            "{impact:#?}"
        );
        let why = graph.why("DEC-store-v1").unwrap();
        assert_eq!(why.superseded_by, vec!["DEC-store-v2".to_string()]);
        assert_eq!(
            why.revisions[0].location.as_deref(),
            Some("decisions.md (superseded, updated 2026-11-05T00:00:00Z)")
        );
    }

    #[test]
    fn a_missing_or_malformed_registry_leaves_the_session_graph_alone() {
        let (_dir, doc, content) = project(None);
        let (_, graph) = crate::answer_ledger::build_graph(&doc, &content);
        assert!(!graph.nodes.keys().any(|id| id.starts_with("DEC-")));

        let (_dir, doc, content) = project(Some("not a registry\n"));
        let (_, graph) = crate::answer_ledger::build_graph(&doc, &content);
        assert!(!graph.nodes.keys().any(|id| id.starts_with("DEC-")));
    }

    #[test]
    fn citations_merge_copies_of_one_item_and_ignore_items_without_links() {
        let observations = agent_doc_answer_ledger::extract::observe(
            concat!(
                "## Todo <!-- agent:todo -->\n",
                "- [ ] [#tda] Apply [DEC-b](decision:DEC-b) and [DEC-a](decision:DEC-a)\n",
                "- [ ] [#tdb] Nothing cited\n",
                "<!-- /agent:todo -->\n",
            ),
            &[],
        );
        let citations = registry_citations(&observations);
        assert_eq!(citations.len(), 1, "{citations:#?}");
        assert_eq!(citations[0].item, "tda");
        assert_eq!(
            citations[0].decisions,
            vec!["DEC-a".to_string(), "DEC-b".to_string()]
        );
        assert!(citations[0].open);
    }
}
