//! Operator answer-ledger enforcement at preflight and closeout
//! (`#answerbyrequestid`).
//!
//! Every preflight and every strict closeout runs [`run_answer_ledger_maintenance`]:
//! it reads the document through the same authority as todo-id maintenance,
//! builds the ledger from the document, its done archives and the state.db
//! index, closes duplicate todo copies of answered questions, refreshes the
//! index, and returns the "already answered" facts for the cycle contract.
//! [`reasks_for_closeout`] is the pre-write gate a closeout runs on the
//! request packets and `--todo-add` texts it is about to record. No agent
//! command is involved in any of it.
//!
//! `#decisionidstamp`: the pass also stamps every open todo copy of an
//! answered question with its decision id, rebuilds the index once from the
//! document's git history, and [`probe_answer_ledger`] reports the same facts
//! read-only for `preflight --probe`.
//!
//! Preflight alone ([`run_answer_ledger_maintenance_with_recall`]) also asks
//! the optional tsift recall source ([`crate::answer_recall`]) for semantic
//! neighbours of the open, unanswered questions, inside this document and in
//! other session documents (`#decisiontsiftrecall`). It runs after the
//! stamped pass, the graph build and the index refresh, so the index and graph
//! never depend on tsift. The results stay "possibly answered" notes; without
//! tsift the pass is exactly the in-process one.
//!
//! The index is pruned by relevance (`#decisionprune`): each pass stores the
//! ledger's hot / archived partition, so the hot tables hold only live
//! decisions (liveness also follows the decision graph's edges), and loads the
//! archived decisions the document (or the closeout's new questions) names, so
//! a dormant decision revives on demand.

use std::collections::BTreeSet;
use std::path::Path;

use agent_doc_answer_ledger::graph::{DecisionGraph, EdgeSource, GraphEdge, Node, NodeKind};
use agent_doc_answer_ledger::recall::{AnswerRecall, OtherDocument, RecallOptions, RecalledAnswer};
use agent_doc_answer_ledger::{
    AnsweredFact, DuplicateClosure, Ledger, PossibleMatch, PriorIndex, PriorKey, PriorRevision,
    Reask, SemanticNeighbours,
};
use anyhow::Result;

use crate::answer_recall::{RecallEnv, TsiftRecall};

/// What one ledger pass produced.
#[derive(Debug, Clone, Default)]
pub struct AnswerLedgerReport {
    pub facts: Vec<AnsweredFact>,
    pub possible: Vec<PossibleMatch>,
    pub closed: Vec<DuplicateClosure>,
    /// Cross-document "possibly answered elsewhere" notes (tsift recall).
    pub recalled: Vec<RecalledAnswer>,
    /// `#decisionstaleaudit`: open todo rows a delivered operator answer
    /// already settled (identity tier), closed by this pass.
    pub stale_closed: Vec<DuplicateClosure>,
    /// Archived decisions a new question named this pass (`#decisionprune`).
    pub revived: Vec<String>,
    pub hot_store: agent_doc_answer_ledger::HotStoreSize,
    /// `#decisionsearchmenu`: ids of open items that ask the operator
    /// something (open question copies, tracked items worded as decisions).
    pub question_items: BTreeSet<String>,
    /// `#decisionsearchmenu`: answered decisions the ledger holds.
    pub answered_decisions: usize,
}

impl AnswerLedgerReport {
    /// Cycle-contract guidance; `None` when nothing is answered, possible or
    /// recalled.
    pub fn guidance(&self) -> Option<String> {
        let parts: Vec<String> = [
            agent_doc_answer_ledger::contract_guidance(&self.facts, &self.possible),
            agent_doc_answer_ledger::recall::recall_guidance(&self.recalled),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
    }

    /// `#decisionsearchmenu`: the short decision-lookup menu for the cycle
    /// contract, present only when this turn is about to ask the operator
    /// something ([`agent_doc_answer_ledger::search::question_cue`]) and
    /// there is an answer to look up. `prompts` are the turn's operator
    /// prompts; `file` is the document as the agent names it.
    pub fn lookup_menu(&self, file: &str, prompts: &[&str]) -> Option<String> {
        use agent_doc_answer_ledger::search;
        search::question_cue(&search::CueInputs {
            prompts,
            question_items: &self.question_items,
            answered_decisions: self.answered_decisions,
            answers_pushed: !self.facts.is_empty()
                || !self.possible.is_empty()
                || !self.recalled.is_empty(),
        })
        .map(|_| search::lookup_menu(file))
    }

    /// `#decisionstaleaudit`: todo signals computed before this pass, minus
    /// the ones about todo rows this pass closed (their audit obligation is
    /// already met, so the contract never asks to close a closed row).
    pub fn without_closed_todos(&self, signals: &[crate::TodoSignal]) -> Vec<crate::TodoSignal> {
        let closed: std::collections::BTreeSet<String> = self
            .closed
            .iter()
            .chain(&self.stale_closed)
            .map(|closure| closure.todo_id.to_ascii_lowercase())
            .collect();
        signals
            .iter()
            .filter(|signal| {
                let about_todo_row = match signal.kind.as_str() {
                    "tracked_work_missing_todo_backlink" => false,
                    "stale_tracked_item" => signal.component.as_deref() == Some("todo"),
                    _ => true,
                };
                !(about_todo_row && closed.contains(&signal.todo.to_ascii_lowercase()))
            })
            .cloned()
            .collect()
    }
}

fn archive_texts(file: &Path, content: &str) -> Vec<String> {
    match agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, content) {
        Ok(sources) => sources.into_iter().map(|source| source.content).collect(),
        Err(err) => {
            eprintln!(
                "[preflight] answer ledger: done archive unreadable ({err:#}); using the document only"
            );
            Vec::new()
        }
    }
}

fn index_target(file: &Path) -> Option<(std::path::PathBuf, String)> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    Some((root, agent_doc_hash::document_id_for_path(file)))
}

fn prior_from_rows(
    keys: Vec<agent_doc_sqlite::answer_ledger_index::AnswerKeyRow>,
    revisions: Vec<agent_doc_sqlite::answer_ledger_index::AnswerRevisionRow>,
) -> PriorIndex {
    PriorIndex {
        keys: keys
            .into_iter()
            .map(|row| PriorKey {
                key: row.key,
                decision_id: row.decision_id,
                first_seen_at: row.first_seen_at,
            })
            .collect(),
        revisions: revisions
            .into_iter()
            .map(|row| PriorRevision {
                decision_id: row.decision_id,
                answer: row.answer,
                location: row.location,
                relation: row.relation,
                relation_source: EdgeSource::parse(&row.relation_source).unwrap_or_default(),
                first_seen_at: row.first_seen_at,
                change_flagged: row.change_flagged,
            })
            .collect(),
        archived_decisions: BTreeSet::new(),
    }
}

/// The hot index rows plus the archived decisions `lookup_keys` name
/// (`#decisionprune`): a dormant decision revives as soon as the document, or
/// a question a closeout is about to add, names one of its keys.
fn load_prior(file: &Path, lookup_keys: &BTreeSet<String>) -> PriorIndex {
    let Some((root, document_id)) = index_target(file) else {
        return PriorIndex::default();
    };
    match agent_doc_sqlite::answer_ledger_index::load_with_archive_for_project(
        &root,
        &document_id,
        lookup_keys,
    ) {
        None => PriorIndex::default(),
        Some(Ok(loaded)) => PriorIndex {
            archived_decisions: loaded.archived_decisions,
            ..prior_from_rows(loaded.keys, loaded.revisions)
        },
        Some(Err(err)) => {
            eprintln!(
                "[preflight] answer ledger: index unreadable ({err:#}); rebuilding from the document"
            );
            PriorIndex::default()
        }
    }
}

fn key_rows(keys: Vec<PriorKey>) -> Vec<agent_doc_sqlite::answer_ledger_index::AnswerKeyRow> {
    keys.into_iter()
        .map(|key| agent_doc_sqlite::answer_ledger_index::AnswerKeyRow {
            key: key.key,
            decision_id: key.decision_id,
            first_seen_at: key.first_seen_at,
        })
        .collect()
}

fn revision_rows(
    revisions: Vec<PriorRevision>,
) -> Vec<agent_doc_sqlite::answer_ledger_index::AnswerRevisionRow> {
    revisions
        .into_iter()
        .map(
            |revision| agent_doc_sqlite::answer_ledger_index::AnswerRevisionRow {
                decision_id: revision.decision_id,
                answer: revision.answer,
                location: revision.location,
                relation: revision.relation,
                relation_source: revision.relation_source.as_str().to_string(),
                first_seen_at: revision.first_seen_at,
                change_flagged: revision.change_flagged,
            },
        )
        .collect()
}

/// Ledger edges (the history rebuild's) as index rows. Operator link kinds
/// keep their source; everything else the rebuild read off the document.
fn edge_rows<'a>(
    edges: impl IntoIterator<Item = &'a agent_doc_answer_ledger::Edge>,
) -> Vec<agent_doc_sqlite::answer_ledger_index::AnswerEdgeRow> {
    use agent_doc_answer_ledger::EdgeKind;
    edges
        .into_iter()
        .map(
            |edge| agent_doc_sqlite::answer_ledger_index::AnswerEdgeRow {
                from: edge.from.clone(),
                kind: edge.kind.as_str().to_string(),
                to: edge.to.clone(),
                source: match edge.kind {
                    EdgeKind::SameAs | EdgeKind::DependsOn | EdgeKind::ConflictsWith => {
                        EdgeSource::Operator
                    }
                    kind if kind.is_revision_relation() => EdgeSource::Proposed,
                    _ => EdgeSource::Derived,
                }
                .as_str()
                .to_string(),
                clause: None,
            },
        )
        .collect()
}

/// Decision-graph nodes and edges as index rows (`#decisiongraph`).
fn graph_rows(
    graph: &DecisionGraph,
) -> (
    Vec<agent_doc_sqlite::answer_ledger_index::AnswerNodeRow>,
    Vec<agent_doc_sqlite::answer_ledger_index::AnswerEdgeRow>,
) {
    let nodes = graph
        .nodes
        .values()
        .filter(|node| node.in_document)
        .map(
            |node| agent_doc_sqlite::answer_ledger_index::AnswerNodeRow {
                id: node.id.clone(),
                kind: node.kind.as_str().to_string(),
                label: node.label.clone(),
                open: node.open,
                location: node.location.clone(),
                first_seen_at: node.first_seen_at,
            },
        )
        .collect();
    let edges = graph
        .edges
        .iter()
        .map(
            |edge| agent_doc_sqlite::answer_ledger_index::AnswerEdgeRow {
                from: edge.from.clone(),
                kind: edge.kind.as_str().to_string(),
                to: edge.to.clone(),
                source: edge.source.as_str().to_string(),
                clause: edge.clause.clone(),
            },
        )
        .collect();
    (nodes, edges)
}

