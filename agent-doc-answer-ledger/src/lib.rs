//! Per-document operator answer ledger (`#answerbyrequestid`).
//!
//! An operator question is often rendered as several copies: an `agent:todo`
//! row, its Waiting-on-you item, a blocking request packet raised for it, and
//! that packet's own todo. An answer typed under one copy used to count for
//! that copy only, so agents kept asking the others. This crate turns every
//! operator answer the document holds into a durable decision record, keyed by
//! every id and the normalized question text of every copy, and derives the
//! facts the binary enforces at its write boundaries:
//!
//! - [`Ledger::answered_facts`]: "already answered" facts for each open copy,
//!   injected into the cycle contract (push, not pull).
//! - [`Ledger::duplicate_closures`]: duplicate open todo copies the binary
//!   closes itself, citing the answer.
//! - [`Ledger::reasks`]: questions a closeout is about to add that the
//!   operator already answered; the closeout refuses them unless they cite the
//!   recorded answer.
//!
//! The document (plus its done archives) is the authority. The state.db index
//! ([`PriorIndex`]) is a rebuildable cache that keeps first-seen times, stable
//! decision ids, key grouping, and answers whose source rows were pruned.
//!
//! ## Matching tiers (precision first)
//!
//! 1. **Identity** (acts): the same id on two rows; the same normalized
//!    question text (at least four tokens); an operator `same as [#id]` reply;
//!    a request linked to a *decision* item (graft, id infix, or packet
//!    `queue_item`). A request linked to a *work* item only `applies_to` it.
//! 2. **Subject anchors** (acts only with an id overlap): same decision kind
//!    and matching anchors (ADR numbers, PR/issue numbers, versions, URLs,
//!    paths, option sets), plus a shared referenced id. Without the id overlap
//!    it is annotated only.
//! 3. **Normalized text similarity** (annotates only): token Jaccard at or
//!    above [`text::TEXT_SIMILARITY_THRESHOLD`] with no conflicting numbers.
//! 4. **Semantic neighbours** (annotates only): an optional
//!    [`SemanticNeighbours`] implementation (for example a tsift embedding
//!    index). Absent, the ledger degrades to tiers 1-3.
//!
//! The data model is graph-ready: decisions and revisions have stable ids, and
//! every relation is a typed [`Edge`] (`answers`, `supersedes`, `amends`,
//! `restates`, `applies_to`, `same_as`).

pub mod extract;
pub mod liveness;
pub mod text;

pub use liveness::{HotStoreSize, IndexPartition, LiveReason, Liveness, NamedEdge};

use std::collections::{BTreeMap, BTreeSet};

use extract::{LinkKind, Observations, QuestionObs, QuestionSource, REQUEST_ID_PREFIX};
use serde::{Deserialize, Serialize};

/// How a revision relates to the revision before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionRelation {
    Initial,
    /// Same decision in other words (`JWT makes this moot` / `JWS makes the
    /// cap moot`).
    Restates,
    /// Keeps the decision and adds or changes a clause.
    Amends,
    /// An outright change; the earlier answer no longer holds.
    Supersedes,
}

impl RevisionRelation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Restates => "restates",
            Self::Amends => "amends",
            Self::Supersedes => "supersedes",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Answers,
    Restates,
    Amends,
    Supersedes,
    AppliesTo,
    SameAs,
}

impl EdgeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Answers => "answers",
            Self::Restates => "restates",
            Self::Amends => "amends",
            Self::Supersedes => "supersedes",
            Self::AppliesTo => "applies_to",
            Self::SameAs => "same_as",
        }
    }
}

/// A typed relation `(from, kind, to)` between decisions, revisions and items.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Edge {
    pub from: String,
    pub kind: EdgeKind,
    pub to: String,
}

/// Which matcher tier produced a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchTier {
    Identity,
    OperatorMerge,
    AnchorWithId,
    Anchor,
    Text,
    Semantic,
}

impl MatchTier {
    /// Only these tiers may close a copy or refuse a closeout.
    pub const fn acts(self) -> bool {
        matches!(
            self,
            Self::Identity | Self::OperatorMerge | Self::AnchorWithId
        )
    }
}

/// One answer revision of a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Revision {
    /// Stable id `<decision>@<answer-hash>`.
    pub id: String,
    pub answer: String,
    pub location: String,
    /// Unix seconds the binary first observed this answer (from the index).
    pub first_seen_at: Option<u64>,
    pub relation: RevisionRelation,
    /// Struck or archived source row.
    pub consumed: bool,
    /// False when only the index still remembers it.
    pub in_document: bool,
    #[serde(skip)]
    pub change_flagged: bool,
}

/// One decision: every copy of one question and its answer revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub id: String,
    pub question: String,
    /// Unique canonical question keys (`id:<id>`, `qt:<hash>`).
    pub keys: BTreeSet<String>,
    pub revisions: Vec<Revision>,
    /// Open copies (todo / waiting ids, or quoted text for id-less rows).
    pub open_items: Vec<String>,
    /// Open `agent:todo` ids among the copies.
    #[serde(skip)]
    pub open_todos: Vec<String>,
    pub tier: MatchTier,
}

