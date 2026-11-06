//! Project decision registry in the decision graph (`#decisionregistrygraph`).
//!
//! The project-level `decisions.md` (`agent-doc-decision-io`) holds settled
//! `DEC-…` records, each with one current text, its response evidence and
//! the records it supersedes. [`DecisionGraph::overlay_registry`] loads them
//! next to a session document's answer-ledger decisions so `decision impact`
//! and `decision why` cover them too:
//!
//! - each record is a `decision` node `DEC-…` with one `revision`
//!   `DEC-…@<hash>` (its current text; the location names `decisions.md` and
//!   the record state) that `answers` it and is `evidenced_by` its `r-<hex>`
//!   responses (the registry's `[#r-<hex>]` form, unbracketed, so it meets
//!   the same evidence node a ledger answer citing that response does);
//! - a record's `Supersedes:` list becomes decision-level `supersedes` edges
//!   `DEC-new → DEC-old` (the ledger's revision-level `supersedes` edges stay
//!   revision → revision; the two never share endpoints);
//! - a session item that cites a record (`[DEC-x](decision:DEC-x)`) gets an
//!   `applies_to` edge to it, and a bracket citation `[#DEC-x]` (which the
//!   ledger lowercases to an item placeholder `dec-x`) is folded onto the
//!   record node.
//!
//! The registry stays the authority: the overlay is applied on read only and
//! never written into the per-document `state.db` cache.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    DecisionGraph, Dependent, EdgeSource, GraphEdge, ImpactAction, ImpactRow, Node, NodeKind,
    WhyPremise,
};
use crate::EdgeKind;

/// Location prefix of every registry node.
pub const REGISTRY_LOCATION: &str = "decisions.md";

/// One `decisions.md` record, as the graph needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryDecision {
    /// `DEC-…`, verbatim.
    pub id: String,
    /// `accepted`, `superseded`, `reversed` or `deprecated`.
    pub state: String,
    pub decision: String,
    pub rationale: String,
    /// Response lookup ids, as the registry stores them (`[#r-<hex>]`) or
    /// bare (`r-<hex>`).
    pub evidence_response_ids: Vec<String>,
    pub updated_at: String,
    /// `DEC-…` ids this record supersedes.
    pub supersedes: Vec<String>,
}

/// A session item that cites registry records (`decision:DEC-…`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryCitation {
    /// The item's own id, lowercase.
    pub item: String,
    pub label: String,
    pub open: bool,
    /// Cited `DEC-…` ids.
    pub decisions: Vec<String>,
}

/// The revision id of a record's current text.
pub fn registry_revision_id(record: &RegistryDecision) -> String {
    format!(
        "{}@{}",
        record.id,
        &agent_doc_hash::content_hash(record.decision.trim())[..10]
    )
}

/// Where a record's current text is given: `decisions.md (<state>, updated <ts>)`.
pub fn registry_revision_location(record: &RegistryDecision) -> String {
    format!(
        "{REGISTRY_LOCATION} ({}, updated {})",
        record.state, record.updated_at
    )
}

impl DecisionGraph {
    /// Add the project registry's records, their `supersedes` links and the
    /// session items citing them. Idempotent.
    pub fn overlay_registry(
        &mut self,
        records: &[RegistryDecision],
        citations: &[RegistryCitation],
    ) {
        let known: BTreeSet<&str> = records.iter().map(|record| record.id.as_str()).collect();
        for record in records {
            self.node(Node {
                id: record.id.clone(),
                kind: NodeKind::Decision,
                label: record.decision.clone(),
                open: None,
                location: Some(REGISTRY_LOCATION.to_string()),
                first_seen_at: None,
                in_document: true,
            });
            let revision = registry_revision_id(record);
            self.node(Node {
                id: revision.clone(),
                kind: NodeKind::Revision,
                label: record.decision.clone(),
                open: None,
                location: Some(registry_revision_location(record)),
                first_seen_at: None,
                in_document: true,
            });
            self.edge(GraphEdge {
                from: revision.clone(),
                kind: EdgeKind::Answers,
                to: record.id.clone(),
                source: EdgeSource::Derived,
                clause: None,
            });
            for evidence in &record.evidence_response_ids {
                // The graph's evidence node is the bare `r-<hex>`.
                let evidence = evidence
                    .trim()
                    .trim_start_matches("[#")
                    .trim_end_matches(']')
                    .to_string();
                self.placeholder(&evidence, NodeKind::Evidence);
                self.edge(GraphEdge {
                    from: revision.clone(),
                    kind: EdgeKind::EvidencedBy,
                    to: evidence,
                    source: EdgeSource::Derived,
                    clause: None,
                });
            }
        }
        for record in records {
            // A dangling target is `decision validate`'s finding, not a node.
            for old in record
                .supersedes
                .iter()
                .filter(|id| known.contains(id.as_str()))
            {
                self.edge(GraphEdge {
                    from: record.id.clone(),
                    kind: EdgeKind::Supersedes,
                    to: old.clone(),
                    source: EdgeSource::Derived,
                    clause: None,
                });
            }
        }
        self.fold_bracket_citations(&known);
        for citation in citations {
            let cited: Vec<&String> = citation
                .decisions
                .iter()
                .filter(|id| known.contains(id.as_str()))
                .collect();
            if cited.is_empty() {
                continue;
            }
            self.node(Node {
                id: citation.item.clone(),
                kind: NodeKind::Item,
                label: citation.label.clone(),
                open: Some(citation.open),
                location: None,
                first_seen_at: None,
                in_document: true,
            });
            for decision in cited {
                self.edge(GraphEdge {
                    from: citation.item.clone(),
                    kind: EdgeKind::AppliesTo,
                    to: decision.clone(),
                    source: EdgeSource::Derived,
                    clause: None,
                });
            }
        }
        self.finish();
    }

