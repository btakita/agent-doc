//! Decision graph over the answer ledger (`#decisiongraph`).
//!
//! The ledger already groups every copy of a question into one decision and
//! orders its answer revisions. This module projects that ledger, plus the
//! observations it was built from, onto typed nodes and edges:
//!
//! - **Nodes** ([`NodeKind`]): questions (todo / waiting / request copies that
//!   ask the operator something), decisions, revisions (one per distinct
//!   operator answer), items (work the decisions apply to) and evidence
//!   (responses `r-<hex>`, commits `commit:<sha>`). Principle, fact and
//!   derivation nodes are reserved for `#firstprinciples`.
//! - **Edges** ([`EdgeKind`]): `answers`, `supersedes`, `amends`,
//!   `restates`, `depends_on`, `conflicts_with`, `applies_to`,
//!   `evidenced_by`, `copy_of`, `same_as`, plus `derived_from` / `subsumes` /
//!   `exception_to` reserved for `#firstprinciples` and `#decisionprune`.
//!   Each edge carries its [`EdgeSource`]: `derived` (the binary read it off
//!   the document structure), `operator` (the operator's explicit words:
//!   `change:`, `amend:`, `same as [#id]`, `depends on [#id]`), or `proposed`
//!   (the binary's classification, awaiting the operator's confirmation).
//!
//! The document stays the authority. The graph is rebuilt on every ledger
//! pass and cached in `state.db` (`answer_ledger_nodes`,
//! `answer_ledger_edges`), so history whose source rows were pruned survives
//! ([`DecisionGraph::merge_prior`]). Queries: [`DecisionGraph::impact`]
//! (what a decision change disturbs) and [`DecisionGraph::why`] (why a
//! decision holds). tsift graph-db may index the cached rows as an optional
//! traversal layer; nothing here depends on it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::extract::{Observations, QuestionSource, bracket_ids};
use crate::{EdgeKind, Ledger, RevisionRelation, id_key, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Question,
    Decision,
    Revision,
    Item,
    Evidence,
    /// Reserved for `#firstprinciples`.
    Principle,
    /// Reserved for `#firstprinciples`.
    Fact,
    /// Reserved for `#firstprinciples`.
    Derivation,
}

impl NodeKind {
    pub const ALL: [Self; 8] = [
        Self::Question,
        Self::Decision,
        Self::Revision,
        Self::Item,
        Self::Evidence,
        Self::Principle,
        Self::Fact,
        Self::Derivation,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Decision => "decision",
            Self::Revision => "revision",
            Self::Item => "item",
            Self::Evidence => "evidence",
            Self::Principle => "principle",
            Self::Fact => "fact",
            Self::Derivation => "derivation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

/// Who asserted an edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeSource {
    /// Read off the document structure by the binary.
    #[default]
    Derived,
    /// The operator's explicit words (`change:`, `amend:`, `same as [#id]`,
    /// `depends on [#id]`, `conflicts with [#id]`).
    Operator,
    /// A classification the binary or agent proposed; it stands until the
    /// operator confirms or corrects it with an explicit form.
    Proposed,
}

impl EdgeSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Derived => "derived",
            Self::Operator => "operator",
            Self::Proposed => "proposed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Derived, Self::Operator, Self::Proposed]
            .into_iter()
            .find(|source| source.as_str() == value)
    }
}

/// One graph node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    /// Question text, verbatim answer, or item text.
    pub label: String,
    /// Still open (questions, items, decisions with open copies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// Where the node's text lives (revisions: `under [#id]`, `queue row`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_at: Option<u64>,
    /// False when only the `state.db` cache still remembers it.
    pub in_document: bool,
}

/// One typed edge `from <kind> to`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub kind: EdgeKind,
    pub to: String,
    pub source: EdgeSource,
    /// For `amends`: the clause the amendment changes or adds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clause: Option<String>,
}

/// Typed nodes and edges for one document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionGraph {
    pub nodes: BTreeMap<String, Node>,
    pub edges: Vec<GraphEdge>,
}