impl Decision {
    pub fn latest(&self) -> Option<&Revision> {
        self.revisions.last()
    }
}

/// A key the index already mapped to a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorKey {
    pub key: String,
    pub decision_id: String,
    pub first_seen_at: u64,
}

/// An answer revision the index remembers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorRevision {
    pub decision_id: String,
    pub answer: String,
    pub location: String,
    pub relation: String,
    pub first_seen_at: u64,
    pub change_flagged: bool,
}

/// The state.db index content for one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PriorIndex {
    pub keys: Vec<PriorKey>,
    pub revisions: Vec<PriorRevision>,
    /// Decisions these rows hold only because the archive was searched for a
    /// key the document names (`#decisionprune`); hot again means revived.
    pub archived_decisions: BTreeSet<String>,
}

/// Tier 4 hook. An implementation may consult an embedding index (tsift);
/// it only ever annotates.
pub trait SemanticNeighbours {
    /// Similarity in `[0, 1]` when `a` and `b` are near neighbours.
    fn similarity(&self, a: &str, b: &str) -> Option<f64>;
}

/// A gray-zone candidate: possibly the same question as an answered one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PossibleMatch {
    pub open_items: Vec<String>,
    pub question: String,
    pub decision_id: String,
    pub answer: String,
    pub location: String,
    pub tier: MatchTier,
    pub score: Option<f64>,
}

/// A cycle-contract fact for one answered decision with open copies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnsweredFact {
    pub decision_id: String,
    pub open_items: Vec<String>,
    pub question: String,
    /// Latest operator answer, verbatim.
    pub answer: String,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_at: Option<u64>,
    /// Earlier answers, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earlier_answers: Vec<String>,
    /// Set once when the latest answer supersedes the previous one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_from: Option<String>,
    pub tier: MatchTier,
}

/// A duplicate open todo copy the binary closes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateClosure {
    pub todo_id: String,
    pub decision_id: String,
    pub answer: String,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_open: Option<String>,
}

/// A question a closeout tries to add although the operator answered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reask {
    pub question_id: Option<String>,
    pub question: String,
    pub decision_id: String,
    pub answer: String,
    pub location: String,
    pub first_seen_at: Option<u64>,
    pub tier: MatchTier,
}

/// One question a closeout is about to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewQuestion {
    pub question: QuestionObs,
    /// Everything the closeout says alongside it (for the citation check).
    pub full_text: String,
}

/// The built ledger for one document.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    pub decisions: Vec<Decision>,
    pub edges: BTreeSet<Edge>,
    pub possible: Vec<PossibleMatch>,
    /// Near-duplicate clusters among open, unanswered decisions (tiers 2-4
    /// without an id overlap), for `decision validate`.
    pub near_duplicates: Vec<Vec<String>>,
    /// Hot / archived partition (`#decisionprune`).
    pub liveness: Liveness,
    prior_archived: BTreeSet<String>,
    key_to_decision: BTreeMap<String, usize>,
    key_first_seen: BTreeMap<String, u64>,
    live_queue_ids: BTreeSet<String>,
    kinds: BTreeMap<String, &'static str>,
    now: u64,
}

#[derive(Default)]
struct UnionFind {
    parent: BTreeMap<String, String>,
}

impl UnionFind {
    fn add(&mut self, key: &str) {
        self.parent
            .entry(key.to_string())
            .or_insert_with(|| key.to_string());
    }

    fn find(&mut self, key: &str) -> String {
        self.add(key);
        let parent = self.parent[key].clone();
        if parent == key {
            return parent;
        }
        let root = self.find(&parent);
        self.parent.insert(key.to_string(), root.clone());
        root
    }

    fn union(&mut self, a: &str, b: &str) {
        let left = self.find(a);
        let right = self.find(b);
        if left != right {
            let (keep, drop) = if left < right {
                (left, right)
            } else {
                (right, left)
            };
            self.parent.insert(drop, keep);
        }
    }
}

pub fn id_key(id: &str) -> String {
    format!("id:{}", id.to_ascii_lowercase())
}

/// `qt:<hash>` for questions with at least four normalized tokens.
pub fn text_key(text: &str) -> Option<String> {
    let normalized = text::normalize_question(text);
    (normalized.split(' ').count() >= 4)
        .then(|| format!("qt:{}", &agent_doc_hash::content_hash(&normalized)[..16]))
}

/// The keys a question can be found under in the index: its own keys plus
/// the id key of every id it references (`#decisionprune` archive lookup).
pub fn lookup_keys_for(question: &QuestionObs) -> Vec<String> {
    let mut keys = question_keys(question);
    keys.extend(question.references.iter().map(|id| id_key(id)));
    keys
}

fn question_keys(question: &QuestionObs) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(id) = &question.id {
        keys.push(id_key(id));
    }
    if let Some(key) = text_key(&question.text) {
        keys.push(key);
    }
    keys
}

