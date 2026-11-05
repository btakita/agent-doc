//! Hot-set liveness for the answer ledger (`#decisionprune`, part 2).
//!
//! The ledger is pruned by relevance, never by age: "the release owner has the final say"
//! is as true in a year as today. Nothing is deleted. Pruning only decides
//! which decisions stay in the *hot set*, the part of the ledger the binary
//! keeps in the `state.db` hot tables and matches against on every cycle.
//! Everything else lives in the append-only archive tables and comes back on
//! demand.
//!
//! Liveness works like a mark phase over the decision graph:
//!
//! - **Roots**: a decision with an open copy (an open todo, waiting head or
//!   question key the document still asks); a decision that `applies_to` an
//!   open tracked item (todo, backlog, icebox, review row, live waiting head,
//!   live queue row); and a **standing** decision, whose latest answer states a
//!   policy ([`crate::text::is_policy_statement`]).
//! - **Marking** follows typed edges by kind name ([`liveness_flow`]), so the
//!   decision-graph kinds (`depends_on`, `derived_from`, `exception_to`, …)
//!   participate as soon as an edge set carries them.
//! - **Dormant** decisions are the rest: every item they applied to is closed,
//!   no open question names them, and no live decision depends on them. They
//!   leave the hot set. A new question that names one of their keys makes the
//!   index load them again (see [`crate::extract::Observations::lookup_keys`]),
//!   which turns them back into a root: revival is automatic.
//! - **Superseded revisions** of a live decision leave the hot set once the
//!   change was surfaced (`change_flagged`); the revision a pending
//!   `decision changed` notice still quotes stays.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

use crate::{Ledger, PriorKey, PriorRevision, RevisionRelation};

/// How liveness travels along an edge of a given kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessFlow {
    /// A live `from` keeps `to` live (`B depends_on A`: B assumes A).
    Forward,
    /// Either side keeps the other live. `applies_to` runs both ways because
    /// the ledger records it in both senses: a request `applies_to` the work
    /// it was raised for (open work still needs the answer), and a work row
    /// citing `(source: [#d])` `applies_to` the decision it carries out (the
    /// decision must stay hot while that work is open). `exception_to`,
    /// `conflicts_with` and `same_as`: one side cannot be read without the
    /// other.
    Both,
    /// The edge carries no liveness (`answers`, `restates`, `amends`,
    /// `supersedes` relate revisions of one decision).
    None,
}

/// The liveness flow of an edge kind, by its wire name. Unknown kinds carry
/// none, so a new kind never silently pins the whole graph.
pub fn liveness_flow(kind: &str) -> LivenessFlow {
    match kind {
        "depends_on" | "derived_from" | "refines" | "evidenced_by" => LivenessFlow::Forward,
        "applies_to" | "exception_to" | "conflicts_with" | "same_as" => LivenessFlow::Both,
        _ => LivenessFlow::None,
    }
}

/// Why a decision is in the hot set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "via", rename_all = "snake_case")]
pub enum LiveReason {
    /// The document still asks it (an open todo, waiting head or key).
    OpenCopy,
    /// It applies to this open tracked item.
    OpenItem(String),
    /// Its latest answer states a standing policy.
    Standing,
    /// This live decision depends on it (or it on this one, for two-way kinds).
    LiveDependent(String),
}

/// The hot / archived partition of one ledger build.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Liveness {
    /// Hot decisions and the first reason each one is live.
    pub hot: BTreeMap<String, LiveReason>,
    /// Dormant decisions: archived, still searchable, revived on demand.
    pub archived: BTreeSet<String>,
    /// Revision ids that left the hot set (superseded and already surfaced),
    /// including every revision of an archived decision.
    pub cold_revisions: BTreeSet<String>,
    /// Hot decisions the index had archived: a new question named them.
    pub revived: BTreeSet<String>,
}

impl Liveness {
    pub fn is_hot(&self, decision_id: &str) -> bool {
        self.hot.contains_key(decision_id)
    }
}

/// A typed edge given by kind name (ledger edges, or decision-graph edges a
/// caller adds).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NamedEdge {
    pub from: String,
    pub kind: String,
    pub to: String,
}