/// The cached graph rows for `file`, as graph values. Rows with an unknown
/// kind (written by a newer binary) are skipped. Read-only.
fn load_prior_graph(file: &Path) -> (Vec<Node>, Vec<GraphEdge>) {
    let Some((root, document_id)) = index_target(file) else {
        return (Vec::new(), Vec::new());
    };
    match agent_doc_sqlite::answer_ledger_index::load_graph_for_project(&root, &document_id) {
        None => (Vec::new(), Vec::new()),
        Some(Ok((nodes, edges))) => (
            nodes
                .into_iter()
                .filter_map(|row| {
                    Some(Node {
                        kind: NodeKind::parse(&row.kind)?,
                        id: row.id,
                        label: row.label,
                        open: row.open,
                        location: row.location,
                        first_seen_at: row.first_seen_at,
                        in_document: false,
                    })
                })
                .collect(),
            edges
                .into_iter()
                .filter_map(|row| {
                    Some(GraphEdge {
                        kind: agent_doc_answer_ledger::EdgeKind::parse(&row.kind)?,
                        source: EdgeSource::parse(&row.source).unwrap_or_default(),
                        from: row.from,
                        to: row.to,
                        clause: row.clause,
                    })
                })
                .collect(),
        ),
        Some(Err(err)) => {
            eprintln!(
                "[preflight] decision graph: cache unreadable ({err:#}); using the document only"
            );
            (Vec::new(), Vec::new())
        }
    }
}

/// The ledger's decision graph plus cached history the document no longer
/// shows (`#decisiongraph`). Read-only.
fn graph_of(
    file: &Path,
    ledger: &Ledger,
    observations: &agent_doc_answer_ledger::extract::Observations,
) -> DecisionGraph {
    let mut graph = DecisionGraph::build(ledger, observations);
    let (nodes, edges) = load_prior_graph(file);
    graph.merge_prior(nodes, edges);
    graph
}

/// The decision graph for `content` (`#decisiongraph`), with the project
/// `decisions.md` records and their `supersedes` links overlaid
/// (`#decisionregistrygraph`; never cached). Read-only.
pub fn build_graph(file: &Path, content: &str) -> (Ledger, DecisionGraph) {
    let (observations, ledger) = build_ledger(file, content);
    let mut graph = graph_of(file, &ledger, &observations);
    crate::decision_registry_graph::overlay_project_registry(file, &mut graph, &observations);
    (ledger, graph)
}

/// Fill `change_impact` on every fact whose latest revision changes or
/// amends the decision.
fn attach_change_impact(facts: &mut [AnsweredFact], graph: &DecisionGraph) {
    for fact in facts {
        if fact.changed_from.is_none() && fact.amends.is_none() {
            continue;
        }
        if let Some(impact) = graph.impact(&fact.decision_id, None) {
            fact.change_impact = agent_doc_answer_ledger::graph::render_impact(&impact);
        }
    }
}

fn store_index(file: &Path, ledger: &Ledger, graph: &DecisionGraph, now: u64) {
    let Some((root, document_id)) = index_target(file) else {
        return;
    };
    // `#decisionprune`: live rows go to the hot tables; dormant decisions
    // and surfaced superseded revisions only to the append-only archive.
    let partition = ledger.partitioned_index_rows();
    let (nodes, edges) = graph_rows(graph);
    if let Err(err) = agent_doc_sqlite::answer_ledger_index::store_partitioned_for_project(
        &root,
        &document_id,
        &key_rows(partition.hot_keys),
        &revision_rows(partition.hot_revisions),
        &key_rows(partition.archived_keys),
        &revision_rows(partition.archived_revisions),
        &edges,
        now,
    ) {
        eprintln!("[preflight] answer ledger: index not refreshed ({err:#})");
    }
    if let Err(err) = agent_doc_sqlite::answer_ledger_index::store_nodes_for_project(
        &root,
        &document_id,
        &nodes,
        now,
    ) {
        eprintln!("[preflight] decision graph: nodes not refreshed ({err:#})");
    }
}

/// Committed versions a one-time history rebuild reads at most, spread evenly
/// over the document's whole history (`#decisionidstamp`). Session documents
/// commit every cycle, so an unbounded walk could read thousands of versions.
pub const ANSWER_LEDGER_HISTORY_MAX_VERSIONS: usize = 512;

/// `#decisionidstamp`: once per document, rebuild the index from the
/// document's committed versions so answers whose rows were pruned before the
/// first index write are on record. Runs only where the index is writable;
/// a failure is reported and the pass continues with the index as it was.
fn seed_index_from_history_once(
    file: &Path,
    content: &str,
    prior: PriorIndex,
    lookup_keys: &BTreeSet<String>,
    now: u64,
) -> PriorIndex {
    let Some((root, document_id)) = index_target(file) else {
        return prior;
    };
    match agent_doc_sqlite::answer_ledger_index::history_seeded_for_project(&root, &document_id) {
        Some(Ok(false)) => {}
        Some(Err(err)) => {
            eprintln!("[preflight] answer ledger: history rebuild state unreadable ({err:#})");
            return prior;
        }
        _ => return prior,
    }
    let versions = agent_doc_git_io::revision::sampled_file_revisions(
        file,
        ANSWER_LEDGER_HISTORY_MAX_VERSIONS,
    )
    .unwrap_or_else(|err| {
        eprintln!("[preflight] answer ledger: git history unreadable ({err:#}); seeding from the document only");
        Vec::new()
    });
    let short: Vec<String> = versions
        .iter()
        .map(|revision| revision.sha.chars().take(10).collect())
        .collect();
    let history: Vec<agent_doc_answer_ledger::history::HistoryVersion<'_>> = versions
        .iter()
        .zip(&short)
        .map(
            |(revision, short)| agent_doc_answer_ledger::history::HistoryVersion {
                committed_at: revision.committed_secs,
                revision: short,
                content: &revision.content,
            },
        )
        .collect();
    let current = observe_document(file, content);
    let seed = agent_doc_answer_ledger::history::seed_index(&history, &current, &prior, now);
    if let Err(err) = agent_doc_sqlite::answer_ledger_index::store_history_seed_for_project(
        &root,
        &document_id,
        &key_rows(seed.index.keys),
        &revision_rows(seed.index.revisions),
        &edge_rows(&seed.edges),
        versions.len(),
        seed.recovered,
        now,
    ) {
        eprintln!("[preflight] answer ledger: history rebuild not stored ({err:#})");
        return prior;
    }
    if seed.recovered > 0 {
        eprintln!(
            "[preflight] answer ledger: rebuilt the index from {} committed version(s); {} answer(s) no longer in the document recovered (#decisionidstamp)",
            versions.len(),
            seed.recovered
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "answer_ledger_history_seeded file={} versions={} recovered={}",
            file.display(),
            versions.len(),
            seed.recovered
        ),
    );
    load_prior(file, lookup_keys)
}

fn observe_document(file: &Path, content: &str) -> agent_doc_answer_ledger::extract::Observations {
    let archives = archive_texts(file, content);
    let refs: Vec<&str> = archives.iter().map(String::as_str).collect();
    agent_doc_answer_ledger::extract::observe(content, &refs)
}

fn build_with_prior(
    file: &Path,
    content: &str,
    prior: &PriorIndex,
    now: u64,
) -> (agent_doc_answer_ledger::extract::Observations, Ledger) {
    build_with_neighbours(file, content, prior, now, None)
}

fn build_with_neighbours(
    file: &Path,
    content: &str,
    prior: &PriorIndex,
    now: u64,
    neighbours: Option<&dyn SemanticNeighbours>,
) -> (agent_doc_answer_ledger::extract::Observations, Ledger) {
    let observations = observe_document(file, content);
    let ledger = Ledger::build(&observations, prior, now, neighbours);
    (observations, ledger)
}

/// Build the ledger for `content` (read-only: no index write).
pub fn build_ledger(
    file: &Path,
    content: &str,
) -> (agent_doc_answer_ledger::extract::Observations, Ledger) {
    build_ledger_naming(file, content, &BTreeSet::new())
}

/// The index keys `content` names, plus `extra_keys` (`#decisionprune`).
fn lookup_keys(file: &Path, content: &str, extra_keys: &BTreeSet<String>) -> BTreeSet<String> {
    let mut keys = observe_document(file, content).lookup_keys();
    keys.extend(extra_keys.iter().cloned());
    keys
}

/// [`build_ledger`], also loading the archived decisions `extra_keys` name
/// (the keys of questions a closeout is about to add).
pub fn build_ledger_naming(
    file: &Path,
    content: &str,
    extra_keys: &BTreeSet<String>,
) -> (agent_doc_answer_ledger::extract::Observations, Ledger) {
    build_with_prior(
        file,
        content,
        &load_prior(file, &lookup_keys(file, content, extra_keys)),
        agent_doc_log_time::current_epoch_secs(),
    )
}

/// Largest other document recall reads.
const MAX_RECALL_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

fn canonical(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The canonical paths recall never reads as "another document": `file`
/// itself and its done archives.
fn recall_excluded(file: &Path, content: &str) -> BTreeSet<std::path::PathBuf> {
    let mut excluded: BTreeSet<std::path::PathBuf> =
        agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, content)
            .map(|sources| {
                sources
                    .into_iter()
                    .map(|source| canonical(&source.path))
                    .collect()
            })
            .unwrap_or_default();
    excluded.insert(canonical(file));
    excluded
}

/// Another session document a recall hit names, built from its text alone;
/// `None` when excluded, too large, unreadable, or holding no answer.
fn load_other_document(
    path: &Path,
    excluded: &BTreeSet<std::path::PathBuf>,
    root: &Path,
    now: u64,
) -> Option<OtherDocument> {
    if excluded.contains(path) || std::fs::metadata(path).ok()?.len() > MAX_RECALL_DOCUMENT_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let rows = agent_doc_answer_ledger::extract::observe(&text, &[]);
    if rows.answers.is_empty() {
        return None;
    }
    Some(OtherDocument {
        display: path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned(),
        ledger: Ledger::build(&rows, &PriorIndex::default(), now, None),
        rows,
    })
}

