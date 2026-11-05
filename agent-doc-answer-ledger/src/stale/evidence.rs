//! What a `done_by_evidence` finding rests on (`#decisionstaleevidence`).
//!
//! The operator's answer text is one source. The others are read off the
//! decision graph and the project decision registry:
//!
//! - **Answer text**: every delivered revision of a linked decision, each
//!   sentence with an un-negated completion word ([`super::completed_deliverables`]).
//! - **Response evidence**: a response a delivered revision cites
//!   (`evidenced_by r-<hex>` on the revision), or that a closed copy of the
//!   question recorded (`evidenced_by r-<hex>` on the decision). Its text is
//!   read the same way as an answer.
//! - **Commit evidence**: a commit a closed copy recorded (`evidenced_by
//!   commit:<sha>` on the decision, a `derived` edge). A closed copy records
//!   work done, so the commit itself is done. A commit an operator answer
//!   cites is not taken on its own: the answer sentence around it decides
//!   (`abc1234 was not merged`).
//! - **Registry records**: a project `decisions.md` record the item cites by
//!   id (`DEC-…`), while it is `accepted`. Its decision and rationale text and
//!   the responses it pins as evidence are read the same way.
//!
//! [`EvidenceGroup`]s are per decision: an item is done by evidence when one
//! group covers every deliverable it names.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::extract::{Observations, TrackedItemObs};
use crate::graph::{DecisionGraph, EdgeSource};
use crate::{Decision, EdgeKind, Ledger, Revision};

use super::{StaleLink, completed_deliverables};

/// Response text by id, resolved on demand. An implementation reads the
/// session document, then the response archive; it never scans git history.
pub trait ResponseSource {
    /// The response's text for `response_id` (`r-<hex>`, lowercase, no
    /// brackets), or `None` when it cannot be found cheaply.
    fn response_text(&self, response_id: &str) -> Option<String>;
}

/// One project `decisions.md` record as the stale audit reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryRecord {
    /// `DEC-…`.
    pub id: String,
    /// `accepted`. A superseded, reversed or deprecated record is history and
    /// records nothing done.
    pub current: bool,
    pub decision: String,
    pub rationale: String,
    pub updated_at: String,
    /// Response evidence ids, `[#r-<hex>]` or `r-<hex>`.
    pub evidence_response_ids: Vec<String>,
}

/// Evidence beyond the answer text, supplied by the IO layer. The default
/// carries none: the graph is then built from the ledger alone, with no
/// cached history, no response text and no registry.
#[derive(Default)]
pub struct DoneEvidence<'a> {
    /// The decision graph with cached history ([`DecisionGraph::merge_prior`]).
    pub graph: Option<&'a DecisionGraph>,
    /// Project `decisions.md` records by id.
    pub records: BTreeMap<String, RegistryRecord>,
    pub responses: Option<&'a dyn ResponseSource>,
}