/// Evidence ids cited in `text`: response lookup ids `[#r-<hex>]` as
/// `r-<hex>`, and commit SHAs (7-40 hex chars with a digit and a letter) as
/// `commit:<sha>`.
pub fn evidence_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for id in bracket_ids(text) {
        if let Some(hex) = id.strip_prefix("r-")
            && !hex.is_empty()
            && hex.chars().all(|c| c.is_ascii_hexdigit())
        {
            out.push(id.clone());
        }
    }
    // A SHA is a whole run of hex: runs joined to `#`, `-`, `:` or `@` are
    // handles and ids (`[#q-…]`, `dec-…`, `<decision>@<hash>`), not commits.
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || "#-:@_/".contains(c))) {
        let lower = word.to_ascii_lowercase();
        if (7..=40).contains(&lower.len())
            && lower.chars().all(|c| c.is_ascii_hexdigit())
            && lower.chars().any(|c| c.is_ascii_digit())
            && lower.chars().any(|c| c.is_ascii_alphabetic())
        {
            out.push(format!("commit:{lower}"));
        }
    }
    out.sort();
    out.dedup();
    out
}

impl DecisionGraph {
    fn node(&mut self, node: Node) {
        match self.nodes.get_mut(&node.id) {
            Some(existing) => {
                // A question or decision outranks an item placeholder.
                if existing.kind == NodeKind::Item && node.kind != NodeKind::Item {
                    existing.kind = node.kind;
                }
                if existing.label.is_empty() {
                    existing.label = node.label;
                }
                if node.open == Some(true) {
                    existing.open = Some(true);
                } else if existing.open.is_none() {
                    existing.open = node.open;
                }
                existing.in_document |= node.in_document;
            }
            None => {
                self.nodes.insert(node.id.clone(), node);
            }
        }
    }

    fn placeholder(&mut self, id: &str, kind: NodeKind) {
        if !self.nodes.contains_key(id) {
            self.node(Node {
                id: id.to_string(),
                kind,
                label: String::new(),
                open: None,
                location: None,
                first_seen_at: None,
                in_document: true,
            });
        }
    }

    fn edge(&mut self, edge: GraphEdge) {
        if edge.from == edge.to {
            return;
        }
        if let Some(existing) = self
            .edges
            .iter_mut()
            .find(|e| e.from == edge.from && e.kind == edge.kind && e.to == edge.to)
        {
            // The operator's word outranks a proposal or a derivation.
            if edge.source == EdgeSource::Operator {
                existing.source = EdgeSource::Operator;
            }
            if existing.clause.is_none() {
                existing.clause = edge.clause;
            }
            return;
        }
        self.edges.push(edge);
    }

    fn finish(&mut self) {
        self.edges.sort();
        self.edges
            .dedup_by(|a, b| a.from == b.from && a.kind == b.kind && a.to == b.to);
    }