/// Index rows split into the hot tables and the append-only archive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexPartition {
    pub hot_keys: Vec<PriorKey>,
    pub hot_revisions: Vec<PriorRevision>,
    pub archived_keys: Vec<PriorKey>,
    pub archived_revisions: Vec<PriorRevision>,
}

/// Hot-store size of one build, for reports and measurement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HotStoreSize {
    pub decisions: usize,
    pub hot_decisions: usize,
    pub archived_decisions: usize,
    pub revived_decisions: usize,
    pub keys: usize,
    pub hot_keys: usize,
    pub revisions: usize,
    pub hot_revisions: usize,
    /// Gray-zone pairs the matcher compares with pruning (hot x all) and
    /// without it (all x all).
    pub gray_zone_pairs: usize,
    pub gray_zone_pairs_unpruned: usize,
}

impl Ledger {
    /// The decision index an edge endpoint names: a decision id, a revision id
    /// (`<decision>@<hash>`), or an item id one of its keys carries.
    fn endpoint_decision(&self, endpoint: &str, by_id: &BTreeMap<&str, usize>) -> Option<usize> {
        if let Some(&index) = by_id.get(endpoint) {
            return Some(index);
        }
        if let Some((decision, _)) = endpoint.split_once('@')
            && let Some(&index) = by_id.get(decision)
        {
            return Some(index);
        }
        self.key_to_decision.get(&crate::id_key(endpoint)).copied()
    }

    /// Mark the hot set from live roots over the ledger's own edges plus
    /// `extra` (decision-graph edges by kind name).
    pub(crate) fn compute_liveness(
        &self,
        open_item_ids: &BTreeSet<String>,
        prior_archived: &BTreeSet<String>,
        extra: &[NamedEdge],
    ) -> Liveness {
        let by_id: BTreeMap<&str, usize> = self
            .decisions
            .iter()
            .enumerate()
            .map(|(index, decision)| (decision.id.as_str(), index))
            .collect();
        let edges: Vec<NamedEdge> = self
            .edges
            .iter()
            .map(|edge| NamedEdge {
                from: edge.from.clone(),
                kind: edge.kind.as_str().to_string(),
                to: edge.to.clone(),
            })
            .chain(extra.iter().cloned())
            .collect();

        let mut reasons: BTreeMap<usize, LiveReason> = BTreeMap::new();
        let mut queue = VecDeque::new();
        let mut mark = |index: usize, reason: LiveReason, queue: &mut VecDeque<usize>| {
            if let std::collections::btree_map::Entry::Vacant(entry) = reasons.entry(index) {
                entry.insert(reason);
                queue.push_back(index);
            }
        };
        for (index, decision) in self.decisions.iter().enumerate() {
            if !decision.open_items.is_empty() {
                mark(index, LiveReason::OpenCopy, &mut queue);
            }
        }
        // An open tracked item at either end of a two-way edge keeps the
        // decision at the other end live, even when the item itself is not a
        // ledger question (a backlog row, a live queue row).
        for edge in &edges {
            if liveness_flow(&edge.kind) != LivenessFlow::Both {
                continue;
            }
            for (item, other) in [(&edge.to, &edge.from), (&edge.from, &edge.to)] {
                if open_item_ids.contains(&item.to_ascii_lowercase())
                    && let Some(index) = self.endpoint_decision(other, &by_id)
                {
                    mark(index, LiveReason::OpenItem(item.clone()), &mut queue);
                }
            }
        }
        for (index, decision) in self.decisions.iter().enumerate() {
            if decision
                .latest()
                .is_some_and(|latest| crate::text::is_policy_statement(&latest.answer))
            {
                mark(index, LiveReason::Standing, &mut queue);
            }
        }
        // Adjacency: live `a` keeps `b` live.
        let mut keeps: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for edge in &edges {
            let flow = liveness_flow(&edge.kind);
            if flow == LivenessFlow::None {
                continue;
            }
            let (Some(from), Some(to)) = (
                self.endpoint_decision(&edge.from, &by_id),
                self.endpoint_decision(&edge.to, &by_id),
            ) else {
                continue;
            };
            if from == to {
                continue;
            }
            keeps.entry(from).or_default().insert(to);
            if flow == LivenessFlow::Both {
                keeps.entry(to).or_default().insert(from);
            }
        }
        while let Some(index) = queue.pop_front() {
            let via = self.decisions[index].id.clone();
            for &next in keeps.get(&index).into_iter().flatten() {
                mark(next, LiveReason::LiveDependent(via.clone()), &mut queue);
            }
        }

        let mut liveness = Liveness::default();
        for (index, decision) in self.decisions.iter().enumerate() {
            match reasons.remove(&index) {
                Some(reason) => {
                    if prior_archived.contains(&decision.id) {
                        liveness.revived.insert(decision.id.clone());
                    }
                    liveness.hot.insert(decision.id.clone(), reason);
                    liveness.cold_revisions.extend(cold_revisions(decision));
                }
                None => {
                    liveness.archived.insert(decision.id.clone());
                    liveness.cold_revisions.extend(
                        decision
                            .revisions
                            .iter()
                            .map(|revision| revision.id.clone()),
                    );
                }
            }
        }
        liveness
    }