/// `r-<hex>` for `[#r-<hex>]`, `#r-<hex>` or `r-<hex>`.
pub fn response_key(id: &str) -> String {
    id.trim()
        .trim_start_matches("[#")
        .trim_start_matches('#')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

/// `DEC-…` registry ids `text` cites, bare or as `decision:DEC-…`.
pub fn registry_citations(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (index, _) in text.match_indices("DEC-") {
        let joined = text[..index]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if joined {
            continue;
        }
        let value = &text[index..];
        let end = value
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .unwrap_or(value.len());
        let id = value[..end].trim_end_matches(['.', '-', '_']);
        if id.len() > "DEC-".len() {
            found.insert(id.to_string());
        }
    }
    found
}

/// One source inside a group and what it records done.
struct Source {
    /// The text quoted as the decision's answer in the finding.
    answer: String,
    revision_id: String,
    location: String,
    /// The operator's own answer text (preferred as the citation).
    answer_text: bool,
    done: BTreeSet<String>,
}

/// Every source linked to one decision or registry record.
pub(super) struct EvidenceGroup {
    pub decision_id: String,
    pub link: StaleLink,
    sources: Vec<Source>,
}

/// The source a finding cites.
pub(super) struct Cited {
    pub revision_id: String,
    pub answer: String,
    pub location: String,
}

impl EvidenceGroup {
    /// Everything the group records done.
    pub fn done(&self) -> BTreeSet<String> {
        self.sources
            .iter()
            .flat_map(|source| source.done.iter().cloned())
            .collect()
    }

    /// The source to cite for `wanted`: the latest operator answer that
    /// records one of them, else the latest other source that does.
    pub fn cited(&self, wanted: &BTreeSet<String>) -> Option<Cited> {
        let records = |source: &&Source| source.done.iter().any(|anchor| wanted.contains(anchor));
        let source = self
            .sources
            .iter()
            .filter(|source| source.answer_text)
            .rfind(records)
            .or_else(|| self.sources.iter().rfind(records))?;
        Some(Cited {
            revision_id: source.revision_id.clone(),
            answer: source.answer.clone(),
            location: source.location.clone(),
        })
    }
}

impl DoneEvidence<'_> {
    fn response_done(&self, response_id: &str) -> Option<BTreeSet<String>> {
        let text = self.responses?.response_text(&response_key(response_id))?;
        let done = completed_deliverables(&text);
        (!done.is_empty()).then_some(done)
    }

    fn response_ids<'g>(
        graph: &'g DecisionGraph,
        from: &'g str,
    ) -> impl Iterator<Item = &'g str> + 'g {
        graph
            .outgoing(from, EdgeKind::EvidencedBy)
            .map(|edge| edge.to.as_str())
            .filter(|to| to.starts_with("r-"))
    }

    fn decision_group(
        &self,
        graph: &DecisionGraph,
        decision: &Decision,
        link: StaleLink,
    ) -> EvidenceGroup {
        let mut sources = Vec::new();
        let delivered: Vec<&Revision> = decision
            .revisions
            .iter()
            .filter(|revision| super::delivered(revision))
            .collect();
        for revision in &delivered {
            sources.push(Source {
                answer: revision.answer.clone(),
                revision_id: revision.id.clone(),
                location: revision.location.clone(),
                answer_text: true,
                done: completed_deliverables(&revision.answer),
            });
            for response in Self::response_ids(graph, &revision.id) {
                if let Some(done) = self.response_done(response) {
                    sources.push(Source {
                        answer: revision.answer.clone(),
                        revision_id: revision.id.clone(),
                        location: format!("response [#{response}] cited by {}", revision.location),
                        answer_text: false,
                        done,
                    });
                }
            }
        }
        // Actions a closed copy recorded are evidence on the decision itself.
        if let Some(anchor) = delivered.last().copied().or_else(|| decision.latest()) {
            for edge in graph.outgoing(&decision.id, EdgeKind::EvidencedBy) {
                let done = if let Some(sha) = edge.to.strip_prefix("commit:") {
                    if edge.source != EdgeSource::Derived || sha.len() < 7 {
                        continue;
                    }
                    BTreeSet::from([format!("sha:{}", &sha[..7])])
                } else if edge.to.starts_with("r-") {
                    match self.response_done(&edge.to) {
                        Some(done) => done,
                        None => continue,
                    }
                } else {
                    continue;
                };
                let what = match edge.to.strip_prefix("commit:") {
                    Some(sha) => format!("commit {sha}"),
                    None => format!("response [#{}]", edge.to),
                };
                sources.push(Source {
                    answer: anchor.answer.clone(),
                    revision_id: anchor.id.clone(),
                    location: format!("{what} recorded on a closed copy"),
                    answer_text: false,
                    done,
                });
            }
        }
        EvidenceGroup {
            decision_id: decision.id.clone(),
            link,
            sources,
        }
    }

    fn registry_group(&self, record: &RegistryRecord) -> EvidenceGroup {
        let revision_id = format!("{}@{}", record.id, record.updated_at);
        let mut sources = vec![Source {
            answer: record.decision.clone(),
            revision_id: revision_id.clone(),
            location: format!("decisions.md {}", record.id),
            answer_text: true,
            done: completed_deliverables(&format!("{}\n{}", record.decision, record.rationale)),
        }];
        for response in &record.evidence_response_ids {
            if let Some(done) = self.response_done(response) {
                sources.push(Source {
                    answer: record.decision.clone(),
                    revision_id: revision_id.clone(),
                    location: format!(
                        "response [#{}] pinned by decisions.md {}",
                        response_key(response),
                        record.id
                    ),
                    answer_text: false,
                    done,
                });
            }
        }
        EvidenceGroup {
            decision_id: record.id.clone(),
            link: StaleLink::Registry,
            sources,
        }
    }

    /// Every evidence group for `item`: its linked ledger decisions, then the
    /// current registry records it cites.
    pub(super) fn groups(
        &self,
        ledger: &Ledger,
        observations: &Observations,
        local_graph: &OnceCell<DecisionGraph>,
        item: &TrackedItemObs,
        linked: &[(&Decision, StaleLink)],
    ) -> Vec<EvidenceGroup> {
        let mut groups = Vec::new();
        if !linked.is_empty() {
            let graph = match self.graph {
                Some(graph) => graph,
                None => local_graph.get_or_init(|| DecisionGraph::build(ledger, observations)),
            };
            for (decision, link) in linked {
                groups.push(self.decision_group(graph, decision, *link));
            }
        }
        for id in registry_citations(&item.text) {
            if let Some(record) = self.records.get(&id).filter(|record| record.current) {
                groups.push(self.registry_group(record));
            }
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_citations_are_whole_dec_ids() {
        let found = registry_citations(
            "Ship it per DEC-auth-001. See decision:DEC-cache_2 and not XDEC-nope, (DEC-x.y).",
        );
        assert_eq!(
            found.into_iter().collect::<Vec<_>>(),
            vec!["DEC-auth-001", "DEC-cache_2", "DEC-x.y"]
        );
        assert!(registry_citations("DEC- alone").is_empty());
    }

    #[test]
    fn response_keys_drop_brackets_and_case() {
        assert_eq!(response_key("[#r-0123456789AB]"), "r-0123456789ab");
        assert_eq!(response_key("r-0123456789ab"), "r-0123456789ab");
    }
}