    /// Project a built ledger and its observations onto the graph.
    pub fn build(ledger: &Ledger, observations: &Observations) -> Self {
        let mut graph = Self::default();
        let decision_of = |id: &str| -> Option<String> {
            ledger
                .decision_for_key(&id_key(id))
                .map(|decision| decision.id.clone())
        };
        // A group is a decision when the operator answered it or one of its
        // copies asks something; a work todo nobody asked about is an item.
        let asks = |question: &crate::extract::QuestionObs| {
            question.source != QuestionSource::Todo || text::decision_kind(&question.text).is_some()
        };
        let asking: BTreeSet<String> = observations
            .questions
            .iter()
            .filter(|question| asks(question))
            .filter_map(|question| {
                question
                    .id
                    .as_deref()
                    .map(id_key)
                    .or_else(|| crate::text_key(&question.text))
            })
            .filter_map(|key| ledger.decision_for_key(&key).map(|d| d.id.clone()))
            .collect();
        for decision in &ledger.decisions {
            if decision.revisions.is_empty() && !asking.contains(&decision.id) {
                continue;
            }
            graph.node(Node {
                id: decision.id.clone(),
                kind: NodeKind::Decision,
                label: decision.question.clone(),
                open: Some(!decision.open_items.is_empty()),
                location: None,
                first_seen_at: decision.revisions.first().and_then(|r| r.first_seen_at),
                in_document: true,
            });
            for revision in &decision.revisions {
                graph.node(Node {
                    id: revision.id.clone(),
                    kind: NodeKind::Revision,
                    label: revision.answer.clone(),
                    open: None,
                    location: Some(revision.location.clone()),
                    first_seen_at: revision.first_seen_at,
                    in_document: revision.in_document,
                });
                for evidence in evidence_refs(&revision.answer) {
                    graph.placeholder(&evidence, NodeKind::Evidence);
                    graph.edge(GraphEdge {
                        from: revision.id.clone(),
                        kind: EdgeKind::EvidencedBy,
                        to: evidence,
                        source: EdgeSource::Operator,
                        clause: None,
                    });
                }
            }
            for pair in decision.revisions.windows(2) {
                let (previous, next) = (&pair[0], &pair[1]);
                let kind = match next.relation {
                    RevisionRelation::Supersedes => EdgeKind::Supersedes,
                    RevisionRelation::Amends => EdgeKind::Amends,
                    _ => EdgeKind::Restates,
                };
                graph.edge(GraphEdge {
                    from: next.id.clone(),
                    kind,
                    to: previous.id.clone(),
                    source: next.relation_source,
                    clause: next.clause.clone(),
                });
            }
            for revision in &decision.revisions {
                graph.edge(GraphEdge {
                    from: revision.id.clone(),
                    kind: EdgeKind::Answers,
                    to: decision.id.clone(),
                    source: EdgeSource::Derived,
                    clause: None,
                });
            }
        }
        // Question copies and items.
        for question in &observations.questions {
            let Some(id) = &question.id else { continue };
            let decision = decision_of(id).filter(|decision| graph.nodes.contains_key(decision));
            let kind = if decision.is_some() {
                NodeKind::Question
            } else {
                NodeKind::Item
            };
            graph.node(Node {
                id: id.clone(),
                kind,
                label: text::question_body(&question.text),
                open: Some(question.open),
                location: None,
                first_seen_at: ledger.key_first_seen.get(&id_key(id)).copied(),
                in_document: true,
            });
            if kind == NodeKind::Question
                && let Some(decision) = &decision
            {
                graph.edge(GraphEdge {
                    from: id.clone(),
                    kind: EdgeKind::CopyOf,
                    to: decision.clone(),
                    source: EdgeSource::Derived,
                    clause: None,
                });
                // Actions recorded on a closed copy (a commit, a response)
                // are evidence for the decision.
                if !question.open {
                    for evidence in evidence_refs(&question.text) {
                        graph.placeholder(&evidence, NodeKind::Evidence);
                        graph.edge(GraphEdge {
                            from: decision.clone(),
                            kind: EdgeKind::EvidencedBy,
                            to: evidence,
                            source: EdgeSource::Derived,
                            clause: None,
                        });
                    }
                }
            }
        }
        // Ledger edges between ids: applies_to, same_as, depends_on,
        // conflicts_with. Revision edges were projected above.
        for edge in &ledger.edges {
            let source = match edge.kind {
                EdgeKind::SameAs | EdgeKind::DependsOn | EdgeKind::ConflictsWith => {
                    EdgeSource::Operator
                }
                EdgeKind::AppliesTo => EdgeSource::Derived,
                _ => continue,
            };
            // Decision-level relations lift both ends onto their decisions;
            // `applies_to` lifts only its source (the target is the item).
            let lift = |id: &str| {
                decision_of(id)
                    .filter(|decision| graph.nodes.contains_key(decision))
                    .unwrap_or_else(|| id.to_string())
            };
            let (from, to) = match edge.kind {
                EdgeKind::DependsOn | EdgeKind::ConflictsWith => (lift(&edge.from), lift(&edge.to)),
                EdgeKind::AppliesTo => (lift(&edge.from), edge.to.clone()),
                _ => (edge.from.clone(), edge.to.clone()),
            };
            graph.placeholder(&from, NodeKind::Item);
            graph.placeholder(&to, NodeKind::Item);
            graph.edge(GraphEdge {
                from,
                kind: edge.kind,
                to,
                source,
                clause: None,
            });
        }
        graph.finish();
        graph
    }