fn is_request(id: &str) -> bool {
    id.starts_with(REQUEST_ID_PREFIX)
}

fn answer_hash(answer: &str) -> String {
    agent_doc_hash::content_hash(&text::tokens(answer).join(" "))[..10].to_string()
}

struct Group {
    root: String,
    keys: BTreeSet<String>,
    questions: Vec<usize>,
    tier: MatchTier,
}

impl Ledger {
    /// Build the ledger from a document's observations and the prior index.
    pub fn build(
        observations: &Observations,
        prior: &PriorIndex,
        now: u64,
        neighbours: Option<&dyn SemanticNeighbours>,
    ) -> Self {
        let mut uf = UnionFind::default();
        // Acting tiers above plain identity, recorded per key and resolved per
        // group once every union has happened.
        let mut tiers: BTreeMap<String, MatchTier> = BTreeMap::new();
        let mut edges = BTreeSet::new();
        let mut kinds: BTreeMap<String, &'static str> = BTreeMap::new();
        let questions = &observations.questions;

        // Tier 1: ids and normalized text.
        for question in questions {
            let keys = question_keys(question);
            for key in &keys {
                uf.add(key);
            }
            for pair in keys.windows(2) {
                uf.union(&pair[0], &pair[1]);
            }
            if let Some(id) = &question.id
                && let Some(kind) = text::decision_kind(&question.text)
                && question.source != QuestionSource::Request
            {
                kinds.entry(id.clone()).or_insert(kind);
            }
        }
        for question in questions {
            if let Some(id) = &question.id
                && let Some(kind) = text::decision_kind(&question.text)
            {
                kinds.entry(id.clone()).or_insert(kind);
            }
        }
        // Links: operator merges always; request -> decision item is identity;
        // request -> work item only applies_to.
        for link in &observations.links {
            let from = id_key(&link.from);
            let to = id_key(&link.to);
            match link.kind {
                LinkKind::OperatorSameAs => {
                    uf.union(&from, &to);
                    tiers.insert(from.clone(), MatchTier::OperatorMerge);
                    edges.insert(Edge {
                        from: link.from.clone(),
                        kind: EdgeKind::SameAs,
                        to: link.to.clone(),
                    });
                }
                _ => {
                    if kinds.contains_key(&link.to) {
                        uf.union(&from, &to);
                    } else {
                        uf.add(&from);
                        edges.insert(Edge {
                            from: link.from.clone(),
                            kind: EdgeKind::AppliesTo,
                            to: link.to.clone(),
                        });
                    }
                }
            }
        }
        for question in questions {
            if let Some(id) = &question.id {
                for reference in &question.references {
                    edges.insert(Edge {
                        from: id.clone(),
                        kind: EdgeKind::AppliesTo,
                        to: reference.clone(),
                    });
                }
            }
        }
        // Durable grouping: keys the index put in one decision stay together.
        let mut prior_by_decision: BTreeMap<&str, Vec<&PriorKey>> = BTreeMap::new();
        let mut key_first_seen = BTreeMap::new();
        for key in &prior.keys {
            uf.add(&key.key);
            prior_by_decision
                .entry(key.decision_id.as_str())
                .or_default()
                .push(key);
            key_first_seen.insert(key.key.clone(), key.first_seen_at);
        }
        for keys in prior_by_decision.values() {
            for pair in keys.windows(2) {
                uf.union(&pair[0].key, &pair[1].key);
            }
        }
        // Answer keys.
        let answer_key = |answer: &extract::AnswerObs| -> Option<String> {
            answer
                .targets
                .first()
                .map(|id| id_key(id))
                .or_else(|| answer.head_text.as_deref().and_then(text_key))
        };
        for answer in &observations.answers {
            if let Some(key) = answer_key(answer) {
                uf.add(&key);
            }
        }

        // Tier 2 with an id overlap: merge; without: remember as possible.
        let groups = Self::groups(&mut uf, questions);
        {
            let profile = |group: &Group| {
                let mut anchors = BTreeSet::new();
                let mut ids = BTreeSet::new();
                let mut refs = BTreeSet::new();
                let mut kind = None;
                for &index in &group.questions {
                    let question = &questions[index];
                    anchors.extend(text::anchors(&question.text));
                    if let Some(id) = &question.id {
                        ids.insert(id.clone());
                    }
                    refs.extend(question.references.iter().cloned());
                    kind = kind.or_else(|| text::decision_kind(&question.text));
                }
                (anchors, ids, refs, kind)
            };
            let profiles: Vec<_> = groups.iter().map(profile).collect();
            for left in 0..groups.len() {
                for right in left + 1..groups.len() {
                    let (a_anchors, a_ids, a_refs, a_kind) = &profiles[left];
                    let (b_anchors, b_ids, b_refs, b_kind) = &profiles[right];
                    if a_kind.is_none() || a_kind != b_kind {
                        continue;
                    }
                    if !text::anchors_match(a_anchors, b_anchors) {
                        continue;
                    }
                    let overlap = a_refs
                        .iter()
                        .any(|id| b_ids.contains(id) || b_refs.contains(id))
                        || b_refs.iter().any(|id| a_ids.contains(id));
                    if overlap {
                        uf.union(&groups[left].root, &groups[right].root);
                        tiers
                            .entry(groups[left].root.clone())
                            .or_insert(MatchTier::AnchorWithId);
                    }
                }
            }
        }
        let mut groups = Self::groups(&mut uf, questions);
        for group in &mut groups {
            let found: Vec<MatchTier> = group
                .keys
                .iter()
                .filter_map(|key| tiers.get(key).copied())
                .collect();
            group.tier = if found.contains(&MatchTier::OperatorMerge) {
                MatchTier::OperatorMerge
            } else if found.contains(&MatchTier::AnchorWithId) {
                MatchTier::AnchorWithId
            } else {
                MatchTier::Identity
            };
        }

        // Decisions.
        let prior_decision_of: BTreeMap<&str, &PriorKey> = prior
            .keys
            .iter()
            .map(|key| (key.key.as_str(), key))
            .collect();
        let mut ledger = Ledger {
            live_queue_ids: observations.live_queue_ids.clone(),
            prior_archived: prior.archived_decisions.clone(),
            key_first_seen,
            now,
            kinds,
            ..Default::default()
        };
        let mut root_to_decision: BTreeMap<String, usize> = BTreeMap::new();
        for group in &groups {
            let id = group
                .keys
                .iter()
                .filter_map(|key| prior_decision_of.get(key.as_str()))
                .min_by(|a, b| {
                    a.first_seen_at
                        .cmp(&b.first_seen_at)
                        .then(a.decision_id.cmp(&b.decision_id))
                })
                .map(|key| key.decision_id.clone())
                .unwrap_or_else(|| {
                    let canonical = group
                        .keys
                        .iter()
                        .find(|key| key.starts_with("id:"))
                        .or(group.keys.iter().next());
                    format!(
                        "dec-{}",
                        &agent_doc_hash::content_hash(canonical.map_or("", String::as_str))[..10]
                    )
                });
            let question = group
                .questions
                .iter()
                .map(|&index| &questions[index])
                .min_by_key(|question| match question.source {
                    QuestionSource::Todo => 0,
                    QuestionSource::Waiting => 1,
                    QuestionSource::Request => 2,
                })
                .map(|question| text::question_body(&question.text))
                .unwrap_or_default();
            let mut open_items = Vec::new();
            let mut open_todos = Vec::new();
            for &index in &group.questions {
                let question = &questions[index];
                if !question.open {
                    continue;
                }
                let label = question
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("“{}”", text::question_body(&question.text)));
                if !open_items.contains(&label) {
                    open_items.push(label);
                }
                if question.source == QuestionSource::Todo
                    && let Some(id) = &question.id
                    && !open_todos.contains(id)
                {
                    open_todos.push(id.clone());
                }
            }
            let index = ledger.decisions.len();
            for key in &group.keys {
                ledger.key_to_decision.insert(key.clone(), index);
            }
            root_to_decision.insert(group.root.clone(), index);
            ledger.decisions.push(Decision {
                id,
                question,
                keys: group.keys.clone(),
                revisions: Vec::new(),
                open_items,
                open_todos,
                tier: group.tier,
            });
        }

