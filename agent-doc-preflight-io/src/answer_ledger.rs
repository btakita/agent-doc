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
    let mut excluded: std::collections::BTreeSet<std::path::PathBuf> =
        agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, content)
            .map(|sources| {
                sources
                    .into_iter()
                    .map(|source| canonical(&source.path))
                    .collect()
            })
            .unwrap_or_default();
    excluded.insert(here.clone());
    let rows = agent_doc_answer_ledger::extract::observe(content, &[]);
    let mut load = |path: &str| -> Option<OtherDocument> {
        let path = Path::new(path);
        if excluded.contains(path)
            || std::fs::metadata(path).ok()?.len() > MAX_RECALL_DOCUMENT_BYTES
        {
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
    };
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
    Ok(report_of(&ledger, &graph, pass.closed, pass.stale_closed))
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
    let mut report = report_of(&report_ledger, &graph, closed, stale_closed);
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
