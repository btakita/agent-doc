//! Rebuild the answer-ledger index from a document's committed versions
//! (`#decisionidstamp`).
//!
//! The state.db index remembers an answer once a ledger pass has seen it, so
//! an answer whose row was pruned before the first index write (a compacted
//! done archive, a consumed queue row the binary later dropped) was lost. Git
//! still has it. [`seed_index`] replays the committed versions the caller
//! read, oldest first, together with the current document, and returns the
//! index rows the ledger would hold had it watched the document all along:
//!
//! - every question key and every answer the history shows, under the same
//!   decision grouping a current build uses (the existing index and the
//!   current document's stamps are inputs, so decision ids do not move);
//! - first-seen times from the commit that first shows a key or an answer;
//! - answers no longer in the document, located as
//!   `git history <rev>, under [#id]`. Their change, if any, predates the
//!   ledger, so it is recorded as already surfaced.
//!
//! The caller stores the rows once per document; afterwards the ordinary
//! index keeps them current.

use std::collections::{BTreeMap, BTreeSet};

use crate::extract::{self, Observations, QuestionSource};
use crate::{Edge, Ledger, PriorIndex, answer_hash, answer_key};

/// One committed version of the document.
#[derive(Debug, Clone, Copy)]
pub struct HistoryVersion<'a> {
    /// Committer time, Unix seconds.
    pub committed_at: u64,
    /// Short revision name shown in a recovered answer's location.
    pub revision: &'a str,
    pub content: &'a str,
}

/// The rows [`seed_index`] produced.
#[derive(Debug, Clone, Default)]
pub struct HistorySeed {
    pub index: PriorIndex,
    pub edges: Vec<Edge>,
    /// Answers the history holds that the current document no longer shows.
    pub recovered: usize,
}

fn source_rank(source: QuestionSource) -> u8 {
    match source {
        QuestionSource::Todo => 0,
        QuestionSource::Waiting => 1,
        QuestionSource::Request => 2,
    }
}

#[derive(Default)]
struct Merge {
    observations: Observations,
    questions: BTreeSet<(Option<String>, String, u8)>,
    answers: BTreeSet<(Vec<String>, Option<String>, String)>,
    links: BTreeSet<(String, String, String)>,
    key_seen: BTreeMap<String, u64>,
    answer_seen: BTreeMap<(String, String), u64>,
}

impl Merge {
    /// Fold one observation set in. `historical` closes its questions and
    /// marks its answers consumed, located at `revision`.
    fn fold(&mut self, observed: Observations, at: u64, revision: Option<&str>) {
        for mut question in observed.questions {
            for key in crate::question_keys(&question) {
                let seen = self.key_seen.entry(key).or_insert(at);
                *seen = (*seen).min(at);
            }
            let identity = (
                question.id.clone(),
                question.text.clone(),
                source_rank(question.source),
            );
            if revision.is_some() {
                question.open = false;
            }
            if self.questions.insert(identity) {
                self.observations.questions.push(question);
            }
        }
        for mut answer in observed.answers {
            let hash = answer_hash(&answer.text);
            if let Some(key) = answer_key(&answer) {
                let seen = self.answer_seen.entry((key, hash.clone())).or_insert(at);
                *seen = (*seen).min(at);
            }
            let identity = (answer.targets.clone(), answer.head_text.clone(), hash);
            if !self.answers.insert(identity) {
                continue;
            }
            if let Some(revision) = revision {
                answer.location = format!("git history {revision}, {}", answer.location);
                answer.consumed = true;
                answer.in_own_packet = false;
            }
            self.observations.answers.push(answer);
        }
        for link in observed.links {
            let identity = (
                link.from.clone(),
                link.to.clone(),
                format!("{:?}", link.kind),
            );
            if self.links.insert(identity) {
                self.observations.links.push(link);
            }
        }
    }
}