    /// Recompute the hot set with decision-graph edges a caller adds (the
    /// `decisiongraph` / `firstprinciples` kinds). Revival is relative to the
    /// index the ledger was built from.
    pub fn recompute_liveness(&mut self, open_item_ids: &BTreeSet<String>, extra: &[NamedEdge]) {
        let prior_archived = self.prior_archived.clone();
        self.liveness = self.compute_liveness(open_item_ids, &prior_archived, extra);
    }

    /// The index rows this build implies, split into the hot tables and the
    /// archive. Every row also belongs in the archive (write-through); the
    /// archived rows are the ones the hot tables drop.
    pub fn partitioned_index_rows(&self) -> IndexPartition {
        let mut partition = IndexPartition::default();
        for decision in &self.decisions {
            let (keys, revisions) = self.decision_rows(decision);
            if !self.liveness.is_hot(&decision.id) {
                // Dormant: every row goes to the archive, including the keys
                // of a question that closed unanswered.
                partition.archived_keys.extend(keys);
                partition.archived_revisions.extend(revisions);
                continue;
            }
            if !Self::stored(decision) {
                continue;
            }
            partition.hot_keys.extend(keys);
            for (revision, row) in decision.revisions.iter().zip(revisions) {
                if self.liveness.cold_revisions.contains(&revision.id) {
                    partition.archived_revisions.push(row);
                } else {
                    partition.hot_revisions.push(row);
                }
            }
        }
        partition
    }

    /// Hot-store size of this build.
    pub fn hot_store_size(&self) -> HotStoreSize {
        let partition = self.partitioned_index_rows();
        let all = self.decisions.len();
        let hot = self.liveness.hot.len();
        HotStoreSize {
            decisions: all,
            hot_decisions: hot,
            archived_decisions: self.liveness.archived.len(),
            revived_decisions: self.liveness.revived.len(),
            keys: partition.hot_keys.len() + partition.archived_keys.len(),
            hot_keys: partition.hot_keys.len(),
            revisions: partition.hot_revisions.len() + partition.archived_revisions.len(),
            hot_revisions: partition.hot_revisions.len(),
            gray_zone_pairs: gray_zone_pairs(all, hot),
            gray_zone_pairs_unpruned: all * all.saturating_sub(1) / 2,
        }
    }
}

/// Unordered pairs with at least one hot side among `all` decisions.
pub fn gray_zone_pairs(all: usize, hot: usize) -> usize {
    let cold = all.saturating_sub(hot);
    all * all.saturating_sub(1) / 2 - cold * cold.saturating_sub(1) / 2
}