        // Revisions from the document, then from the index.
        let mut pending: BTreeMap<usize, Vec<(Option<u64>, usize, Revision)>> = BTreeMap::new();
        let prior_revisions: BTreeMap<(String, String), &PriorRevision> = prior
            .revisions
            .iter()
            .map(|revision| {
                (
                    (revision.decision_id.clone(), answer_hash(&revision.answer)),
                    revision,
                )
            })
            .collect();
        // Without an index time, a consumed (struck or archived) answer is
        // older than a live one: it was already acted on, and an archived one
        // is older still. Order is then (first seen, consumed before live,
        // archived before inline, document line).
        for (order, answer) in observations.answers.iter().enumerate() {
            let _ = order;
            let archived = answer.location.starts_with("done archive");
            let order = (usize::from(!answer.consumed) << 40)
                + (usize::from(!archived) << 39)
                + answer.line;
            let Some(key) = answer_key(answer) else {
                continue;
            };
            let root = uf.find(&key);
            let Some(&index) = root_to_decision.get(&root) else {
                continue;
            };
            let decision_id = ledger.decisions[index].id.clone();
            let hash = answer_hash(&answer.text);
            let remembered = prior_revisions.get(&(decision_id.clone(), hash.clone()));
            let entry = pending.entry(index).or_default();
            if let Some(existing) = entry
                .iter_mut()
                .find(|(_, _, revision)| revision.id.ends_with(&hash))
            {
                existing.2.consumed |= answer.consumed;
                continue;
            }
            entry.push((
                Some(remembered.map_or(now, |revision| revision.first_seen_at)),
                order,
                Revision {
                    id: format!("{decision_id}@{hash}"),
                    answer: answer.text.clone(),
                    location: answer.location.clone(),
                    first_seen_at: Some(remembered.map_or(now, |revision| revision.first_seen_at)),
                    relation: RevisionRelation::Initial,
                    consumed: answer.consumed,
                    in_document: true,
                    change_flagged: remembered.is_some_and(|revision| revision.change_flagged),
                },
            ));
        }
        let decision_index: BTreeMap<String, usize> = ledger
            .decisions
            .iter()
            .enumerate()
            .map(|(index, decision)| (decision.id.clone(), index))
            .collect();
        for revision in &prior.revisions {
            // A decision id the document no longer produces keeps its keys
            // through the prior-key union above, so it maps to a group.
            let Some(&index) = decision_index.get(&revision.decision_id).or_else(|| {
                prior
                    .keys
                    .iter()
                    .find(|key| key.decision_id == revision.decision_id)
                    .and_then(|key| root_to_decision.get(&uf.find(&key.key)))
            }) else {
                continue;
            };
            let hash = answer_hash(&revision.answer);
            let entry = pending.entry(index).or_default();
            if entry
                .iter()
                .any(|(_, _, existing)| existing.id.ends_with(&hash))
            {
                continue;
            }
            let decision_id = ledger.decisions[index].id.clone();
            entry.push((
                Some(revision.first_seen_at),
                usize::MAX,
                Revision {
                    id: format!("{decision_id}@{hash}"),
                    answer: revision.answer.clone(),
                    location: format!("{} (no longer in the document)", revision.location),
                    first_seen_at: Some(revision.first_seen_at),
                    relation: RevisionRelation::Initial,
                    consumed: true,
                    in_document: false,
                    change_flagged: revision.change_flagged,
                },
            ));
        }
        for (index, mut revisions) in pending {
            revisions.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut ordered: Vec<Revision> = Vec::new();
            for (_, _, mut revision) in revisions {
                if let Some(previous) = ordered.last() {
                    revision.relation = text::answer_relation(&previous.answer, &revision.answer);
                    let kind = match revision.relation {
                        RevisionRelation::Supersedes => EdgeKind::Supersedes,
                        RevisionRelation::Amends => EdgeKind::Amends,
                        _ => EdgeKind::Restates,
                    };
                    edges.insert(Edge {
                        from: revision.id.clone(),
                        kind,
                        to: previous.id.clone(),
                    });
                }
                edges.insert(Edge {
                    from: revision.id.clone(),
                    kind: EdgeKind::Answers,
                    to: ledger.decisions[index].id.clone(),
                });
                ordered.push(revision);
            }
            ledger.decisions[index].revisions = ordered;
        }