/// `#decisiontsiftrecall`: one recall pass over `ledger` (built from
/// `content`). Returns the ledger rebuilt with in-document neighbours (when
/// any) and the cross-document notes. Annotation only.
fn recall_pass(
    file: &Path,
    content: &str,
    prior: &PriorIndex,
    now: u64,
    ledger: Ledger,
    source: &dyn AnswerRecall,
    root: &Path,
) -> (Ledger, Vec<RecalledAnswer>) {
    let here = canonical(file);
    let excluded = recall_excluded(file, content);
    let rows = agent_doc_answer_ledger::extract::observe(content, &[]);
    let mut load = |path: &str| load_other_document(Path::new(path), &excluded, root, now);
    let pass = agent_doc_answer_ledger::recall::run_recall(
        &ledger,
        &rows,
        &here.to_string_lossy(),
        source,
        RecallOptions::default(),
        &mut load,
    );
    if pass.queries > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "answer_ledger_recall file={} queries={} neighbours={} recalled={}",
                file.display(),
                pass.queries,
                pass.neighbours.len(),
                pass.recalled.len()
            ),
        );
    }
    let ledger = if pass.neighbours.is_empty() {
        ledger
    } else {
        build_with_neighbours(file, content, prior, now, Some(&pass.neighbours)).1
    };
    (ledger, pass.recalled)
}

/// Decision-graph edges by kind name, for liveness (`#decisionprune`).
fn named_edges(graph: &DecisionGraph) -> Vec<agent_doc_answer_ledger::NamedEdge> {
    graph
        .edges
        .iter()
        .map(|edge| agent_doc_answer_ledger::NamedEdge {
            from: edge.from.clone(),
            kind: edge.kind.as_str().to_string(),
            to: edge.to.clone(),
        })
        .collect()
}

/// One planned ledger pass over `content`, nothing written yet.
struct LedgerPass {
    ledger: Ledger,
    /// The observations `ledger` was built from (for the decision graph).
    observations: agent_doc_answer_ledger::extract::Observations,
    closed: Vec<DuplicateClosure>,
    /// `#decisionstaleaudit`: settled todo rows this pass closes.
    stale_closed: Vec<DuplicateClosure>,
    /// Todo copies whose decision stamp this pass writes.
    stamped: Vec<(String, String)>,
    /// The document after closures and stamps, when either changed it.
    target: Option<String>,
}

/// Close duplicate copies of answered questions, then stamp each remaining
/// open copy with its decision id (`#decisionidstamp`), in memory.
fn plan_pass(file: &Path, content: &str, prior: &PriorIndex, now: u64) -> Result<LedgerPass> {
    let (observations, ledger) = build_with_prior(file, content, prior, now);
    let closed = ledger.duplicate_closures(&observations);
    // `#decisionstaleaudit`: identity-tier stale todo rows close in the same
    // write; every other stale finding is only a signal.
    let stale_closed: Vec<DuplicateClosure> = ledger
        .stale_auto_closures(&observations)
        .into_iter()
        .filter(|stale| !closed.iter().any(|dup| dup.todo_id == stale.todo_id))
        .collect();
    let all_closed: Vec<DuplicateClosure> =
        closed.iter().chain(stale_closed.iter()).cloned().collect();
    let mut target = content.to_string();
    let mut ledger = ledger;
    let mut observations = observations;
    if !all_closed.is_empty()
        && let Some(next) = agent_doc_answer_ledger::apply_duplicate_closures(&target, &all_closed)?
    {
        target = next;
        (observations, ledger) = build_with_prior(file, &target, prior, now);
    }
    let stamps = ledger.decision_stamps();
    let mut stamped = Vec::new();
    if let Some(next) = agent_doc_answer_ledger::apply_decision_stamps(&target, &stamps) {
        let before = agent_doc_answer_ledger::extract::observe(&target, &[]);
        let after = agent_doc_answer_ledger::extract::observe(&next, &[]);
        for question in &after.questions {
            if let (Some(id), Some(stamp)) = (&question.id, &question.stamp)
                && !before
                    .questions
                    .iter()
                    .any(|old| old.id.as_ref() == Some(id) && old.stamp.as_ref() == Some(stamp))
            {
                stamped.push((id.clone(), stamp.clone()));
            }
        }
        target = next;
    }
    Ok(LedgerPass {
        ledger,
        observations,
        closed,
        stale_closed,
        stamped,
        target: (target != content).then_some(target),
    })
}

fn report_of(
    ledger: &Ledger,
    observations: &agent_doc_answer_ledger::extract::Observations,
    graph: &DecisionGraph,
    closed: Vec<DuplicateClosure>,
    stale_closed: Vec<DuplicateClosure>,
) -> AnswerLedgerReport {
    let mut facts = ledger.answered_facts();
    attach_change_impact(&mut facts, graph);
    AnswerLedgerReport {
        facts,
        possible: ledger.possible.clone(),
        closed,
        recalled: Vec::new(),
        stale_closed,
        revived: ledger.liveness.revived.iter().cloned().collect(),
        hot_store: ledger.hot_store_size(),
        question_items: agent_doc_answer_ledger::search::question_item_ids(observations),
        answered_decisions: ledger
            .decisions
            .iter()
            .filter(|decision| !decision.revisions.is_empty())
            .count(),
    }
}

fn read_authoritative(file: &Path, force_disk: bool) -> Result<Option<String>> {
    if force_disk {
        return Ok(std::fs::read_to_string(file).ok());
    }
    Ok(
        match super::current_text_via_preflight_authority_retrying(file, "preflight_answer_ledger")?
        {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => Some(text),
            Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
                std::fs::read_to_string(file).ok()
            }
            Some(_) => None,
        },
    )
}

/// Preflight / closeout pass: close duplicate copies of answered questions,
/// stamp the remaining copies with their decision id, refresh the index, and
/// return the facts for the cycle contract.
pub fn run_answer_ledger_maintenance(file: &Path) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, false, None, None)
}

/// Preflight pass: [`run_answer_ledger_maintenance`] plus the optional tsift
/// recall notes (`#decisiontsiftrecall`). Only preflight carries the notes
/// into a cycle contract, so only preflight pays for the search.
pub fn run_answer_ledger_maintenance_with_recall(file: &Path) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_recall_env(file, &RecallEnv::from_process())
}

/// [`run_answer_ledger_maintenance_with_recall`] with an explicit environment.
pub fn run_answer_ledger_maintenance_with_recall_env(
    file: &Path,
    env: &RecallEnv,
) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, false, None, Some(env))
}

/// Force-disk counterpart for an explicitly force-disk strict closeout.
pub fn run_answer_ledger_maintenance_force_disk(
    file: &Path,
    write_effects: &dyn super::PreflightMaintenanceWriteEffects,
) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, true, Some(write_effects), None)
}

/// `#decisionidstamp`: the probe-mode (`preflight --probe`) pass. It reads the
/// document from disk and the index read-only, simulates the closures and
/// stamps the real pass would write, and reports the facts of the simulated
/// document. Nothing is written: not the document, not the index, and no
/// history rebuild.
pub fn probe_answer_ledger(file: &Path) -> Result<AnswerLedgerReport> {
    let Ok(content) = std::fs::read_to_string(file) else {
        return Ok(AnswerLedgerReport::default());
    };
    let now = agent_doc_log_time::current_epoch_secs();
    let keys = lookup_keys(file, &content, &BTreeSet::new());
    let pass = plan_pass(file, &content, &load_prior(file, &keys), now)?;
    let mut ledger = pass.ledger;
    let graph = graph_of(file, &ledger, &pass.observations);
    ledger.recompute_liveness(&pass.observations.open_item_ids, &named_edges(&graph));
    Ok(report_of(
        &ledger,
        &pass.observations,
        &graph,
        pass.closed,
        pass.stale_closed,
    ))
}

fn run_answer_ledger_maintenance_with_options(
    file: &Path,
    force_disk: bool,
    write_effects: Option<&dyn super::PreflightMaintenanceWriteEffects>,
    recall: Option<&RecallEnv>,
) -> Result<AnswerLedgerReport> {
    let source = "preflight_answer_ledger";
    let Some(content) = read_authoritative(file, force_disk)? else {
        return Ok(AnswerLedgerReport::default());
    };
    let now = agent_doc_log_time::current_epoch_secs();
    let keys = lookup_keys(file, &content, &BTreeSet::new());
    let prior = seed_index_from_history_once(file, &content, load_prior(file, &keys), &keys, now);
    let pass = plan_pass(file, &content, &prior, now)?;
    let closed = pass.closed;
    let stale_closed = pass.stale_closed;
    let mut report_ledger = pass.ledger;
    let mut report_observations = pass.observations;
    let mut final_content = content.clone();
    if let Some(target) = pass.target {
        let persisted = if force_disk {
            let effects = write_effects.ok_or_else(|| {
                anyhow::anyhow!("force-disk answer-ledger maintenance requires write effects")
            })?;
            super::persist_pending_maintenance_doc(file, &content, &target, source, true, effects)?;
            target.clone()
        } else {
            let root = agent_doc_project_root_io::project_root_containing(file);
            super::persist_queue_maintenance_doc(file, &target, &content, root.as_deref(), source)?
        };
        // Closures and stamps only touch `agent:todo`, so carrying the todo
        // body keeps the snapshot from reading them as operator edits.
        if let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file)
            && let Some(new_snapshot) = agent_doc_element::todo::carry_todo_body_into_snapshot(
                &snapshot, &content, &persisted,
            )
            && let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
                file,
                &new_snapshot,
                agent_doc_ops_log_io::log_op,
            )
        {
            eprintln!("[preflight] answer ledger: snapshot sync warning: {err}");
        }
        for closure in &closed {
            eprintln!(
                "[preflight] answer ledger: closed duplicate todo [#{}] — already answered `{}`: “{}” ({}) (#answerbyrequestid)",
                closure.todo_id, closure.decision_id, closure.answer, closure.location
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "answer_ledger_duplicate_closed file={} todo={} decision={}",
                    file.display(),
                    closure.todo_id,
                    closure.decision_id
                ),
            );
        }
        for closure in &stale_closed {
            eprintln!(
                "[preflight] answer ledger: closed answered todo [#{}] — already answered `{}`: “{}” ({}) (#decisionstaleaudit)",
                closure.todo_id, closure.decision_id, closure.answer, closure.location
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "answer_ledger_stale_closed file={} todo={} decision={}",
                    file.display(),
                    closure.todo_id,
                    closure.decision_id
                ),
            );
        }
        for (todo, decision) in &pass.stamped {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "answer_ledger_decision_stamped file={} todo={todo} decision={decision}",
                    file.display(),
                ),
            );
        }
        if !pass.stamped.is_empty() {
            eprintln!(
                "[preflight] answer ledger: stamped the decision id on {} todo cop{} (#decisionidstamp)",
                pass.stamped.len(),
                if pass.stamped.len() == 1 { "y" } else { "ies" }
            );
        }
        (report_observations, report_ledger) = build_with_prior(file, &persisted, &prior, now);
        final_content = persisted;
    }
    let graph = graph_of(file, &report_ledger, &report_observations);
    // `#decisionprune` x `#decisiongraph`: graph edges (depends_on,
    // conflicts_with, derived_from, exception_to, ...) carry liveness too.
    report_ledger.recompute_liveness(&report_observations.open_item_ids, &named_edges(&graph));
    store_index(file, &report_ledger, &graph, now);
    // `#firstprinciplesstore`: operator confirm / reject rows for stored
    // derivation steps.
    crate::derivation_store::apply_reviews(file, &report_observations, now);
    // `#decisiontsiftrecall`: annotation only, after the index and graph are
    // stored, so neither depends on whether tsift is available.
    let mut recalled = Vec::new();
    if let Some(source) = recall.and_then(|env| TsiftRecall::discover(file, env)) {
        let root = source.root().to_path_buf();
        let (ledger, notes) = recall_pass(
            file,
            &final_content,
            &prior,
            now,
            report_ledger,
            &source,
            &root,
        );
        report_ledger = ledger;
        // The recall rebuild drops the graph edges from liveness; restore them.
        report_ledger.recompute_liveness(&report_observations.open_item_ids, &named_edges(&graph));
        recalled = notes;
    }
    let mut report = report_of(
        &report_ledger,
        &report_observations,
        &graph,
        closed,
        stale_closed,
    );
    report.recalled = recalled;
    for decision in &report.revived {
        eprintln!(
            "[preflight] answer ledger: revived archived decision `{decision}`: a question names it again (#decisionprune)"
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "answer_ledger_decision_revived file={} decision={decision}",
                file.display()
            ),
        );
    }
    for fact in &report.facts {
        eprintln!(
            "[preflight] answer ledger: already answered `{}` ({}): “{}”",
            fact.decision_id,
            fact.open_items.join(", "),
            fact.answer
        );
    }
    Ok(report)
}