    /// Add cached nodes and edges the current document no longer produces.
    /// The current build wins: a cached revision relation between the same
    /// two revisions under another kind (a proposal the operator since
    /// corrected) is dropped.
    pub fn merge_prior(&mut self, nodes: Vec<Node>, edges: Vec<GraphEdge>) {
        for mut node in nodes {
            if !self.nodes.contains_key(&node.id) {
                node.in_document = false;
                self.nodes.insert(node.id.clone(), node);
            }
        }
        let current: BTreeSet<(String, String)> = self
            .edges
            .iter()
            .filter(|edge| edge.kind.is_revision_relation())
            .map(|edge| (edge.from.clone(), edge.to.clone()))
            .collect();
        for edge in edges {
            if !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to) {
                continue;
            }
            if edge.kind.is_revision_relation()
                && current.contains(&(edge.from.clone(), edge.to.clone()))
            {
                continue;
            }
            if self
                .edges
                .iter()
                .any(|e| e.from == edge.from && e.kind == edge.kind && e.to == edge.to)
            {
                continue;
            }
            self.edges.push(edge);
        }
        self.finish();
    }

    pub fn outgoing<'a>(
        &'a self,
        id: &'a str,
        kind: EdgeKind,
    ) -> impl Iterator<Item = &'a GraphEdge> + 'a {
        self.edges
            .iter()
            .filter(move |edge| edge.from == id && edge.kind == kind)
    }

    pub fn incoming<'a>(
        &'a self,
        id: &'a str,
        kind: EdgeKind,
    ) -> impl Iterator<Item = &'a GraphEdge> + 'a {
        self.edges
            .iter()
            .filter(move |edge| edge.to == id && edge.kind == kind)
    }

    /// The decision a node id names: a decision id, a revision id
    /// (`<decision>@<hash>`), or a question copy. `None` for anything else.
    pub fn resolve_decision(&self, id: &str) -> Option<String> {
        let id = id.trim().trim_start_matches("[#").trim_start_matches('#');
        let id = id.trim_end_matches(']');
        let node = self
            .nodes
            .get(id)
            .or_else(|| self.nodes.get(&id.to_ascii_lowercase()))?;
        match node.kind {
            NodeKind::Decision => Some(node.id.clone()),
            NodeKind::Revision => self
                .outgoing(&node.id, EdgeKind::Answers)
                .next()
                .map(|edge| edge.to.clone()),
            _ => self
                .outgoing(&node.id, EdgeKind::CopyOf)
                .next()
                .map(|edge| edge.to.clone()),
        }
    }

    /// The decisions that apply to an item.
    pub fn decisions_applying_to(&self, id: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .incoming(id, EdgeKind::AppliesTo)
            .filter_map(|edge| {
                self.nodes
                    .get(&edge.from)
                    .filter(|node| node.kind == NodeKind::Decision)
                    .map(|node| node.id.clone())
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// A decision's revisions, oldest first, following the revision-relation
    /// chain back from the latest.
    pub fn revisions_of(&self, decision: &str) -> Vec<&Node> {
        let members: BTreeSet<&str> = self
            .incoming(decision, EdgeKind::Answers)
            .map(|edge| edge.from.as_str())
            .collect();
        let previous: BTreeMap<&str, &str> = self
            .edges
            .iter()
            .filter(|edge| edge.kind.is_revision_relation() && members.contains(edge.from.as_str()))
            .map(|edge| (edge.from.as_str(), edge.to.as_str()))
            .collect();
        let has_successor: BTreeSet<&str> = previous.values().copied().collect();
        let mut ordered = Vec::new();
        let mut seen = BTreeSet::new();
        let mut heads: Vec<&str> = members
            .iter()
            .copied()
            .filter(|id| !has_successor.contains(id))
            .collect();
        heads.sort_by_key(|id| {
            self.nodes
                .get(*id)
                .and_then(|node| node.first_seen_at)
                .unwrap_or(0)
        });
        for head in heads {
            let mut chain = Vec::new();
            let mut cursor = Some(head);
            while let Some(id) = cursor {
                if !seen.insert(id) {
                    break;
                }
                if let Some(node) = self.nodes.get(id) {
                    chain.push(node);
                }
                cursor = previous.get(id).copied();
            }
            chain.reverse();
            ordered.extend(chain);
        }
        ordered
    }

    /// The relation edge from `revision` to its predecessor.
    pub fn relation_of(&self, revision: &str) -> Option<&GraphEdge> {
        self.edges
            .iter()
            .find(|edge| edge.from == revision && edge.kind.is_revision_relation())
    }
}

/// What a dependent should do about a decision change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactAction {
    /// Open dependent: re-evaluate against the new answer.
    Flag,
    /// Closed dependent that relied on the old answer: reopen it.
    Reopen,
    /// Open dependent touching an amended clause: review that clause.
    ReviewClause,
    /// Something already done on the old answer (a closed copy, a commit, a
    /// response): the operator keeps it or undoes it.
    KeepOrUndo,
}

/// One affected node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactRow {
    pub node: String,
    pub kind: NodeKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    /// How it reaches the changed decision (`copy_of`, `applies_to`,
    /// `depends_on`, `evidenced_by`).
    pub relation: EdgeKind,
    /// Decision ids walked from the changed decision to this node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
    pub action: ImpactAction,
}

