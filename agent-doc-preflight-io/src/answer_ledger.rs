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
//! The index is pruned by relevance (`#decisionprune`): each pass stores the
//! ledger's hot / archived partition, so the hot tables hold only live
//! decisions (liveness also follows the decision graph's edges), and loads the
//! archived decisions the document (or the closeout's new questions) names, so
//! a dormant decision revives on demand.

use std::collections::BTreeSet;
use std::path::Path;

use agent_doc_answer_ledger::graph::{DecisionGraph, EdgeSource, GraphEdge, Node, NodeKind};
use agent_doc_answer_ledger::{
    AnsweredFact, DuplicateClosure, Ledger, PossibleMatch, PriorIndex, PriorKey, PriorRevision,
    Reask,
};
use anyhow::Result;

/// What one ledger pass produced.
#[derive(Debug, Clone, Default)]
pub struct AnswerLedgerReport {
    pub facts: Vec<AnsweredFact>,
    pub possible: Vec<PossibleMatch>,
    pub closed: Vec<DuplicateClosure>,
    /// Archived decisions a new question named this pass (`#decisionprune`).
    pub revived: Vec<String>,
    pub hot_store: agent_doc_answer_ledger::HotStoreSize,
}

impl AnswerLedgerReport {
    /// Cycle-contract guidance; `None` when nothing is answered or possible.
    pub fn guidance(&self) -> Option<String> {
        agent_doc_answer_ledger::contract_guidance(&self.facts, &self.possible)
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

/// The decision graph for `content` (`#decisiongraph`). Read-only.
pub fn build_graph(file: &Path, content: &str) -> (Ledger, DecisionGraph) {
    let (observations, ledger) = build_ledger(file, content);
    let graph = graph_of(file, &ledger, &observations);
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
    let observations = observe_document(file, content);
    let ledger = Ledger::build(&observations, prior, now, None);
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
    let mut target = content.to_string();
    let mut ledger = ledger;
    let mut observations = observations;
    if !closed.is_empty()
        && let Some(next) = agent_doc_answer_ledger::apply_duplicate_closures(&target, &closed)?
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
        stamped,
        target: (target != content).then_some(target),
    })
}

fn report_of(
    ledger: &Ledger,
    graph: &DecisionGraph,
    closed: Vec<DuplicateClosure>,
) -> AnswerLedgerReport {
    let mut facts = ledger.answered_facts();
    attach_change_impact(&mut facts, graph);
    AnswerLedgerReport {
        facts,
        possible: ledger.possible.clone(),
        closed,
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
    run_answer_ledger_maintenance_with_options(file, false, None)
}

/// Force-disk counterpart for an explicitly force-disk strict closeout.
pub fn run_answer_ledger_maintenance_force_disk(
    file: &Path,
    write_effects: &dyn super::PreflightMaintenanceWriteEffects,
) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, true, Some(write_effects))
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
    Ok(report_of(&ledger, &graph, pass.closed))
}

fn run_answer_ledger_maintenance_with_options(
    file: &Path,
    force_disk: bool,
    write_effects: Option<&dyn super::PreflightMaintenanceWriteEffects>,
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
    let mut report_ledger = pass.ledger;
    let mut report_observations = pass.observations;
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
    }
    let graph = graph_of(file, &report_ledger, &report_observations);
    // `#decisionprune` x `#decisiongraph`: graph edges (depends_on,
    // conflicts_with, derived_from, exception_to, ...) carry liveness too.
    report_ledger.recompute_liveness(&report_observations.open_item_ids, &named_edges(&graph));
    store_index(file, &report_ledger, &graph, now);
    let report = report_of(&report_ledger, &graph, closed);
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
            "- ~~[#tdport]: Use port 8080.~~\n",
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