/// `#decisionstaleaudit`: every open tracked item a delivered decision makes
/// stale, read-only (no index write, no document write).
pub fn stale_tracked_items(
    file: &Path,
    content: &str,
) -> Vec<agent_doc_answer_ledger::stale::StaleTrackedItem> {
    let (observations, ledger) = build_ledger(file, content);
    stale_tracked_items_of(file, content, &ledger, &observations)
}

/// `#decisionstaleevidence`: response text for the stale audit from the
/// session document, then the tracked response index and archive. It never
/// scans git history, so a per-cycle audit stays bounded.
struct LocalResponses<'a> {
    file: &'a Path,
    content: &'a str,
}

impl agent_doc_answer_ledger::stale::ResponseSource for LocalResponses<'_> {
    fn response_text(&self, response_id: &str) -> Option<String> {
        agent_doc_archive_io::read_response_locally(
            self.file,
            &format!("[#{response_id}]"),
            Some(self.content),
        )
    }
}

/// `#decisionstaleevidence`: the project `decisions.md` records, as the stale
/// audit reads them. An unreadable registry yields none.
fn registry_records(
    file: &Path,
) -> std::collections::BTreeMap<String, agent_doc_answer_ledger::stale::RegistryRecord> {
    let Some(root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Default::default();
    };
    match agent_doc_decision_io::read_registry(&root) {
        Ok(registry) => registry
            .records
            .into_values()
            .map(|record| {
                (
                    record.id.clone(),
                    agent_doc_answer_ledger::stale::RegistryRecord {
                        current: record.state == agent_doc_decision_io::DecisionState::Accepted,
                        id: record.id,
                        decision: record.decision,
                        rationale: record.rationale,
                        updated_at: record.updated_at,
                        evidence_response_ids: record.evidence_response_ids,
                    },
                )
            })
            .collect(),
        Err(err) => {
            eprintln!(
                "[preflight] stale audit: decision registry unreadable ({err:#}); using the document only"
            );
            Default::default()
        }
    }
}

/// `#decisionstaleevidence`: the stale audit over an already-built ledger,
/// with done-by-evidence read from the decision graph (cached history
/// included), the responses it cites, and the `decisions.md` records items
/// cite. Read-only.
pub fn stale_tracked_items_of(
    file: &Path,
    content: &str,
    ledger: &Ledger,
    observations: &agent_doc_answer_ledger::extract::Observations,
) -> Vec<agent_doc_answer_ledger::stale::StaleTrackedItem> {
    use agent_doc_answer_ledger::stale::{deliverable_anchors, evidence::registry_citations};
    // Evidence only matters for an open item that names a deliverable; skip
    // the cache, archive and registry reads when none does.
    let wanting: Vec<_> = observations
        .tracked
        .iter()
        .filter(|item| item.open && !deliverable_anchors(&item.text).is_empty())
        .collect();
    if wanting.is_empty() {
        return ledger.stale_tracked_items(observations);
    }
    let records = if wanting
        .iter()
        .any(|item| !registry_citations(&item.text).is_empty())
    {
        registry_records(file)
    } else {
        Default::default()
    };
    let graph = graph_of(file, ledger, observations);
    let responses = LocalResponses { file, content };
    let evidence = agent_doc_answer_ledger::stale::DoneEvidence {
        graph: Some(&graph),
        records,
        responses: Some(&responses),
    };
    ledger.stale_tracked_items_with(observations, &evidence)
}

/// Pre-write closeout gate: the questions this closeout would add that the
/// operator already answered and that do not cite the answer.
pub fn reasks_for_closeout(
    file: &Path,
    content: &str,
    response: &str,
    todo_adds: &[String],
) -> Vec<Reask> {
    let new_questions =
        agent_doc_answer_ledger::extract::observe_new_questions(response, todo_adds);
    if new_questions.is_empty() {
        return Vec::new();
    }
    let keys: BTreeSet<String> = new_questions
        .iter()
        .flat_map(|new| agent_doc_answer_ledger::lookup_keys_for(&new.question))
        .collect();
    let (_, ledger) = build_ledger_naming(file, content, &keys);
    ledger.reasks(&new_questions)
}

/// How `decision search` used the optional tsift recall source
/// (`#decisionsearchmenu`). Without it the ranking is the ledger's own.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SearchRecallStatus {
    /// tsift ran; `hits` raw hits came back. `note` says what was skipped.
    Used {
        hits: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// No source: disabled, no index, or no executable.
    Unavailable { detail: String },
    /// The source ran but returned nothing usable (deadline or failure).
    Failed { detail: String },
}

impl SearchRecallStatus {
    /// One line for the human-readable report.
    pub fn summary(&self) -> String {
        match self {
            Self::Used { hits, note: None } => format!("tsift recall: used ({hits} hits)"),
            Self::Used {
                hits,
                note: Some(note),
            } => {
                format!("tsift recall: used ({hits} hits); {note}")
            }
            Self::Unavailable { detail } => {
                format!("tsift recall: unavailable ({detail}); ranked by the answer ledger only")
            }
            Self::Failed { detail } => {
                format!("tsift recall: failed ({detail}); ranked by the answer ledger only")
            }
        }
    }
}

/// The `decisions.md` registry in a project search (`#decisionsearchnofile`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RegistrySearchStatus {
    /// Read; `records` records searched.
    Read { records: usize },
    /// The project has no `decisions.md`.
    Missing,
    /// `decisions.md` exists but could not be read, so it was not searched.
    Unreadable { detail: String },
}

/// A session document a project search found but could not read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SkippedDocument {
    pub document: String,
    pub reason: String,
}

/// What a project search covered (`#decisionsearchnofile`). A document that
/// was not searched is named, so a miss here is never read as "not answered"
/// when the answer could sit in a document the search passed over.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProjectSearchScope {
    pub root: String,
    /// Session documents searched.
    pub documents: usize,
    pub skipped: Vec<SkippedDocument>,
    /// The document scan hit its entry budget, so documents may be missing.
    pub scan_exhausted: bool,
    pub registry: RegistrySearchStatus,
}

/// What `agent-doc decision search` found (`#decisionsearchmenu`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecisionSearchReport {
    pub query: String,
    /// The document searched, when the search named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// What a project search covered, when the search named no document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectSearchScope>,
    pub hits: Vec<agent_doc_answer_ledger::search::SearchHit>,
    pub recall: SearchRecallStatus,
}

/// Raw hits asked of tsift per search.
const SEARCH_RECALL_HITS: usize = 12;
/// Other documents a search reads at most.
const SEARCH_MAX_DOCUMENTS: usize = 8;

/// One document's text and ledger, as a search reads them.
struct LoadedDocument {
    path: std::path::PathBuf,
    display: String,
    content: String,
    ledger: Ledger,
    rows: agent_doc_answer_ledger::extract::Observations,
}

impl LoadedDocument {
    /// `path`'s ledger as `decision answered` sees it (text, done archives,
    /// index), and the rows of its text alone, for recall's line numbers.
    fn load(path: &Path, display: &str, content: String) -> Self {
        let (_, ledger) = build_ledger(path, &content);
        let rows = agent_doc_answer_ledger::extract::observe(&content, &[]);
        Self {
            path: path.to_path_buf(),
            display: display.to_string(),
            content,
            ledger,
            rows,
        }
    }
}