/// The impact of one revision of a decision on everything that depends on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impact {
    pub decision_id: String,
    pub question: String,
    /// `supersedes`, `amends`, `restates`, or `initial`.
    pub change: String,
    pub change_source: EdgeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_answer: Option<String>,
    pub to_revision: String,
    pub to_answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clause: Option<String>,
    /// Dependents to reopen, flag or review.
    pub flagged: Vec<ImpactRow>,
    /// Actions already taken on the old answer, for the operator to keep or
    /// undo (supersede only).
    pub actions_taken: Vec<ImpactRow>,
    /// Decisions recorded as conflicting with this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts_with: Vec<String>,
    /// Dependents an amend leaves alone (they do not touch the clause).
    pub unaffected: usize,
}

/// Does `text` touch `clause`? A shared id, a shared subject anchor, or at
/// least two shared content tokens.
pub fn touches_clause(clause: &str, text: &str) -> bool {
    let clause_ids: BTreeSet<String> = bracket_ids(clause).into_iter().collect();
    if bracket_ids(text).iter().any(|id| clause_ids.contains(id)) {
        return true;
    }
    let left = text::anchors(clause);
    let right = text::anchors(text);
    if !left.is_disjoint(&right) {
        return true;
    }
    text::content_tokens(clause)
        .intersection(&text::content_tokens(text))
        .count()
        >= 2
}

/// A dependent found by the impact walk.
struct Dependent {
    node: String,
    relation: EdgeKind,
    via: Vec<String>,
}

impl DecisionGraph {
    /// Every node that depends on `decision`, transitively through
    /// `depends_on`: its copies, the items it applies to, and decisions or
    /// items that assume it (with their own copies and items).
    fn dependents(&self, decision: &str) -> Vec<Dependent> {
        let mut out = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::from([decision.to_string()]);
        let mut queue: VecDeque<(String, Vec<String>)> =
            VecDeque::from([(decision.to_string(), Vec::new())]);
        while let Some((current, via)) = queue.pop_front() {
            let copies: Vec<String> = self
                .incoming(&current, EdgeKind::CopyOf)
                .map(|edge| edge.from.clone())
                .collect();
            let mut found: Vec<(String, EdgeKind)> = copies
                .iter()
                .map(|copy| (copy.clone(), EdgeKind::CopyOf))
                .collect();
            let members: Vec<&String> = std::iter::once(&current).chain(copies.iter()).collect();
            // `applies_to` reads "from concerns to": the items a decision
            // concerns and the items that cite it both depend on it.
            for member in &members {
                found.extend(
                    self.outgoing(member, EdgeKind::AppliesTo)
                        .map(|edge| (edge.to.clone(), EdgeKind::AppliesTo)),
                );
                found.extend(
                    self.incoming(member, EdgeKind::AppliesTo)
                        .map(|edge| (edge.from.clone(), EdgeKind::AppliesTo)),
                );
            }
            for member in &members {
                found.extend(
                    self.incoming(member, EdgeKind::DependsOn)
                        .map(|edge| (edge.from.clone(), EdgeKind::DependsOn)),
                );
            }
            for (node, relation) in found {
                if !seen.insert(node.clone()) {
                    continue;
                }
                if relation == EdgeKind::DependsOn {
                    let mut path = via.clone();
                    path.push(node.clone());
                    queue.push_back((node.clone(), path));
                }
                out.push(Dependent {
                    node,
                    relation,
                    via: via.clone(),
                });
            }
        }
        out
    }