    /// Move edges on a lowercased bracket placeholder (`[#DEC-x]` read as the
    /// item `dec-x`) onto the record node `DEC-x`, and drop the placeholder.
    fn fold_bracket_citations(&mut self, known: &BTreeSet<&str>) {
        let aliases: BTreeMap<String, String> = known
            .iter()
            .map(|id| (id.to_ascii_lowercase(), (*id).to_string()))
            .filter(|(lower, id)| {
                lower != id
                    && self
                        .nodes
                        .get(lower)
                        .is_some_and(|node| node.kind == NodeKind::Item)
            })
            .collect();
        if aliases.is_empty() {
            return;
        }
        let edges = std::mem::take(&mut self.edges);
        for mut edge in edges {
            if let Some(id) = aliases.get(&edge.from) {
                edge.from = id.clone();
            }
            if let Some(id) = aliases.get(&edge.to) {
                edge.to = id.clone();
            }
            self.edge(edge);
        }
        for alias in aliases.keys() {
            self.nodes.remove(alias);
        }
    }

    /// Decisions `decision` replaces outright: decision-level `supersedes`
    /// edges (a registry record's `Supersedes:` list).
    pub fn superseded_decisions(&self, decision: &str) -> Vec<String> {
        self.outgoing(decision, EdgeKind::Supersedes)
            .filter(|edge| self.is_decision(&edge.to))
            .map(|edge| edge.to.clone())
            .collect()
    }

    /// Decisions that replace `decision` outright.
    pub fn superseding_decisions(&self, decision: &str) -> Vec<String> {
        self.incoming(decision, EdgeKind::Supersedes)
            .filter(|edge| self.is_decision(&edge.from))
            .map(|edge| edge.from.clone())
            .collect()
    }

    fn is_decision(&self, id: &str) -> bool {
        self.nodes
            .get(id)
            .is_some_and(|node| node.kind == NodeKind::Decision)
    }

    /// The decision-level supersede edge `decision → old`, for its source.
    pub(super) fn decision_supersede_source(&self, decision: &str) -> Option<EdgeSource> {
        self.outgoing(decision, EdgeKind::Supersedes)
            .filter(|edge| self.is_decision(&edge.to))
            .map(|edge| edge.source)
            .max()
    }

    /// Dependents of `decision` plus those of every decision it supersedes
    /// outright; a replaced decision's dependents name it first in `via`.
    pub(super) fn dependents_with_replaced(
        &self,
        decision: &str,
        replaced: &[String],
    ) -> Vec<Dependent> {
        let mut out = self.dependents(decision);
        let mut seen: BTreeSet<String> = out.iter().map(|d| d.node.clone()).collect();
        seen.insert(decision.to_string());
        seen.extend(replaced.iter().cloned());
        for old in replaced {
            for mut dependent in self.dependents(old) {
                if !seen.insert(dependent.node.clone()) {
                    continue;
                }
                dependent.via.insert(0, old.clone());
                out.push(dependent);
            }
        }
        out
    }

    /// Evidence of the decisions `replaced` (their revisions' and their own),
    /// as actions already taken on the old answers.
    pub(super) fn replaced_decision_evidence(
        &self,
        replaced: &[String],
        seen: &mut BTreeSet<String>,
    ) -> Vec<ImpactRow> {
        let mut rows = Vec::new();
        for old in replaced {
            let sources: Vec<String> = self
                .revisions_of(old)
                .into_iter()
                .rev()
                .map(|node| node.id.clone())
                .chain(std::iter::once(old.clone()))
                .collect();
            for source in sources {
                for edge in self.outgoing(&source, EdgeKind::EvidencedBy) {
                    if seen.insert(edge.to.clone())
                        && let Some(node) = self.nodes.get(&edge.to)
                    {
                        rows.push(ImpactRow {
                            node: node.id.clone(),
                            kind: node.kind,
                            label: node.label.clone(),
                            open: node.open,
                            relation: EdgeKind::EvidencedBy,
                            via: vec![source.clone()],
                            action: ImpactAction::KeepOrUndo,
                        });
                    }
                }
            }
        }
        rows
    }

    /// The decisions `decision` supersedes outright, with their texts.
    pub(super) fn why_supersedes(&self, decision: &str) -> Vec<WhyPremise> {
        self.superseded_decisions(decision)
            .into_iter()
            .filter_map(|id| {
                let node = self.nodes.get(&id)?;
                Some(WhyPremise {
                    id: node.id.clone(),
                    kind: node.kind,
                    label: node.label.clone(),
                    answer: self.revisions_of(&id).last().map(|r| r.label.clone()),
                    via: Vec::new(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