/// Run the optional tsift recall source once over `query` and merge its hits
/// into `hits`, mapped onto `documents` or, for a hit elsewhere, onto another
/// session document. `anchor` locates the project; `stale_note` words the hits
/// skipped because `n` of `documents` are newer than the index.
fn add_search_recall(
    anchor: &Path,
    query: &str,
    env: &RecallEnv,
    documents: &[LoadedDocument],
    stale_note: &dyn Fn(usize) -> String,
    hits: &mut Vec<agent_doc_answer_ledger::search::SearchHit>,
) -> SearchRecallStatus {
    use agent_doc_answer_ledger::search;
    match TsiftRecall::discover_for_search(anchor, env) {
        Err(detail) => SearchRecallStatus::Unavailable {
            detail: detail.to_string(),
        },
        Ok(source) => {
            let (results, outcome) =
                source.recall_with_outcome(&[query.to_string()], SEARCH_RECALL_HITS);
            let raw = results.into_iter().next().unwrap_or_default();
            if raw.is_empty() && outcome.timed_out > 0 {
                SearchRecallStatus::Failed {
                    detail: format!("no result within {}ms", source.deadline().as_millis()),
                }
            } else if raw.is_empty() && outcome.failed > 0 {
                SearchRecallStatus::Failed {
                    detail: "tsift search failed; the index may be stale or missing".to_string(),
                }
            } else {
                // An index older than the document has stale line numbers
                // for it: skip those hits rather than map them to the wrong
                // decision.
                let index_modified = source.index_modified();
                let hits_current: Vec<bool> = documents
                    .iter()
                    .map(|document| {
                        let modified =
                            std::fs::metadata(&document.path).and_then(|meta| meta.modified());
                        match (index_modified, modified) {
                            (Some(index), Ok(document)) => index >= document,
                            _ => true,
                        }
                    })
                    .collect();
                let paths: Vec<String> = documents
                    .iter()
                    .map(|document| canonical(&document.path).to_string_lossy().into_owned())
                    .collect();
                let views: Vec<search::SearchDocument<'_>> = documents
                    .iter()
                    .enumerate()
                    .map(|(index, document)| search::SearchDocument {
                        ledger: &document.ledger,
                        rows: &document.rows,
                        path: &paths[index],
                        display: &document.display,
                        hits_current: hits_current[index],
                    })
                    .collect();
                let now = agent_doc_log_time::current_epoch_secs();
                let root = source.root().to_path_buf();
                // Only a hit outside `documents` needs the excluded set.
                let mut excluded: Option<BTreeSet<std::path::PathBuf>> = None;
                let mut load = |path: &str| {
                    let excluded = excluded.get_or_insert_with(|| {
                        documents
                            .iter()
                            .flat_map(|document| recall_excluded(&document.path, &document.content))
                            .collect()
                    });
                    load_other_document(Path::new(path), excluded, &root, now)
                };
                search::add_recall_hits(hits, &views, &raw, SEARCH_MAX_DOCUMENTS, &mut load);
                let stale = hits_current.iter().filter(|current| !**current).count();
                SearchRecallStatus::Used {
                    hits: raw.len(),
                    note: (stale > 0).then(|| stale_note(stale)),
                }
            }
        }
    }
}

/// `#decisionsearchmenu`: rank the answered decisions of `file` (its text,
/// done archives and index, as `decision answered` sees them) against
/// `query`, best first, plus answered decisions in other session documents
/// the optional tsift recall source finds. Read-only: nothing is written.
/// `display` is the document as the caller names it.
pub fn search_decisions(
    file: &Path,
    display: &str,
    query: &str,
    env: &RecallEnv,
    limit: usize,
) -> Result<DecisionSearchReport> {
    let content = std::fs::read_to_string(file)
        .map_err(|err| anyhow::anyhow!("failed to read {}: {err}", file.display()))?;
    let document = LoadedDocument::load(file, display, content);
    let mut hits = agent_doc_answer_ledger::search::search_ledger(&document.ledger, query, display);
    let recall = add_search_recall(
        file,
        query,
        env,
        std::slice::from_ref(&document),
        &|_| {
            "the tsift index is older than this document, so its hits on this document were skipped (run `tsift status --fix` to reindex)".to_string()
        },
        &mut hits,
    );
    hits.truncate(limit.max(1));
    Ok(DecisionSearchReport {
        query: query.to_string(),
        document: Some(display.to_string()),
        project: None,
        hits,
        recall,
    })
}

/// The `decisions.md` records at `root`, and how reading them went.
fn project_registry(
    root: &Path,
) -> (
    Vec<agent_doc_answer_ledger::graph::RegistryDecision>,
    RegistrySearchStatus,
) {
    if !agent_doc_decision_io::registry_path(root).is_file() {
        return (Vec::new(), RegistrySearchStatus::Missing);
    }
    match agent_doc_decision_io::read_registry(root) {
        Ok(registry) => {
            let records = crate::decision_registry_graph::registry_decisions(&registry);
            let status = RegistrySearchStatus::Read {
                records: records.len(),
            };
            (records, status)
        }
        Err(err) => (
            Vec::new(),
            RegistrySearchStatus::Unreadable {
                detail: format!("{err:#}"),
            },
        ),
    }
}

/// The session documents a project search covers.
#[derive(Debug, Clone, Default)]
pub struct ProjectDocuments {
    pub documents: Vec<std::path::PathBuf>,
    /// The scan that found them ran out of budget.
    pub scan_exhausted: bool,
}