    /// What `revision` (default: the decision's latest) disturbs.
    ///
    /// - `supersedes`: every dependent is flagged (open) or reopened (closed,
    ///   it relied on the old answer); closed copies and the evidence of the
    ///   replaced revisions are listed as actions taken, to keep or undo.
    /// - `amends`: only dependents whose text touches the amended clause are
    ///   flagged for review; nothing is reopened.
    /// - `restates` / `initial`: nothing is disturbed.
    pub fn impact(&self, decision: &str, revision: Option<&str>) -> Option<Impact> {
        let decision_node = self.nodes.get(decision)?;
        let revisions = self.revisions_of(decision);
        let target_index = match revision {
            Some(id) => revisions.iter().position(|node| node.id == id)?,
            None => revisions.len().checked_sub(1)?,
        };
        let target = revisions[target_index];
        let relation = self.relation_of(&target.id);
        let change = relation.map_or("initial", |edge| edge.kind.as_str());
        let previous = target_index.checked_sub(1).map(|index| revisions[index]);
        let clause = relation.and_then(|edge| edge.clause.clone());
        let mut impact = Impact {
            decision_id: decision.to_string(),
            question: decision_node.label.clone(),
            change: change.to_string(),
            change_source: relation.map_or(EdgeSource::Derived, |edge| edge.source),
            from_revision: previous.map(|node| node.id.clone()),
            from_answer: previous.map(|node| node.label.clone()),
            to_revision: target.id.clone(),
            to_answer: target.label.clone(),
            clause: clause.clone(),
            flagged: Vec::new(),
            actions_taken: Vec::new(),
            conflicts_with: self
                .outgoing(decision, EdgeKind::ConflictsWith)
                .map(|edge| edge.to.clone())
                .chain(
                    self.incoming(decision, EdgeKind::ConflictsWith)
                        .map(|edge| edge.from.clone()),
                )
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            unaffected: 0,
        };
        let kind = relation.map(|edge| edge.kind);
        if !matches!(kind, Some(EdgeKind::Supersedes | EdgeKind::Amends)) {
            return Some(impact);
        }
        let row = |dependent: &Dependent, action: ImpactAction| -> Option<ImpactRow> {
            let node = self.nodes.get(&dependent.node)?;
            Some(ImpactRow {
                node: node.id.clone(),
                kind: node.kind,
                label: node.label.clone(),
                open: node.open,
                relation: dependent.relation,
                via: dependent.via.clone(),
                action,
            })
        };
        for dependent in self.dependents(decision) {
            let Some(node) = self.nodes.get(&dependent.node) else {
                continue;
            };
            if kind == Some(EdgeKind::Amends) {
                let text = self.dependent_text(node);
                let touched = clause
                    .as_deref()
                    .is_some_and(|clause| touches_clause(clause, &text));
                if touched && node.open != Some(false) {
                    impact
                        .flagged
                        .extend(row(&dependent, ImpactAction::ReviewClause));
                } else {
                    impact.unaffected += 1;
                }
                continue;
            }
            let closed = node.open == Some(false);
            if dependent.relation == EdgeKind::CopyOf {
                // A copy is the same question; the new answer covers it. A
                // closed copy was acted on under the old answer.
                if closed {
                    impact
                        .actions_taken
                        .extend(row(&dependent, ImpactAction::KeepOrUndo));
                }
                continue;
            }
            let action = if closed {
                ImpactAction::Reopen
            } else {
                ImpactAction::Flag
            };
            impact.flagged.extend(row(&dependent, action));
        }
        if kind == Some(EdgeKind::Supersedes) {
            // Evidence of every revision the change replaces.
            let mut replaced: Vec<&Node> = revisions[..target_index].to_vec();
            replaced.reverse();
            let mut seen = BTreeSet::new();
            for old in replaced {
                for edge in self.outgoing(&old.id, EdgeKind::EvidencedBy) {
                    if seen.insert(edge.to.clone())
                        && let Some(node) = self.nodes.get(&edge.to)
                    {
                        impact.actions_taken.push(ImpactRow {
                            node: node.id.clone(),
                            kind: node.kind,
                            label: node.label.clone(),
                            open: node.open,
                            relation: EdgeKind::EvidencedBy,
                            via: vec![old.id.clone()],
                            action: ImpactAction::KeepOrUndo,
                        });
                    }
                }
            }
            for edge in self.outgoing(decision, EdgeKind::EvidencedBy) {
                if seen.insert(edge.to.clone())
                    && let Some(node) = self.nodes.get(&edge.to)
                {
                    impact.actions_taken.push(ImpactRow {
                        node: node.id.clone(),
                        kind: node.kind,
                        label: node.label.clone(),
                        open: node.open,
                        relation: EdgeKind::EvidencedBy,
                        via: Vec::new(),
                        action: ImpactAction::KeepOrUndo,
                    });
                }
            }
        }
        Some(impact)
    }