        // Liveness (`#decisionprune`): the gray zone below and the index
        // partition both work on the hot set.
        ledger.edges = edges;
        ledger.liveness =
            ledger.compute_liveness(&observations.open_item_ids, &prior.archived_decisions, &[]);

        // Gray zone: tier 2 without an id overlap, tier 3, tier 4. These only
        // annotate; they never merge decisions. A pair of two dormant
        // decisions is skipped: neither side is asked or applied any more, so
        // the comparison is hot x all instead of all x all.
        let profiles: Vec<(String, BTreeSet<String>)> = groups
            .iter()
            .map(|group| {
                let text = group
                    .questions
                    .iter()
                    .map(|&index| text::question_body(&questions[index].text))
                    .max_by_key(String::len)
                    .unwrap_or_default();
                let anchors = group
                    .questions
                    .iter()
                    .flat_map(|&index| text::anchors(&questions[index].text))
                    .collect();
                (text, anchors)
            })
            .collect();
        let decision_of_group: Vec<Option<usize>> = groups
            .iter()
            .map(|group| root_to_decision.get(&uf.find(&group.root)).copied())
            .collect();
        let mut possible = Vec::new();
        let mut near_duplicates: Vec<BTreeSet<String>> = Vec::new();
        for left in 0..groups.len() {
            for right in left + 1..groups.len() {
                let (Some(a), Some(b)) = (decision_of_group[left], decision_of_group[right]) else {
                    continue;
                };
                if a == b || profiles[left].0.is_empty() || profiles[right].0.is_empty() {
                    continue;
                }
                if !ledger.liveness.is_hot(&ledger.decisions[a].id)
                    && !ledger.liveness.is_hot(&ledger.decisions[b].id)
                {
                    continue;
                }
                let kind = text::decision_kind(&profiles[left].0);
                let (tier, score) = if kind.is_some()
                    && kind == text::decision_kind(&profiles[right].0)
                    && text::anchors_match(&profiles[left].1, &profiles[right].1)
                {
                    (MatchTier::Anchor, None)
                } else if let Some(score) =
                    text::similar_questions(&profiles[left].0, &profiles[right].0)
                {
                    (MatchTier::Text, Some(score))
                } else if let Some(score) = neighbours
                    .and_then(|source| source.similarity(&profiles[left].0, &profiles[right].0))
                {
                    (MatchTier::Semantic, Some(score))
                } else {
                    continue;
                };
                let ids: BTreeSet<String> = [
                    ledger.decisions[a].id.clone(),
                    ledger.decisions[b].id.clone(),
                ]
                .into_iter()
                .collect();
                if let Some(cluster) = near_duplicates
                    .iter_mut()
                    .find(|cluster| !cluster.is_disjoint(&ids))
                {
                    cluster.extend(ids);
                } else {
                    near_duplicates.push(ids);
                }
                for (open_index, answered_index) in [(a, b), (b, a)] {
                    let open = &ledger.decisions[open_index];
                    let answered = &ledger.decisions[answered_index];
                    if open.open_items.is_empty() || !open.revisions.is_empty() {
                        continue;
                    }
                    let Some(latest) = answered.latest() else {
                        continue;
                    };
                    possible.push(PossibleMatch {
                        open_items: open.open_items.clone(),
                        question: open.question.clone(),
                        decision_id: answered.id.clone(),
                        answer: latest.answer.clone(),
                        location: latest.location.clone(),
                        tier,
                        score,
                    });
                }
            }
        }
        ledger.possible = possible;
        ledger.near_duplicates = near_duplicates
            .into_iter()
            .map(|cluster| cluster.into_iter().collect())
            .collect();
        ledger
    }

    fn groups(uf: &mut UnionFind, questions: &[QuestionObs]) -> Vec<Group> {
        let keys: Vec<String> = uf.parent.keys().cloned().collect();
        let mut by_root: BTreeMap<String, Group> = BTreeMap::new();
        for key in keys {
            let root = uf.find(&key);
            by_root
                .entry(root.clone())
                .or_insert_with(|| Group {
                    root,
                    keys: BTreeSet::new(),
                    questions: Vec::new(),
                    tier: MatchTier::Identity,
                })
                .keys
                .insert(key);
        }
        for (index, question) in questions.iter().enumerate() {
            if let Some(key) = question_keys(question).first() {
                let root = uf.find(key);
                if let Some(group) = by_root.get_mut(&root) {
                    group.questions.push(index);
                }
            }
        }
        by_root.into_values().collect()
    }

    /// The decision a key belongs to.
    pub fn decision_for_key(&self, key: &str) -> Option<&Decision> {
        self.key_to_decision
            .get(key)
            .map(|&index| &self.decisions[index])
    }

    /// "Already answered" facts for every answered decision with open copies.
    pub fn answered_facts(&self) -> Vec<AnsweredFact> {
        self.decisions
            .iter()
            .filter(|decision| !decision.open_items.is_empty())
            .filter_map(|decision| {
                let latest = decision.latest()?;
                let earlier_answers = decision.revisions[..decision.revisions.len() - 1]
                    .iter()
                    .map(|revision| revision.answer.clone())
                    .collect();
                let changed_from = (latest.relation == RevisionRelation::Supersedes
                    && !latest.change_flagged)
                    .then(|| {
                        decision.revisions[decision.revisions.len() - 2]
                            .answer
                            .clone()
                    });
                Some(AnsweredFact {
                    decision_id: decision.id.clone(),
                    open_items: decision.open_items.clone(),
                    question: decision.question.clone(),
                    answer: latest.answer.clone(),
                    location: latest.location.clone(),
                    first_seen_at: latest.first_seen_at,
                    earlier_answers,
                    changed_from,
                    tier: decision.tier,
                })
            })
            .collect()
    }

    /// Duplicate open todo copies the binary closes on its own.
    ///
    /// Within one acting-tier decision that has an answer, a copy closes when:
    /// it is not the copy kept open for the agent to act on; the operator's
    /// answer does not sit in that copy's own waiting packet (that packet is
    /// the dispatch path); it is a request-question copy or an exact text copy;
    /// no live queue row still names it (that row carries the answer); and it
    /// was asked no later than the latest answer, or was answered directly.
    pub fn duplicate_closures(&self, observations: &Observations) -> Vec<DuplicateClosure> {
        let mut closures = Vec::new();
        for decision in &self.decisions {
            if !decision.tier.acts() || decision.open_todos.is_empty() {
                continue;
            }
            let Some(latest) = decision.latest() else {
                continue;
            };
            let packet_held: BTreeSet<&str> = observations
                .answers
                .iter()
                .filter(|answer| answer.in_own_packet)
                .filter_map(|answer| answer.targets.first().map(String::as_str))
                .collect();
            let directly: BTreeSet<&str> = observations
                .answers
                .iter()
                .filter_map(|answer| answer.targets.first().map(String::as_str))
                .collect();
            let primary = decision
                .open_todos
                .iter()
                .find(|id| packet_held.contains(id.as_str()) && !is_request(id))
                .or_else(|| decision.open_todos.iter().find(|id| !is_request(id)))
                .or_else(|| {
                    decision
                        .open_todos
                        .iter()
                        .find(|id| packet_held.contains(id.as_str()))
                })
                .cloned();
            let todo_text_keys: BTreeMap<&str, Option<String>> = observations
                .questions
                .iter()
                .filter(|question| question.source == QuestionSource::Todo && question.open)
                .filter_map(|question| {
                    question
                        .id
                        .as_deref()
                        .map(|id| (id, text_key(&question.text)))
                })
                .collect();
            for id in &decision.open_todos {
                if Some(id) == primary.as_ref() || packet_held.contains(id.as_str()) {
                    continue;
                }
                let exact_copy = todo_text_keys
                    .get(id.as_str())
                    .cloned()
                    .flatten()
                    .is_some_and(|key| {
                        todo_text_keys.iter().any(|(other, other_key)| {
                            *other != id.as_str() && other_key.as_deref() == Some(key.as_str())
                        })
                    });
                if !(is_request(id) || exact_copy) || self.live_queue_ids.contains(id) {
                    continue;
                }
                let asked_at = self.key_first_seen.get(&id_key(id)).copied();
                let answered_at = latest.first_seen_at.unwrap_or(0);
                if !directly.contains(id.as_str())
                    && asked_at.is_some_and(|asked| asked > answered_at)
                {
                    continue;
                }
                closures.push(DuplicateClosure {
                    todo_id: id.clone(),
                    decision_id: decision.id.clone(),
                    answer: latest.answer.clone(),
                    location: latest.location.clone(),
                    kept_open: primary.clone(),
                });
            }
        }
        closures
    }

    /// Questions a closeout is about to add that an acting tier resolves to an
    /// answered decision, and that do not cite the recorded answer.
    pub fn reasks(&self, new_questions: &[NewQuestion]) -> Vec<Reask> {
        let mut out = Vec::new();
        for new in new_questions {
            let question = &new.question;
            let mut candidates: Vec<(usize, MatchTier)> = Vec::new();
            if let Some(id) = &question.id
                && let Some(&index) = self.key_to_decision.get(&id_key(id))
            {
                candidates.push((index, MatchTier::Identity));
            }
            if let Some(key) = text_key(&question.text)
                && let Some(&index) = self.key_to_decision.get(&key)
            {
                candidates.push((index, MatchTier::Identity));
            }
            if question.source == QuestionSource::Request {
                for reference in &question.references {
                    if self.kinds.contains_key(reference)
                        && let Some(&index) = self.key_to_decision.get(&id_key(reference))
                    {
                        candidates.push((index, MatchTier::Identity));
                    }
                }
            }
            let kind = text::decision_kind(&question.text);
            let anchors = text::anchors(&question.text);
            if kind.is_some() && !anchors.is_empty() {
                for (index, decision) in self.decisions.iter().enumerate() {
                    if decision.revisions.is_empty()
                        || text::decision_kind(&decision.question) != kind
                    {
                        continue;
                    }
                    let overlap = question.references.iter().any(|reference| {
                        decision.keys.contains(&id_key(reference))
                            || self.edges.iter().any(|edge| {
                                edge.kind == EdgeKind::AppliesTo
                                    && &edge.to == reference
                                    && decision.keys.contains(&id_key(&edge.from))
                            })
                    });
                    if overlap && text::anchors_match(&anchors, &text::anchors(&decision.question))
                    {
                        candidates.push((index, MatchTier::AnchorWithId));
                    }
                }
            }
            let Some(&(index, tier)) = candidates
                .iter()
                .find(|(index, _)| !self.decisions[*index].revisions.is_empty())
            else {
                continue;
            };
            let decision = &self.decisions[index];
            let latest = decision.latest().expect("answered");
            // Citation must be explicit: offering the recorded answer back as
            // a choice is still asking again.
            let cited = {
                let haystack = new.full_text.to_lowercase();
                haystack.contains(&decision.id) || haystack.contains("already answered")
            };
            if cited {
                continue;
            }
            out.push(Reask {
                question_id: question.id.clone(),
                question: text::question_body(&question.text),
                decision_id: decision.id.clone(),
                answer: latest.answer.clone(),
                location: latest.location.clone(),
                first_seen_at: latest.first_seen_at,
                tier,
            });
        }
        out
    }

    /// Whether a decision's rows belong in the index at all.
    pub(crate) fn stored(decision: &Decision) -> bool {
        !decision.revisions.is_empty() || !decision.open_items.is_empty()
    }

    /// The index rows of one decision (every key, every revision).
    pub(crate) fn decision_rows(&self, decision: &Decision) -> (Vec<PriorKey>, Vec<PriorRevision>) {
        let keys = decision
            .keys
            .iter()
            .map(|key| PriorKey {
                key: key.clone(),
                decision_id: decision.id.clone(),
                first_seen_at: self.key_first_seen.get(key).copied().unwrap_or(self.now),
            })
            .collect();
        let revisions = decision
            .revisions
            .iter()
            .map(|revision| PriorRevision {
                decision_id: decision.id.clone(),
                answer: revision.answer.clone(),
                location: revision
                    .location
                    .trim_end_matches(" (no longer in the document)")
                    .to_string(),
                relation: revision.relation.as_str().to_string(),
                first_seen_at: revision.first_seen_at.unwrap_or(0),
                change_flagged: revision.change_flagged
                    || (revision.relation == RevisionRelation::Supersedes
                        && decision
                            .latest()
                            .is_some_and(|latest| latest.id == revision.id)),
            })
            .collect();
        (keys, revisions)
    }

    /// Every index row this build implies (keys, revisions), hot and
    /// archived alike. [`Ledger::partitioned_index_rows`] splits them.
    pub fn index_rows(&self) -> (Vec<PriorKey>, Vec<PriorRevision>) {
        let mut keys = Vec::new();
        let mut revisions = Vec::new();
        for decision in self
            .decisions
            .iter()
            .filter(|decision| Self::stored(decision))
        {
            let (decision_keys, decision_revisions) = self.decision_rows(decision);
            keys.extend(decision_keys);
            revisions.extend(decision_revisions);
        }
        (keys, revisions)
    }
}

