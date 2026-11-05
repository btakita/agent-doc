//! Premise change detection at preflight (`#firstprinciplesrederive`).
//!
//! Every preflight digests each principle and fact in scope for the document
//! ([`agent_doc_principles::rederive`]) and compares the digests with the ones
//! stored in the project `state.db`
//! ([`agent_doc_sqlite::premise_statement_index`]). A premise whose statement
//! changed or that disappeared puts every conclusion derived from it into the
//! cycle contract (`premise_changes`: re-derive unreviewed conclusions, flag
//! confirmed ones), together with the derivation graph's contradictions
//! (`derivation_contradictions`).
//!
//! The stored digests advance only when a turn starts, so a principle edited
//! on its own (a content-only edit, which starts no turn) is reported on the
//! next turn that runs instead of being consumed by a contract nobody acts
//! on. A probe never writes. The first pass for a document records a baseline
//! and reports no changes.

use std::path::Path;

use agent_doc_principles::derivation::{Derivation, Graph, NodeKind};
use agent_doc_principles::rederive::{self, PremiseRecord, RederiveReport};
use agent_doc_principles::{Layer, Principle};
use agent_doc_sqlite::premise_statement_index::PremiseStatementRow;

/// The derivations stored for `file` (`#firstprinciplesstore`: the
/// decision-graph tables in the project `state.db`), with the operator
/// reviews `content` shows applied, so a confirmed step is flagged and an
/// unreviewed one re-derived. Read-only. With none (or no project, or an
/// unreadable store), premise changes are still reported, each with an empty
/// `affected` list, and there are no contradictions.
pub fn stored_derivations(file: &Path, content: &str) -> Vec<Derivation> {
    if agent_doc_project_root_io::project_root_containing(file).is_none() {
        return Vec::new();
    }
    match crate::derivation_store::load(file, content) {
        Ok(stored) => stored
            .iter()
            .map(crate::derivation_store::StoredDerivation::to_model)
            .collect(),
        Err(err) => {
            eprintln!(
                "[preflight] stored derivations unreadable ({err:#}); premise changes reach no conclusions this turn"
            );
            Vec::new()
        }
    }
}

fn index_target(file: &Path) -> Option<(std::path::PathBuf, String)> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    Some((root, agent_doc_hash::document_id_for_path(file)))
}

fn kind_name(kind: NodeKind) -> &'static str {
    kind.as_str()
}

fn kind_of(name: &str) -> Option<NodeKind> {
    match name {
        "principle" => Some(NodeKind::Principle),
        "fact" => Some(NodeKind::Fact),
        _ => None,
    }
}

fn to_row(record: &PremiseRecord) -> PremiseStatementRow {
    PremiseStatementRow {
        kind: kind_name(record.kind).to_string(),
        premise_id: record.id.clone(),
        digest: record.digest.clone(),
        statement: record.statement.clone(),
        source: record.source.clone(),
    }
}

fn from_row(row: PremiseStatementRow) -> Option<PremiseRecord> {
    Some(PremiseRecord {
        kind: kind_of(&row.kind)?,
        id: row.premise_id,
        statement: row.statement,
        digest: row.digest,
        source: row.source,
    })
}

/// What the stored digests say about `file`.
enum Prior {
    /// Nothing stored yet (or no state database here): record a baseline.
    Unrecorded,
    /// The stored set could not be read: report nothing, write nothing.
    Unreadable,
    Recorded(Vec<PremiseRecord>),
}

fn load_prior(root: &Path, document_id: &str) -> Prior {
    match agent_doc_sqlite::premise_statement_index::load_for_project(root, document_id) {
        None | Some(Ok(None)) => Prior::Unrecorded,
        Some(Ok(Some(rows))) => Prior::Recorded(rows.into_iter().filter_map(from_row).collect()),
        Some(Err(err)) => {
            eprintln!(
                "[preflight] premise digests unreadable ({err:#}); premise changes not checked this turn"
            );
            Prior::Unreadable
        }
    }
}

fn graph_of(derivations: Vec<Derivation>) -> Graph {
    Graph::new(derivations).unwrap_or_else(|err| {
        eprintln!("[preflight] derivation graph skipped: {err}");
        Graph::default()
    })
}