    /// Text an amend clause is matched against: a node's own label, plus the
    /// latest answer for a decision.
    fn dependent_text(&self, node: &Node) -> String {
        let mut text = node.label.clone();
        if node.kind == NodeKind::Decision
            && let Some(latest) = self.revisions_of(&node.id).last()
        {
            text.push(' ');
            text.push_str(&latest.label);
        }
        text
    }
}

/// One revision in a why-was-this-decided answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhyRevision {
    pub id: String,
    /// The operator's words, verbatim.
    pub answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_at: Option<u64>,
    /// `initial`, `restates`, `amends` or `supersedes`.
    pub relation: String,
    pub relation_source: EdgeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clause: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    pub in_document: bool,
}

/// A decision this one assumes, with its current answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhyPremise {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Decision ids walked from the asked decision to this premise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
}

/// Why a decision holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Why {
    pub decision_id: String,
    pub question: String,
    /// The answer that holds now (latest revision), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_answer: Option<String>,
    /// Every revision, oldest first.
    pub revisions: Vec<WhyRevision>,
    /// Question copies (todo, waiting, request ids).
    pub copies: Vec<String>,
    /// Decisions and items this decision assumes (`depends_on`, transitive).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<WhyPremise>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts_with: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applies_to: Vec<String>,
    /// Evidence on the decision itself (actions recorded on closed copies).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// Operator `same as` merges among its copies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged: Vec<String>,
}