fn open_todo_action(content: &str, id: &str) -> Option<String> {
    let todo = agent_doc_element::todo::find_todo_component(content).ok()??;
    todo.content(content).lines().find_map(|line| {
        let parsed = agent_doc_element::todo::parse_todo_line(line)?;
        (parsed.state.is_open()
            && parsed
                .own_id
                .is_some_and(|own| own.eq_ignore_ascii_case(id)))
        .then(|| parsed.action.trim_end().to_string())
    })
}

/// Close each duplicate todo in `content`, appending the recorded answer to
/// its action text. Returns `None` when nothing changed.
pub fn apply_duplicate_closures(
    content: &str,
    closures: &[DuplicateClosure],
) -> anyhow::Result<Option<String>> {
    let mut current = content.to_string();
    for closure in closures {
        let Some(action) = open_todo_action(&current, &closure.todo_id) else {
            continue;
        };
        let kept = closure
            .kept_open
            .as_deref()
            .map(|id| format!("; acted on under #{id}"))
            .unwrap_or_default();
        let note = format!(
            " (already answered `{}`: “{}” — {}{kept})",
            closure.decision_id,
            closure.answer.replace("[#", "#").replace(']', ""),
            closure.location.replace("[#", "#").replace(']', ""),
        );
        if !action.contains(&format!("already answered `{}`", closure.decision_id)) {
            let edited = agent_doc_element::todo::edit_todo_item_in_document(
                &current,
                &closure.todo_id,
                &format!("[#{}] {action}{note}", closure.todo_id),
            )?;
            current = edited.content;
        }
        let done = agent_doc_element::todo::set_todo_item_state_in_document(
            &current,
            &closure.todo_id,
            agent_doc_element::todo::TodoTransition::Done,
        )?;
        current = done.content;
    }
    Ok((current != content).then_some(current))
}