/// Superseded revisions of a live decision that left the hot set: everything
/// before its latest `supersedes` revision, except the predecessor a pending
/// (not yet flagged) change notice still quotes.
fn cold_revisions(decision: &crate::Decision) -> Vec<String> {
    let Some(last_supersede) = decision
        .revisions
        .iter()
        .rposition(|revision| revision.relation == RevisionRelation::Supersedes)
    else {
        return Vec::new();
    };
    let latest = decision.revisions.len() - 1;
    let keep_from = if last_supersede == latest && !decision.revisions[latest].change_flagged {
        last_supersede.saturating_sub(1)
    } else {
        last_supersede
    };
    decision.revisions[..keep_from]
        .iter()
        .map(|revision| revision.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PriorIndex, extract, id_key};

    const FIXTURE: &str = include_str!("../tests/fixtures/release_session.md");
    const NOW: u64 = 1_800_000_000;

    fn build_with(content: &str, prior: &PriorIndex) -> Ledger {
        Ledger::build(&extract::observe(content, &[]), prior, NOW, None)
    }

    fn build(content: &str) -> Ledger {
        build_with(content, &PriorIndex::default())
    }

    fn id_of(ledger: &Ledger, item: &str) -> String {
        ledger.decision_for_key(&id_key(item)).unwrap().id.clone()
    }

    /// The prior index a pass leaves behind: the hot rows only, as the hot
    /// tables hold them after pruning.
    fn hot_prior(ledger: &Ledger) -> PriorIndex {
        let partition = ledger.partitioned_index_rows();
        PriorIndex {
            keys: partition.hot_keys,
            revisions: partition.hot_revisions,
            ..Default::default()
        }
    }

    #[test]
    fn open_copies_are_hot_and_a_closed_archived_answer_is_dormant() {
        let ledger = build(FIXTURE);
        let y2fn = id_of(&ledger, "y2fn");
        assert_eq!(ledger.liveness.hot.get(&y2fn), Some(&LiveReason::OpenCopy));
        // `#tdbranch` was answered, its todo closed and its packet archived:
        // nothing asks it or applies it any more.
        let branch = id_of(&ledger, "tdbranch");
        assert!(
            ledger.liveness.archived.contains(&branch),
            "{:?}",
            ledger.liveness
        );
        assert!(!ledger.liveness.is_hot(&branch));
        // Its rows leave the hot tables but stay in the archive partition.
        let partition = ledger.partitioned_index_rows();
        assert!(
            partition
                .hot_keys
                .iter()
                .all(|key| key.decision_id != branch)
        );
        assert!(
            partition
                .archived_keys
                .iter()
                .any(|key| key.decision_id == branch)
        );
        assert!(
            partition
                .archived_revisions
                .iter()
                .any(|revision| revision.answer == "Target sdk-dev. Keep 0.3.0-preview.1")
        );
    }

    /// Nothing is deleted: every row the unpruned index would hold is in the
    /// hot or the archived partition.
    #[test]
    fn the_partition_loses_no_row() {
        let ledger = build(FIXTURE);
        let (keys, revisions) = ledger.index_rows();
        let partition = ledger.partitioned_index_rows();
        for key in &keys {
            assert!(
                partition.hot_keys.contains(key) || partition.archived_keys.contains(key),
                "{key:?}"
            );
        }
        for revision in &revisions {
            assert!(
                partition.hot_revisions.contains(revision)
                    || partition.archived_revisions.contains(revision),
                "{revision:?}"
            );
        }
    }

    #[test]
    fn a_decision_applying_to_open_work_stays_hot() {
        // The request is answered and its own todo is closed, but the backlog
        // work it was raised for is still open.
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- ~~[#subagent-operator-request-tdport-allowlist]: Use the standard http ports.~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [x] [#subagent-operator-request-tdport-allowlist] Answer the blocking subagent question for queue item ‘do [#tdport]’ in agent:exchange: Choose how to add the demo callback (choices: stop, studio append)\n",
            "<!-- /agent:todo -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] ☑️ [#tdport] Register the demo sign-in redirect\n",
            "<!-- /agent:backlog -->\n",
        );
        let ledger = build(doc);
        let request = id_of(&ledger, "subagent-operator-request-tdport-allowlist");
        assert_eq!(
            ledger.liveness.hot.get(&request),
            Some(&LiveReason::OpenItem("tdport".into())),
            "{:?}",
            ledger.liveness
        );
        // Close the work: the answer goes dormant.
        let closed = doc.replace("- [ ] ☑️ [#tdport]", "- [x] ☑️ [#tdport]");
        let ledger = build(&closed);
        assert!(
            ledger.liveness.archived.contains(&request),
            "{:?}",
            ledger.liveness
        );
    }

    #[test]
    fn open_work_citing_its_source_decision_keeps_it_hot() {
        // The decision is answered and closed; the work that carries it out
        // cites it as its source and is still open.
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- ~~[#tdpublic]: Public and open source.~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [x] [#tdpublic] Decide whether the CLI is public and open source\n",
            "- [ ] [#tdreleaseapp] Configure the release App permissions (source: [#tdpublic])\n",
            "<!-- /agent:todo -->\n",
        );
        let ledger = build(doc);
        let public = id_of(&ledger, "tdpublic");
        assert_eq!(
            ledger.liveness.hot.get(&public),
            Some(&LiveReason::OpenItem("tdreleaseapp".into())),
            "{:?}",
            ledger.liveness
        );
        let done = doc.replace("- [ ] [#tdreleaseapp]", "- [x] [#tdreleaseapp]");
        assert!(build(&done).liveness.archived.contains(&public));
    }

    #[test]
    fn a_standing_policy_stays_hot_with_nothing_open() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- ~~[#tdbranchpolicy]: From now on, always target sdk-dev.~~\n",
            "- ~~[#tdoneoff]: Use port 8080.~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [x] [#tdbranchpolicy] Choose the target branch for SDK pull requests: sdk-dev or dev\n",
            "- [x] [#tdoneoff] Choose the demo port for the sign-in redirect: 8080 or 9090\n",
            "<!-- /agent:todo -->\n",
        );
        let ledger = build(doc);
        let policy = id_of(&ledger, "tdbranchpolicy");
        assert_eq!(
            ledger.liveness.hot.get(&policy),
            Some(&LiveReason::Standing)
        );
        assert!(
            ledger
                .liveness
                .archived
                .contains(&id_of(&ledger, "tdoneoff"))
        );
    }

    #[test]
    fn a_dormant_decision_revives_when_a_new_question_names_it() {
        let ledger = build(FIXTURE);
        let branch = id_of(&ledger, "tdbranch");
        let partition = ledger.partitioned_index_rows();
        // The document drops the archived packet; only the archive holds it.
        let start = FIXTURE.find("## Done").unwrap();
        let without = &FIXTURE[..start];
        let quiet = build_with(without, &hot_prior(&ledger));
        assert!(quiet.decision_for_key(&id_key("tdbranch")).is_none());
        // A new copy of the question appears. The archive lookup (what the
        // state.db load does for the keys the document names) returns the
        // decision's rows, marked as archived.
        let reopened = without.replace(
            "<!-- /agent:todo -->",
            "- [ ] [#tdbranch] Choose the target branch for the telemetry pull request: sdk-dev or dev\n<!-- /agent:todo -->",
        );
        let observations = extract::observe(&reopened, &[]);
        assert!(observations.lookup_keys().contains(&id_key("tdbranch")));
        let mut prior = hot_prior(&ledger);
        prior.keys.extend(
            partition
                .archived_keys
                .iter()
                .filter(|key| key.decision_id == branch)
                .cloned(),
        );
        prior.revisions.extend(
            partition
                .archived_revisions
                .iter()
                .filter(|revision| revision.decision_id == branch)
                .cloned(),
        );
        prior.archived_decisions.insert(branch.clone());
        let revived = Ledger::build(&observations, &prior, NOW, None);
        assert_eq!(id_of(&revived, "tdbranch"), branch, "stable id");
        assert!(
            revived.liveness.revived.contains(&branch),
            "{:?}",
            revived.liveness
        );
        let facts = revived.answered_facts();
        let fact = facts
            .iter()
            .find(|fact| fact.decision_id == branch)
            .expect("the revived answer is pushed again");
        assert_eq!(fact.answer, "Target sdk-dev. Keep 0.3.0-preview.1");
    }

    #[test]
    fn a_superseded_revision_goes_cold_once_the_change_was_surfaced() {
        let first = build(FIXTURE);
        let approvers = first.decision_for_key(&id_key("tdadrapprovers")).unwrap();
        let old = approvers.revisions[0].id.clone();
        // First pass: the `decision changed` notice still quotes the old one.
        assert!(!first.liveness.cold_revisions.contains(&old));
        let (keys, revisions) = first.index_rows();
        let second = build_with(
            FIXTURE,
            &PriorIndex {
                keys,
                revisions,
                ..Default::default()
            },
        );
        assert!(second.liveness.cold_revisions.contains(&old));
        let partition = second.partitioned_index_rows();
        assert!(
            partition
                .archived_revisions
                .iter()
                .any(|revision| revision.answer == "Add Alex + Sam")
        );
        assert!(
            partition
                .hot_revisions
                .iter()
                .any(|revision| revision.answer.starts_with("Keep the approver as is."))
        );
        // The decision itself stays hot and keeps its full document history.
        assert!(second.liveness.is_hot(&approvers.id));
        assert_eq!(
            second
                .answered_facts()
                .iter()
                .find(|fact| fact.decision_id == approvers.id)
                .unwrap()
                .earlier_answers,
            vec!["Add Alex + Sam".to_string()]
        );
    }

    #[test]
    fn decision_graph_edges_keep_a_dependency_hot() {
        let mut ledger = build(FIXTURE);
        let branch = id_of(&ledger, "tdbranch");
        let y2fn = id_of(&ledger, "y2fn");
        assert!(ledger.liveness.archived.contains(&branch));
        // A live decision that assumes the dormant one keeps it hot.
        let open_items = extract::observe(FIXTURE, &[]).open_item_ids;
        ledger.recompute_liveness(
            &open_items,
            &[NamedEdge {
                from: y2fn.clone(),
                kind: "depends_on".into(),
                to: branch.clone(),
            }],
        );
        assert_eq!(
            ledger.liveness.hot.get(&branch),
            Some(&LiveReason::LiveDependent(y2fn.clone()))
        );
        // The reverse direction carries nothing: a dormant dependent does not
        // pin what it assumed... and an unknown kind pins nothing.
        ledger.recompute_liveness(
            &open_items,
            &[NamedEdge {
                from: branch.clone(),
                kind: "mentions".into(),
                to: y2fn,
            }],
        );
        assert!(ledger.liveness.archived.contains(&branch));
    }

    #[test]
    fn the_gray_zone_skips_pairs_of_dormant_decisions() {
        // Two closed, answered near-duplicates: comparing them helps nobody.
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- ~~[#tdalpha]: Target sdk-dev.~~\n",
            "- ~~[#tdbeta]: Target sdk-dev too.~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [x] [#tdalpha] Choose the target branch for the telemetry pull request now\n",
            "- [x] [#tdbeta] Choose the target branch for the telemetry pull request today\n",
            "<!-- /agent:todo -->\n",
        );
        let ledger = build(doc);
        assert_eq!(ledger.liveness.hot.len(), 0, "{:?}", ledger.liveness);
        assert!(
            ledger.near_duplicates.is_empty(),
            "{:?}",
            ledger.near_duplicates
        );
        // Reopen one: the pair is compared again.
        let reopened = doc.replace("- [x] [#tdbeta]", "- [ ] [#tdbeta]");
        let ledger = build(&reopened);
        assert_eq!(
            ledger.near_duplicates.len(),
            1,
            "{:?}",
            ledger.near_duplicates
        );
        let size = ledger.hot_store_size();
        assert_eq!(
            (size.gray_zone_pairs, size.gray_zone_pairs_unpruned),
            (1, 1)
        );
    }

    #[test]
    fn gray_zone_pair_counts() {
        assert_eq!(gray_zone_pairs(28, 28), 378);
        assert_eq!(gray_zone_pairs(28, 0), 0);
        assert_eq!(gray_zone_pairs(4, 1), 3);
    }
}