impl DecisionGraph {
    /// Why `decision` holds: the operator's words for every revision with
    /// where and when they were given, the evidence, and what it assumes.
    pub fn why(&self, decision: &str) -> Option<Why> {
        let node = self.nodes.get(decision)?;
        if node.kind != NodeKind::Decision {
            return None;
        }
        let revisions: Vec<WhyRevision> = self
            .revisions_of(decision)
            .into_iter()
            .map(|revision| {
                let relation = self.relation_of(&revision.id);
                WhyRevision {
                    id: revision.id.clone(),
                    answer: revision.label.clone(),
                    location: revision.location.clone(),
                    first_seen_at: revision.first_seen_at,
                    relation: relation
                        .map_or("initial", |edge| edge.kind.as_str())
                        .to_string(),
                    relation_source: relation.map_or(EdgeSource::Derived, |edge| edge.source),
                    clause: relation.and_then(|edge| edge.clause.clone()),
                    evidence: self
                        .outgoing(&revision.id, EdgeKind::EvidencedBy)
                        .map(|edge| edge.to.clone())
                        .collect(),
                    in_document: revision.in_document,
                }
            })
            .collect();
        let copies: Vec<String> = self
            .incoming(decision, EdgeKind::CopyOf)
            .map(|edge| edge.from.clone())
            .collect();
        let members: BTreeSet<String> = std::iter::once(decision.to_string())
            .chain(copies.iter().cloned())
            .collect();
        let mut depends_on = Vec::new();
        let mut seen = members.clone();
        let mut queue: VecDeque<(String, Vec<String>)> =
            VecDeque::from([(decision.to_string(), Vec::new())]);
        while let Some((current, via)) = queue.pop_front() {
            let sources: Vec<String> = std::iter::once(current.clone())
                .chain(
                    self.incoming(&current, EdgeKind::CopyOf)
                        .map(|edge| edge.from.clone()),
                )
                .collect();
            for source in sources {
                for edge in self.outgoing(&source, EdgeKind::DependsOn) {
                    if !seen.insert(edge.to.clone()) {
                        continue;
                    }
                    let Some(premise) = self.nodes.get(&edge.to) else {
                        continue;
                    };
                    let answer = (premise.kind == NodeKind::Decision)
                        .then(|| {
                            self.revisions_of(&premise.id)
                                .last()
                                .map(|r| r.label.clone())
                        })
                        .flatten();
                    depends_on.push(WhyPremise {
                        id: premise.id.clone(),
                        kind: premise.kind,
                        label: premise.label.clone(),
                        answer,
                        via: via.clone(),
                    });
                    if premise.kind == NodeKind::Decision {
                        let mut path = via.clone();
                        path.push(current.clone());
                        queue.push_back((premise.id.clone(), path));
                    }
                }
            }
        }
        let related = |kind: EdgeKind| -> Vec<String> {
            let mut out: BTreeSet<String> = BTreeSet::new();
            for member in &members {
                out.extend(self.outgoing(member, kind).map(|edge| edge.to.clone()));
                if matches!(kind, EdgeKind::ConflictsWith | EdgeKind::AppliesTo) {
                    out.extend(self.incoming(member, kind).map(|edge| edge.from.clone()));
                }
            }
            out.retain(|id| !members.contains(id));
            out.into_iter().collect()
        };
        let mut merged: BTreeSet<String> = BTreeSet::new();
        for member in &members {
            for edge in self.outgoing(member, EdgeKind::SameAs) {
                merged.insert(format!("{} same_as {}", edge.from, edge.to));
            }
        }
        Some(Why {
            decision_id: decision.to_string(),
            question: node.label.clone(),
            current_answer: revisions.last().map(|revision| revision.answer.clone()),
            revisions,
            copies,
            depends_on,
            conflicts_with: related(EdgeKind::ConflictsWith),
            applies_to: related(EdgeKind::AppliesTo),
            evidence: self
                .outgoing(decision, EdgeKind::EvidencedBy)
                .map(|edge| edge.to.clone())
                .collect(),
            merged: merged.into_iter().collect(),
        })
    }
}

/// One line per flagged row and action, for the cycle contract.
pub fn render_impact(impact: &Impact) -> Vec<String> {
    let mut lines = Vec::new();
    for row in &impact.flagged {
        let verb = match row.action {
            ImpactAction::Reopen => "reopen",
            ImpactAction::ReviewClause => "review the amended clause",
            _ => "re-evaluate",
        };
        lines.push(format!(
            "{verb} #{} ({}): “{}”",
            row.node,
            row.relation.as_str(),
            row.label
        ));
    }
    for row in &impact.actions_taken {
        lines.push(format!(
            "already done on the old answer, ask the operator to keep or undo: {} (“{}”)",
            row.node, row.label
        ));
    }
    lines
}

#[cfg(test)]
mod tests;