/// Render the cycle-contract guidance for answered facts and gray-zone
/// matches; `None` when both are empty.
pub fn contract_guidance(facts: &[AnsweredFact], possible: &[PossibleMatch]) -> Option<String> {
    if facts.is_empty() && possible.is_empty() {
        return None;
    }
    let mut out = String::from(
        "The operator already answered these questions (#answerbyrequestid). Act on the recorded answer; do not ask again, and do not open a new request, todo or waiting question for them. A closeout that re-asks one is refused unless it cites the decision id and says why the answer does not resolve it.",
    );
    for fact in facts {
        out.push_str(&format!(
            "\n- already answered `{}` ({}): “{}” ({})",
            fact.decision_id,
            fact.open_items
                .iter()
                .map(|item| format!("#{item}"))
                .collect::<Vec<_>>()
                .join(", "),
            fact.answer,
            fact.location
        ));
        if let Some(from) = &fact.changed_from {
            out.push_str(&format!(
                " — decision changed: “{from}” -> “{}”",
                fact.answer
            ));
        }
    }
    for candidate in possible {
        out.push_str(&format!(
            "\n- possibly already answered ({}; {}): “{}” under `{}` ({}). If it is the same question, ask the operator once to confirm with a `same as [#id]` reply; never merge it yourself.",
            candidate.open_items.iter().map(|item| format!("#{item}")).collect::<Vec<_>>().join(", "),
            match candidate.tier {
                MatchTier::Anchor => "same subject anchors",
                MatchTier::Semantic => "semantic neighbour",
                _ => "similar wording",
            },
            candidate.answer,
            candidate.decision_id,
            candidate.location
        ));
    }
    Some(out)
}

/// The refusal message for re-asked questions.
pub fn reask_refusal(reasks: &[Reask]) -> String {
    let mut out = String::from(
        "closeout refused (#answerbyrequestid): it asks the operator a question they already answered.",
    );
    for reask in reasks {
        out.push_str(&format!(
            "\n- {}“{}” is decision `{}`, already answered: “{}” ({})",
            reask
                .question_id
                .as_deref()
                .map(|id| format!("[#{id}] "))
                .unwrap_or_default(),
            reask.question,
            reask.decision_id,
            reask.answer,
            reask.location
        ));
    }
    out.push_str(
        "\nAct on the recorded answer instead. If the answer truly does not resolve a new question, cite the decision id or write `already answered: <answer>` in the request and state what is still open.",
    );
    out
}

#[cfg(test)]
mod tests;