/// Compare the principles and facts in scope with the stored digests and
/// build the contract payload. When `record` is set (a turn starts and this is
/// not a probe), store the current digests afterwards.
pub fn premise_rederive(
    file: &Path,
    principles: &[Principle],
    layers: &[Layer],
    derivations: Vec<Derivation>,
    record: bool,
) -> RederiveReport {
    let facts = rederive::facts_from_layers(layers);
    let current = rederive::premise_records(principles, &facts, agent_doc_hash::content_hash);
    let graph = graph_of(derivations);
    let Some((root, document_id)) = index_target(file) else {
        return rederive::report(Vec::new(), &graph);
    };
    let prior = load_prior(&root, &document_id);
    let changes = match &prior {
        Prior::Recorded(previous) => rederive::diff(previous, &current),
        Prior::Unrecorded | Prior::Unreadable => Vec::new(),
    };
    let report = rederive::report(changes, &graph);
    for change in &report.premise_changes {
        let premise = &change.change.premise;
        eprintln!(
            "[preflight] premise {} [#{}] {:?}: {} conclusion(s) affected (#firstprinciplesrederive)",
            premise.kind.as_str(),
            premise.id,
            change.change.change,
            change.affected.len()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "premise_changed file={} kind={} id={} change={} affected={} recorded={record}",
                file.display(),
                premise.kind.as_str(),
                premise.id,
                match change.change.change {
                    rederive::PremiseChangeKind::Changed => "changed",
                    rederive::PremiseChangeKind::Removed => "removed",
                },
                change.affected.len()
            ),
        );
    }
    if record && !matches!(prior, Prior::Unreadable) {
        let rows: Vec<PremiseStatementRow> = current.iter().map(to_row).collect();
        if let Err(err) = agent_doc_sqlite::premise_statement_index::replace_for_project(
            &root,
            &document_id,
            &rows,
            agent_doc_log_time::current_epoch_secs(),
        ) {
            eprintln!("[preflight] premise digests not recorded ({err:#})");
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_principles::derivation::{Conclusion, DerivationStatus, ImpactAction, NodeRef};
    use agent_doc_principles::{PrincipleScope, compose};

    struct Fixture {
        _dir: tempfile::TempDir,
        doc: std::path::PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "").unwrap();
        Fixture { _dir: dir, doc }
    }

    fn layers(doc: &Path, rules: &[&str]) -> Vec<Layer> {
        vec![Layer {
            scope: PrincipleScope::Document,
            source: doc.display().to_string(),
            rules: rules.iter().map(|rule| rule.to_string()).collect(),
        }]
    }

    fn pass(doc: &Path, rules: &[&str], record: bool) -> RederiveReport {
        let layers = layers(doc, rules);
        let principles = compose(&layers).principles;
        premise_rederive(doc, &principles, &layers, derivations(), record)
    }

    fn derivations() -> Vec<Derivation> {
        vec![
            Derivation {
                id: "drv-ship".into(),
                premises: vec![
                    NodeRef::new(NodeKind::Principle, "velocity"),
                    NodeRef::new(NodeKind::Fact, "blocked"),
                ],
                inference: "a blocked release favours velocity".into(),
                conclusion: Conclusion {
                    decision: "dec-ship".into(),
                    answer: "ship now".into(),
                },
                status: DerivationStatus::AgentDerivedUnreviewed,
            },
            Derivation {
                id: "drv-confirmed".into(),
                premises: vec![NodeRef::new(NodeKind::Fact, "blocked")],
                inference: "operator confirmed".into(),
                conclusion: Conclusion {
                    decision: "dec-hotfix".into(),
                    answer: "hotfix branch".into(),
                },
                status: DerivationStatus::OperatorConfirmed,
            },
        ]
    }

    fn affected(report: &RederiveReport) -> Vec<(String, Vec<(String, ImpactAction)>)> {
        report
            .premise_changes
            .iter()
            .map(|change| {
                (
                    format!(
                        "{}:{}",
                        change.change.premise.kind.as_str(),
                        change.change.premise.id
                    ),
                    change
                        .affected
                        .iter()
                        .map(|impact| (impact.derivation.clone(), impact.action))
                        .collect(),
                )
            })
            .collect()
    }

    const BASE: &[&str] = &[
        "principle [#velocity]: When a release is blocked, velocity over polish.",
        "fact [#blocked]: The 0.36 release is blocked on review.",
    ];

    /// The stored derivations (`#firstprinciplesstore`) reach the premise
    /// check: a changed fact re-derives a stored unreviewed step and flags a
    /// stored step the operator confirmed; a rejected step is not affected.
    #[test]
    fn a_changed_fact_reaches_the_stored_derivations() {
        let fixture = fixture();
        let content = concat!(
            "---\nagent_doc_session: stored-rederive\n---\n\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#tdcap] Decide whether to cap the session token lifetime\n",
            "- [ ] [#tdrot] Decide whether to rotate the signing key monthly\n",
            "<!-- /agent:todo -->\n",
        );
        std::fs::write(&fixture.doc, content).unwrap();
        let rules: &[&str] = &[
            "principle [#simple]: Prefer the simplest mechanism that is correct.",
            "fact [#jws]: Session tokens are JWS and carry their own expiry.",
        ];
        let base_layers = layers(&fixture.doc, rules);
        let principles = compose(&base_layers).principles;
        let step = |decision: &str, answer: &str, inference: &str| {
            crate::derivation_store::derive(
                &fixture.doc,
                content,
                &principles,
                &crate::derivation_store::DeriveRequest {
                    premises: vec![
                        "principle:simple".into(),
                        "fact:jws=Session tokens are JWS and carry their own expiry.".into(),
                    ],
                    inference: inference.into(),
                    decision: decision.into(),
                    answer: answer.into(),
                    id: None,
                },
            )
            .unwrap()
            .derivation
            .id
        };
        let unreviewed = step("tdcap", "No cap.", "A self-expiring JWS already bounds it.");
        let confirmed = step("tdrot", "Rotate yearly.", "JWS expiry bounds a leaked key.");
        let rejected = step("tdrot", "Never rotate.", "JWS expiry is enough.");
        let (root, document_id) = index_target(&fixture.doc).unwrap();
        for (id, status) in [(&confirmed, "operator_confirmed"), (&rejected, "rejected")] {
            assert!(
                agent_doc_sqlite::answer_ledger_derivations::set_derivation_status_for_project(
                    &root,
                    &document_id,
                    id,
                    status,
                    None,
                    1,
                )
                .unwrap()
            );
        }
        let stored = stored_derivations(&fixture.doc, content);
        assert_eq!(stored.len(), 3, "{stored:?}");

        let pass = |rules: &[&str]| {
            let rule_layers = layers(&fixture.doc, rules);
            let principles = compose(&rule_layers).principles;
            premise_rederive(
                &fixture.doc,
                &principles,
                &rule_layers,
                stored_derivations(&fixture.doc, content),
                true,
            )
        };
        assert!(pass(rules).premise_changes.is_empty(), "baseline");
        let report = pass(&[
            "principle [#simple]: Prefer the simplest mechanism that is correct.",
            "fact [#jws]: Session tokens are opaque and never expire on their own.",
        ]);
        let mut expected = vec![
            (unreviewed.clone(), ImpactAction::Rederive),
            (confirmed.clone(), ImpactAction::Flag),
        ];
        expected.sort_by(|a, b| a.0.cmp(&b.0));
        let mut got = affected(&report);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].0, "fact:jws");
        got[0].1.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(got[0].1, expected);
    }

    #[test]
    fn the_first_pass_records_a_baseline_and_reports_nothing() {
        let fixture = fixture();
        assert!(pass(&fixture.doc, BASE, true).premise_changes.is_empty());
        assert!(pass(&fixture.doc, BASE, true).premise_changes.is_empty());
    }

    #[test]
    fn an_edited_principle_and_a_removed_fact_reach_their_conclusions_once_recorded() {
        let fixture = fixture();
        pass(&fixture.doc, BASE, true);
        let edited = &[
            "principle [#velocity]: Correctness over velocity, always.",
            "Run the focused test first.",
        ];
        let report = pass(&fixture.doc, edited, true);
        assert_eq!(
            affected(&report),
            vec![
                (
                    "principle:velocity".into(),
                    vec![("drv-ship".into(), ImpactAction::Rederive)]
                ),
                (
                    "fact:blocked".into(),
                    vec![
                        ("drv-confirmed".into(), ImpactAction::Flag),
                        ("drv-ship".into(), ImpactAction::Rederive),
                    ]
                ),
            ]
        );
        assert_eq!(
            report.premise_changes[0].change.statement.as_deref(),
            Some("Correctness over velocity, always.")
        );
        assert!(
            pass(&fixture.doc, edited, true).premise_changes.is_empty(),
            "a recorded change is not reported again"
        );
    }

    #[test]
    fn a_change_seen_without_a_turn_is_reported_again_until_one_starts() {
        let fixture = fixture();
        pass(&fixture.doc, BASE, true);
        let edited = &[
            "principle [#velocity]: When a release is blocked, velocity over polish.",
            "fact [#blocked]: The 0.36 release shipped.",
        ];
        assert_eq!(pass(&fixture.doc, edited, false).premise_changes.len(), 1);
        assert_eq!(
            pass(&fixture.doc, edited, false).premise_changes.len(),
            1,
            "a probe or a no-turn pass leaves the digests alone"
        );
        assert_eq!(pass(&fixture.doc, edited, true).premise_changes.len(), 1);
        assert!(pass(&fixture.doc, edited, true).premise_changes.is_empty());
    }

    #[test]
    fn reflowing_a_statement_is_not_a_change() {
        let fixture = fixture();
        pass(&fixture.doc, BASE, true);
        let reflowed = &[
            "principle [#velocity]:\n  When a release is blocked,\n  velocity over polish.",
            "fact [#blocked]: The 0.36 release   is blocked on review.",
        ];
        assert!(
            pass(&fixture.doc, reflowed, true)
                .premise_changes
                .is_empty()
        );
    }
}