/// `#decisionsearchnofile`: [`search_decisions`] over every session document
/// of the project at `root`, plus its `decisions.md` records. Every hit names
/// its document, relative to `root`. Read-only: nothing is written.
pub fn search_project_decisions(
    root: &Path,
    project: &ProjectDocuments,
    query: &str,
    env: &RecallEnv,
    limit: usize,
) -> Result<DecisionSearchReport> {
    use agent_doc_answer_ledger::search;
    let mut documents = Vec::new();
    let mut skipped = Vec::new();
    for path in &project.documents {
        let display = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let read = match std::fs::metadata(path) {
            Ok(meta) if meta.len() > MAX_RECALL_DOCUMENT_BYTES => Err(format!(
                "{} bytes, over the {MAX_RECALL_DOCUMENT_BYTES}-byte project limit; name it as FILE to search it",
                meta.len()
            )),
            Ok(_) => std::fs::read_to_string(path).map_err(|err| err.to_string()),
            Err(err) => Err(err.to_string()),
        };
        match read {
            Ok(content) => documents.push(LoadedDocument::load(path, &display, content)),
            Err(reason) => skipped.push(SkippedDocument {
                document: display,
                reason,
            }),
        }
    }
    let (records, registry) = project_registry(root);
    let mut hits = search::search_registry(&records, query);
    for document in &documents {
        hits.extend(search::search_ledger(
            &document.ledger,
            query,
            &document.display,
        ));
    }
    search::rank(&mut hits);
    let recall = add_search_recall(
        root,
        query,
        env,
        &documents,
        &|stale| {
            format!(
                "the tsift index is older than {stale} searched documents, so its hits on them were skipped; the ledger still searched them (run `tsift status --fix` to reindex)"
            )
        },
        &mut hits,
    );
    hits.truncate(limit.max(1));
    Ok(DecisionSearchReport {
        query: query.to_string(),
        document: None,
        project: Some(ProjectSearchScope {
            root: root.display().to_string(),
            documents: documents.len(),
            skipped,
            scan_exhausted: project.scan_exhausted,
            registry,
        }),
        hits,
        recall,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DiskEffects;
    impl crate::PreflightMaintenanceWriteEffects for DiskEffects {
        fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}

        fn guard_visible_write_expected_current(
            &self,
            _file: &Path,
            _source: &str,
            _expected: &str,
        ) -> Result<()> {
            Ok(())
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current: &str,
            target: &str,
            _source: &str,
        ) -> Result<()> {
            std::fs::write(file, target)?;
            Ok(())
        }
    }

    fn index_counts(doc: &Path) -> agent_doc_sqlite::answer_ledger_index::IndexCounts {
        let (root, document_id) = index_target(doc).unwrap();
        agent_doc_sqlite::answer_ledger_index::counts_for_project(&root, &document_id)
            .unwrap()
            .unwrap()
    }

    /// `#decisionprune` end to end through state.db: a dormant decision leaves
    /// the hot tables, stays in the archive after the document forgets it,
    /// and revives with its recorded answer when a new question names it.
    #[test]
    fn dormant_decisions_leave_the_hot_tables_and_revive_from_the_archive() {
        const PRUNED: &str = concat!(
            "---\nagent_doc_session: answer-ledger-prune\n---\n\n",
            "<!-- agent:queue -->\n",
            "- ~~[#tdbranch]: Target sdk-dev. Keep 0.3.0-preview.1~~\n",
            // A live answer row keeps `tdport` open: `#decisionstaleaudit`
            // would otherwise close its delivered (struck) answer's todo.
            "- [#tdport]: Use port 8080.\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [x] [#tdbranch] Choose the target branch for the telemetry pull request: sdk-dev or dev\n",
            "- [ ] [#tdport] Choose the demo port for the sign-in redirect: 8080 or 9090\n",
            "<!-- /agent:todo -->\n",
        );
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, PRUNED).unwrap();
        let first = run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        assert_eq!(first.hot_store.hot_decisions, 1, "{:?}", first.hot_store);
        assert_eq!(first.hot_store.archived_decisions, 1);
        let counts = index_counts(&doc);
        // `#decisionidstamp` stamps the open answered todo: id, text and
        // stamp keys.
        assert_eq!(
            (counts.hot_keys, counts.hot_revisions),
            (3, 1),
            "{counts:?}"
        );
        assert_eq!((counts.archived_keys, counts.archived_revisions), (5, 2));

        // The document forgets the dormant decision entirely.
        let forgotten = PRUNED
            .replace("- ~~[#tdbranch]: Target sdk-dev. Keep 0.3.0-preview.1~~\n", "")
            .replace(
                "- [x] [#tdbranch] Choose the target branch for the telemetry pull request: sdk-dev or dev\n",
                "",
            );
        std::fs::write(&doc, &forgotten).unwrap();
        let quiet = run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        assert!(quiet.revived.is_empty());
        assert_eq!(
            quiet.hot_store.decisions, 1,
            "not loaded: {:?}",
            quiet.hot_store
        );
        assert_eq!(index_counts(&doc), counts, "the archive never shrinks");

        // A new question with the same id revives it with its answer.
        let reasked = forgotten.replace(
            "<!-- /agent:todo -->",
            "- [ ] [#tdbranch] Choose the target branch for the telemetry pull request: sdk-dev or dev\n<!-- /agent:todo -->",
        );
        std::fs::write(&doc, &reasked).unwrap();
        let revived = run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        assert_eq!(revived.revived.len(), 1, "{revived:?}");
        let fact = revived
            .facts
            .iter()
            .find(|fact| fact.open_items.contains(&"tdbranch".to_string()))
            .expect("the archived answer is pushed again");
        assert_eq!(fact.answer, "Target sdk-dev. Keep 0.3.0-preview.1");
        let after = index_counts(&doc);
        assert_eq!((after.hot_keys, after.hot_revisions), (6, 2), "{after:?}");

        // A closeout re-asking a dormant decision's question is refused even
        // though the document no longer shows it.
        std::fs::write(&doc, &forgotten).unwrap();
        run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        let reasks = reasks_for_closeout(
            &doc,
            &forgotten,
            "",
            &["[#tdbranch] Choose the target branch for the telemetry pull request: sdk-dev or dev".to_string()],
        );
        assert_eq!(reasks.len(), 1, "{reasks:?}");
    }

    const DOC: &str = concat!(
        "---\nagent_doc_session: answer-ledger\n---\n\n",
        "<!-- agent:queue -->\n",
        "- ~~[#subagent-operator-request-tdport-allowlist]: Use the standard http ports.~~\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:todo -->\n",
        "- [ ] [#tdport] Register the demo sign-in redirect, or pick an already-registered port\n",
        "- [ ] [#subagent-operator-request-tdport-allowlist] Choose how to add the demo callback: stop, studio append, or local demo only\n",
        "<!-- /agent:todo -->\n",
    );

    /// A decision todo and a request copy of it, answered under the request.
    const DECISION_DOC: &str = concat!(
        "---\nagent_doc_session: answer-ledger\n---\n\n",
        "<!-- agent:queue -->\n",
        "- ~~[#subagent-operator-request-tdport-pick]: Use port 3000.~~\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:todo -->\n",
        "- [ ] [#tdport] Choose the demo callback port for local sign-in runs\n",
        "- [ ] [#subagent-operator-request-tdport-pick] Choose the demo callback port for local sign-in runs: 3000 or 47616\n",
        "<!-- /agent:todo -->\n",
    );

    fn tdport_fact(report: &AnswerLedgerReport) -> &AnsweredFact {
        report
            .facts
            .iter()
            .find(|fact| fact.open_items.contains(&"tdport".to_string()))
            .unwrap_or_else(|| panic!("{report:?}"))
    }

    /// `#decisionidstamp`: the copy kept open carries its decision id after
    /// the pass, and the next pass writes nothing.
    #[test]
    fn maintenance_stamps_the_open_copy_with_its_decision_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, DECISION_DOC).unwrap();
        let report = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        let decision = tdport_fact(&report).decision_id.clone();
        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(
            written.contains(&format!(
                "- [ ] [#tdport] Choose the demo callback port for local sign-in runs ⟦decision:{decision}⟧\n"
            )),
            "{written}"
        );
        assert!(
            written.contains("- [x] [#subagent-operator-request-tdport-pick]"),
            "{written}"
        );
        let again = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        assert_eq!(tdport_fact(&again).decision_id, decision);
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            written,
            "idempotent"
        );
    }

    /// `#decisionstaleaudit`: an idle todo whose own id a delivered operator
    /// answer settled closes in the maintenance write (identity tier), once;
    /// a backlog question with the same kind of answer is never closed.
    #[test]
    fn maintenance_closes_a_settled_todo_once_and_leaves_tracked_work_open() {
        const STALE_DOC: &str = concat!(
            "---\nagent_doc_session: stale-audit\n---\n\n",
            "<!-- agent:run id=\"main\" -->\n",
            "- ### ~~[#tdapprovers]: Make a PR to link the ADRs ⟦published⟧~~\n",
            "- ### ~~[#nextsteps]: Run the go-live checklist first.~~\n",
            "<!-- /agent:run -->\n\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#tdapprovers] Name the approver for docs PR #88 and request their review\n",
            "<!-- /agent:todo -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#nextsteps] What are next steps to make the docs site public?\n",
            "<!-- /agent:backlog -->\n",
        );
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, STALE_DOC).unwrap();
        let report = run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        assert!(report.closed.is_empty(), "{report:?}");
        assert_eq!(report.stale_closed.len(), 1, "{report:?}");
        assert_eq!(report.stale_closed[0].todo_id, "tdapprovers");
        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(written.contains("- [x] [#tdapprovers]"), "{written}");
        assert!(written.contains("already answered `"), "{written}");
        assert!(written.contains("- [ ] [#nextsteps]"), "{written}");
        let again = run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        assert!(again.stale_closed.is_empty(), "{again:?}");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), written);
        let stale = stale_tracked_items(&doc, &written);
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert_eq!(stale[0].item, "nextsteps");
        assert!(!stale[0].auto_close);
    }

    /// `#decisionstaleevidence`: done-by-evidence reads the response a
    /// delivered answer cites (from the session document) and the project
    /// `decisions.md` record an item cites, not only the answer text.
    #[test]
    fn stale_audit_reads_cited_responses_and_registry_records() {
        const DOC: &str = concat!(
            "---\nagent_doc_session: stale-evidence\n---\n\n",
            "<!-- agent:queue -->\n",
            "- ~~[#mergedocs]: Done, see [#r-0123456789ab].~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: docs [#r-0123456789ab]\n\n",
            "PR #42 merged into main.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#mergedocs] Merge PR #42.\n",
            "- [ ] [#lrucache] Merge PR #77 per DEC-cache-001.\n",
            "- [ ] [#oldcache] Merge PR #79 per DEC-cache-000.\n",
            "<!-- /agent:backlog -->\n",
        );
        const REGISTRY: &str = concat!(
            "# Decisions\n\nThis file is the authoritative current decision registry.\n\n",
            "<!-- agent:decision id=\"DEC-cache-000\" -->\n## DEC-cache-000\n",
            "- State: superseded\n- Updated: 2026-11-01T00:00:00Z\n- Evidence: (none)\n- Supersedes: (none)\n",
            "### Decision\nUse a FIFO cache; PR #79 merged.\n### Rationale\nSimplest.\n<!-- /agent:decision -->\n\n",
            "<!-- agent:decision id=\"DEC-cache-001\" -->\n## DEC-cache-001\n",
            "- State: accepted\n- Updated: 2026-11-05T00:00:00Z\n- Evidence: (none)\n- Supersedes: DEC-cache-000\n",
            "### Decision\nUse the LRU cache; PR #77 merged.\n### Rationale\nHit rate.\n<!-- /agent:decision -->\n\n",
        );
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join("decisions.md"), REGISTRY).unwrap();
        agent_doc_decision_io::read_registry(dir.path()).expect("fixture registry parses");
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, DOC).unwrap();

        let stale = stale_tracked_items(&doc, DOC);
        let docs = stale
            .iter()
            .find(|found| found.item == "mergedocs")
            .expect("done by the cited response");
        assert_eq!(
            docs.kind,
            agent_doc_answer_ledger::stale::StaleKind::DoneByEvidence
        );
        assert!(
            docs.location
                .starts_with("response [#r-0123456789ab] cited by"),
            "{docs:?}"
        );
        let lru = stale
            .iter()
            .find(|found| found.item == "lrucache")
            .expect("done by the cited registry record");
        assert_eq!(lru.decision_id, "DEC-cache-001");
        assert_eq!(
            lru.link,
            agent_doc_answer_ledger::stale::StaleLink::Registry
        );
        // A superseded record records nothing done.
        assert!(
            stale.iter().all(|found| found.item != "oldcache"),
            "{stale:?}"
        );
        // Without a registry the record citation proves nothing.
        std::fs::remove_file(dir.path().join("decisions.md")).unwrap();
        let stale = stale_tracked_items(&doc, DOC);
        assert!(
            stale.iter().all(|found| found.item != "lrucache"),
            "{stale:?}"
        );
        assert!(
            stale.iter().any(|found| found.item == "mergedocs"),
            "{stale:?}"
        );
    }

    struct NoEffects;
    impl crate::PreflightMaintenanceWriteEffects for NoEffects {
        fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}

        fn guard_visible_write_expected_current(
            &self,
            _file: &Path,
            _source: &str,
            _expected: &str,
        ) -> Result<()> {
            Ok(())
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current: &str,
            target: &str,
            _source: &str,
        ) -> Result<()> {
            std::fs::write(file, target)?;
            Ok(())
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
    }

    /// `#decisiontsiftrecall` fixture: a project with a tsift index marker,
    /// this document (an answered request copy plus an open, unanswered
    /// redirect question) and another session document that answered the
    /// redirect question.
    fn recall_project() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/index.db"), b"").unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/session.md");
        let content = DOC.replace(
            "<!-- /agent:todo -->\n",
            "- [ ] [#redir] Choose the demo sign-in redirect port for local runs\n<!-- /agent:todo -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let other = dir.path().join("tasks/other.md");
        std::fs::write(
            &other,
            concat!(
                "<!-- agent:todo -->\n",
                "- [x] [#portpick] Which port should the demo sign-in redirect use for local runs\n",
                "<!-- /agent:todo -->\n",
                "<!-- agent:queue -->\n",
                "- ~~[#portpick]: Use port 8443.~~\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        (dir, doc, other)
    }

    /// A fake `tsift` that prints one search envelope hitting `other` line 2,
    /// after `delay` seconds.
    fn fake_tsift(dir: &Path, other: &Path, delay: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin_dir = dir.join("fakebin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script = bin_dir.join("tsift");
        let report = serde_json::json!({
            "tool": "search",
            "report": {"ranked": [
                {"path": other.to_string_lossy(), "line": 2, "score": 7.5, "source": "vector_chunk"}
            ]}
        });
        std::fs::write(
            &script,
            format!("#!/bin/sh\nsleep {delay}\ncat <<'JSON'\n{report}\nJSON\n"),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin_dir
    }

    fn recall_env(path: &Path, timeout_ms: u64) -> RecallEnv {
        RecallEnv {
            disabled: false,
            tsift_bin: None,
            path: Some(path.as_os_str().to_owned()),
            timeout_ms: Some(timeout_ms),
            search_timeout_ms: Some(timeout_ms),
        }
    }

    /// No tsift on PATH: recall is not discovered and the pass is exactly the
    /// in-process one (tiers 1-2 act, no semantic or cross-document notes).
    #[test]
    fn recall_without_tsift_on_path_degrades_to_the_in_process_tiers() {
        let (dir, doc, _) = recall_project();
        let empty_path = dir.path().join("empty-path");
        std::fs::create_dir_all(&empty_path).unwrap();
        let env = recall_env(&empty_path, 2000);
        assert!(TsiftRecall::discover(&doc, &env).is_none());

        let report =
            run_answer_ledger_maintenance_with_options(&doc, true, Some(&NoEffects), Some(&env))
                .unwrap();
        // The acting tiers still close the answered request copy.
        assert_eq!(report.closed.len(), 1, "{report:?}");
        assert!(report.recalled.is_empty(), "{report:?}");
        assert!(
            report
                .possible
                .iter()
                .all(|candidate| candidate.tier != agent_doc_answer_ledger::MatchTier::Semantic)
        );
        let guidance = report.guidance().unwrap_or_default();
        assert!(!guidance.contains("#decisiontsiftrecall"), "{guidance}");
    }

    /// A project without a tsift index never looks for the executable.
    #[test]
    fn recall_needs_a_tsift_index() {
        let (dir, doc, other) = recall_project();
        std::fs::remove_file(dir.path().join(".tsift/index.db")).unwrap();
        let bin_dir = fake_tsift(dir.path(), &other, "0");
        assert!(TsiftRecall::discover(&doc, &recall_env(&bin_dir, 2000)).is_none());
        let disabled = RecallEnv {
            disabled: true,
            ..recall_env(&bin_dir, 2000)
        };
        std::fs::write(dir.path().join(".tsift/index.db"), b"").unwrap();
        assert!(TsiftRecall::discover(&doc, &disabled).is_none());
    }

    /// With tsift present, an answer in another document becomes a
    /// "possibly answered elsewhere" note; nothing closes because of it.
    #[test]
    fn recall_with_tsift_notes_answers_from_other_documents() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "0");
        let env = recall_env(&bin_dir, 10_000);
        assert!(TsiftRecall::discover(&doc, &env).is_some());

        let report =
            run_answer_ledger_maintenance_with_options(&doc, true, Some(&NoEffects), Some(&env))
                .unwrap();
        assert_eq!(report.closed.len(), 1, "{report:?}");
        // The fake answers every open question with the same hit.
        let note = report
            .recalled
            .iter()
            .find(|note| note.open_items == ["redir"])
            .unwrap_or_else(|| panic!("{report:?}"));
        assert_eq!(note.document, "tasks/other.md");
        assert_eq!(note.answer, "Use port 8443.");
        assert_eq!(note.tier, agent_doc_answer_ledger::MatchTier::Semantic);
        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(written.contains("- [ ] [#redir]"), "{written}");
        let guidance = report.guidance().unwrap();
        assert!(
            guidance.contains("possibly answered elsewhere"),
            "{guidance}"
        );
        assert!(guidance.contains("Use port 8443."), "{guidance}");
    }

    /// A tsift that outlives the deadline is killed; the pass returns
    /// without notes well before the fake would have answered.
    #[test]
    fn recall_is_bounded_by_its_deadline() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "30");
        let env = recall_env(&bin_dir, 300);
        let started = std::time::Instant::now();
        let report =
            run_answer_ledger_maintenance_with_options(&doc, true, Some(&NoEffects), Some(&env))
                .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert!(report.recalled.is_empty(), "{report:?}");
        assert_eq!(report.closed.len(), 1, "{report:?}");
    }

    /// `#decisionsearchmenu`: without tsift the search still ranks the
    /// ledger's answered decisions and says recall was unavailable.
    #[test]
    fn decision_search_without_tsift_ranks_the_ledger_and_says_so() {
        let (dir, doc, _) = recall_project();
        let empty_path = dir.path().join("empty-path");
        std::fs::create_dir_all(&empty_path).unwrap();
        let env = recall_env(&empty_path, 2000);
        // The request copy's own id, and the work item it was raised for.
        let report = search_decisions(
            &doc,
            "tasks/session.md",
            "[#subagent-operator-request-tdport-allowlist]",
            &env,
            10,
        )
        .unwrap();
        let top = report.hits.first().unwrap_or_else(|| panic!("{report:?}"));
        assert_eq!(top.answer, "Use the standard http ports.");
        assert_eq!(
            top.matched,
            agent_doc_answer_ledger::search::SearchMatch::CopyId
        );
        assert!(
            top.open_items
                .contains(&"subagent-operator-request-tdport-allowlist".to_string()),
            "{top:?}"
        );
        let report = search_decisions(&doc, "tasks/session.md", "#tdport", &env, 10).unwrap();
        let top = report.hits.first().unwrap_or_else(|| panic!("{report:?}"));
        assert_eq!(top.answer, "Use the standard http ports.");
        assert_eq!(
            top.matched,
            agent_doc_answer_ledger::search::SearchMatch::AppliesTo
        );
        assert_eq!(
            report.recall,
            SearchRecallStatus::Unavailable {
                detail: "no tsift executable on PATH".to_string()
            }
        );
        assert!(
            report
                .recall
                .summary()
                .contains("ranked by the answer ledger only")
        );
        // Unanswered: nothing to look up.
        let none = search_decisions(&doc, "tasks/session.md", "#redir", &env, 10).unwrap();
        assert!(none.hits.is_empty(), "{none:?}");
        // Read-only.
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("- [ ] [#subagent-operator-request-tdport-allowlist]")
        );
    }

    /// With tsift, an answer in another session document is found; an index
    /// older than this document is reported, and its hits here skipped.
    #[test]
    fn decision_search_with_tsift_finds_answers_in_other_documents() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "0");
        // Make the index older than the document.
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(dir.path().join(".tsift/index.db"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        let env = recall_env(&bin_dir, 10_000);
        let report = search_decisions(
            &doc,
            "tasks/session.md",
            "which port does the demo redirect use",
            &env,
            10,
        )
        .unwrap();
        let hit = report
            .hits
            .iter()
            .find(|hit| hit.document == "tasks/other.md")
            .unwrap_or_else(|| panic!("{report:?}"));
        assert_eq!(hit.answer, "Use port 8443.");
        assert_eq!(
            hit.matched,
            agent_doc_answer_ledger::search::SearchMatch::RecallSemantic
        );
        match &report.recall {
            SearchRecallStatus::Used {
                hits: 1,
                note: Some(note),
            } => {
                assert!(note.contains("older than this document"), "{note}")
            }
            other => panic!("{other:?}"),
        }
    }

    /// `#decisionsearchtsiftdeadline`: the search waits on its own deadline,
    /// so a tsift that takes longer than the preflight pass may spend still
    /// answers it, and the preflight pass stays bounded by its own.
    #[test]
    fn decision_search_has_its_own_deadline() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "1");
        let env = RecallEnv {
            timeout_ms: Some(100),
            search_timeout_ms: Some(30_000),
            ..recall_env(&bin_dir, 0)
        };

        let source = TsiftRecall::discover_for_search(&doc, &env).unwrap();
        assert_eq!(source.deadline(), std::time::Duration::from_secs(30));
        assert_eq!(
            TsiftRecall::discover(&doc, &env).unwrap().deadline(),
            std::time::Duration::from_millis(100)
        );

        let report = search_decisions(
            &doc,
            "tasks/session.md",
            "which port does the demo use",
            &env,
            10,
        )
        .unwrap();
        assert!(
            matches!(&report.recall, SearchRecallStatus::Used { hits, .. } if *hits > 0),
            "{:?}",
            report.recall
        );
    }

    /// Without an override the search gets the long deadline, not the
    /// preflight pass's.
    #[test]
    fn decision_search_defaults_past_the_preflight_deadline() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "0");
        let env = RecallEnv {
            timeout_ms: None,
            search_timeout_ms: None,
            ..recall_env(&bin_dir, 0)
        };
        let preflight = TsiftRecall::discover(&doc, &env).unwrap().deadline();
        let search = TsiftRecall::discover_for_search(&doc, &env)
            .unwrap()
            .deadline();
        assert_eq!(preflight, std::time::Duration::from_millis(2500));
        assert_eq!(search, std::time::Duration::from_secs(60));
    }

    /// A `tsift` that writes its arguments to the returned file, one per line.
    fn args_recording_tsift(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let bin_dir = dir.join("argsbin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let args_file = dir.join("tsift.args");
        let script = bin_dir.join("tsift");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho '{{}}'\n",
                args_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (bin_dir, args_file)
    }

    /// Only Markdown hits are eligible, so a code-heavy workspace's symbols
    /// must not fill the budgeted result rows.
    #[test]
    fn recall_asks_tsift_for_markdown_only() {
        let (dir, doc, _) = recall_project();
        let (bin_dir, args_file) = args_recording_tsift(dir.path());

        let env = recall_env(&bin_dir, 5_000);
        search_decisions(&doc, "tasks/session.md", "which port", &env, 10).unwrap();

        let args = std::fs::read_to_string(&args_file).unwrap();
        assert!(args.contains("--lang\nmarkdown\n"), "{args}");
    }

    /// Preflight stays on the project's own index, where an unscoped search at
    /// a workspace root visits every nested scope. `decision search` has a
    /// caller waiting and keeps that reach.
    #[test]
    fn only_preflight_recall_is_held_to_the_project_scope() {
        let (dir, doc, _) = recall_project();
        let (bin_dir, args_file) = args_recording_tsift(dir.path());
        let env = recall_env(&bin_dir, 5_000);

        let source = TsiftRecall::discover(&doc, &env).unwrap();
        source.recall_with_outcome(&["which port".to_string()], 5);
        let args = std::fs::read_to_string(&args_file).unwrap();
        assert!(args.contains("--scope\n<root>\n"), "{args}");

        std::fs::remove_file(&args_file).unwrap();
        search_decisions(&doc, "tasks/session.md", "which port", &env, 10).unwrap();
        let args = std::fs::read_to_string(&args_file).unwrap();
        assert!(!args.contains("--scope"), "{args}");
    }

    /// A tsift past its deadline is reported as failed; the ledger ranking
    /// still comes back.
    #[test]
    fn decision_search_reports_a_tsift_deadline() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "30");
        let env = recall_env(&bin_dir, 300);
        let report = search_decisions(&doc, "tasks/session.md", "#tdport", &env, 10).unwrap();
        assert_eq!(report.hits.len(), 1, "{report:?}");
        assert!(
            matches!(&report.recall, SearchRecallStatus::Failed { detail } if detail.contains("300ms")),
            "{:?}",
            report.recall
        );
    }

    const SEARCH_REGISTRY: &str = concat!(
        "# Decisions\n\nThis file is the authoritative current decision registry.\n\n",
        "<!-- agent:decision id=\"DEC-cache-000\" -->\n## DEC-cache-000\n",
        "- State: superseded\n- Updated: 2026-11-01T00:00:00Z\n- Evidence: (none)\n- Supersedes: (none)\n",
        "### Decision\nUse a FIFO cache for the demo sign-in sessions.\n### Rationale\nSimplest.\n<!-- /agent:decision -->\n\n",
        "<!-- agent:decision id=\"DEC-cache-001\" -->\n## DEC-cache-001\n",
        "- State: accepted\n- Updated: 2026-11-05T00:00:00Z\n- Evidence: (none)\n- Supersedes: DEC-cache-000\n",
        "### Decision\nUse an LRU cache for the demo sign-in sessions.\n### Rationale\nHit rate.\n<!-- /agent:decision -->\n\n",
    );

    fn project_documents(doc: &Path, other: &Path) -> ProjectDocuments {
        ProjectDocuments {
            documents: vec![doc.to_path_buf(), other.to_path_buf()],
            scan_exhausted: false,
        }
    }

    /// `#decisionsearchnofile`: with no FILE the ledger of every session
    /// document and the registry are searched, without tsift, and every hit
    /// names where it came from.
    #[test]
    fn project_search_covers_every_document_and_the_registry() {
        let (dir, doc, other) = recall_project();
        std::fs::write(dir.path().join("decisions.md"), SEARCH_REGISTRY).unwrap();
        let before = std::fs::read_to_string(&doc).unwrap();
        let empty_path = dir.path().join("empty-path");
        std::fs::create_dir_all(&empty_path).unwrap();
        let env = recall_env(&empty_path, 2000);
        let project = project_documents(&doc, &other);
        let search =
            |query: &str| search_project_decisions(dir.path(), &project, query, &env, 10).unwrap();

        // One document's answer, by id, from either document.
        let here = search("#tdport");
        assert_eq!(here.hits[0].document, "tasks/session.md", "{here:?}");
        assert_eq!(here.hits[0].answer, "Use the standard http ports.");
        let there = search("#portpick");
        assert_eq!(there.hits[0].document, "tasks/other.md", "{there:?}");
        assert_eq!(there.hits[0].answer, "Use port 8443.");

        // A wording match spans documents and the registry.
        let report = search("demo sign-in");
        let documents: BTreeSet<&str> = report
            .hits
            .iter()
            .map(|hit| hit.document.as_str())
            .collect();
        assert!(documents.contains("tasks/other.md"), "{report:?}");
        assert!(documents.contains("decisions.md"), "{report:?}");

        // The registry, by id.
        let record = search("DEC-cache-001");
        assert_eq!(record.hits[0].document, "decisions.md");
        assert_eq!(
            record.hits[0].answer,
            "Use an LRU cache for the demo sign-in sessions."
        );
        let replaced = search("DEC-cache-000");
        assert_eq!(replaced.hits[0].superseded_by, ["DEC-cache-001"]);
        assert!(!replaced.hits[0].is_current());

        let scope = report.project.as_ref().unwrap();
        assert!(report.document.is_none());
        assert_eq!(scope.documents, 2);
        assert!(scope.skipped.is_empty());
        assert_eq!(scope.registry, RegistrySearchStatus::Read { records: 2 });
        assert!(matches!(
            report.recall,
            SearchRecallStatus::Unavailable { .. }
        ));

        // Limits apply to the merged list.
        let one = search_project_decisions(dir.path(), &project, "demo sign-in", &env, 1).unwrap();
        assert_eq!(one.hits.len(), 1);

        // Read-only.
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), before);
    }

    /// A miss must not read as "not answered" when part of the project was
    /// not searched: every document or registry that was passed over is named.
    #[test]
    fn project_search_names_what_it_did_not_search() {
        let (dir, doc, other) = recall_project();
        let empty_path = dir.path().join("empty-path");
        std::fs::create_dir_all(&empty_path).unwrap();
        let env = recall_env(&empty_path, 2000);

        let report = search_project_decisions(
            dir.path(),
            &project_documents(&doc, &other),
            "#tdport",
            &env,
            10,
        )
        .unwrap();
        assert_eq!(
            report.project.as_ref().unwrap().registry,
            RegistrySearchStatus::Missing
        );

        std::fs::write(dir.path().join("decisions.md"), "<!-- agent:decision -->\n").unwrap();
        let huge = dir.path().join("tasks/huge.md");
        std::fs::write(&huge, "x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&huge)
            .unwrap()
            .set_len(MAX_RECALL_DOCUMENT_BYTES + 1)
            .unwrap();
        let project = ProjectDocuments {
            documents: vec![doc.clone(), huge, dir.path().join("tasks/gone.md")],
            scan_exhausted: true,
        };
        let report = search_project_decisions(dir.path(), &project, "#tdport", &env, 10).unwrap();
        let scope = report.project.as_ref().unwrap();
        assert_eq!(report.hits[0].document, "tasks/session.md", "{report:?}");
        assert_eq!(scope.documents, 1);
        assert!(scope.scan_exhausted);
        assert!(
            matches!(&scope.registry, RegistrySearchStatus::Unreadable { detail } if !detail.is_empty()),
            "{scope:?}"
        );
        let skipped: Vec<&str> = scope
            .skipped
            .iter()
            .map(|row| row.document.as_str())
            .collect();
        assert_eq!(skipped, ["tasks/huge.md", "tasks/gone.md"], "{scope:?}");
        assert!(
            scope.skipped[0].reason.contains("name it as FILE"),
            "{scope:?}"
        );
    }

    /// A project search maps a tsift hit onto the document's own ledger and
    /// says how many documents the index is older than.
    #[test]
    fn project_search_maps_recall_onto_the_documents_it_read() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "0");
        let env = recall_env(&bin_dir, 10_000);
        let project = project_documents(&doc, &other);
        let report =
            search_project_decisions(dir.path(), &project, "kubernetes namespace", &env, 10)
                .unwrap();
        assert_eq!(report.hits.len(), 1, "{report:?}");
        assert_eq!(report.hits[0].document, "tasks/other.md");
        assert_eq!(
            report.hits[0].matched,
            agent_doc_answer_ledger::search::SearchMatch::RecallSemantic
        );

        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(dir.path().join(".tsift/index.db"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        let report =
            search_project_decisions(dir.path(), &project, "kubernetes namespace", &env, 10)
                .unwrap();
        assert!(report.hits.is_empty(), "{report:?}");
        match &report.recall {
            SearchRecallStatus::Used {
                hits: 1,
                note: Some(note),
            } => {
                assert!(note.contains("older than 2 searched documents"), "{note}")
            }
            other => panic!("{other:?}"),
        }
    }

    /// `#decisionsearchmenu`: the report offers the lookup menu only on a
    /// turn that is about to ask the operator something.
    #[test]
    fn the_lookup_menu_needs_a_question_cue_and_an_answer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- ~~[#lic]: Apache-2.0.~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:todo -->\n",
            "- [x] [#lic] Pick the license for the crates\n",
            "- [ ] [#port] Choose the demo port\n",
            "- [ ] [#build] Implement the release build\n",
            "<!-- /agent:todo -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let report = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        assert_eq!(report.answered_decisions, 1, "{report:?}");
        assert!(report.facts.is_empty(), "{report:?}");
        assert!(report.lookup_menu("session.md", &["do [#build]"]).is_none());
        let menu = report
            .lookup_menu("session.md", &["do [#port]"])
            .unwrap_or_else(|| panic!("{report:?}"));
        assert!(
            menu.contains("agent-doc decision search session.md"),
            "{menu}"
        );
        assert!(
            AnswerLedgerReport::default()
                .lookup_menu("session.md", &["do [#port]"])
                .is_none()
        );
    }

    /// Agent-free path: a plain force-disk maintenance pass (the closeout
    /// path) closes the stale request copy, keeps the work todo open, and the
    /// facts carry the answer; no agent command is involved.
    #[test]
    fn maintenance_closes_the_answered_copy_and_reports_facts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, DOC).unwrap();
        let report = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        assert_eq!(report.closed.len(), 1, "{report:?}");
        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(
            written.contains("- [x] [#subagent-operator-request-tdport-allowlist]"),
            "{written}"
        );
        assert!(written.contains("already answered `"), "{written}");
        assert!(written.contains("- [ ] [#tdport]"), "{written}");

        // Idempotent second pass.
        let again = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        assert!(again.closed.is_empty());
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), written);

        // Re-asking the same request id at closeout is refused.
        let response = "[#subagent-operator-request-tdport-allowlist] subagent operator request\n- *kind:* question\n- *blocking:* true\n- *request:*\n  - Choose how to add the demo callback.\n- *choices:* []\n";
        let reasks = reasks_for_closeout(&doc, &written, response, &[]);
        assert_eq!(reasks.len(), 1, "{reasks:?}");
        assert!(
            agent_doc_answer_ledger::reask_refusal(&reasks)
                .contains("Use the standard http ports.")
        );
    }

    /// `#decisionidstamp`: `preflight --probe` gets the same facts the real
    /// pass reports, from a simulation that writes neither the document nor
    /// the index.
    #[test]
    fn the_probe_reports_facts_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, DECISION_DOC).unwrap();
        let report = probe_answer_ledger(&doc).unwrap();
        assert_eq!(report.closed.len(), 1, "{report:?}");
        let fact = tdport_fact(&report);
        assert_eq!(fact.answer, "Use port 3000.");
        assert_eq!(
            fact.open_items,
            vec!["tdport".to_string()],
            "the simulated closure is reflected"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            DECISION_DOC,
            "document untouched"
        );
        let (root, document_id) = index_target(&doc).unwrap();
        if let Some(Ok((keys, _))) =
            agent_doc_sqlite::answer_ledger_index::load_for_project(&root, &document_id)
        {
            assert!(keys.is_empty(), "index untouched: {keys:?}");
        }
        if let Some(Ok(seeded)) =
            agent_doc_sqlite::answer_ledger_index::history_seeded_for_project(&root, &document_id)
        {
            assert!(!seeded, "no history rebuild from a probe");
        }
    }

    /// `#decisionidstamp`: the consumed answer row was pruned from the
    /// document before any ledger pass wrote the index. The first pass
    /// rebuilds the index from git history, so the answer still reaches the
    /// open copy; the rebuild runs once.
    #[test]
    fn the_first_pass_recovers_an_answer_pruned_before_the_index_existed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        let doc = root.join("session.md");
        std::fs::write(&doc, DECISION_DOC).unwrap();
        git(root, &["add", "session.md"]);
        git(root, &["commit", "-q", "--no-verify", "-m", "answered"]);
        let pruned = DECISION_DOC.replace(
            "- ~~[#subagent-operator-request-tdport-pick]: Use port 3000.~~\n",
            "",
        );
        std::fs::write(&doc, &pruned).unwrap();
        git(root, &["commit", "-q", "--no-verify", "-am", "pruned"]);

        let report = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        let fact = tdport_fact(&report);
        assert_eq!(fact.answer, "Use port 3000.");
        assert!(fact.location.starts_with("git history "), "{fact:?}");
        let (index_root, document_id) = index_target(&doc).unwrap();
        assert!(
            agent_doc_sqlite::answer_ledger_index::history_seeded_for_project(
                &index_root,
                &document_id
            )
            .unwrap()
            .unwrap()
        );
        // Without the history rebuild the open copy would have no answer.
        let observations = agent_doc_answer_ledger::extract::observe(&pruned, &[]);
        let document_only = Ledger::build(&observations, &PriorIndex::default(), 0, None);
        assert!(document_only.answered_facts().is_empty());
    }
}