/// Index rows for `versions` (any order) plus the current document.
pub fn seed_index(
    versions: &[HistoryVersion<'_>],
    current: &Observations,
    prior: &PriorIndex,
    now: u64,
) -> HistorySeed {
    let current_answers: BTreeSet<(String, String)> = current
        .answers
        .iter()
        .filter_map(|answer| answer_key(answer).map(|key| (key, answer_hash(&answer.text))))
        .collect();
    let mut ordered: Vec<&HistoryVersion<'_>> = versions.iter().collect();
    ordered.sort_by_key(|version| version.committed_at);
    let mut merge = Merge::default();
    // The current document first, so its rows (open state, live locations)
    // win the de-duplication; the history only adds what it no longer shows
    // and earlier first-seen times.
    merge.fold(current.clone(), now, None);
    for version in ordered {
        merge.fold(
            extract::observe(version.content, &[]),
            version.committed_at,
            Some(version.revision),
        );
    }
    merge.observations.live_queue_ids = current.live_queue_ids.clone();
    let ledger = Ledger::build(&merge.observations, prior, now, None);

    let (mut keys, mut revisions) = ledger.index_rows();
    for key in &mut keys {
        if let Some(&seen) = merge.key_seen.get(&key.key) {
            key.first_seen_at = key.first_seen_at.min(seen);
        }
    }
    let decision_keys: BTreeMap<&str, &BTreeSet<String>> = ledger
        .decisions
        .iter()
        .map(|decision| (decision.id.as_str(), &decision.keys))
        .collect();
    let mut recovered = 0;
    for revision in &mut revisions {
        let hash = answer_hash(&revision.answer);
        let Some(keys) = decision_keys.get(revision.decision_id.as_str()) else {
            continue;
        };
        if let Some(seen) = keys
            .iter()
            .filter_map(|key| merge.answer_seen.get(&(key.clone(), hash.clone())))
            .min()
        {
            revision.first_seen_at = revision.first_seen_at.min(*seen);
        }
        let in_document = keys
            .iter()
            .any(|key| current_answers.contains(&(key.clone(), hash.clone())));
        if !in_document {
            revision.change_flagged = true;
            if revision.location.starts_with("git history ") {
                recovered += 1;
            }
        } else {
            // `index_rows` marks the latest superseding revision flagged
            // because the pass that writes it also reports it. The seed
            // reports nothing, so a live revision keeps the flag the index
            // already held; the ordinary pass flags (and reports) the change
            // once (`#decisiongraph`).
            revision.change_flagged = prior.revisions.iter().any(|old| {
                old.decision_id == revision.decision_id
                    && old.change_flagged
                    && answer_hash(&old.answer) == hash
            });
        }
    }
    HistorySeed {
        index: PriorIndex {
            keys,
            revisions,
            ..Default::default()
        },
        edges: ledger.edges.into_iter().collect(),
        recovered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PriorIndex, id_key};

    const ANSWERED: &str = concat!(
        "<!-- agent:queue -->\n",
        "- ~~[#tdport]: Use port 3000.~~\n",
        "<!-- /agent:queue -->\n",
        "<!-- agent:todo -->\n",
        "- [ ] [#tdport] Choose the demo callback port for local sign-in runs\n",
        "<!-- /agent:todo -->\n",
    );

    /// The consumed queue row that held the answer was dropped before any
    /// index write. The history still has it: the seed records it with the
    /// commit's time, and a later build reports it for the open copy.
    #[test]
    fn an_answer_pruned_before_the_first_index_write_is_recovered() {
        let pruned = ANSWERED.replace("- ~~[#tdport]: Use port 3000.~~\n", "");
        let current = extract::observe(&pruned, &[]);
        let versions = [
            HistoryVersion {
                committed_at: 100,
                revision: "1111111",
                content: ANSWERED,
            },
            HistoryVersion {
                committed_at: 200,
                revision: "2222222",
                content: &pruned,
            },
        ];
        let seed = seed_index(&versions, &current, &PriorIndex::default(), 1_000);
        assert_eq!(seed.recovered, 1, "{seed:?}");
        let revision = &seed.index.revisions[0];
        assert_eq!(revision.answer, "Use port 3000.");
        assert_eq!(revision.first_seen_at, 100);
        assert!(
            revision.location.starts_with("git history 1111111"),
            "{revision:?}"
        );
        assert!(revision.change_flagged, "a pre-ledger change is not news");
        let key = seed
            .index
            .keys
            .iter()
            .find(|key| key.key == id_key("tdport"))
            .unwrap();
        assert_eq!(key.first_seen_at, 100);

        let ledger = Ledger::build(&current, &seed.index, 1_000, None);
        let facts = ledger.answered_facts();
        let fact = facts
            .iter()
            .find(|fact| fact.open_items.contains(&"tdport".to_string()))
            .expect("recovered answer reaches the open copy");
        assert_eq!(fact.answer, "Use port 3000.");
        assert!(
            fact.location.contains("no longer in the document"),
            "{fact:?}"
        );
    }

    /// The seed keeps the decision id the existing index already gave a key.
    /// A live `change:` is news on the first pass: the seed must not mark it
    /// already flagged, or the contract never says "decision changed".
    #[test]
    fn the_seed_leaves_a_live_change_unflagged() {
        let changed = concat!(
            "<!-- agent:queue -->\n",
            "- ~~[#tdport]: Use port 3000.~~\n",
            "- [#tdport]: change: Use the standard http ports.\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#tdport] Choose the demo callback port for local sign-in runs\n",
            "<!-- /agent:todo -->\n",
        );
        let current = extract::observe(changed, &[]);
        let versions = [HistoryVersion {
            committed_at: 100,
            revision: "1111111",
            content: changed,
        }];
        let seed = seed_index(&versions, &current, &PriorIndex::default(), 1_000);
        assert_eq!(seed.index.revisions.len(), 2, "{seed:?}");
        assert!(
            seed.index
                .revisions
                .iter()
                .all(|revision| !revision.change_flagged),
            "{seed:?}"
        );
        let ledger = Ledger::build(&current, &seed.index, 1_000, None);
        let fact = &ledger.answered_facts()[0];
        assert_eq!(fact.changed_from.as_deref(), Some("Use port 3000."));
    }

    #[test]
    fn the_seed_keeps_existing_decision_ids() {
        let current = extract::observe(ANSWERED, &[]);
        let prior = PriorIndex {
            keys: vec![crate::PriorKey {
                key: id_key("tdport"),
                decision_id: "dec-kept".into(),
                first_seen_at: 500,
            }],
            revisions: Vec::new(),
            ..Default::default()
        };
        let versions = [HistoryVersion {
            committed_at: 100,
            revision: "1111111",
            content: ANSWERED,
        }];
        let seed = seed_index(&versions, &current, &prior, 1_000);
        assert_eq!(seed.recovered, 0);
        assert!(
            seed.index
                .keys
                .iter()
                .all(|key| key.decision_id == "dec-kept"),
            "{seed:?}"
        );
        let revision = &seed.index.revisions[0];
        assert_eq!(revision.first_seen_at, 100, "history moves first-seen back");
        assert!(
            !revision.change_flagged,
            "a live answer is flagged by the ordinary pass"
        );
        assert_eq!(revision.location, "consumed queue row");
    }
}
